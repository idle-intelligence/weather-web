//! Station list loading and nearest-k search.
//!
//! Port of knn.js: a BallTree(haversine) search restated as a linear scan,
//! since the station list is small enough for that to be plenty fast.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

const EARTH_RADIUS_KM: f64 = 6371.0;

/// Number of neighbours the trucs.ai page uses for its kNN estimate.
pub const DEFAULT_K: usize = 5;

/// Default number of nearest stations a caller lists (e.g. the weather-web
/// repo demo), separate from DEFAULT_K, which the trucs.ai page's estimate
/// keeps using.
pub const DEFAULT_LIST_K: usize = 12;

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
    /// Whether this station counts toward an estimate. True for every row
    /// in a plain 6-column list (stations.json); for an 8-column row
    /// (stations_all.json: [..., reported, lastReportUtc]) this is that
    /// row's "reported in the last 7 days" flag.
    pub active: bool,
}

/// Parses one stations.json/stations_all.json row: a JSON array, either
/// [icao, lat, lon, elev_m, name, country] (active is assumed true) or
/// [icao, lat, lon, elev_m, name, country, reported, lastReportUtc]
/// (active = reported; lastReportUtc is not kept -- selection and
/// estimation only need the flag). Any list in this row shape works,
/// including a caller's own list of any size.
fn station_from_row(value: &Value) -> Result<Station> {
    let arr = value
        .as_array()
        .context("station row is not a JSON array")?;
    if arr.len() < 6 {
        anyhow::bail!("station row has {} fields, need at least 6", arr.len());
    }
    let field_str = |i: usize, name: &str| -> Result<String> {
        arr[i]
            .as_str()
            .map(str::to_string)
            .with_context(|| format!("station row field {i} ({name}) is not a string"))
    };
    let field_f64 = |i: usize, name: &str| -> Result<f64> {
        arr[i]
            .as_f64()
            .with_context(|| format!("station row field {i} ({name}) is not a number"))
    };
    let active = arr.get(6).and_then(Value::as_bool).unwrap_or(true);
    Ok(Station {
        icao: field_str(0, "icao")?,
        lat: field_f64(1, "lat")?,
        lon: field_f64(2, "lon")?,
        elev_m: field_f64(3, "elevM")?,
        name: field_str(4, "name")?,
        country: field_str(5, "country")?,
        active,
    })
}

/// Parses a stations.json/stations_all.json body: a JSON array of station
/// rows (see `station_from_row`).
pub fn parse_stations(text: &str) -> Result<Vec<Station>> {
    let rows: Vec<Value> = serde_json::from_str(text).context("parsing stations JSON")?;
    rows.iter()
        .enumerate()
        .map(|(i, row)| station_from_row(row).with_context(|| format!("station row {i}")))
        .collect()
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
