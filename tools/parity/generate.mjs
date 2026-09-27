// generate.mjs — runs the trucs.ai knn-weather JS modules once, on hand-made
// inputs, and writes the outputs as JSON fixtures for the Rust parity tests
// (weather/tests/parity.rs) to check against.
//
// Usage: node tools/parity/generate.mjs <path-to-knn-weather-dir>
//
// <path-to-knn-weather-dir> must contain knn.js, idw.js, corrections.js,
// physics.js. No network access is made: fetchIem's `fetch` is stubbed with
// canned response bodies below.

import { pathToFileURL } from 'node:url';
import { writeFileSync, mkdirSync, copyFileSync, mkdtempSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import os from 'node:os';
import path from 'node:path';

const srcDir = process.argv[2];
if (!srcDir) {
  console.error('usage: node generate.mjs <path-to-knn-weather-dir>');
  process.exit(1);
}

// The source files are plain .js with no package.json "type": "module" in
// their own tree, so Node's loader would treat them as CommonJS. Copy them
// (read-only source, never modified) into a scratch directory that declares
// itself an ES module package, and import from there instead.
const scratchDir = mkdtempSync(path.join(os.tmpdir(), 'weather-parity-'));
writeFileSync(path.join(scratchDir, 'package.json'), JSON.stringify({ type: 'module' }));
for (const file of ['idw.js', 'physics.js', 'corrections.js', 'sources.js']) {
  copyFileSync(path.join(srcDir, file), path.join(scratchDir, file));
}

function importFrom(file) {
  return import(pathToFileURL(path.join(scratchDir, file)).href);
}

const { idw, idwCircularDeg } = await importFrom('idw.js');
const {
  leastSquaresSlope,
  fitLapseRateKPerKm,
  reduceTempToElevation,
  vaporPressureHpa,
  dewpointFromVaporPressureHpa,
  stationPressureHpa,
  windComponents,
  windFromComponents,
  stdDev,
  idwAverage,
} = await importFrom('physics.js');
const { computeCorrections, MAX_AGE_MIN } = await importFrom('corrections.js');

const fixtures = {};

// ---- idw.js ----
fixtures.idw = {
  parisFive: idw([
    { value: 14.2, distance: 3.1 },
    { value: 13.8, distance: 7.4 },
    { value: 14.5, distance: 9.0 },
    { value: 13.1, distance: 12.6 },
    { value: 12.0, distance: 41.3 },
  ]),
  zeroDistance: idw([
    { value: 20.0, distance: 0 },
    { value: 10.0, distance: 5.0 },
    { value: 5.0, distance: 12.0 },
  ]),
  empty: idw([]),
};

fixtures.idwCircularDeg = {
  parisFive: idwCircularDeg([
    { value: 250, distance: 3.1 },
    { value: 260, distance: 7.4 },
    { value: 10, distance: 9.0 },
    { value: 240, distance: 12.6 },
    { value: 255, distance: 41.3 },
  ]),
  zeroDistance: idwCircularDeg([
    { value: 90, distance: 0 },
    { value: 270, distance: 5.0 },
  ]),
};

// ---- physics.js ----
fixtures.physics = {
  leastSquaresSlope: leastSquaresSlope([65, 89, 119, 164, 108], [14.6, 14.4, 14.2, 13.8, 14.0]),
  fitLapseRateNormal: fitLapseRateKPerKm([
    { elevM: 235, tempC: 18.4 },
    { elevM: 384, tempC: 17.1 },
    { elevM: 821, tempC: 13.9 },
    { elevM: 2004, tempC: 4.2 },
  ]),
  fitLapseRateTooFew: fitLapseRateKPerKm([
    { elevM: 235, tempC: 18.4 },
    { elevM: 384, tempC: 17.1 },
  ]),
  fitLapseRateImplausible: fitLapseRateKPerKm([
    { elevM: 100, tempC: 20.0 },
    { elevM: 110, tempC: 19.8 },
    { elevM: 120, tempC: 10.0 },
  ]),
  reduceTempToElevation: reduceTempToElevation(14.2, 119, 2004, -6.5),
  vaporPressureHpa: vaporPressureHpa(11.4),
  dewpointFromVaporPressureHpa: dewpointFromVaporPressureHpa(vaporPressureHpa(11.4)),
  stationPressureHpa: stationPressureHpa(1018.3, 2004),
  windComponents: windComponents(5.6, 230),
  windFromComponentsRoundTrip: windFromComponents(
    windComponents(5.6, 230).u,
    windComponents(5.6, 230).v,
  ),
  stdDev: stdDev([14.2, 13.8, 14.5, 13.1, 12.0]),
  stdDevEmpty: stdDev([]),
  idwAverage: idwAverage(
    [{ distance: 3.1 }, { distance: 7.4 }, { distance: 9.0 }],
    [14.2, 13.8, 14.5],
  ),
};

// ---- corrections.js — Paris five ----
// Real METAR stations around Paris (approximate published coordinates).
const now = new Date('2026-09-27T12:00:00Z');
function obsAt(icao, tempC, dewpointC, windMs, windDirDeg, pressureHpa, minutesAgo) {
  const t = new Date(now.getTime() - minutesAgo * 60000);
  return {
    icao,
    obs: {
      tempC,
      dewpointC,
      windMs,
      windDirDeg,
      pressureHpa,
      obsTime: t,
      source: 'IEM',
    },
  };
}

const parisRows = [
  { icao: 'LFPG', distance: 12.9, elev: 119, ...obsAt('LFPG', 14.2, 11.4, 5.6, 230, 1018.3, 10) },
  { icao: 'LFPO', distance: 8.1, elev: 89, ...obsAt('LFPO', 14.6, 11.8, 4.9, 220, 1018.6, 15) },
  { icao: 'LFPB', distance: 9.7, elev: 65, ...obsAt('LFPB', 14.8, 11.9, 5.2, 225, 1018.5, 20) },
  { icao: 'LFPN', distance: 15.4, elev: 164, ...obsAt('LFPN', 13.9, 11.1, 4.5, 215, 1018.1, 25) },
  { icao: 'LFOB', distance: 68.2, elev: 108, ...obsAt('LFOB', 13.5, 10.8, 6.1, 240, 1017.9, 30) },
];
fixtures.correctionsParisFive = computeCorrections(parisRows, 35, now);

// ---- corrections.js — mountain case (Alps, wide elevation spread) ----
const mountainRows = [
  { icao: 'LFLB', distance: 24.5, elev: 235, ...obsAt('LFLB', 18.4, 12.0, 2.1, 180, 1015.2, 5) },
  { icao: 'LFLS', distance: 38.1, elev: 384, ...obsAt('LFLS', 17.1, 11.5, 1.8, 190, 1014.8, 5) },
  { icao: 'LFLL', distance: 71.3, elev: 821, ...obsAt('LFLL', 13.9, 9.2, 3.4, 160, 1013.9, 5) },
  { icao: 'LFLJ', distance: 0, elev: 2004, ...obsAt('LFLJ', 4.2, -1.5, 6.7, 300, 1011.0, 5) },
];
fixtures.correctionsMountain = computeCorrections(mountainRows, 2004, now);

// ---- corrections.js — stale observation excluded ----
const staleRows = [
  { icao: 'LFPG', distance: 12.9, elev: 119, ...obsAt('LFPG', 14.2, 11.4, 5.6, 230, 1018.3, 10) },
  { icao: 'LFPO', distance: 8.1, elev: 89, ...obsAt('LFPO', 14.6, 11.8, 4.9, 220, 1018.6, 150) },
];
fixtures.correctionsStale = computeCorrections(staleRows, 35, now);

fixtures.maxAgeMin = MAX_AGE_MIN;

// ---- sources.js — fetchIem parsing, network stubbed ----
const iemBody = {
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
    {
      // no usable fields beyond station: must be dropped.
      station: 'XXXX',
      utc_valid: '2026-09-27T11:45Z',
    },
  ],
};
globalThis.fetch = async () => ({
  ok: true,
  json: async () => iemBody,
});
const { fetchIem } = await importFrom('sources.js');
const iemResult = await fetchIem(['LFPG', 'LFPO', 'XXXX']);
fixtures.fetchIem = Object.fromEntries(
  [...iemResult.entries()].map(([k, v]) => [
    k,
    { ...v, obsTime: v.obsTime ? v.obsTime.toISOString() : null },
  ]),
);

// ---- write ----
const outDir = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'weather', 'tests', 'fixtures');
mkdirSync(outDir, { recursive: true });
writeFileSync(path.join(outDir, 'parity.json'), JSON.stringify(fixtures, null, 2) + '\n');
console.log(`wrote ${path.join(outDir, 'parity.json')}`);
