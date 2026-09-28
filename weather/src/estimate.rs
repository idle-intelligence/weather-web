//! estimate.rs: turns a station Selection plus per-station observations
//! into the values the trucs.ai page's result panel shows.
//!
//! Port of the tail of the page's `compute()` (index.html): the NWS-then-IEM
//! merge is done by the caller (one `observations` map, already merged), and
//! this module does the rest: per-station age/freshness accounting, the
//! fresh/stale/missing counts previously computed inline in page JS, and the
//! displayed values, preferring `compute_corrections`'s elevation/QNH/vector
//! corrected values and falling back to its plain distance-weighted average
//! when a corrected value is unavailable (same fallback as the page's
//! `correctedValueFor`).

use crate::corrections::{compute_corrections_with, NeighborRow, MAX_AGE_MIN};
use crate::observation::Observation;
use crate::select::Selection;
use crate::stations::DEFAULT_K;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Parameters for `estimate_with`: how many of the selected stations to
/// average over, and how old an observation may be to still count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EstimateParams {
    pub estimate_k: usize,
    pub max_age_min: f64,
}

impl Default for EstimateParams {
    /// `estimate_k: 5`, `max_age_min: 90`: the trucs.ai page's own numbers,
    /// so `estimate`'s hard-coded behaviour and `estimate_with`'s default
    /// agree.
    fn default() -> Self {
        EstimateParams {
            estimate_k: DEFAULT_K,
            max_age_min: MAX_AGE_MIN,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EstimateStatus {
    Ok,
    #[serde(rename_all = "camelCase")]
    NoStationWithinRadius { nearest_id: String, nearest_km: f64 },
    NoFreshObservation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationEstimate {
    pub icao: String,
    pub name: String,
    pub distance_km: f64,
    pub age_minutes: Option<f64>,
    pub fresh: bool,
    pub has_observation: bool,
    /// The station's dataset "reported in the last 7 days" flag (always
    /// true for a plain 6-column station list).
    pub active: bool,
    /// Whether this station's observation fed the average: `estimate`
    /// (unparameterized) marks every selected station used; `estimate_with`
    /// marks only the nearest `estimate_k` that are `active` and fresh.
    pub used: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    pub status: EstimateStatus,
    pub temperature_c: Option<f64>,
    pub dewpoint_c: Option<f64>,
    pub wind_speed_ms: Option<f64>,
    pub wind_dir_deg: Option<f64>,
    /// Sea-level altimeter setting (QNH), comparable across stations at
    /// different elevations.
    pub pressure_qnh_hpa: Option<f64>,
    /// QNH reduced to the target point's own elevation, if it is known.
    pub pressure_station_hpa: Option<f64>,
    pub stations: Vec<StationEstimate>,
    pub stations_used: usize,
    pub fresh_count: usize,
    pub stale_count: usize,
    pub missing_count: usize,
}

fn empty_estimate(status: EstimateStatus) -> Estimate {
    Estimate {
        status,
        temperature_c: None,
        dewpoint_c: None,
        wind_speed_ms: None,
        wind_dir_deg: None,
        pressure_qnh_hpa: None,
        pressure_station_hpa: None,
        stations: vec![],
        stations_used: 0,
        fresh_count: 0,
        stale_count: 0,
        missing_count: 0,
    }
}

/// `observations`: one map from ICAO to observation, already merged
/// NWS-first (the caller looks up NWS, then IEM, per selected station,
/// before calling this: mirrors the page's `nwsMap.get(icao) ?? iemMap[icao]`).
/// `target_elev_m`: the target point's own elevation, or `None` if
/// unavailable (Open-Meteo fetch failed).
/// `now_millis`: epoch milliseconds, used for the MAX_AGE_MIN freshness cutoff.
pub fn estimate(
    selection: &Selection,
    observations: &HashMap<String, Observation>,
    target_elev_m: Option<f64>,
    now_millis: i64,
) -> Estimate {
    // estimate_k: None means every selected station is used, regardless of
    // its active flag: the trucs.ai page's own behaviour, unchanged.
    estimate_inner(selection, observations, target_elev_m, now_millis, MAX_AGE_MIN, None)
}

/// Like `estimate`, but with a caller-chosen freshness cutoff and a cap on
/// how many of the selected stations feed the average: only stations
/// flagged `active` with an observation no older than `params.max_age_min`
/// are used, nearest first, up to `params.estimate_k` of them. Every
/// selected station -- active or not, fresh or not -- is still reported in
/// `stations`/`fresh_count`/`stale_count`/`missing_count`; each row's `used`
/// flag says which ones fed the average.
pub fn estimate_with(
    selection: &Selection,
    observations: &HashMap<String, Observation>,
    target_elev_m: Option<f64>,
    now_millis: i64,
    params: &EstimateParams,
) -> Estimate {
    estimate_inner(
        selection,
        observations,
        target_elev_m,
        now_millis,
        params.max_age_min,
        Some(params.estimate_k),
    )
}

fn estimate_inner(
    selection: &Selection,
    observations: &HashMap<String, Observation>,
    target_elev_m: Option<f64>,
    now_millis: i64,
    max_age_min: f64,
    estimate_k: Option<usize>,
) -> Estimate {
    if selection.stations.is_empty() {
        return empty_estimate(EstimateStatus::NoStationWithinRadius {
            nearest_id: selection.nearest.station.icao.clone(),
            nearest_km: selection.nearest.distance,
        });
    }

    let is_fresh = |icao: &str| -> bool {
        observations
            .get(icao)
            .and_then(|o| o.obs_time_millis)
            .map(|t| (now_millis - t) as f64 / 60000.0 <= max_age_min)
            .unwrap_or(false)
    };

    // Stations that feed the average, nearest first. `estimate` (estimate_k:
    // None) uses every selected station, as before. `estimate_with`
    // restricts this to stations flagged active with a fresh observation,
    // capped at estimate_k.
    let used: Vec<&crate::stations::Neighbor> = match estimate_k {
        Some(k) => selection
            .stations
            .iter()
            .filter(|n| n.station.active && is_fresh(&n.station.icao))
            .take(k)
            .collect(),
        None => selection.stations.iter().collect(),
    };
    let used_icaos: std::collections::HashSet<&str> =
        used.iter().map(|n| n.station.icao.as_str()).collect();

    let rows: Vec<NeighborRow> = used
        .iter()
        .map(|n| NeighborRow {
            icao: n.station.icao.clone(),
            distance: n.distance,
            elev_m: Some(n.station.elev_m),
            obs: observations.get(&n.station.icao).cloned(),
        })
        .collect();

    let corr = compute_corrections_with(&rows, target_elev_m, now_millis, max_age_min);

    let stations: Vec<StationEstimate> = selection
        .stations
        .iter()
        .map(|n| {
            let obs = observations.get(&n.station.icao);
            let age_minutes = obs
                .and_then(|o| o.obs_time_millis)
                .map(|t| (now_millis - t) as f64 / 60000.0);
            let fresh = age_minutes.map(|a| a <= max_age_min).unwrap_or(false);
            StationEstimate {
                icao: n.station.icao.clone(),
                name: n.station.name.clone(),
                distance_km: n.distance,
                age_minutes,
                fresh,
                has_observation: obs.is_some(),
                active: n.station.active,
                used: used_icaos.contains(n.station.icao.as_str()),
            }
        })
        .collect();

    let fresh_count = stations.iter().filter(|s| s.fresh).count();
    let missing_count = stations.iter().filter(|s| !s.has_observation).count();
    let stale_count = stations.len() - fresh_count - missing_count;

    // Whether the rows actually fed to compute_corrections include a fresh
    // observation: for `estimate` this is every selected station (matching
    // its old fresh_count check); for `estimate_with` `rows` is already
    // filtered to fresh ones, so this is just "rows is non-empty".
    let fresh_used_count = rows.iter().filter(|r| is_fresh(&r.icao)).count();
    let status = if fresh_used_count == 0 {
        EstimateStatus::NoFreshObservation
    } else {
        EstimateStatus::Ok
    };

    // With no fresh observation, every value field is None: a plain average
    // computed only from stale observations must never be shown as an
    // estimate. The per-station rows and counts are kept either way.
    let (temperature_c, dewpoint_c, wind_speed_ms, wind_dir_deg, pressure_qnh_hpa, pressure_station_hpa) =
        if matches!(status, EstimateStatus::Ok) {
            (
                corr.temperature.corrected.or(corr.temperature.plain),
                corr.dewpoint.corrected.or(corr.dewpoint.plain),
                corr.wind.corrected_speed.or(corr.wind.plain_scalar_speed),
                corr.wind.corrected_dir,
                corr.pressure.corrected_qnh.or(corr.pressure.plain),
                corr.pressure.corrected_station,
            )
        } else {
            (None, None, None, None, None, None)
        };

    Estimate {
        status,
        temperature_c,
        dewpoint_c,
        wind_speed_ms,
        wind_dir_deg,
        pressure_qnh_hpa,
        pressure_station_hpa,
        stations,
        stations_used: rows.len(),
        fresh_count,
        stale_count,
        missing_count,
    }
}
