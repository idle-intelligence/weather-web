//! Station list loading and nearest-k search.
//!
//! Port of knn.js: a BallTree(haversine) search restated as a linear scan,
//! since the station list is small enough for that to be plenty fast.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const EARTH_RADIUS_KM: f64 = 6371.0;

/// Number of neighbours the trucs.ai page uses for its kNN estimate.
pub const DEFAULT_K: usize = 5;

/// Maximum neighbour distance (km) the trucs.ai page uses for its kNN estimate.
pub const MAX_RADIUS_KM: f64 = 100.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Station {
    pub icao: String,
    pub lat: f64,
    pub lon: f64,
    pub elev_m: f64,
    pub name: String,
    pub country: String,
}

/// The stations.json row shape: [icao, lat, lon, elev_m, name, country].
#[derive(Debug, Deserialize)]
struct StationRow(String, f64, f64, f64, String, String);

impl From<StationRow> for Station {
    fn from(row: StationRow) -> Self {
        Station {
            icao: row.0,
            lat: row.1,
            lon: row.2,
            elev_m: row.3,
            name: row.4,
            country: row.5,
        }
    }
}

/// Parses a stations.json body: a JSON array of [icao, lat, lon, elev_m, name, country] rows.
pub fn parse_stations(text: &str) -> Result<Vec<Station>> {
    let rows: Vec<StationRow> =
        serde_json::from_str(text).context("parsing stations JSON")?;
    Ok(rows.into_iter().map(Station::from).collect())
}

/// Loads a stations.json file: a JSON array of [icao, lat, lon, elev_m, name, country] rows.
pub fn load_stations(path: impl AsRef<Path>) -> Result<Vec<Station>> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading stations file {}", path.display()))?;
    parse_stations(&text).with_context(|| format!("parsing stations file {}", path.display()))
}

pub fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let p1 = lat1.to_radians();
    let p2 = lat2.to_radians();
    let dphi = (lat2 - lat1).to_radians();
    let dlambda = (lon2 - lon1).to_radians();
    let a = (dphi / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dlambda / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * a.sqrt().asin()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Neighbor {
    pub station: Station,
    pub distance: f64,
}

/// Returns the k nearest stations to (lat, lon), sorted by ascending distance (km).
pub fn nearest(stations: &[Station], lat: f64, lon: f64, k: usize) -> Vec<Neighbor> {
    let mut scored: Vec<Neighbor> = stations
        .iter()
        .map(|s| Neighbor {
            station: s.clone(),
            distance: haversine_km(lat, lon, s.lat, s.lon),
        })
        .collect();
    scored.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap());
    scored.truncate(k);
    scored
}

/// Like `nearest`, but drops candidates beyond `max_radius_km` and can
/// exclude one station by icao. Used for leave-one-out validation, where the
/// station being estimated must not appear among its own neighbours.
pub fn nearest_within(
    stations: &[Station],
    lat: f64,
    lon: f64,
    k: usize,
    max_radius_km: f64,
    exclude_icao: Option<&str>,
) -> Vec<Neighbor> {
    let mut scored: Vec<Neighbor> = stations
        .iter()
        .filter(|s| exclude_icao != Some(s.icao.as_str()))
        .map(|s| Neighbor {
            station: s.clone(),
            distance: haversine_km(lat, lon, s.lat, s.lon),
        })
        .filter(|n| n.distance <= max_radius_km)
        .collect();
    scored.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap());
    scored.truncate(k);
    scored
}
