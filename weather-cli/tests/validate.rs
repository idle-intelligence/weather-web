//! Checks that `validate` hides each station's own observation before
//! estimating it: the leave-one-out neighbour search must never return the
//! station itself (which would show up as a neighbour at distance 0).
//!
//! Fixtures: five real Paris-area airports (LFPG, LFPO, LFPB, LFPN, LFOB)
//! with their real coordinates and elevations, and observations reused from
//! the `correctionsParisFive` parity fixture (weather/tests/fixtures/parity.json).

use std::process::Command;

#[test]
fn validate_excludes_the_station_being_estimated() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let stations_path = format!("{manifest_dir}/tests/fixtures/stations_paris.json");
    let snapshot_path = format!("{manifest_dir}/tests/fixtures/snapshot_paris.json");
    let out_path = std::env::temp_dir().join(format!(
        "weather-cli-validate-test-{}.csv",
        std::process::id()
    ));

    let output = Command::new(env!("CARGO_BIN_EXE_weather-cli"))
        .arg("validate")
        .arg("--snapshot")
        .arg(&snapshot_path)
        .arg("--stations")
        .arg(&stations_path)
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("failed to run weather-cli validate");

    assert!(
        output.status.success(),
        "validate failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let csv = std::fs::read_to_string(&out_path).expect("reading validate output CSV");
    std::fs::remove_file(&out_path).ok();

    let header = csv.lines().next().expect("CSV has a header");
    let icao_col = header.split(',').position(|c| c == "icao").unwrap();
    let dist_col = header
        .split(',')
        .position(|c| c == "nearest_dist_km")
        .unwrap();
    let n_col = header
        .split(',')
        .position(|c| c == "n_neighbours")
        .unwrap();

    let rows: Vec<&str> = csv.lines().skip(1).collect();
    assert_eq!(rows.len(), 5, "all 5 stations have a fresh observation");

    for row in rows {
        let fields: Vec<&str> = row.split(',').collect();
        let icao = fields[icao_col];
        let nearest_dist: f64 = fields[dist_col].parse().unwrap();
        let n_neighbours: usize = fields[n_col].parse().unwrap();

        // If the station's own observation had leaked into its neighbour
        // list, it would be its own nearest neighbour at distance 0.
        assert!(
            nearest_dist > 0.0,
            "{icao}: nearest neighbour distance is {nearest_dist}, self was not hidden"
        );
        // 5 stations total, so at most 4 other candidates.
        assert!(
            n_neighbours <= 4,
            "{icao}: used {n_neighbours} neighbours, self would make 5 possible"
        );
    }
}
