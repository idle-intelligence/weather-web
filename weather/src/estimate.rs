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

use crate::corrections::{compute_corrections, NeighborRow, MAX_AGE_MIN};
use crate::observation::Observation;
use crate::select::Selection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    if selection.stations.is_empty() {
        return empty_estimate(EstimateStatus::NoStationWithinRadius {
            nearest_id: selection.nearest.station.icao.clone(),
            nearest_km: selection.nearest.distance,
        });
    }

    let rows: Vec<NeighborRow> = selection
        .stations
        .iter()
        .map(|n| NeighborRow {
            icao: n.station.icao.clone(),
            distance: n.distance,
            elev_m: Some(n.station.elev_m),
            obs: observations.get(&n.station.icao).cloned(),
        })
        .collect();

    let corr = compute_corrections(&rows, target_elev_m, now_millis);

    let stations: Vec<StationEstimate> = selection
        .stations
        .iter()
        .map(|n| {
            let obs = observations.get(&n.station.icao);
            let age_minutes = obs
                .and_then(|o| o.obs_time_millis)
                .map(|t| (now_millis - t) as f64 / 60000.0);
            let fresh = age_minutes.map(|a| a <= MAX_AGE_MIN).unwrap_or(false);
            StationEstimate {
                icao: n.station.icao.clone(),
                name: n.station.name.clone(),
                distance_km: n.distance,
                age_minutes,
                fresh,
                has_observation: obs.is_some(),
            }
        })
        .collect();

    let fresh_count = stations.iter().filter(|s| s.fresh).count();
    let missing_count = stations.iter().filter(|s| !s.has_observation).count();
    let stale_count = stations.len() - fresh_count - missing_count;

    let status = if fresh_count == 0 {
        EstimateStatus::NoFreshObservation
    } else {
        EstimateStatus::Ok
    };

    Estimate {
        status,
        temperature_c: corr.temperature.corrected.or(corr.temperature.plain),
        dewpoint_c: corr.dewpoint.corrected.or(corr.dewpoint.plain),
        wind_speed_ms: corr.wind.corrected_speed.or(corr.wind.plain_scalar_speed),
        wind_dir_deg: corr.wind.corrected_dir,
        pressure_qnh_hpa: corr.pressure.corrected_qnh.or(corr.pressure.plain),
        pressure_station_hpa: corr.pressure.corrected_station,
        stations,
        stations_used: rows.len(),
        fresh_count,
        stale_count,
        missing_count,
    }
}
