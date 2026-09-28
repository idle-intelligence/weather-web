//! Tests for select.rs, estimate.rs and nws.rs: the logic that moved out of
//! the trucs.ai page's index.html/sources.js (station selection, the 100 km
//! radius, the removed farther-station fallback, the 90-minute freshness
//! cutoff, and NWS unit parsing).
//!
//! Station data: tests/fixtures/stations_subset.json, a small real subset of
//! the full station list (published as the idle-intelligence/metar-stations
//! dataset on Hugging Face) covering the points these tests check: the
//! Sahara point (nearest station DATM, far outside the 100 km radius), the
//! Lille 100 km ring (10 real stations within range: LFQQ, EBOS, LFAQ, LFAC,
//! EHFS and EHSG report; LFQI, EBCV, EBFN and LFOW are real IEM roster
//! stations that never reported in the dataset's 7-day window and are
//! carried here with `active: false`, closer to Lille than some of the
//! reporting ones -- EBSZ and LFYG are not IEM ASOS roster stations at all
//! and so have no real row to add), a Paris cluster (LFPG and its nearest
//! neighbours), an Alps cluster (Aosta/Sion/Annecy/Geneva/Chambery/Payerne,
//! a sparser mountain ring), and the New York and Toronto city clusters.
//! NWS fixtures: two real api.weather.gov /observations/latest responses,
//! fetched once with `curl -H 'User-Agent: weather-web-tests'` (no email).

use std::collections::HashMap;
use weather::estimate::{estimate, estimate_with, EstimateParams, EstimateStatus};
use weather::nws::{nws_station_id, parse_nws_latest};
use weather::observation::Observation;
use weather::select::{select, select_with, Selection, SelectParams};
use weather::stations::{haversine_km, load_stations, Neighbor};

const NOW_MILLIS: i64 = 1790524969585; // matches tests/fixtures/snapshot_subset.json's fetched_at_millis

fn stations() -> Vec<weather::stations::Station> {
    load_stations(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/stations_subset.json"
    ))
    .expect("loading tests/fixtures/stations_subset.json")
}

fn fresh_obs(temp_c: f64, dewpoint_c: f64, wind_ms: f64, wind_dir_deg: f64, pressure_hpa: f64, minutes_ago: f64) -> Observation {
    Observation {
        temp_c: Some(temp_c),
        dewpoint_c: Some(dewpoint_c),
        wind_ms: Some(wind_ms),
        wind_dir_deg: Some(wind_dir_deg),
        pressure_hpa: Some(pressure_hpa),
        obs_time_millis: Some(NOW_MILLIS - (minutes_ago * 60_000.0) as i64),
        source: "IEM".to_string(),
    }
}

// ---- select ----

#[test]
fn sahara_point_has_no_station_within_radius() {
    let stations = stations();
    let sel = select(&stations, 24.20, 2.03).expect("stations list is non-empty");
    assert!(
        sel.stations.is_empty(),
        "expected no station within 100 km of the Sahara point, got {:?}",
        sel.stations.iter().map(|n| &n.station.icao).collect::<Vec<_>>()
    );
    assert_eq!(sel.nearest.station.icao, "DATM");
    let expected_km = haversine_km(24.20, 2.03, sel.nearest.station.lat, sel.nearest.station.lon);
    assert!((sel.nearest.distance - expected_km).abs() < 1e-6);
    assert!(sel.nearest.distance > weather::stations::MAX_RADIUS_KM);
}

/// Lat/lon used by every Lille test in this file.
const LILLE: (f64, f64) = (50.6292, 3.0573);

/// Builds a Selection from an explicit list of ICAOs, at the Lille point,
/// bypassing `select`'s own ranking. Used by tests that exercise the
/// estimate/corrections logic and want a fixed, known set of stations
/// regardless of how the fixture's distances happen to rank.
fn lille_selection_of(icaos: &[&str]) -> Selection {
    let stations = stations();
    let neighbours: Vec<Neighbor> = icaos
        .iter()
        .map(|icao| {
            let station = stations
                .iter()
                .find(|s| s.icao == *icao)
                .unwrap_or_else(|| panic!("{icao} not in stations_subset.json"))
                .clone();
            let distance = haversine_km(LILLE.0, LILLE.1, station.lat, station.lon);
            Neighbor { station, distance }
        })
        .collect();
    let nearest = neighbours[0].clone();
    Selection {
        stations: neighbours,
        nearest,
    }
}

#[test]
fn lille_select_is_active_agnostic_and_picks_the_nearest_five() {
    // `select` (the trucs.ai page's k=5, 100 km defaults) ranks by distance
    // only: LFQI, EBCV and EBFN are silent (active: false) but are closer to
    // Lille than EBOS/LFAQ/LFAC/EHFS, so they win the top five. Filtering by
    // activity is `estimate`'s job, not `select`'s.
    let stations = stations();
    let sel = select(&stations, LILLE.0, LILLE.1).expect("stations list is non-empty");
    let icaos: Vec<&str> = sel.stations.iter().map(|n| n.station.icao.as_str()).collect();
    assert_eq!(icaos, vec!["LFQQ", "LFQI", "EBCV", "EBFN", "EBOS"]);
    for (icao, active) in [("LFQQ", true), ("LFQI", false), ("EBCV", false), ("EBFN", false), ("EBOS", true)] {
        let n = sel.stations.iter().find(|n| n.station.icao == icao).unwrap();
        assert_eq!(n.station.active, active, "{icao} active flag");
    }
    for farther in ["LFAQ", "LFAC", "LFOW", "EHFS", "EHSG"] {
        assert!(
            !icaos.contains(&farther),
            "{farther} is farther than the nearest five and should be cut by k=5, got {icaos:?}"
        );
    }
}

#[test]
fn lille_select_with_lists_up_to_twelve_including_silent_stations() {
    let stations = stations();
    let params = SelectParams {
        k: weather::stations::DEFAULT_LIST_K,
        max_radius_km: weather::stations::MAX_RADIUS_KM,
    };
    let sel = select_with(&stations, LILLE.0, LILLE.1, &params).expect("stations list is non-empty");
    let icaos: Vec<&str> = sel.stations.iter().map(|n| n.station.icao.as_str()).collect();
    // Every real station within 100 km of Lille in the fixture, nearest
    // first, whether it reports or not.
    assert_eq!(
        icaos,
        vec!["LFQQ", "LFQI", "EBCV", "EBFN", "EBOS", "LFAQ", "LFAC", "LFOW", "EHFS", "EHSG"]
    );
}

#[test]
fn estimate_with_uses_only_active_fresh_stations_nearest_first() {
    let sel = lille_selection_of(&[
        "LFQQ", "LFQI", "EBCV", "EBFN", "EBOS", "LFAQ", "LFAC", "LFOW", "EHFS", "EHSG",
    ]);

    // Observations are fetched only for the reporting (active) stations, as
    // the weather-web demo does: LFQI/EBCV/EBFN/LFOW never get an
    // observation at all.
    let mut obs: HashMap<String, Observation> = HashMap::new();
    obs.insert("LFQQ".into(), fresh_obs(14.2, 11.4, 5.6, 230.0, 1018.3, 10.0));
    obs.insert("EBOS".into(), fresh_obs(14.6, 11.8, 4.9, 220.0, 1018.6, 15.0));
    obs.insert("LFAQ".into(), fresh_obs(14.8, 11.9, 5.2, 225.0, 1018.5, 20.0));
    obs.insert("LFAC".into(), fresh_obs(13.9, 11.1, 4.5, 215.0, 1018.1, 25.0));
    obs.insert("EHFS".into(), fresh_obs(13.5, 10.8, 6.1, 240.0, 1017.9, 5.0));
    obs.insert("EHSG".into(), fresh_obs(13.6, 10.9, 5.8, 235.0, 1017.8, 30.0));

    let params = EstimateParams { estimate_k: 5, max_age_min: weather::corrections::MAX_AGE_MIN };
    let est = estimate_with(&sel, &obs, Some(35.0), NOW_MILLIS, &params);

    assert!(matches!(est.status, EstimateStatus::Ok));
    assert_eq!(est.stations.len(), 10, "every listed station is still reported");
    assert_eq!(est.stations_used, 5, "capped at estimate_k");

    let used: Vec<&str> = est
        .stations
        .iter()
        .filter(|s| s.used)
        .map(|s| s.icao.as_str())
        .collect();
    // Nearest five active stations with a fresh observation: the silent
    // LFQI/EBCV/EBFN/LFOW, despite being closer, are skipped.
    assert_eq!(used, vec!["LFQQ", "EBOS", "LFAQ", "LFAC", "EHFS"]);

    for silent in ["LFQI", "EBCV", "EBFN", "LFOW"] {
        let s = est.stations.iter().find(|s| s.icao == silent).unwrap();
        assert!(!s.active, "{silent} should be flagged inactive");
        assert!(!s.used, "{silent} should not have fed the average");
        assert!(!s.has_observation, "{silent} was never fetched");
    }
    let ehsg = est.stations.iter().find(|s| s.icao == "EHSG").unwrap();
    assert!(ehsg.active && !ehsg.used, "EHSG has an observation but is 6th nearest, cut by estimate_k");

    assert!(est.temperature_c.is_some());
}

#[test]
fn paris_selects_lfpg_and_its_four_nearest_neighbours() {
    let stations = stations();
    let sel = select(&stations, 49.0153, 2.5344).expect("stations list is non-empty");
    let icaos: Vec<&str> = sel.stations.iter().map(|n| n.station.icao.as_str()).collect();
    assert_eq!(icaos, vec!["LFPG", "LFPB", "LFPO", "LFPV", "LFPT"]);
}

#[test]
fn alps_selects_the_sparser_mountain_ring() {
    let stations = stations();
    let sel = select(&stations, 45.9237, 6.8694).expect("stations list is non-empty");
    let icaos: Vec<&str> = sel.stations.iter().map(|n| n.station.icao.as_str()).collect();
    assert_eq!(icaos, vec!["LIMW", "LSGS", "LFLP", "LSGG", "LFLB"]);
    assert!(
        !icaos.contains(&"LSMP"),
        "LSMP is one ring step farther out and should be cut by k=5, got {icaos:?}"
    );
}

#[test]
fn new_york_selects_the_five_closest_stations() {
    let stations = stations();
    let sel = select(&stations, 40.7128, -74.0060).expect("stations list is non-empty");
    let icaos: Vec<&str> = sel.stations.iter().map(|n| n.station.icao.as_str()).collect();
    assert_eq!(icaos, vec!["JRB", "NYC", "LGA", "EWR", "TEB"]);
    assert!(
        !icaos.contains(&"JFK"),
        "JFK is the 6th closest station in this cluster and should be cut by k=5, got {icaos:?}"
    );
}

#[test]
fn toronto_selects_the_five_closest_stations() {
    let stations = stations();
    let sel = select(&stations, 43.6532, -79.3832).expect("stations list is non-empty");
    let icaos: Vec<&str> = sel.stations.iter().map(|n| n.station.icao.as_str()).collect();
    assert_eq!(icaos, vec!["CYTZ", "CXTO", "CYYZ", "CWWZ", "CYOO"]);
    assert!(
        !icaos.contains(&"CWWB"),
        "CWWB is the 6th closest station in this cluster and should be cut by k=5, got {icaos:?}"
    );
}

// ---- estimate: no station within radius -> no estimate, no fallback ----

#[test]
fn estimate_reports_no_station_within_radius_with_no_fallback() {
    let stations = stations();
    let sel = select(&stations, 24.20, 2.03).unwrap();
    let est = estimate(&sel, &HashMap::new(), None, NOW_MILLIS);
    match est.status {
        EstimateStatus::NoStationWithinRadius { nearest_id, nearest_km } => {
            assert_eq!(nearest_id, "DATM");
            assert!((nearest_km - sel.nearest.distance).abs() < 1e-9);
        }
        other => panic!("expected NoStationWithinRadius, got {other:?}"),
    }
    assert_eq!(est.stations_used, 0);
    assert!(est.temperature_c.is_none());
    assert!(est.stations.is_empty());
}

// ---- estimate: stale observation excluded from the averages ----

#[test]
fn stale_observation_is_marked_stale_and_excluded_from_the_average() {
    // A fixed set of five active, reporting stations, independent of the
    // fixture's added silent stations (see lille_selection_of).
    let sel = lille_selection_of(&["LFQQ", "EBOS", "LFAQ", "LFAC", "EHFS"]);
    assert_eq!(sel.stations.len(), 5);

    // Four fresh, plausible observations, plus one wildly different value at
    // EHFS that is 150 minutes old (past the 90-minute cutoff).
    let mut fresh_map: HashMap<String, Observation> = HashMap::new();
    fresh_map.insert("LFQQ".into(), fresh_obs(14.2, 11.4, 5.6, 230.0, 1018.3, 10.0));
    fresh_map.insert("EBOS".into(), fresh_obs(14.6, 11.8, 4.9, 220.0, 1018.6, 15.0));
    fresh_map.insert("LFAQ".into(), fresh_obs(14.8, 11.9, 5.2, 225.0, 1018.5, 20.0));
    fresh_map.insert("LFAC".into(), fresh_obs(13.9, 11.1, 4.5, 215.0, 1018.1, 25.0));

    let mut with_stale = fresh_map.clone();
    with_stale.insert("EHFS".into(), fresh_obs(99.0, 99.0, 99.0, 99.0, 1099.0, 150.0));

    let est_with_stale = estimate(&sel, &with_stale, Some(35.0), NOW_MILLIS);
    let est_without = estimate(&sel, &fresh_map, Some(35.0), NOW_MILLIS);

    let ehfs = est_with_stale
        .stations
        .iter()
        .find(|s| s.icao == "EHFS")
        .expect("EHFS is one of the selected stations");
    assert!(!ehfs.fresh, "a 150-minute-old observation must not be fresh");
    assert!(ehfs.has_observation);
    assert_eq!(est_with_stale.stale_count, 1);
    assert_eq!(est_with_stale.fresh_count, 4);
    assert_eq!(est_with_stale.missing_count, 0);

    // The corrected average must be identical whether the stale station's
    // wild value is present or the station has no observation at all: it is
    // excluded from the average either way.
    assert!(matches!(est_with_stale.status, EstimateStatus::Ok));
    let t1 = est_with_stale.temperature_c.unwrap();
    let t2 = est_without.temperature_c.unwrap();
    assert!(
        (t1 - t2).abs() < 1e-9,
        "stale observation leaked into the temperature average: {t1} vs {t2}"
    );
}

#[test]
fn no_fresh_observation_leaves_every_value_field_none() {
    let sel = lille_selection_of(&["LFQQ", "EBOS", "LFAQ", "LFAC", "EHFS"]);
    assert_eq!(sel.stations.len(), 5);

    // All five selected stations have an observation, but every one is past
    // the 90-minute freshness cutoff.
    let mut all_stale: HashMap<String, Observation> = HashMap::new();
    for (icao, values) in [
        ("LFQQ", (14.2, 11.4, 5.6, 230.0, 1018.3)),
        ("EBOS", (14.6, 11.8, 4.9, 220.0, 1018.6)),
        ("LFAQ", (14.8, 11.9, 5.2, 225.0, 1018.5)),
        ("LFAC", (13.9, 11.1, 4.5, 215.0, 1018.1)),
        ("EHFS", (13.5, 10.8, 6.1, 240.0, 1017.9)),
    ] {
        let (temp_c, dewpoint_c, wind_ms, wind_dir_deg, pressure_hpa) = values;
        all_stale.insert(
            icao.into(),
            fresh_obs(temp_c, dewpoint_c, wind_ms, wind_dir_deg, pressure_hpa, 150.0),
        );
    }

    let est = estimate(&sel, &all_stale, Some(35.0), NOW_MILLIS);

    assert!(matches!(est.status, EstimateStatus::NoFreshObservation));
    assert_eq!(est.fresh_count, 0);
    assert_eq!(est.stale_count, 5);
    assert_eq!(est.missing_count, 0);
    assert!(est.temperature_c.is_none());
    assert!(est.dewpoint_c.is_none());
    assert!(est.wind_speed_ms.is_none());
    assert!(est.wind_dir_deg.is_none());
    assert!(est.pressure_qnh_hpa.is_none());
    assert!(est.pressure_station_hpa.is_none());

    // The per-station rows and counts are still reported.
    assert_eq!(est.stations.len(), 5);
    assert_eq!(est.stations_used, 5);
    for s in &est.stations {
        assert!(!s.fresh);
        assert!(s.has_observation);
    }
}

// ---- estimate: QNH vs station pressure ----

#[test]
fn qnh_stays_near_station_qnh_while_station_pressure_is_lower() {
    let stations = stations();
    let sel = select(&stations, 50.6292, 3.0573).unwrap();

    let mut obs: HashMap<String, Observation> = HashMap::new();
    let qnhs = [1018.3, 1018.6, 1018.5, 1018.1, 1018.0];
    for (n, &qnh) in sel.stations.iter().zip(qnhs.iter()) {
        obs.insert(
            n.station.icao.clone(),
            fresh_obs(14.0, 11.0, 5.0, 220.0, qnh, 10.0),
        );
    }

    let target_elev_m = 400.0;
    let est = estimate(&sel, &obs, Some(target_elev_m), NOW_MILLIS);

    let qnh = est.pressure_qnh_hpa.expect("qnh should be computed");
    let station_p = est.pressure_station_hpa.expect("station pressure should be computed at a known target elevation");

    let mean_input_qnh = qnhs.iter().sum::<f64>() / qnhs.len() as f64;
    assert!(
        (qnh - mean_input_qnh).abs() < 1.0,
        "QNH {qnh} should stay near the stations' own QNH ({mean_input_qnh})"
    );
    assert!(
        station_p < qnh - 30.0,
        "station pressure at {target_elev_m} m ({station_p}) should be well below QNH ({qnh})"
    );
}

// ---- NWS parsing ----

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("reading fixture {name}: {e}"))
}

#[test]
fn parses_nws_kjfk_full_observation() {
    let text = read_fixture("nws_kjfk.json");
    let obs = parse_nws_latest(&text).expect("KJFK fixture has usable fields");
    assert_eq!(obs.source, "NWS");
    assert!((obs.temp_c.unwrap() - 17.0).abs() < 1e-9);
    assert!((obs.dewpoint_c.unwrap() - 17.0).abs() < 1e-9);
    assert!((obs.wind_ms.unwrap() - 18.504 / 3.6).abs() < 1e-9);
    assert!((obs.wind_dir_deg.unwrap() - 40.0).abs() < 1e-9);
    assert!((obs.pressure_hpa.unwrap() - 1005.7574).abs() < 1e-6);
    let expected_millis = weather::time::parse_utc_millis("2026-09-27T21:15:00Z");
    assert_eq!(obs.obs_time_millis, expected_millis);
}

#[test]
fn parses_nws_panc_with_null_fields_left_out() {
    let text = read_fixture("nws_panc.json");
    let obs = parse_nws_latest(&text).expect("PANC fixture has usable fields");
    assert!((obs.temp_c.unwrap() - 10.0).abs() < 1e-9);
    assert!((obs.dewpoint_c.unwrap() - 6.0).abs() < 1e-9);
    // windSpeed and windDirection are `value: null` in this fixture.
    assert!(obs.wind_ms.is_none());
    assert!(obs.wind_dir_deg.is_none());
    assert!((obs.pressure_hpa.unwrap() - 990.8573).abs() < 1e-6);
}

#[test]
fn parses_wind_speed_reported_in_meters_per_second() {
    let json = serde_json::json!({
        "properties": {
            "temperature": {"unitCode": "wmoUnit:degC", "value": 15.0},
            "windSpeed": {"unitCode": "wmoUnit:m_s-1", "value": 5.0},
        }
    })
    .to_string();
    let obs = parse_nws_latest(&json).unwrap();
    assert!((obs.wind_ms.unwrap() - 5.0).abs() < 1e-9);
}

#[test]
fn nws_station_id_matches_sources_js() {
    assert_eq!(nws_station_id("JFK"), "KJFK");
    assert_eq!(nws_station_id("00U"), "K00U");
    assert_eq!(nws_station_id("PANC"), "PANC");
    assert_eq!(nws_station_id("PHNL"), "PHNL");
}
