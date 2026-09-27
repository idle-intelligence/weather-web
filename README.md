# weather-web

A Rust reimplementation of the trucs.ai kNN weather estimator: nearest-station
search, inverse-distance-weighted averaging, and the temperature/dew
point/pressure/wind physics corrections, so the method can run and be
validated locally instead of only in a browser tab. A browser build (wasm)
from the same crate comes later.

## Build and test

From a fresh clone:

```
cargo build --workspace
cargo test --workspace
```

`cargo clippy --workspace --all-targets` should also be clean.

## Layout

- `weather/` — the library crate: station loading, haversine nearest-k
  search, IDW, and the physics corrections.
- `weather-cli/` — a native CLI (`weather-cli estimate`, `weather-cli
  snapshot`) that fetches live observations from IEM.
- `tools/parity/` — a Node script that runs the original JS modules once to
  generate the fixtures the Rust parity tests check against
  (`weather/tests/fixtures/parity.json`).

## Station list

The library reads a `stations.json` file: a JSON array of
`[icao, lat, lon, elev_m, name, country]` rows. This will be published as the
`idle-intelligence/metar-stations` dataset on Hugging Face; until then, pass
a local copy with `--stations`.

## License

Code is under MIT; the station list and observations keep their sources' terms (IEM / Iowa State University; see the metar-stations dataset card).
