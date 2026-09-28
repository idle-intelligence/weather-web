# weather-web

A weather estimate for any point on Earth from the nearest METAR-reporting
stations.

[**Try the demo →**](https://idle-intelligence.github.io/weather-web/web/)

The method comes from SenseAI's weather tools (2015), where the author was
CTO. This crate is its Rust implementation and the source of truth for the
computation.

The same code runs the [kNN weather](https://trucs.ai/knn-weather/) map on
trucs.ai, and the [Browser weather](https://trucs.ai/blog/browser-weather)
blog post explains how it works, step by step, with live data.

The k nearest stations within a 100 km radius are averaged by inverse
distance weighting, with corrections for elevation (temperature reduced to
the target's own elevation using a lapse rate fitted across the neighbours),
dew point (averaged as vapour pressure, not linearly), pressure (averaged as
the sea-level value the airports report, QNH, and kept at sea level), and
wind (averaged as vector components, not scalar speed and direction).
Observations older than 90 minutes are excluded from the corrected average.

## Prerequisites

Rust with the `wasm32-unknown-unknown` target, `wasm-pack`, Node.js (for the
parity check), and Python 3 or any static file server for the demo.

## Demo

A browser page at `web/index.html`: pick a city or type coordinates, Fetch
finds the nearest stations and their live observations, Compute runs the
`weather` crate's estimate on them. Runs entirely client-side (WebAssembly).

Published at https://idle-intelligence.github.io/weather-web/web/ (`tools/publish-pages.sh`
builds `weather-wasm` and pushes `web/` to an orphan `gh-pages` branch).

To run locally:

```
mkdir -p web/pkg web/data
wasm-pack build weather-wasm --target web --release
cp weather-wasm/pkg/weather_wasm.js weather-wasm/pkg/weather_wasm_bg.wasm web/pkg/
python3 -m http.server 8000 --directory web
```

Then open http://localhost:8000/. The page fetches the station list from
the `idle-intelligence/metar-stations` Hugging Face dataset; `?local=1`
reads a local copy from the gitignored `web/data/stations_all.json` instead.
Get that file with

```
hf download idle-intelligence/metar-stations stations_all.json --repo-type dataset --local-dir web/data
```

or a plain

```
curl -L -o web/data/stations_all.json https://huggingface.co/datasets/idle-intelligence/metar-stations/resolve/main/stations_all.json
```

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

After `cargo build --release -p weather-cli`, run it as `./target/release/weather-cli`,
or use `cargo run -p weather-cli --release -- estimate ...` directly.

```
weather-cli estimate --lat LAT --lon LON --stations PATH [--k N] [--elev METERS]
weather-cli snapshot --stations PATH --out FILE
weather-cli validate --snapshot FILE --stations PATH --out CSV
```

`estimate` fetches live observations from IEM for the nearest stations to a
point and prints the corrected estimate, including both the sea-level value
(QNH) and the station pressure at the target's elevation. `snapshot` fetches
and saves one observation for every station in a list, in batches that
respect IEM's query-length limit. `validate` runs a leave-one-out check: for
every station with a fresh observation in a snapshot, it hides that station,
estimates its weather from its own neighbours, and compares the estimate to
what the station actually reported.

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

That dataset's roster comes from every Iowa Environmental Mesonet (IEM)
ASOS/AWOS network, one network per country or per US state/Canadian
province: 7,534 stations in all, each flagged for whether it reported at
least once in the trailing 7-day window used to build the file. IEM's
elevation values are used as given, except for 17 coastal and offshore
stations (oil platforms, islands, lighthouses) where IEM lists a wrong
negative or depth value; those 17 are set to 0 m in the dataset build.

`data/` is a gitignored scratch directory for a local copy of the station
list and of saved snapshots; it is not committed, and no test depends on it.

### Using your own station list

`Stations` takes the JSON text of any list of rows
`[id, lat, lon, elev_m, name, country]`, optionally followed by `reported`
(true or false) and the last report time; rows without the flag count as
reporting. Nothing in the format is specific to METAR.

The tunables are parameters, not hard-coded: how many stations to list, the
search radius, how many of the listed stations the estimate averages over,
and the maximum observation age. They are set through `select_with` /
`estimate_with` in Rust and `selectWithParams` / `estimateWithParams` in
JavaScript. The library's own default is 12 stations listed within 100 km,
5 of them used for the estimate, with observations up to 90 minutes old; the
browser demo instead passes `k: null` (`None` in Rust) to list every station
within the 100 km radius, with no count cap.

Selection is a linear scan, about 290 ns per station in a release build;
measure it with `cargo run --release --example bench_select -- <station list>`.
That comes to about 2.2 ms for the published stations_all.json (7,534
stations), once per query.

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

There are a few ways we could improve our approach here:

1. making sure the fit can't run away, by limiting our correction rate to
   values that have a physical sense.
2. fitting those rates on more stations.
3. Also weight the neighbours by height difference as well as distance.

## License

Code is under MIT (see `LICENSE`); the station list and observations keep
their sources' terms above.
