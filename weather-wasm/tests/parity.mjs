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

if (failures > 0) {
  console.error(`\n${failures} parity check(s) failed`);
  process.exit(1);
}
console.log('all wasm parity checks passed');
