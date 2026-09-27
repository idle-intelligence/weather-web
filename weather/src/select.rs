//! select.rs: station selection for a point estimate.
//!
//! Port of the trucs.ai page's `nearest(lat, lon, K + 15)` /
//! `withinRadius` / `noneWithinRadius` / `final` logic (index.html), with
//! one change from the original page: when no station is within
//! MAX_RADIUS_KM there is no estimate. The page used to fall back to the K
//! nearest stations overall in that case; that fallback is gone. The
//! nearest station overall is still returned, but only so the caller can
//! report how far away it is.

use crate::stations::{nearest, nearest_within, Neighbor, Station, DEFAULT_K, MAX_RADIUS_KM};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    /// Up to DEFAULT_K nearest stations within MAX_RADIUS_KM, closest
    /// first. Empty when none are within range: there is no fallback to
    /// farther stations.
    pub stations: Vec<Neighbor>,
    /// The single nearest station overall, regardless of range. Used for
    /// the "no station within range" message when `stations` is empty.
    pub nearest: Neighbor,
}

/// Selects the station(s) an estimate at (lat, lon) should use.
/// Returns `None` if `stations` is empty.
pub fn select(stations: &[Station], lat: f64, lon: f64) -> Option<Selection> {
    let nearest_overall = nearest(stations, lat, lon, 1).into_iter().next()?;
    let within = nearest_within(stations, lat, lon, DEFAULT_K, MAX_RADIUS_KM, None);
    Some(Selection {
        stations: within,
        nearest: nearest_overall,
    })
}
