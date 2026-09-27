//! physics.rs: small numeric primitives for the elevation, vapour-pressure,
//! QNH and vector-wind corrections in corrections.rs (see that file for the
//! method description and its source, after Nalder & Wein 1998).
//! Port of physics.js.

use serde::Serialize;

pub fn least_squares_slope(xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len() as f64;
    let mx: f64 = xs.iter().sum::<f64>() / n;
    let my: f64 = ys.iter().sum::<f64>() / n;
    let mut num = 0.0;
    let mut den = 0.0;
    for i in 0..xs.len() {
        num += (xs[i] - mx) * (ys[i] - my);
        den += (xs[i] - mx).powi(2);
    }
    if den == 0.0 {
        0.0
    } else {
        num / den
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LapseFit {
    pub lapse_k_per_km: f64,
    pub fallback: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted: Option<f64>,
}

const STANDARD_LAPSE_K_PER_KM: f64 = -6.5;

/// points: (elevM, tempC). Falls back to the standard atmosphere lapse rate
/// (-6.5 K/km) if there are too few stations or the fit is implausible.
pub fn fit_lapse_rate_k_per_km(points: &[(f64, f64)]) -> LapseFit {
    if points.len() < 3 {
        return LapseFit {
            lapse_k_per_km: STANDARD_LAPSE_K_PER_KM,
            fallback: true,
            reason: Some(format!(
                "only {} station(s) with elevation and temperature (need >=3)",
                points.len()
            )),
            fitted: None,
        };
    }
    let elevs: Vec<f64> = points.iter().map(|p| p.0).collect();
    let temps: Vec<f64> = points.iter().map(|p| p.1).collect();
    let slope_per_m = least_squares_slope(&elevs, &temps);
    let lapse_k_per_km = slope_per_m * 1000.0;
    if lapse_k_per_km.abs() > 15.0 {
        return LapseFit {
            lapse_k_per_km: STANDARD_LAPSE_K_PER_KM,
            fallback: true,
            reason: Some(format!(
                "fitted lapse rate {:.1} K/km is implausible (>15 K/km)",
                lapse_k_per_km
            )),
            fitted: Some(lapse_k_per_km),
        };
    }
    LapseFit {
        lapse_k_per_km,
        fallback: false,
        reason: None,
        fitted: None,
    }
}

pub fn reduce_temp_to_elevation(
    temp_c: f64,
    station_elev_m: f64,
    target_elev_m: f64,
    lapse_k_per_km: f64,
) -> f64 {
    temp_c + (lapse_k_per_km / 1000.0) * (target_elev_m - station_elev_m)
}

/// Magnus formula, hPa, td in degC.
pub fn vapor_pressure_hpa(td_c: f64) -> f64 {
    6.112 * ((17.62 * td_c) / (243.12 + td_c)).exp()
}

pub fn dewpoint_from_vapor_pressure_hpa(e_hpa: f64) -> f64 {
    let ln = (e_hpa / 6.112).ln();
    (243.12 * ln) / (17.62 - ln)
}

/// QNH (altimeter setting, hPa) -> station pressure at elevM.
pub fn station_pressure_hpa(qnh_hpa: f64, elev_m: f64) -> f64 {
    qnh_hpa * (1.0 - (0.0065 * elev_m) / 288.15).powf(5.255)
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct WindComponents {
    pub u: f64,
    pub v: f64,
}

/// Meteorological convention: dir is where the wind blows FROM.
pub fn wind_components(speed_ms: f64, dir_deg: f64) -> WindComponents {
    let rad = dir_deg.to_radians();
    WindComponents {
        u: -speed_ms * rad.sin(),
        v: -speed_ms * rad.cos(),
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct WindFromComponents {
    pub speed: f64,
    pub dir: f64,
}

pub fn wind_from_components(u: f64, v: f64) -> WindFromComponents {
    let speed = (u * u + v * v).sqrt();
    let dir = (((-u).atan2(-v)).to_degrees() + 360.0) % 360.0;
    WindFromComponents { speed, dir }
}

pub fn std_dev(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let m: f64 = values.iter().sum::<f64>() / values.len() as f64;
    let variance: f64 =
        values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / values.len() as f64;
    Some(variance.sqrt())
}

/// Same weighting as idw.rs (weight = 1/distance, exact-match override at d=0),
/// applied to an arbitrary derived value per point rather than the raw value.
pub fn idw_average(distances: &[f64], values: &[f64]) -> f64 {
    let weights: Vec<f64> = match distances.iter().position(|&d| d == 0.0) {
        Some(zero_idx) => (0..distances.len())
            .map(|i| if i == zero_idx { 1.0 } else { 0.0 })
            .collect(),
        None => distances.iter().map(|&d| 1.0 / d).collect(),
    };
    let total: f64 = weights.iter().sum();
    values
        .iter()
        .zip(&weights)
        .map(|(v, w)| v * w)
        .sum::<f64>()
        / total
}
