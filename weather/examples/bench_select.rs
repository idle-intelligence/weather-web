//! Timing check for select_with()'s linear scan, referenced in the repo's
//! review of station-list scalability (see README.md, "Using your own
//! station list"). Not part of the library's public surface: run with
//! `cargo run --release --example bench_select -- <stations.json>`.
//!
//! Runs `select_with` (list_k=12, radius=100 km) 200 times at a fixed point
//! and reports the mean per-call time, so a caller plugging in their own
//! list -- of any size -- can see what a linear scan over it costs before
//! deciding whether they need a spatial index.

use std::time::Instant;
use weather::select::{select_with, SelectParams};
use weather::stations::load_stations;

fn main() {
    let path = std::env::args().nth(1).expect("usage: bench_select <stations.json>");
    let stations = load_stations(&path).expect("loading stations file");
    let params = SelectParams { k: 12, max_radius_km: 100.0 };
    let lat = 50.6292;
    let lon = 3.0573;

    const RUNS: u32 = 200;
    // One untimed warmup call.
    select_with(&stations, lat, lon, &params);
    let start = Instant::now();
    for _ in 0..RUNS {
        select_with(&stations, lat, lon, &params);
    }
    let elapsed = start.elapsed();
    println!(
        "{path}: {} stations, {} runs, {:.1} us/call, {:.1} ns/station/call",
        stations.len(),
        RUNS,
        elapsed.as_secs_f64() * 1e6 / RUNS as f64,
        elapsed.as_secs_f64() * 1e9 / RUNS as f64 / stations.len() as f64,
    );
}
