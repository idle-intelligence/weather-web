//! Parity tests against fixtures that were generated once by running the
//! original trucs.ai knn-weather JS modules (idw.js, physics.js,
//! corrections.js, sources.js) in Node on the same hand-made inputs used
//! below, before those modules were replaced by this crate. The generator
//! script no longer exists (it depended on JS modules outside this repo);
//! the fixtures it produced are frozen and live in tests/fixtures/parity.json.
//! This crate is now the source of truth for the computation.
//!
//! Tolerance: 1e-9 relative (absolute for values near zero). Rust and V8
//! both use IEEE-754 doubles and the arithmetic here mirrors the JS
//! reduce/map order exactly, so any residual difference should only come
//! from last-ULP differences in transcendental functions (exp, ln, atan2)
//! between V8's and Rust's libm.

use serde_json::{json, Value};
use weather::corrections::{compute_corrections, NeighborRow};
use weather::idw::{idw, idw_circular_deg, Point};
use weather::observation::{parse_iem_currents, Observation};
use weather::physics::*;
use weather::time::parse_utc_millis;

const TOL: f64 = 1e-9;

fn assert_close(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => {
            let a = a.as_f64().unwrap();
            let b = b.as_f64().unwrap();
            let allowed = TOL.max(TOL * b.abs());
            assert!(
                (a - b).abs() <= allowed,
                "{path}: {a} != {b} (diff {}, allowed {})",
                (a - b).abs(),
                allowed
            );
        }
        (Value::String(a), Value::String(b)) => {
            assert_eq!(a, b, "{path}");
        }
        (Value::Bool(a), Value::Bool(b)) => {
            assert_eq!(a, b, "{path}");
        }
        (Value::Null, Value::Null) => {}
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}: array length");
            for (i, (av, bv)) in a.iter().zip(b.iter()).enumerate() {
                assert_close(av, bv, &format!("{path}[{i}]"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let av = a.get(k).unwrap_or(&Value::Null);
                let bv = b.get(k).unwrap_or(&Value::Null);
                assert_close(av, bv, &format!("{path}.{k}"));
            }
        }
        _ => panic!("{path}: type mismatch, actual={actual:?} expected={expected:?}"),
    }
}

fn fixtures() -> Value {
    let text = include_str!("fixtures/parity.json");
    serde_json::from_str(text).unwrap()
}

#[test]
fn idw_matches_js() {
    let f = fixtures();

    let paris = idw(&[
        Point { value: 14.2, distance: 3.1 },
        Point { value: 13.8, distance: 7.4 },
        Point { value: 14.5, distance: 9.0 },
        Point { value: 13.1, distance: 12.6 },
        Point { value: 12.0, distance: 41.3 },
    ])
    .unwrap();
    assert_close(&json!(paris), &f["idw"]["parisFive"], "idw.parisFive");

    let zero = idw(&[
        Point { value: 20.0, distance: 0.0 },
        Point { value: 10.0, distance: 5.0 },
        Point { value: 5.0, distance: 12.0 },
    ])
    .unwrap();
    assert_close(&json!(zero), &f["idw"]["zeroDistance"], "idw.zeroDistance");

    assert!(idw(&[]).is_none());
    assert_eq!(f["idw"]["empty"], Value::Null);
}

#[test]
fn idw_circular_deg_matches_js() {
    let f = fixtures();

    let paris = idw_circular_deg(&[
        Point { value: 250.0, distance: 3.1 },
        Point { value: 260.0, distance: 7.4 },
        Point { value: 10.0, distance: 9.0 },
        Point { value: 240.0, distance: 12.6 },
        Point { value: 255.0, distance: 41.3 },
    ])
    .unwrap();
    assert_close(&json!(paris), &f["idwCircularDeg"]["parisFive"], "idwCircularDeg.parisFive");

    let zero = idw_circular_deg(&[
        Point { value: 90.0, distance: 0.0 },
        Point { value: 270.0, distance: 5.0 },
    ])
    .unwrap();
    assert_close(&json!(zero), &f["idwCircularDeg"]["zeroDistance"], "idwCircularDeg.zeroDistance");
}

#[test]
fn physics_matches_js() {
    let f = fixtures();

    let slope = least_squares_slope(&[65.0, 89.0, 119.0, 164.0, 108.0], &[14.6, 14.4, 14.2, 13.8, 14.0]);
    assert_close(&json!(slope), &f["physics"]["leastSquaresSlope"], "leastSquaresSlope");

    let normal = fit_lapse_rate_k_per_km(&[(235.0, 18.4), (384.0, 17.1), (821.0, 13.9), (2004.0, 4.2)]);
    assert_close(&json!(normal), &f["physics"]["fitLapseRateNormal"], "fitLapseRateNormal");

    let too_few = fit_lapse_rate_k_per_km(&[(235.0, 18.4), (384.0, 17.1)]);
    assert_close(&json!(too_few), &f["physics"]["fitLapseRateTooFew"], "fitLapseRateTooFew");

    let implausible = fit_lapse_rate_k_per_km(&[(100.0, 20.0), (110.0, 19.8), (120.0, 10.0)]);
    assert_close(&json!(implausible), &f["physics"]["fitLapseRateImplausible"], "fitLapseRateImplausible");

    let reduced = reduce_temp_to_elevation(14.2, 119.0, 2004.0, -6.5);
    assert_close(&json!(reduced), &f["physics"]["reduceTempToElevation"], "reduceTempToElevation");

    let e = vapor_pressure_hpa(11.4);
    assert_close(&json!(e), &f["physics"]["vaporPressureHpa"], "vaporPressureHpa");

    let td = dewpoint_from_vapor_pressure_hpa(e);
    assert_close(&json!(td), &f["physics"]["dewpointFromVaporPressureHpa"], "dewpointFromVaporPressureHpa");

    let station_p = station_pressure_hpa(1018.3, 2004.0);
    assert_close(&json!(station_p), &f["physics"]["stationPressureHpa"], "stationPressureHpa");

    let wc = wind_components(5.6, 230.0);
    assert_close(&json!(wc), &f["physics"]["windComponents"], "windComponents");

    let round_trip = wind_from_components(wc.u, wc.v);
    assert_close(&json!(round_trip), &f["physics"]["windFromComponentsRoundTrip"], "windFromComponentsRoundTrip");

    let sd = std_dev(&[14.2, 13.8, 14.5, 13.1, 12.0]);
    assert_close(&json!(sd), &f["physics"]["stdDev"], "stdDev");
    assert_eq!(std_dev(&[]), None);

    let avg = idw_average(&[3.1, 7.4, 9.0], &[14.2, 13.8, 14.5]);
    assert_close(&json!(avg), &f["physics"]["idwAverage"], "idwAverage");
}

fn now_millis() -> i64 {
    parse_utc_millis("2026-09-27T12:00:00Z").unwrap()
}

#[allow(clippy::too_many_arguments)]
fn row(icao: &str, distance: f64, elev_m: f64, temp_c: f64, dewpoint_c: f64, wind_ms: f64, wind_dir_deg: f64, pressure_hpa: f64, minutes_ago: i64) -> NeighborRow {
    NeighborRow {
        icao: icao.to_string(),
        distance,
        elev_m: Some(elev_m),
        obs: Some(Observation {
            temp_c: Some(temp_c),
            dewpoint_c: Some(dewpoint_c),
            wind_ms: Some(wind_ms),
            wind_dir_deg: Some(wind_dir_deg),
            pressure_hpa: Some(pressure_hpa),
            obs_time_millis: Some(now_millis() - minutes_ago * 60_000),
            source: "IEM".to_string(),
        }),
    }
}

#[test]
fn corrections_paris_five_matches_js() {
    let f = fixtures();
    let rows = [
        row("LFPG", 12.9, 119.0, 14.2, 11.4, 5.6, 230.0, 1018.3, 10),
        row("LFPO", 8.1, 89.0, 14.6, 11.8, 4.9, 220.0, 1018.6, 15),
        row("LFPB", 9.7, 65.0, 14.8, 11.9, 5.2, 225.0, 1018.5, 20),
        row("LFPN", 15.4, 164.0, 13.9, 11.1, 4.5, 215.0, 1018.1, 25),
        row("LFOB", 68.2, 108.0, 13.5, 10.8, 6.1, 240.0, 1017.9, 30),
    ];
    let result = compute_corrections(&rows, Some(35.0), now_millis());
    assert_close(&json!(result), &f["correctionsParisFive"], "correctionsParisFive");
}

#[test]
fn corrections_mountain_matches_js() {
    let f = fixtures();
    let rows = [
        row("LFLB", 24.5, 235.0, 18.4, 12.0, 2.1, 180.0, 1015.2, 5),
        row("LFLS", 38.1, 384.0, 17.1, 11.5, 1.8, 190.0, 1014.8, 5),
        row("LFLL", 71.3, 821.0, 13.9, 9.2, 3.4, 160.0, 1013.9, 5),
        row("LFLJ", 0.0, 2004.0, 4.2, -1.5, 6.7, 300.0, 1011.0, 5),
    ];
    let result = compute_corrections(&rows, Some(2004.0), now_millis());
    assert_close(&json!(result), &f["correctionsMountain"], "correctionsMountain");
}

#[test]
fn corrections_stale_observation_excluded() {
    let f = fixtures();
    let rows = [
        row("LFPG", 12.9, 119.0, 14.2, 11.4, 5.6, 230.0, 1018.3, 10),
        row("LFPO", 8.1, 89.0, 14.6, 11.8, 4.9, 220.0, 1018.6, 150),
    ];
    let result = compute_corrections(&rows, Some(35.0), now_millis());
    assert_close(&json!(result), &f["correctionsStale"], "correctionsStale");
}

#[test]
fn max_age_min_matches_js() {
    let f = fixtures();
    assert_eq!(weather::corrections::MAX_AGE_MIN, f["maxAgeMin"].as_f64().unwrap());
}

#[test]
fn fetch_iem_parsing_matches_js() {
    let f = fixtures();
    let body = json!({
        "data": [
            {
                "station": "LFPG",
                "tmpf": 57.6,
                "dwpf": 52.5,
                "sknt": 10.9,
                "drct": 230,
                "alti": 30.07,
                "utc_valid": "2026-09-27T11:50Z",
            },
            {
                "station": "LFPO",
                "tmpf": 58.3,
                "dwpf": 53.2,
                "sknt": 9.5,
                "drct": 220,
                "mslp": 1018.6,
                "utc_valid": "2026-09-27T11:45Z",
            },
            {
                "station": "XXXX",
                "utc_valid": "2026-09-27T11:45Z",
            },
        ]
    });
    let result = parse_iem_currents(&body);
    let mut result_json = serde_json::Map::new();
    for (k, v) in result {
        let mut obj = serde_json::to_value(&v).unwrap();
        // The JS fixture stores obsTime as an ISO string; ours is epoch millis.
        // Convert both to the same representation (epoch millis) before compare.
        if let Value::Object(ref mut m) = obj {
            m.remove("obsTimeMillis");
            m.insert("obsTimeMillis".to_string(), json!(v.obs_time_millis));
        }
        result_json.insert(k, obj);
    }
    let actual = Value::Object(result_json);

    let mut expected = serde_json::Map::new();
    for (k, v) in f["fetchIem"].as_object().unwrap() {
        let mut v = v.clone();
        if let Some(obs_time) = v.get("obsTime").and_then(|t| t.as_str()) {
            let millis = parse_utc_millis_from_rfc3339_with_seconds(obs_time);
            v.as_object_mut().unwrap().remove("obsTime");
            v.as_object_mut().unwrap().insert("obsTimeMillis".to_string(), json!(millis));
        }
        expected.insert(k.clone(), v);
    }
    assert_close(&actual, &Value::Object(expected), "fetchIem");
}

// The JS fixture's obsTime is `Date#toISOString()`, e.g. "2026-09-27T11:50:00.000Z",
// which our minimal parser (built only for IEM's own "...Z" / "...:SSZ" shapes)
// already supports directly.
fn parse_utc_millis_from_rfc3339_with_seconds(s: &str) -> i64 {
    parse_utc_millis(s).unwrap()
}
