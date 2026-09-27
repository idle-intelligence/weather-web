# weather-web

A weather estimate for any point on Earth from the nearest METAR-reporting
stations: the k nearest stations within a 100 km radius are averaged by
inverse distance weighting, with corrections for elevation (temperature
reduced to the target's own elevation using a lapse rate fitted across the
neighbours), dew point (averaged as vapour pressure, not linearly), pressure
(altimeter setting reduced to station pressure), and wind (averaged as
vector components, not scalar speed and direction). Observations older than
90 minutes are excluded from the corrected average.

This is a Rust reimplementation of a kNN weather estimator originally
written in JavaScript for a browser demo. The crate is the source of truth
for the computation now.

## Demo

A browser page at `web/index.html`: pick a city or type coordinates, Fetch
finds the nearest stations and their live observations, Compute runs the
`weather` crate's estimate on them. Runs entirely client-side (WebAssembly).

Published at https://idle-intelligence.github.io/weather-web/ (`tools/publish-pages.sh`
builds `weather-wasm` and pushes `web/` to an orphan `gh-pages` branch).

To run locally:

```
wasm-pack build weather-wasm --target web --release
cp weather-wasm/pkg/weather_wasm.js weather-wasm/pkg/weather_wasm_bg.wasm web/pkg/
python3 -m http.server 8000 --directory web
```

Then open http://localhost:8000/. The page fetches the station list from
the `idle-intelligence/metar-stations` Hugging Face dataset; `?local=1`
reads a local copy from the gitignored `web/data/stations.json` instead.

## Crates

- `weather/` - the library: station loading, haversine nearest-k search,
  inverse-distance weighting, and the physics corrections.
- `weather-cli/` - a native CLI (`estimate`, `snapshot`, `validate`) that
  fetches live observations from IEM.
- `weather-wasm/` - wasm-bindgen exports of the same library for the
  browser and Node.

## Build and test

From a fresh clone:

```
cargo test --workspace
cargo clippy --workspace --all-targets
```

To build the WebAssembly bindings and check them against the Rust library:

```
wasm-pack build weather-wasm --target web --release
wasm-pack build weather-wasm --target nodejs --release --out-dir pkg-node
node weather-wasm/tests/parity.mjs
```

Tests use small, real, committed fixtures under `weather/tests/fixtures/`
and `weather-cli/tests/fixtures/` (subsets of the station list and of a
saved observation snapshot), so none of this needs network access or a
local copy of the full dataset.

## CLI usage

```
weather-cli estimate --lat LAT --lon LON --stations PATH [--k N] [--elev METERS]
weather-cli snapshot --stations PATH --out FILE
weather-cli validate --snapshot FILE --stations PATH --out CSV
```

`estimate` fetches live observations from IEM for the nearest stations to a
point and prints the corrected estimate. `snapshot` fetches and saves one
observation for every station in a list, in batches that respect IEM's
query-length limit. `validate` runs a leave-one-out check: for every station
with a fresh observation in a snapshot, it hides that station, estimates its
weather from its own neighbours, and compares the estimate to what the
station actually reported.

Example, using the small station subset committed for the tests:

```
$ weather-cli estimate --lat 49.0153 --lon 2.5344 --stations weather/tests/fixtures/stations_subset.json --elev 109
estimate for (49.0153, 2.5344), 5 neighbours:
  LFPG       0.0 km  elev    109 m  [ok]
  LFPB       9.5 km  elev     65 m  [ok]
  LFPO      35.0 km  elev     96 m  [ok]
  LFPV      36.2 km  elev    179 m  [ok]
  LFPT      37.1 km  elev     99 m  [ok]

temperature (plain IDW):       22.00 C
temperature (corrected):       22.00 C  lapse -0.93 K/km
dew point (plain IDW):         12.00 C
dew point (corrected):         12.00 C
pressure QNH (corrected):    1017.00 hPa
pressure station (corr.):    1003.93 hPa
wind speed (corrected):         2.57 m/s
wind dir (corrected):          290.0 deg
```

`estimate` fetches live observations from IEM, so its numbers will differ
run to run and it needs network access; `snapshot` and `validate` work
offline once a snapshot file exists.

## Station list

The library reads a `stations.json` file: a JSON array of
`[icao, lat, lon, elev_m, name, country]` rows. The full list is published
as the `idle-intelligence/metar-stations` dataset on Hugging Face:
https://huggingface.co/datasets/idle-intelligence/metar-stations

`data/` is a gitignored scratch directory for a local copy of the station
list and of saved snapshots; it is not committed, and no test depends on it.

## Data sources

- Observations: IEM's `currents.json` API (Iowa Environmental Mesonet /
  Iowa State University), public domain, attribution appreciated.
- Observations: api.weather.gov (US National Weather Service), public
  domain.
- Elevation for a target point (used by the caller, not fetched by this
  crate): Open-Meteo's elevation API; check open-meteo.com for current
  terms before relying on it.

## Leave-one-out results

The leave-one-out cross-validation run
(`docs/runs/2026-09-27-leave-one-out.md`) found a temperature MAE of
1.19 C overall. Accuracy depends much more on the nearest neighbour's
elevation than on its distance: MAE is 1.02 C when the nearest neighbour is
within 100 m of the target's elevation, but rises to 2.52 C between 300 and
1000 m of elevation difference and 3.36 C above 1000 m. The lapse-rate
correction helps but does not close this gap in mountainous terrain.

## Known limits

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

Code is under MIT (see `LICENSE`); the station list and observations keep
their sources' terms above.
