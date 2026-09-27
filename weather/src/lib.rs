//! weather — Rust reimplementation of the trucs.ai kNN weather estimator
//! (station list, inverse-distance-weighted average, and physics
//! corrections), so it can be run and cross-validated locally.

pub mod corrections;
pub mod idw;
pub mod observation;
pub mod physics;
pub mod stations;
pub mod time;

pub use corrections::{compute_corrections, Corrections, NeighborRow, MAX_AGE_MIN};
pub use idw::{idw, idw_circular_deg, IdwResult};
pub use observation::{parse_iem_currents, Observation};
pub use stations::{
    haversine_km, load_stations, nearest, nearest_within, parse_stations, Neighbor, Station,
    DEFAULT_K, MAX_RADIUS_KM,
};
