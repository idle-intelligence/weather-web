//! weather-wasm — a thin wasm-bindgen layer over the `weather` crate.
//!
//! No network code here: fetching stations.json and the live observation
//! sources stays in the browser (JS `fetch`); this crate only takes the
//! parsed data JS already has and runs the same nearest-neighbour search,
//! inverse-distance-weighted averaging, and physics corrections the
//! `weather` crate already implements and the parity tests already check
//! against the original trucs.ai JS.

use serde::Deserialize;
use wasm_bindgen::prelude::*;
use weather::corrections::{compute_corrections, NeighborRow};
use weather::idw::{idw as idw_core, idw_circular_deg as idw_circular_deg_core, Point};
use weather::observation::parse_iem_currents;
use weather::physics::station_pressure_hpa;
use weather::stations::{haversine_km, nearest, parse_stations, Station};

fn to_js<T: serde::Serialize + ?Sized>(value: &T) -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(value).map_err(|e| JsValue::from_str(&e.to_string()))
}

fn from_js<T: for<'de> Deserialize<'de>>(value: JsValue) -> Result<T, JsValue> {
    serde_wasm_bindgen::from_value(value).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// A loaded station list, searchable by nearest-k. Construct once per
/// stations.json body (the browser fetches the file; this just parses it).
#[wasm_bindgen]
pub struct Stations(Vec<Station>);

#[wasm_bindgen]
impl Stations {
    /// `json_text`: the stations.json body, a JSON array of
    /// [icao, lat, lon, elevM, name, country] rows.
    #[wasm_bindgen(constructor)]
    pub fn new(json_text: &str) -> Result<Stations, JsValue> {
        let stations = parse_stations(json_text).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Stations(stations))
    }

    /// Number of stations loaded.
    #[wasm_bindgen(getter)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[wasm_bindgen(getter, js_name = isEmpty)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the k nearest stations to (lat, lon), sorted by ascending
    /// distance (km): an array of
    /// `{ icao, lat, lon, elevM, name, country, distance }`.
    pub fn nearest(&self, lat: f64, lon: f64, k: usize) -> Result<JsValue, JsValue> {
        let neighbors = nearest(&self.0, lat, lon, k);
        to_js(&neighbors)
    }
}

/// Great-circle distance between two points, in km.
#[wasm_bindgen(js_name = haversineKm)]
pub fn haversine_km_js(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    haversine_km(lat1, lon1, lat2, lon2)
}

/// Parses an IEM currents.json body (as text) into a map from ICAO station
/// id to observation: `{ tempC?, dewpointC?, windMs?, windDirDeg?,
/// pressureHpa?, obsTimeMillis?, source }`.
#[wasm_bindgen(js_name = parseIemCurrents)]
pub fn parse_iem_currents_js(currents_json_text: &str) -> Result<JsValue, JsValue> {
    let body: serde_json::Value = serde_json::from_str(currents_json_text)
        .map_err(|e| JsValue::from_str(&format!("parsing currents.json: {e}")))?;
    let observations = parse_iem_currents(&body);
    to_js(&observations)
}

/// `points`: array of `{ value, distance }`. Inverse-distance-weighted
/// average (weight = 1/distance; if any point is at distance 0, only that
/// point is used). Returns `{ value, terms }`, or `null` for an empty input.
#[wasm_bindgen]
pub fn idw(points: JsValue) -> Result<JsValue, JsValue> {
    let points: Vec<Point> = from_js(points)?;
    to_js(&idw_core(&points))
}

/// Like `idw`, but for a circular quantity in degrees (wind direction):
/// weighted vector mean instead of weighted arithmetic mean.
#[wasm_bindgen(js_name = idwCircularDeg)]
pub fn idw_circular_deg_js(points: JsValue) -> Result<JsValue, JsValue> {
    let points: Vec<Point> = from_js(points)?;
    to_js(&idw_circular_deg_core(&points))
}

/// `rows`: array of `{ icao, distance, elevM, obs }`, `obs` the same shape
/// `parseIemCurrents` returns (or null/absent). `targetElevM`: the target
/// point's elevation, or undefined/null if unavailable. `nowMillis`: epoch
/// milliseconds (`Date.now()`), used for the 90-minute observation-age
/// cutoff. Returns the same corrections object shape as the original
/// trucs.ai `computeCorrections`.
#[wasm_bindgen(js_name = computeCorrections)]
pub fn compute_corrections_js(
    rows: JsValue,
    target_elev_m: Option<f64>,
    now_millis: f64,
) -> Result<JsValue, JsValue> {
    let rows: Vec<NeighborRow> = from_js(rows)?;
    let result = compute_corrections(&rows, target_elev_m, now_millis as i64);
    to_js(&result)
}

/// Reduces a QNH altimeter setting (hPa) to station pressure at `elevM`.
#[wasm_bindgen(js_name = stationPressureHpa)]
pub fn station_pressure_hpa_js(qnh_hpa: f64, elev_m: f64) -> f64 {
    station_pressure_hpa(qnh_hpa, elev_m)
}

/// Observations older than this many minutes are excluded from the
/// corrected average (matches `weather::corrections::MAX_AGE_MIN`).
#[wasm_bindgen(js_name = maxAgeMin)]
pub fn max_age_min() -> f64 {
    weather::corrections::MAX_AGE_MIN
}
