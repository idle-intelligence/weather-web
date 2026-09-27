//! observation.rs — normalized station observations and the IEM currents.json
//! parser. Port of the `fetchIem` row-parsing logic in sources.js; the
//! network fetch itself lives in the CLI (feature `native`).

use crate::time::parse_utc_millis;
use std::collections::HashMap;

fn f_to_c(f: f64) -> f64 {
    (f - 32.0) * 5.0 / 9.0
}

fn kt_to_ms(kt: f64) -> f64 {
    kt * 0.514444
}

fn inhg_to_hpa(inhg: f64) -> f64 {
    inhg * 33.8639
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temp_c: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dewpoint_c: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wind_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wind_dir_deg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pressure_hpa: Option<f64>,
    /// Epoch milliseconds (UTC), the Rust equivalent of the JS `obsTime: Date`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obs_time_millis: Option<i64>,
    pub source: String,
}

impl Observation {
    fn field_count(&self) -> usize {
        [
            self.temp_c.is_some(),
            self.dewpoint_c.is_some(),
            self.wind_ms.is_some(),
            self.wind_dir_deg.is_some(),
            self.pressure_hpa.is_some(),
            self.obs_time_millis.is_some(),
        ]
        .into_iter()
        .filter(|x| *x)
        .count()
    }
}

/// Parses one IEM currents.json `data[]` row. `row` is that row's JSON object.
/// Returns None if the row carries no usable field beyond `source` (mirrors
/// `Object.keys(obs).length > 1` in sources.js, which counts `source` as 1).
fn parse_iem_row(row: &serde_json::Value) -> Option<Observation> {
    let mut obs = Observation {
        source: "IEM".to_string(),
        ..Default::default()
    };
    if let Some(tmpf) = row.get("tmpf").and_then(|v| v.as_f64()) {
        obs.temp_c = Some(f_to_c(tmpf));
    }
    if let Some(dwpf) = row.get("dwpf").and_then(|v| v.as_f64()) {
        obs.dewpoint_c = Some(f_to_c(dwpf));
    }
    if let Some(sknt) = row.get("sknt").and_then(|v| v.as_f64()) {
        obs.wind_ms = Some(kt_to_ms(sknt));
    }
    if let Some(drct) = row.get("drct").and_then(|v| v.as_f64()) {
        obs.wind_dir_deg = Some(drct);
    }
    if let Some(alti) = row.get("alti").and_then(|v| v.as_f64()) {
        obs.pressure_hpa = Some(inhg_to_hpa(alti));
    } else if let Some(mslp) = row.get("mslp").and_then(|v| v.as_f64()) {
        obs.pressure_hpa = Some(mslp);
    }
    if let Some(utc_valid) = row.get("utc_valid").and_then(|v| v.as_str()) {
        obs.obs_time_millis = parse_utc_millis(utc_valid);
    }
    if obs.field_count() >= 1 {
        Some(obs)
    } else {
        None
    }
}

/// Parses a full IEM currents.json body (`{"data": [...]}`) into a map from
/// ICAO station id to observation.
pub fn parse_iem_currents(body: &serde_json::Value) -> HashMap<String, Observation> {
    let mut out = HashMap::new();
    let Some(rows) = body.get("data").and_then(|v| v.as_array()) else {
        return out;
    };
    for row in rows {
        let Some(station) = row.get("station").and_then(|v| v.as_str()) else {
            continue;
        };
        if let Some(obs) = parse_iem_row(row) {
            out.insert(station.to_string(), obs);
        }
    }
    out
}
