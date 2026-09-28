//! weather-wasm: a thin wasm-bindgen layer over the `weather` crate.
//!
//! No network code here: fetching stations.json and the live observation
//! sources stays in the browser (JS `fetch`); this crate only takes the
//! parsed data JS already has and runs the same nearest-neighbour search,
//! inverse-distance-weighted averaging, and physics corrections the
//! `weather` crate already implements and the parity tests already check
//! against the original trucs.ai JS.

use serde::Deserialize;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use weather::corrections::{compute_corrections, compute_corrections_with, NeighborRow};
use weather::estimate::{estimate as estimate_core, estimate_with as estimate_with_core, Estimate, EstimateParams};
use weather::idw::{idw as idw_core, idw_circular_deg as idw_circular_deg_core, Point};
use weather::nws::{nws_station_id, parse_nws_latest};
use weather::observation::{parse_iem_currents, Observation};
use weather::physics::station_pressure_hpa;
use weather::select::{select as select_core, select_with as select_with_core, SelectParams, Selection};
use weather::stations::{haversine_km, nearest, parse_stations, Station};

fn to_js<T: serde::Serialize + ?Sized>(value: &T) -> Result<JsValue, JsValue> {
    // Plain JS objects (icao/lat/lon/... properties, obj[icao] lookups) are
    // easier for page authors than the Map instances serde-wasm-bindgen
    // returns by default for Rust maps and struct-turned-Values.
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    value
        .serialize(&serializer)
        .map_err(|e| JsValue::from_str(&e.to_string()))
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
        // Flatten { station: {...}, distance } to { ...station, distance },
        // the shape the pages read (icao/lat/lon/elevM/name/country/distance).
        let flat: Vec<serde_json::Value> = neighbors
            .into_iter()
            .map(|n| {
                let mut v = serde_json::to_value(&n.station).unwrap();
                v.as_object_mut()
                    .unwrap()
                    .insert("distance".to_string(), serde_json::json!(n.distance));
                v
            })
            .collect();
        to_js(&flat)
    }

    /// Selects the station(s) an estimate at (lat, lon) should use: up to 5
    /// nearest stations within 100 km (closest first), plus the nearest
    /// station overall (for the "no station in range" message). Returns
    /// `null` if no stations are loaded. There is no fallback to farther
    /// stations when none are within range.
    pub fn select(&self, lat: f64, lon: f64) -> Result<JsValue, JsValue> {
        match select_core(&self.0, lat, lon) {
            Some(selection) => to_js(&selection),
            None => Ok(JsValue::NULL),
        }
    }

    /// Like `select`, but with a caller-chosen station count and radius
    /// (km), e.g. listing 12 stations within 100 km including ones that
    /// never report (their `active` field is false).
    #[wasm_bindgen(js_name = selectWithParams)]
    pub fn select_with_params(
        &self,
        lat: f64,
        lon: f64,
        k: usize,
        max_radius_km: f64,
    ) -> Result<JsValue, JsValue> {
        let params = SelectParams { k, max_radius_km };
        match select_with_core(&self.0, lat, lon, &params) {
            Some(selection) => to_js(&selection),
            None => Ok(JsValue::NULL),
        }
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
    max_age_min: Option<f64>,
) -> Result<JsValue, JsValue> {
    let rows: Vec<NeighborRow> = from_js(rows)?;
    let result = match max_age_min {
        Some(max_age_min) => compute_corrections_with(&rows, target_elev_m, now_millis as i64, max_age_min),
        None => compute_corrections(&rows, target_elev_m, now_millis as i64),
    };
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

/// `selection`: the `{ stations, nearest }` object `Stations.select`
/// returns. `observations`: a map from ICAO id to observation (the same
/// shape `parseIemCurrents`/`parseNwsLatest` return), already merged
/// NWS-first per station by the caller. `targetElevM`: the target point's
/// elevation, or undefined/null if unavailable. `nowMillis`: epoch
/// milliseconds (`Date.now()`).
///
/// Returns `{ status, temperatureC, dewpointC, windSpeedMs, windDirDeg,
/// pressureQnhHpa, pressureStationHpa, stations, stationsUsed, freshCount,
/// staleCount, missingCount }`, where `status` is one of
/// `{ kind: "ok" }`, `{ kind: "noStationWithinRadius", nearestId, nearestKm }`
/// or `{ kind: "noFreshObservation" }`.
#[wasm_bindgen]
pub fn estimate(
    selection: JsValue,
    observations: JsValue,
    target_elev_m: Option<f64>,
    now_millis: f64,
) -> Result<JsValue, JsValue> {
    let selection: Selection = from_js(selection)?;
    let observations: HashMap<String, Observation> = from_js(observations)?;
    let result: Estimate = estimate_core(&selection, &observations, target_elev_m, now_millis as i64);
    to_js(&result)
}

/// Like `estimate`, but with a caller-chosen `estimateK` (how many of the
/// selected stations to average over) and `maxAgeMin` (how old an
/// observation may be to still count): only stations flagged active with an
/// observation no older than `maxAgeMin` are used, nearest first, up to
/// `estimateK` of them.
#[wasm_bindgen(js_name = estimateWithParams)]
pub fn estimate_with_params(
    selection: JsValue,
    observations: JsValue,
    target_elev_m: Option<f64>,
    now_millis: f64,
    estimate_k: usize,
    max_age_min: f64,
) -> Result<JsValue, JsValue> {
    let selection: Selection = from_js(selection)?;
    let observations: HashMap<String, Observation> = from_js(observations)?;
    let params = EstimateParams { estimate_k, max_age_min };
    let result: Estimate =
        estimate_with_core(&selection, &observations, target_elev_m, now_millis as i64, &params);
    to_js(&result)
}

/// Parses one api.weather.gov `/stations/{id}/observations/latest` response
/// body (as text) into an observation: `{ tempC?, dewpointC?, windMs?,
/// windDirDeg?, pressureHpa?, obsTimeMillis?, source }`, or `null` if it
/// carries no usable field. Units are converted from whatever `unitCode`
/// NWS reports (degC, km/h or m/s, Pa, …), not assumed.
#[wasm_bindgen(js_name = parseNwsLatest)]
pub fn parse_nws_latest_js(json_text: &str) -> Result<JsValue, JsValue> {
    match parse_nws_latest(json_text) {
        Some(obs) => to_js(&obs),
        None => Ok(JsValue::NULL),
    }
}

/// NWS station id for an ICAO id: 3-character ids (CONUS) get a "K" prefix,
/// 4-character ids (Alaska, Hawaii, Puerto Rico, Guam, US Virgin Islands)
/// are kept as-is.
#[wasm_bindgen(js_name = nwsStationId)]
pub fn nws_station_id_js(icao: &str) -> String {
    nws_station_id(icao)
}
