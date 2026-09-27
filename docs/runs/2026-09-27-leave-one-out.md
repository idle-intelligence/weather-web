# Leave-one-out cross-validation, 2026-09-27

## Method

For every station with a fresh observation in a saved snapshot, hide it, find its
neighbours among every other station in the list, and estimate its temperature,
dew point, pressure and wind from the neighbours' observations using the
library's normal kNN plus inverse-distance-weighted estimate path. Compare the
estimate against what the hidden station itself reported.

Stations with no neighbour within the search radius are counted and skipped,
not estimated.

## Parameters

All three parameters are read from the `weather` crate, not redefined by the
validation tool:

- neighbours per estimate (k): 5 (`weather::stations::DEFAULT_K`)
- search radius: 100 km (`weather::stations::MAX_RADIUS_KM`)
- max observation age: 90 minutes (`weather::corrections::MAX_AGE_MIN`)

Station list: the metar-stations dataset, 5,619 active stations, each row
`[id, lat, lon, elevation_m, name, country]`.

Snapshot: one `weather-cli snapshot` run, fetched from IEM's `currents.json`
API in 10 batched requests (station ids grouped so each request's query
string stays under IEM's URL-length limit, rather than one request per
station). Fetch completed at 2026-09-27T16:02:49Z UTC, 5,619 observations
retrieved, no rate-limit (429) responses.

Validated with `weather-cli validate` against that snapshot.

## Results

4,077 of 5,619 stations had a fresh observation and at least one neighbour
within 100 km and were validated; 903 stations had a fresh observation but no
neighbour within range and were skipped; the rest had no fresh observation in
the snapshot.

### Temperature error (estimated minus observed, degrees C)

| split | n | MAE | RMSE | bias | median abs error | p90 abs error |
|---|---|---|---|---|---|---|
| overall | 4056 | 1.19 | 1.89 | -0.01 | 0.80 | 2.56 |
| distance 0-25 km | 1325 | 1.08 | 1.91 | 0.01 | 0.65 | 2.19 |
| distance 25-50 km | 1614 | 1.09 | 1.63 | -0.02 | 0.79 | 2.38 |
| distance 50-100 km | 1117 | 1.46 | 2.17 | -0.01 | 1.03 | 3.36 |
| elevation diff 0-100 m | 3235 | 1.02 | 1.57 | 0.01 | 0.72 | 2.14 |
| elevation diff 100-300 m | 530 | 1.44 | 2.13 | -0.10 | 1.00 | 3.26 |
| elevation diff 300-1000 m | 235 | 2.52 | 3.56 | -0.05 | 1.65 | 5.34 |
| elevation diff >1000 m | 56 | 3.36 | 4.43 | 0.05 | 2.45 | 7.34 |

"Distance" is the nearest used neighbour's distance to the validated station.
"Elevation diff" is the absolute elevation difference to that same nearest
neighbour.

### Other variables (estimated minus observed), overall

| variable | n | MAE | RMSE | bias | median abs error | p90 abs error |
|---|---|---|---|---|---|---|
| dew point (C) | 4045 | 1.31 | 2.09 | 0.08 | 0.93 | 2.76 |
| pressure QNH (hPa) | 4014 | 0.77 | 1.55 | -0.03 | 0.40 | 1.66 |
| wind speed (m/s) | 3970 | 1.36 | 1.89 | -0.21 | 1.03 | 2.93 |
| wind direction (deg) | 3597 | 41.30 | 60.81 | 2.31 | 23.68 | 117.53 |

## Observations

Temperature bias is close to zero overall and in every distance and elevation
bin; the estimator is not systematically warm or cold, only imprecise.

Error grows with the nearest neighbour's elevation difference much more than
with its distance: MAE goes from 1.02 C at 0-100 m elevation difference to
3.36 C above 1000 m, while MAE only goes from 1.08 C to 1.46 C across the
full 0-100 km distance range. The lapse-rate correction is doing real work
but does not fully close the gap in mountainous terrain.

903 stations, 16% of the fresh-observation set, had no neighbour within
100 km. These are in sparse networks and the page would show no estimate for
them rather than a bad one.

Wind direction error is large (median 23.7 degrees, p90 118 degrees) because
it is estimated as a distance-weighted vector average of neighbours, which
degrades sharply if the neighbours span a local wind-direction change (coastal
or terrain-driven), not because of an implementation issue: the parity tests
already check this vector-averaging math against the reference JS
implementation.

Pressure QNH is the most accurate of the corrected quantities, consistent
with pressure fields varying smoothly over 10-100 km scales.
