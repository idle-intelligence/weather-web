//! nws.rs — api.weather.gov `/stations/{id}/observations/latest` parsing.
//! Port of `fetchNws`/`nwsId` in sources.js.
//!
//! Each quantity there ({ value, unitCode }) is converted from whatever unit
//! NWS reports rather than assumed: temperature/dewpoint in `wmoUnit:degC`,
//! wind speed in `wmoUnit:km_h-1` or `wmoUnit:m_s-1`, wind direction in
//! `wmoUnit:degree_(angle)`, pressure in `wmoUnit:Pa`. A field with a value
//! of `null` (no reading) or an unrecognized unit is left out, same as a
//! missing field.

use crate::observation::Observation;
use crate::time::parse_utc_millis;

fn value_f64(v: &serde_json::Value) -> Option<f64> {
    v.get("value").and_then(|x| x.as_f64())
}

fn unit_code(v: &serde_json::Value) -> Option<&str> {
    v.get("unitCode").and_then(|x| x.as_str())
}

fn parse_temp_c(v: &serde_json::Value) -> Option<f64> {
    let value = value_f64(v)?;
    match unit_code(v)? {
        "wmoUnit:degC" => Some(value),
        _ => None,
    }
}

fn parse_wind_ms(v: &serde_json::Value) -> Option<f64> {
    let value = value_f64(v)?;
    match unit_code(v)? {
        "wmoUnit:km_h-1" => Some(value / 3.6),
        "wmoUnit:m_s-1" => Some(value),
        _ => None,
    }
}

fn parse_pressure_hpa(v: &serde_json::Value) -> Option<f64> {
    let value = value_f64(v)?;
    match unit_code(v)? {
        "wmoUnit:Pa" => Some(value / 100.0),
        _ => None,
    }
}

fn parse_wind_dir_deg(v: &serde_json::Value) -> Option<f64> {
    let value = value_f64(v)?;
    match unit_code(v)? {
        "wmoUnit:degree_(angle)" => Some(value),
        _ => None,
    }
}

/// Parses one api.weather.gov `/observations/latest` response body. Returns
/// `None` if it carries no usable field beyond `source` (mirrors
/// `Object.keys(obs).length > 1` in sources.js).
pub fn parse_nws_latest(json_text: &str) -> Option<Observation> {
    let body: serde_json::Value = serde_json::from_str(json_text).ok()?;
    let p = body.get("properties")?;

    let mut obs = Observation {
        source: "NWS".to_string(),
        ..Default::default()
    };
    if let Some(v) = p.get("temperature") {
        obs.temp_c = parse_temp_c(v);
    }
    if let Some(v) = p.get("dewpoint") {
        obs.dewpoint_c = parse_temp_c(v);
    }
    if let Some(v) = p.get("windSpeed") {
        obs.wind_ms = parse_wind_ms(v);
    }
    if let Some(v) = p.get("windDirection") {
        obs.wind_dir_deg = parse_wind_dir_deg(v);
    }
    if let Some(v) = p.get("barometricPressure") {
        obs.pressure_hpa = parse_pressure_hpa(v);
    }
    if let Some(ts) = p.get("timestamp").and_then(|v| v.as_str()) {
        obs.obs_time_millis = parse_utc_millis(ts);
    }

    if obs.field_count() >= 1 {
        Some(obs)
    } else {
        None
    }
}

/// NWS needs the full 4-letter ICAO id: CONUS state networks carry a bare
/// 3-character id (e.g. "JFK", "00U") and need a "K" prefix; Alaska, Hawaii,
/// Puerto Rico, Guam and the US Virgin Islands already carry their real
/// 4-letter ICAO id (e.g. "PANC", "PHNL") and are used as-is.
pub fn nws_station_id(icao: &str) -> String {
    if icao.len() == 3 {
        format!("K{icao}")
    } else {
        icao.to_string()
    }
}
