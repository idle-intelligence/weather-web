//! select.rs: station selection for a point estimate.
//!
//! Port of the trucs.ai page's `nearest(lat, lon, K + 15)` /
//! `withinRadius` / `noneWithinRadius` / `final` logic (index.html), with
//! one change from the original page: when no station is within
//! MAX_RADIUS_KM there is no estimate. The page used to fall back to the K
//! nearest stations overall in that case; that fallback is gone. The
//! nearest station overall is still returned, but only so the caller can
//! report how far away it is.

use crate::stations::{
    nearest, nearest_within, Neighbor, Station, DEFAULT_K, DEFAULT_LIST_K, MAX_RADIUS_KM,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    /// Up to `SelectParams::k` nearest stations within
    /// `SelectParams::max_radius_km`, closest first, active or not: `select`
    /// does not filter by the station's `active` flag, only `estimate`
    /// does. Empty when none are within range: there is no fallback to
    /// farther stations.
    pub stations: Vec<Neighbor>,
    /// The single nearest station overall, regardless of range. Used for
    /// the "no station within range" message when `stations` is empty.
    pub nearest: Neighbor,
}

/// Parameters for `select_with`: how many nearest stations to list, and how
/// far away one may be to count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectParams {
    pub k: usize,
    pub max_radius_km: f64,
}

impl Default for SelectParams {
    /// `k: DEFAULT_LIST_K (12)`, `max_radius_km: MAX_RADIUS_KM (100)`: the
    /// weather-web repo demo's defaults. The trucs.ai page's own default
    /// (`k: 5`) is `select`'s hard-coded behaviour, not this default, so
    /// that page keeps working unchanged.
    fn default() -> Self {
        SelectParams {
            k: DEFAULT_LIST_K,
            max_radius_km: MAX_RADIUS_KM,
        }
    }
}

/// Selects the station(s) an estimate at (lat, lon) should use: the
/// trucs.ai page's fixed defaults (5 nearest stations within 100 km). For a
/// caller-chosen station count or radius, use `select_with`.
/// Returns `None` if `stations` is empty.
pub fn select(stations: &[Station], lat: f64, lon: f64) -> Option<Selection> {
    select_with(
        stations,
        lat,
        lon,
        &SelectParams {
            k: DEFAULT_K,
            max_radius_km: MAX_RADIUS_KM,
        },
    )
}

/// Like `select`, but with a caller-chosen station count and radius, e.g.
/// listing 12 stations within 100 km (including ones that never report) for
/// the weather-web repo demo's station list.
/// Returns `None` if `stations` is empty.
pub fn select_with(
    stations: &[Station],
    lat: f64,
    lon: f64,
    params: &SelectParams,
) -> Option<Selection> {
    let nearest_overall = nearest(stations, lat, lon, 1).into_iter().next()?;
    let within = nearest_within(stations, lat, lon, params.k, params.max_radius_km, None);
    Some(Selection {
        stations: within,
        nearest: nearest_overall,
    })
}
