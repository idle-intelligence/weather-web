//! idw.rs — inverse-distance-weighted kNN regression.
//! Port of idw.js. sklearn's 'distance' weighting is weight = 1/d, with the
//! special case that if any neighbour has distance exactly 0, only that
//! neighbour is used (weight 1) and all others get weight 0.

#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub value: f64,
    pub distance: f64,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdwTerm {
    pub value: f64,
    pub distance: f64,
    pub weight: f64,
    pub weight_norm: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IdwResult {
    pub value: f64,
    pub terms: Vec<IdwTerm>,
}

fn weights_for(points: &[Point]) -> Vec<f64> {
    match points.iter().position(|p| p.distance == 0.0) {
        Some(zero_idx) => (0..points.len())
            .map(|i| if i == zero_idx { 1.0 } else { 0.0 })
            .collect(),
        None => points.iter().map(|p| 1.0 / p.distance).collect(),
    }
}

pub fn idw(points: &[Point]) -> Option<IdwResult> {
    if points.is_empty() {
        return None;
    }
    let weights = weights_for(points);
    let total_weight: f64 = weights.iter().sum();
    let value: f64 = points
        .iter()
        .zip(&weights)
        .map(|(p, w)| p.value * w)
        .sum::<f64>()
        / total_weight;

    let terms = points
        .iter()
        .zip(&weights)
        .map(|(p, &w)| IdwTerm {
            value: p.value,
            distance: p.distance,
            weight: w,
            weight_norm: w / total_weight,
        })
        .collect();

    Some(IdwResult { value, terms })
}

/// Circular version for wind direction (degrees). Weighted vector mean.
pub fn idw_circular_deg(points: &[Point]) -> Option<IdwResult> {
    if points.is_empty() {
        return None;
    }
    let weights = weights_for(points);
    let total_weight: f64 = weights.iter().sum();

    let mut sum_sin = 0.0;
    let mut sum_cos = 0.0;
    for (p, &w) in points.iter().zip(&weights) {
        let rad = p.value.to_radians();
        sum_sin += w * rad.sin();
        sum_cos += w * rad.cos();
    }
    let value = ((sum_sin / total_weight)
        .atan2(sum_cos / total_weight)
        .to_degrees()
        + 360.0)
        % 360.0;

    let terms = points
        .iter()
        .zip(&weights)
        .map(|(p, &w)| IdwTerm {
            value: p.value,
            distance: p.distance,
            weight: w,
            weight_norm: w / total_weight,
        })
        .collect();

    Some(IdwResult { value, terms })
}
