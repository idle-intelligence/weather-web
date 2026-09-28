//! corrections.rs: physics-correct averaging on top of the plain
//! distance-weighted average, after Nalder & Wein (1998) and standard
//! meteorological practice. Port of corrections.js.
//!
//!   - temperature: reduced to the target elevation with a lapse rate fitted
//!     by least squares across the neighbours (falls back to the standard
//!     atmosphere, -6.5 K/km, if fewer than 3 stations or an implausible fit).
//!   - dew point: averaged as vapour pressure (Magnus formula), then converted
//!     back, since dew point itself is not a linearly averageable quantity.
//!   - pressure: the altimeter setting (QNH) is averaged, then reduced to
//!     station pressure at the target elevation.
//!   - wind: averaged as vector components (u, v), not scalar speed/direction.
//!
//! Observations older than MAX_AGE_MIN are excluded from the corrected
//! average (the plain average does not apply this cutoff, for comparison).

use crate::idw::{idw, Point};
use crate::observation::Observation;
use crate::physics::{
    dewpoint_from_vapor_pressure_hpa, fit_lapse_rate_k_per_km, idw_average,
    reduce_temp_to_elevation, station_pressure_hpa, std_dev, vapor_pressure_hpa,
    wind_components, wind_from_components, LapseFit,
};
use serde::{Deserialize, Serialize};

pub const MAX_AGE_MIN: f64 = 90.0;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeighborRow {
    pub icao: String,
    pub distance: f64,
    pub elev_m: Option<f64>,
    pub obs: Option<Observation>,
}

fn age_minutes(obs_time_millis: Option<i64>, now_millis: i64) -> Option<f64> {
    obs_time_millis.map(|t| (now_millis - t) as f64 / 60000.0)
}

/// Observations older than `max_age_min` are excluded from the corrected
/// average. Passing `MAX_AGE_MIN` reproduces `compute_corrections`'s cutoff.
pub fn compute_corrections_with(
    rows: &[NeighborRow],
    target_elev_m: Option<f64>,
    now_millis: i64,
    max_age_min: f64,
) -> Corrections {
    compute_corrections_inner(rows, target_elev_m, now_millis, max_age_min)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TempPoint {
    pub icao: String,
    pub distance: f64,
    pub elev_m: Option<f64>,
    pub value: f64,
    pub age: Option<f64>,
    pub included: bool,
    pub corrected_value: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Temperature {
    pub plain: Option<f64>,
    pub corrected: Option<f64>,
    pub points: Vec<TempPoint>,
    pub lapse: LapseFit,
    pub spread: Option<f64>,
    pub nearest_dist: Option<f64>,
    pub elev_spread: Option<f64>,
    pub max_age: Option<f64>,
    pub excluded: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DewPointPoint {
    pub icao: String,
    pub distance: f64,
    pub value: f64,
    pub age: Option<f64>,
    pub included: bool,
    pub e: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DewPoint {
    pub plain: Option<f64>,
    pub corrected: Option<f64>,
    pub points: Vec<DewPointPoint>,
    pub spread: Option<f64>,
    pub nearest_dist: Option<f64>,
    pub max_age: Option<f64>,
    pub excluded: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PressurePoint {
    pub icao: String,
    pub distance: f64,
    pub value: f64,
    pub age: Option<f64>,
    pub included: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pressure {
    pub plain: Option<f64>,
    pub corrected_qnh: Option<f64>,
    pub corrected_station: Option<f64>,
    pub points: Vec<PressurePoint>,
    pub spread: Option<f64>,
    pub nearest_dist: Option<f64>,
    pub max_age: Option<f64>,
    pub excluded: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindPoint {
    pub icao: String,
    pub distance: f64,
    pub speed: f64,
    pub dir: f64,
    pub age: Option<f64>,
    pub included: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Wind {
    pub plain_scalar_speed: Option<f64>,
    pub corrected_speed: Option<f64>,
    pub corrected_dir: Option<f64>,
    pub points: Vec<WindPoint>,
    pub spread: Option<f64>,
    pub nearest_dist: Option<f64>,
    pub max_age: Option<f64>,
    pub excluded: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Corrections {
    pub target_elev_m: Option<f64>,
    pub lapse: LapseFit,
    pub temperature: Temperature,
    pub dewpoint: DewPoint,
    pub pressure: Pressure,
    pub wind: Wind,
}

/// rows: final neighbours (each carrying its observation), any order (the JS
/// version expects ascending distance but does not depend on it).
/// target_elev_m: target-point elevation (Open-Meteo), or None if unavailable.
pub fn compute_corrections(
    rows: &[NeighborRow],
    target_elev_m: Option<f64>,
    now_millis: i64,
) -> Corrections {
    compute_corrections_inner(rows, target_elev_m, now_millis, MAX_AGE_MIN)
}

fn compute_corrections_inner(
    rows: &[NeighborRow],
    target_elev_m: Option<f64>,
    now_millis: i64,
    max_age_min: f64,
) -> Corrections {
    let with_age: Vec<(&NeighborRow, Option<f64>)> = rows
        .iter()
        .map(|s| {
            let age = age_minutes(s.obs.as_ref().and_then(|o| o.obs_time_millis), now_millis);
            (s, age)
        })
        .collect();
    let has_target = target_elev_m.is_some();

    // ---- temperature ----
    let t_pts: Vec<TempPoint> = with_age
        .iter()
        .filter_map(|(s, age)| {
            let temp_c = s.obs.as_ref()?.temp_c?;
            Some(TempPoint {
                icao: s.icao.clone(),
                distance: s.distance,
                elev_m: s.elev_m,
                value: temp_c,
                age: *age,
                included: age.map(|a| a <= max_age_min).unwrap_or(false),
                corrected_value: None,
            })
        })
        .collect();
    let t_fresh: Vec<&TempPoint> = t_pts
        .iter()
        .filter(|p| p.included && p.elev_m.is_some())
        .collect();
    let lapse = fit_lapse_rate_k_per_km(
        &t_fresh
            .iter()
            .map(|p| (p.elev_m.unwrap(), p.value))
            .collect::<Vec<_>>(),
    );
    let temp_corr_points: Vec<(String, f64)> = if has_target {
        t_fresh
            .iter()
            .map(|p| {
                (
                    p.icao.clone(),
                    reduce_temp_to_elevation(
                        p.value,
                        p.elev_m.unwrap(),
                        target_elev_m.unwrap(),
                        lapse.lapse_k_per_km,
                    ),
                )
            })
            .collect()
    } else {
        vec![]
    };
    let temperature = {
        let plain = if !t_pts.is_empty() {
            idw(&t_pts
                .iter()
                .map(|p| Point {
                    value: p.value,
                    distance: p.distance,
                })
                .collect::<Vec<_>>())
            .map(|r| r.value)
        } else {
            None
        };
        let corrected = if !temp_corr_points.is_empty() {
            let distances: Vec<f64> = t_fresh.iter().map(|p| p.distance).collect();
            let values: Vec<f64> = temp_corr_points.iter().map(|(_, v)| *v).collect();
            Some(idw_average(&distances, &values))
        } else {
            None
        };
        let points: Vec<TempPoint> = t_pts
            .iter()
            .map(|p| {
                let mut p = p.clone();
                p.corrected_value = temp_corr_points
                    .iter()
                    .find(|(icao, _)| icao == &p.icao)
                    .map(|(_, v)| *v);
                p
            })
            .collect();
        Temperature {
            plain,
            corrected,
            points,
            lapse: lapse.clone(),
            spread: std_dev(&t_pts.iter().map(|p| p.value).collect::<Vec<_>>()),
            nearest_dist: t_pts
                .iter()
                .map(|p| p.distance)
                .fold(None, |acc: Option<f64>, d| {
                    Some(acc.map_or(d, |a| a.min(d)))
                }),
            elev_spread: if !t_fresh.is_empty() {
                let elevs: Vec<f64> = t_fresh.iter().map(|p| p.elev_m.unwrap()).collect();
                let max = elevs.iter().cloned().fold(f64::MIN, f64::max);
                let min = elevs.iter().cloned().fold(f64::MAX, f64::min);
                Some(max - min)
            } else {
                None
            },
            max_age: if !t_pts.is_empty() {
                Some(
                    t_pts
                        .iter()
                        .map(|p| p.age.unwrap_or(0.0))
                        .fold(f64::MIN, f64::max),
                )
            } else {
                None
            },
            excluded: t_pts.iter().filter(|p| !p.included).count(),
        }
    };

    // ---- dew point ----
    let d_pts: Vec<DewPointPoint> = with_age
        .iter()
        .filter_map(|(s, age)| {
            let dewpoint_c = s.obs.as_ref()?.dewpoint_c?;
            Some(DewPointPoint {
                icao: s.icao.clone(),
                distance: s.distance,
                value: dewpoint_c,
                age: *age,
                included: age.map(|a| a <= max_age_min).unwrap_or(false),
                e: vapor_pressure_hpa(dewpoint_c),
            })
        })
        .collect();
    let d_fresh: Vec<&DewPointPoint> = d_pts.iter().filter(|p| p.included).collect();
    let e_avg = if !d_fresh.is_empty() {
        let distances: Vec<f64> = d_fresh.iter().map(|p| p.distance).collect();
        let values: Vec<f64> = d_fresh.iter().map(|p| p.e).collect();
        Some(idw_average(&distances, &values))
    } else {
        None
    };
    let dewpoint = build_dewpoint(&d_pts, e_avg);

    // ---- pressure ----
    let p_pts: Vec<PressurePoint> = with_age
        .iter()
        .filter_map(|(s, age)| {
            let pressure_hpa = s.obs.as_ref()?.pressure_hpa?;
            Some(PressurePoint {
                icao: s.icao.clone(),
                distance: s.distance,
                value: pressure_hpa,
                age: *age,
                included: age.map(|a| a <= max_age_min).unwrap_or(false),
            })
        })
        .collect();
    let p_fresh: Vec<&PressurePoint> = p_pts.iter().filter(|p| p.included).collect();
    let qnh_avg = if !p_fresh.is_empty() {
        let distances: Vec<f64> = p_fresh.iter().map(|p| p.distance).collect();
        let values: Vec<f64> = p_fresh.iter().map(|p| p.value).collect();
        Some(idw_average(&distances, &values))
    } else {
        None
    };
    let pressure = Pressure {
        plain: if !p_pts.is_empty() {
            idw(&p_pts
                .iter()
                .map(|p| Point {
                    value: p.value,
                    distance: p.distance,
                })
                .collect::<Vec<_>>())
            .map(|r| r.value)
        } else {
            None
        },
        corrected_qnh: qnh_avg,
        corrected_station: match (qnh_avg, target_elev_m) {
            (Some(qnh), Some(elev)) => Some(station_pressure_hpa(qnh, elev)),
            _ => None,
        },
        points: p_pts.clone(),
        spread: std_dev(&p_pts.iter().map(|p| p.value).collect::<Vec<_>>()),
        nearest_dist: p_pts
            .iter()
            .map(|p| p.distance)
            .fold(None, |acc: Option<f64>, d| {
                Some(acc.map_or(d, |a| a.min(d)))
            }),
        max_age: if !p_pts.is_empty() {
            Some(
                p_pts
                    .iter()
                    .map(|p| p.age.unwrap_or(0.0))
                    .fold(f64::MIN, f64::max),
            )
        } else {
            None
        },
        excluded: p_pts.iter().filter(|p| !p.included).count(),
    };

    // ---- wind ----
    let w_pts: Vec<WindPoint> = with_age
        .iter()
        .filter_map(|(s, age)| {
            let obs = s.obs.as_ref()?;
            let speed = obs.wind_ms?;
            let dir = obs.wind_dir_deg?;
            Some(WindPoint {
                icao: s.icao.clone(),
                distance: s.distance,
                speed,
                dir,
                age: *age,
                included: age.map(|a| a <= max_age_min).unwrap_or(false),
            })
        })
        .collect();
    let w_fresh: Vec<&WindPoint> = w_pts.iter().filter(|p| p.included).collect();
    let w_vec: Vec<(f64, crate::physics::WindComponents)> = w_fresh
        .iter()
        .map(|p| (p.distance, wind_components(p.speed, p.dir)))
        .collect();
    let u_avg = if !w_vec.is_empty() {
        let distances: Vec<f64> = w_vec.iter().map(|(d, _)| *d).collect();
        let values: Vec<f64> = w_vec.iter().map(|(_, c)| c.u).collect();
        Some(idw_average(&distances, &values))
    } else {
        None
    };
    let v_avg = if !w_vec.is_empty() {
        let distances: Vec<f64> = w_vec.iter().map(|(d, _)| *d).collect();
        let values: Vec<f64> = w_vec.iter().map(|(_, c)| c.v).collect();
        Some(idw_average(&distances, &values))
    } else {
        None
    };
    let vec_result = match (u_avg, v_avg) {
        (Some(u), Some(v)) => Some(wind_from_components(u, v)),
        _ => None,
    };
    let wind = Wind {
        plain_scalar_speed: if !w_pts.is_empty() {
            let distances: Vec<f64> = w_pts.iter().map(|p| p.distance).collect();
            let values: Vec<f64> = w_pts.iter().map(|p| p.speed).collect();
            Some(idw_average(&distances, &values))
        } else {
            None
        },
        corrected_speed: vec_result.map(|r| r.speed),
        corrected_dir: vec_result.map(|r| r.dir),
        points: w_pts.clone(),
        spread: std_dev(&w_pts.iter().map(|p| p.speed).collect::<Vec<_>>()),
        nearest_dist: w_pts
            .iter()
            .map(|p| p.distance)
            .fold(None, |acc: Option<f64>, d| {
                Some(acc.map_or(d, |a| a.min(d)))
            }),
        max_age: if !w_pts.is_empty() {
            Some(
                w_pts
                    .iter()
                    .map(|p| p.age.unwrap_or(0.0))
                    .fold(f64::MIN, f64::max),
            )
        } else {
            None
        },
        excluded: w_pts.iter().filter(|p| !p.included).count(),
    };

    Corrections {
        target_elev_m: if has_target { target_elev_m } else { None },
        lapse,
        temperature,
        dewpoint,
        pressure,
        wind,
    }
}

fn build_dewpoint(d_pts: &[DewPointPoint], e_avg: Option<f64>) -> DewPoint {
    DewPoint {
        plain: if !d_pts.is_empty() {
            idw(&d_pts
                .iter()
                .map(|p| Point {
                    value: p.value,
                    distance: p.distance,
                })
                .collect::<Vec<_>>())
            .map(|r| r.value)
        } else {
            None
        },
        corrected: e_avg.map(dewpoint_from_vapor_pressure_hpa),
        points: d_pts.to_vec(),
        spread: std_dev(&d_pts.iter().map(|p| p.value).collect::<Vec<_>>()),
        nearest_dist: d_pts
            .iter()
            .map(|p| p.distance)
            .fold(None, |acc: Option<f64>, d| {
                Some(acc.map_or(d, |a| a.min(d)))
            }),
        max_age: if !d_pts.is_empty() {
            Some(
                d_pts
                    .iter()
                    .map(|p| p.age.unwrap_or(0.0))
                    .fold(f64::MIN, f64::max),
            )
        } else {
            None
        },
        excluded: d_pts.iter().filter(|p| !p.included).count(),
    }
}
