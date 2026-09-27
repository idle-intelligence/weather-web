// Node-side parity check for the weather-wasm bindings: runs the same inputs
// as weather/tests/parity.rs (which itself checks the Rust library against
// fixtures generated from the original trucs.ai JS) through the compiled
// wasm module, and checks the results against the same fixture file.
//
// Run with: node tests/parity.mjs
// (needs `wasm-pack build --target nodejs --release --out-dir pkg-node` first)

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import * as wasm from '../pkg-node/weather_wasm.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const fixtures = JSON.parse(
  readFileSync(path.join(__dirname, '../../weather/tests/fixtures/parity.json'), 'utf8'),
);

const TOL = 1e-9;
let failures = 0;

function assertClose(actual, expected, p) {
  if (typeof expected === 'number') {
    if (typeof actual !== 'number') {
      console.error(`FAIL ${p}: expected number, got ${typeof actual} (${JSON.stringify(actual)})`);
      failures++;
      return;
    }
    const allowed = Math.max(TOL, TOL * Math.abs(expected));
    if (Math.abs(actual - expected) > allowed) {
      console.error(`FAIL ${p}: ${actual} != ${expected} (diff ${Math.abs(actual - expected)}, allowed ${allowed})`);
      failures++;
    }
    return;
  }
  if (expected === null) {
    if (actual !== null && actual !== undefined) {
      console.error(`FAIL ${p}: expected null, got ${JSON.stringify(actual)}`);
      failures++;
    }
    return;
  }
  if (typeof expected === 'boolean' || typeof expected === 'string') {
    if (actual !== expected) {
      console.error(`FAIL ${p}: ${JSON.stringify(actual)} != ${JSON.stringify(expected)}`);
      failures++;
    }
    return;
  }
  if (Array.isArray(expected)) {
    if (!Array.isArray(actual) || actual.length !== expected.length) {
      console.error(`FAIL ${p}: array length mismatch`);
      failures++;
      return;
    }
    expected.forEach((e, i) => assertClose(actual[i], e, `${p}[${i}]`));
    return;
  }
  if (typeof expected === 'object') {
    const keys = new Set([...Object.keys(actual ?? {}), ...Object.keys(expected)]);
    for (const k of keys) {
      assertClose((actual ?? {})[k], expected[k], `${p}.${k}`);
    }
    return;
  }
  console.error(`FAIL ${p}: unhandled type`);
  failures++;
}

// ---- idw ----
const parisFive = wasm.idw([
  { value: 14.2, distance: 3.1 },
  { value: 13.8, distance: 7.4 },
  { value: 14.5, distance: 9.0 },
  { value: 13.1, distance: 12.6 },
  { value: 12.0, distance: 41.3 },
]);
assertClose(parisFive, fixtures.idw.parisFive, 'idw.parisFive');

const zeroDistance = wasm.idw([
  { value: 20.0, distance: 0.0 },
  { value: 10.0, distance: 5.0 },
  { value: 5.0, distance: 12.0 },
]);
assertClose(zeroDistance, fixtures.idw.zeroDistance, 'idw.zeroDistance');

assertClose(wasm.idw([]), fixtures.idw.empty, 'idw.empty');

// ---- idwCircularDeg ----
const circParisFive = wasm.idwCircularDeg([
  { value: 250.0, distance: 3.1 },
  { value: 260.0, distance: 7.4 },
  { value: 10.0, distance: 9.0 },
  { value: 240.0, distance: 12.6 },
  { value: 255.0, distance: 41.3 },
]);
assertClose(circParisFive, fixtures.idwCircularDeg.parisFive, 'idwCircularDeg.parisFive');

const circZero = wasm.idwCircularDeg([
  { value: 90.0, distance: 0.0 },
  { value: 270.0, distance: 5.0 },
]);
assertClose(circZero, fixtures.idwCircularDeg.zeroDistance, 'idwCircularDeg.zeroDistance');

// ---- stationPressureHpa ----
const stationP = wasm.stationPressureHpa(1018.3, 2004.0);
assertClose(stationP, fixtures.physics.stationPressureHpa, 'physics.stationPressureHpa');

// ---- maxAgeMin ----
assertClose(wasm.maxAgeMin(), fixtures.maxAgeMin, 'maxAgeMin');

// ---- computeCorrections ----
const NOW_MILLIS = Date.parse('2026-09-27T12:00:00Z');

function row(icao, distance, elevM, tempC, dewpointC, windMs, windDirDeg, pressureHpa, minutesAgo) {
  return {
    icao,
    distance,
    elevM,
    obs: {
      tempC,
      dewpointC,
      windMs,
      windDirDeg,
      pressureHpa,
      obsTimeMillis: NOW_MILLIS - minutesAgo * 60_000,
      source: 'IEM',
    },
  };
}

const parisFiveRows = [
  row('LFPG', 12.9, 119.0, 14.2, 11.4, 5.6, 230.0, 1018.3, 10),
  row('LFPO', 8.1, 89.0, 14.6, 11.8, 4.9, 220.0, 1018.6, 15),
  row('LFPB', 9.7, 65.0, 14.8, 11.9, 5.2, 225.0, 1018.5, 20),
  row('LFPN', 15.4, 164.0, 13.9, 11.1, 4.5, 215.0, 1018.1, 25),
  row('LFOB', 68.2, 108.0, 13.5, 10.8, 6.1, 240.0, 1017.9, 30),
];
assertClose(
  wasm.computeCorrections(parisFiveRows, 35.0, NOW_MILLIS),
  fixtures.correctionsParisFive,
  'correctionsParisFive',
);

const mountainRows = [
  row('LFLB', 24.5, 235.0, 18.4, 12.0, 2.1, 180.0, 1015.2, 5),
  row('LFLS', 38.1, 384.0, 17.1, 11.5, 1.8, 190.0, 1014.8, 5),
  row('LFLL', 71.3, 821.0, 13.9, 9.2, 3.4, 160.0, 1013.9, 5),
  row('LFLJ', 0.0, 2004.0, 4.2, -1.5, 6.7, 300.0, 1011.0, 5),
];
assertClose(
  wasm.computeCorrections(mountainRows, 2004.0, NOW_MILLIS),
  fixtures.correctionsMountain,
  'correctionsMountain',
);

const staleRows = [
  row('LFPG', 12.9, 119.0, 14.2, 11.4, 5.6, 230.0, 1018.3, 10),
  row('LFPO', 8.1, 89.0, 14.6, 11.8, 4.9, 220.0, 1018.6, 150),
];
assertClose(
  wasm.computeCorrections(staleRows, 35.0, NOW_MILLIS),
  fixtures.correctionsStale,
  'correctionsStale',
);

// ---- parseIemCurrents ----
const currentsJson = JSON.stringify({
  data: [
    {
      station: 'LFPG',
      tmpf: 57.6,
      dwpf: 52.5,
      sknt: 10.9,
      drct: 230,
      alti: 30.07,
      utc_valid: '2026-09-27T11:50Z',
    },
    {
      station: 'LFPO',
      tmpf: 58.3,
      dwpf: 53.2,
      sknt: 9.5,
      drct: 220,
      mslp: 1018.6,
      utc_valid: '2026-09-27T11:45Z',
    },
    { station: 'XXXX', utc_valid: '2026-09-27T11:45Z' },
  ],
});
const parsed = wasm.parseIemCurrents(currentsJson);
for (const [icao, obs] of Object.entries(fixtures.fetchIem)) {
  const expectedMillis = Date.parse(obs.obsTime);
  const expected = { ...obs, obsTimeMillis: expectedMillis };
  delete expected.obsTime;
  assertClose(parsed[icao], expected, `fetchIem.${icao}`);
}

// ---- select / estimate: matches weather/tests/select_estimate_nws.rs ----

const stationsText = readFileSync(
  path.join(__dirname, '../../weather/tests/fixtures/stations_subset.json'),
  'utf8',
);
const stations = new wasm.Stations(stationsText);

// Sahara point: no station within 100 km, no fallback.
const saharaSel = stations.select(24.2, 2.03);
if (saharaSel.stations.length !== 0) {
  console.error(`FAIL select.sahara: expected 0 stations within radius, got ${saharaSel.stations.length}`);
  failures++;
}
if (saharaSel.nearest.station.icao !== 'DATM') {
  console.error(`FAIL select.sahara: expected nearest DATM, got ${saharaSel.nearest.station.icao}`);
  failures++;
}
const saharaEst = wasm.estimate(saharaSel, {}, null, NOW_MILLIS);
if (saharaEst.status.kind !== 'noStationWithinRadius' || saharaEst.status.nearestId !== 'DATM') {
  console.error(`FAIL estimate.sahara: expected noStationWithinRadius/DATM, got ${JSON.stringify(saharaEst.status)}`);
  failures++;
}

// Lille: exactly LFQQ, EBOS, LFAQ, LFAC, EHFS, closest first.
const lilleSel = stations.select(50.6292, 3.0573);
const lilleIcaos = lilleSel.stations.map((s) => s.station.icao);
assertClose(lilleIcaos, ['LFQQ', 'EBOS', 'LFAQ', 'LFAC', 'EHFS'], 'select.lille');

function obsRow(tempC, dewpointC, windMs, windDirDeg, pressureHpa, minutesAgo) {
  return {
    tempC,
    dewpointC,
    windMs,
    windDirDeg,
    pressureHpa,
    obsTimeMillis: NOW_MILLIS - minutesAgo * 60_000,
    source: 'IEM',
  };
}

const freshObs = {
  LFQQ: obsRow(14.2, 11.4, 5.6, 230.0, 1018.3, 10),
  EBOS: obsRow(14.6, 11.8, 4.9, 220.0, 1018.6, 15),
  LFAQ: obsRow(14.8, 11.9, 5.2, 225.0, 1018.5, 20),
  LFAC: obsRow(13.9, 11.1, 4.5, 215.0, 1018.1, 25),
};
const withStale = { ...freshObs, EHFS: obsRow(99.0, 99.0, 99.0, 99.0, 1099.0, 150) };

const estWithStale = wasm.estimate(lilleSel, withStale, 35.0, NOW_MILLIS);
const estWithoutStale = wasm.estimate(lilleSel, freshObs, 35.0, NOW_MILLIS);
const ehfs = estWithStale.stations.find((s) => s.icao === 'EHFS');
if (!ehfs || ehfs.fresh) {
  console.error(`FAIL estimate.stale: expected EHFS stale, got ${JSON.stringify(ehfs)}`);
  failures++;
}
assertClose(estWithStale.staleCount, 1, 'estimate.stale.staleCount');
assertClose(estWithStale.freshCount, 4, 'estimate.stale.freshCount');
assertClose(estWithStale.temperatureC, estWithoutStale.temperatureC, 'estimate.stale.temperatureC');

// QNH vs station pressure at a 400 m target.
const qnhs = { LFQQ: 1018.3, EBOS: 1018.6, LFAQ: 1018.5, LFAC: 1018.1, EHFS: 1018.0 };
const qnhObs = Object.fromEntries(
  Object.entries(qnhs).map(([icao, qnh]) => [icao, obsRow(14.0, 11.0, 5.0, 220.0, qnh, 10)]),
);
const qnhEst = wasm.estimate(lilleSel, qnhObs, 400.0, NOW_MILLIS);
const meanInputQnh = Object.values(qnhs).reduce((a, b) => a + b, 0) / Object.values(qnhs).length;
if (Math.abs(qnhEst.pressureQnhHpa - meanInputQnh) >= 1.0) {
  console.error(`FAIL estimate.qnh: ${qnhEst.pressureQnhHpa} not near ${meanInputQnh}`);
  failures++;
}
if (!(qnhEst.pressureStationHpa < qnhEst.pressureQnhHpa - 30.0)) {
  console.error(`FAIL estimate.qnh: station pressure ${qnhEst.pressureStationHpa} not well below QNH ${qnhEst.pressureQnhHpa}`);
  failures++;
}

// ---- parseNwsLatest / nwsStationId ----

const kjfkJson = readFileSync(
  path.join(__dirname, '../../weather/tests/fixtures/nws_kjfk.json'),
  'utf8',
);
const kjfkObs = wasm.parseNwsLatest(kjfkJson);
assertClose(kjfkObs.tempC, 17.0, 'nws.kjfk.tempC');
assertClose(kjfkObs.dewpointC, 17.0, 'nws.kjfk.dewpointC');
assertClose(kjfkObs.windMs, 18.504 / 3.6, 'nws.kjfk.windMs');
assertClose(kjfkObs.windDirDeg, 40.0, 'nws.kjfk.windDirDeg');
assertClose(kjfkObs.pressureHpa, 1005.7574, 'nws.kjfk.pressureHpa');
assertClose(kjfkObs.obsTimeMillis, Date.parse('2026-09-27T21:15:00Z'), 'nws.kjfk.obsTimeMillis');

const pancJson = readFileSync(
  path.join(__dirname, '../../weather/tests/fixtures/nws_panc.json'),
  'utf8',
);
const pancObs = wasm.parseNwsLatest(pancJson);
assertClose(pancObs.tempC, 10.0, 'nws.panc.tempC');
assertClose(pancObs.dewpointC, 6.0, 'nws.panc.dewpointC');
assertClose(pancObs.windMs, null, 'nws.panc.windMs');
assertClose(pancObs.windDirDeg, null, 'nws.panc.windDirDeg');
assertClose(pancObs.pressureHpa, 990.8573, 'nws.panc.pressureHpa');

assertClose(wasm.nwsStationId('JFK'), 'KJFK', 'nwsStationId.JFK');
assertClose(wasm.nwsStationId('00U'), 'K00U', 'nwsStationId.00U');
assertClose(wasm.nwsStationId('PANC'), 'PANC', 'nwsStationId.PANC');

if (failures > 0) {
  console.error(`\n${failures} parity check(s) failed`);
  process.exit(1);
}
console.log('all wasm parity checks passed');
