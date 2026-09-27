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

## Known limits

The leave-one-out cross-validation run (`docs/runs/2026-09-27-leave-one-out.md`)
found a temperature MAE of 1.19 C overall. Accuracy depends much more on the
nearest neighbour's elevation than on its distance: MAE is 1.02 C when the
nearest neighbour is within 100 m of the target's elevation, but rises to
2.52 C between 300 and 1000 m of elevation difference and 3.36 C above
1000 m. The lapse-rate correction helps but does not close this gap in
mountainous terrain.

Altitude handling has been tabled for now. Three ways forward, not started:

- Clamp the fitted lapse rate to a physical range (for example -10 to +8
  K/km) instead of only falling back on an implausible fit, so the estimate
  still benefits from a fitted rate while keeping inversions in range.
- Fit the lapse rate on a wider pool of stations (15 to 20 within 150-200 km)
  while still averaging the estimate itself on the nearest 5, so the fit has
  enough elevation spread to be stable.
- Weight neighbours by height difference as well as distance, using an
  effective distance `sqrt(d^2 + (lambda*dz)^2)` with `lambda` tuned on the
  validation set, so a close neighbour at a very different elevation counts
  for less.

## License

Code is under MIT; the station list and observations keep their sources' terms (IEM / Iowa State University; see the metar-stations dataset card).
