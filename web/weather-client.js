// weather-client.js - thin browser client over the weather-wasm package.
//
// All computation (nearest-station search, inverse-distance weighting,
// physics corrections) runs in the `weather` Rust crate compiled to
// WebAssembly. This file only does what has to happen in JS: fetching the
// station list and live observations, and calling the wasm exports with the
// parsed results.
//
// Bump the ?v= tag below whenever weather-wasm/pkg is rebuilt, in the same
// commit (import specifiers must be a static string literal, not a
// template expression).
import init, { Stations, estimate, estimateWithParams, parseIemCurrents, parseNwsLatest, nwsStationId, maxAgeMin } from './pkg/weather_wasm.js?v=9f05f31';

// stations_all.json carries every roster station, reporting or not (see
// idle-intelligence/metar-stations on Hugging Face); stations.json (active
// only) still works with the same parser for a caller who only wants
// reporting stations.
export const STATIONS_URL_HF = 'https://huggingface.co/datasets/idle-intelligence/metar-stations/resolve/main/stations_all.json';
export const STATIONS_URL_LOCAL = './data/stations_all.json';

let initPromise = null;
function ensureInit() {
  if (!initPromise) initPromise = init();
  return initPromise;
}

// Loads and parses the station list. `local`: true reads the gitignored
// scratch copy at STATIONS_URL_LOCAL instead of the published HF dataset.
export async function loadStations(local) {
  await ensureInit();
  const url = local ? STATIONS_URL_LOCAL : STATIONS_URL_HF;
  const res = await fetch(url);
  if (!res.ok) throw new Error(`stations.json ${res.status}`);
  const text = await res.text();
  return new Stations(text);
}

// Iowa Environmental Mesonet currents.json - global METAR observations, one
// call for any number of stations. Parsing happens in the wasm package.
async function fetchIem(icaos) {
  const url = new URL('https://mesonet.agron.iastate.edu/api/1/currents.json');
  for (const icao of icaos) url.searchParams.append('station', icao);
  const res = await fetch(url);
  if (!res.ok) throw new Error(`IEM ${res.status}`);
  const text = await res.text();
  return parseIemCurrents(text);
}

// api.weather.gov - US stations only. Id mapping and body parsing happen in
// the wasm package.
async function fetchNws(icao) {
  const res = await fetch(`https://api.weather.gov/stations/${nwsStationId(icao)}/observations/latest`);
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`NWS ${res.status}`);
  const text = await res.text();
  return parseNwsLatest(text);
}

// Fetches observations for a list of station rows (as returned by
// `Stations.select()`/`Stations.selectWithParams().stations`, each with
// `.station.icao`/`.station.country`) and merges them NWS-first, then IEM,
// per station. Returns a plain object keyed by ICAO id, the shape
// `estimate()`/`estimateWithParams()` expects. The caller decides which
// rows to pass in; the page only fetches for stations flagged `active`.
export async function fetchObservations(neighbors) {
  const icaos = neighbors.map((n) => n.station.icao);
  let iemMap = {};
  try {
    iemMap = await fetchIem(icaos);
  } catch (err) {
    console.log(`IEM fetch failed: ${err.message}`);
  }
  const usIcaos = neighbors.filter((n) => n.station.country === 'US').map((n) => n.station.icao);
  const nwsMap = {};
  if (usIcaos.length > 0) {
    const results = await Promise.allSettled(usIcaos.map(fetchNws));
    results.forEach((r, i) => {
      if (r.status === 'fulfilled' && r.value) nwsMap[usIcaos[i]] = r.value;
    });
  }
  const merged = {};
  for (const icao of icaos) merged[icao] = nwsMap[icao] ?? iemMap[icao] ?? null;
  return merged;
}

// Open-Meteo forecast endpoint, used only for the `elevation` field it
// returns for the queried point (same call sources.js's fetchOpenMeteoPoint
// makes; CORS verified for this endpoint).
export async function fetchElevation(lat, lon) {
  const url = new URL('https://api.open-meteo.com/v1/forecast');
  url.searchParams.set('latitude', lat);
  url.searchParams.set('longitude', lon);
  url.searchParams.set('current', 'temperature_2m');
  const res = await fetch(url);
  if (!res.ok) throw new Error(`Open-Meteo ${res.status}`);
  const body = await res.json();
  return typeof body.elevation === 'number' ? body.elevation : undefined;
}

export { estimate, estimateWithParams, maxAgeMin };
