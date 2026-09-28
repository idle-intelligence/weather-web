use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;
use weather::corrections::{compute_corrections, NeighborRow, MAX_AGE_MIN};
use weather::observation::{parse_iem_currents, Observation};
use weather::stations::{load_stations, nearest, nearest_within, DEFAULT_K, MAX_RADIUS_KM};

const IEM_CURRENTS_URL: &str = "https://mesonet.agron.iastate.edu/api/1/currents.json";
const MIN_REQUEST_GAP: Duration = Duration::from_secs(3);

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    match command.as_str() {
        "estimate" => run_estimate(args),
        "snapshot" => run_snapshot(args),
        "validate" => run_validate(args),
        _ => {
            eprintln!("usage: weather-cli <estimate|snapshot|validate> [options]");
            eprintln!("  estimate --lat LAT --lon LON --stations PATH [--k N] [--elev METERS]");
            eprintln!("  snapshot --stations PATH --out FILE");
            eprintln!("  validate --snapshot FILE --stations PATH --out CSV");
            std::process::exit(2);
        }
    }
}

fn parse_flags(args: impl Iterator<Item = String>) -> HashMap<String, String> {
    let mut flags = HashMap::new();
    let args: Vec<String> = args.collect();
    let mut i = 0;
    while i < args.len() {
        if let Some(key) = args[i].strip_prefix("--") {
            let value = args.get(i + 1).cloned().unwrap_or_default();
            flags.insert(key.to_string(), value);
            i += 2;
        } else {
            i += 1;
        }
    }
    flags
}

fn run_estimate(args: impl Iterator<Item = String>) -> Result<()> {
    let flags = parse_flags(args);
    let lat: f64 = flags
        .get("lat")
        .context("--lat is required")?
        .parse()
        .context("--lat must be a number")?;
    let lon: f64 = flags
        .get("lon")
        .context("--lon is required")?
        .parse()
        .context("--lon must be a number")?;
    let stations_path = flags.get("stations").context("--stations is required")?;
    let k: usize = flags
        .get("k")
        .map(|s| s.parse())
        .transpose()
        .context("--k must be an integer")?
        .unwrap_or(weather::stations::DEFAULT_K);
    let target_elev_m: Option<f64> = flags
        .get("elev")
        .map(|s| s.parse())
        .transpose()
        .context("--elev must be a number")?;

    let stations = load_stations(stations_path)?;
    let neighbors = nearest(&stations, lat, lon, k);
    if neighbors.is_empty() {
        bail!("no stations found in {stations_path}");
    }

    let icaos: Vec<String> = neighbors.iter().map(|n| n.station.icao.clone()).collect();
    let observations = fetch_iem_currents(&icaos)?;

    let now_millis = current_time_millis();
    let rows: Vec<NeighborRow> = neighbors
        .iter()
        .map(|n| NeighborRow {
            icao: n.station.icao.clone(),
            distance: n.distance,
            elev_m: Some(n.station.elev_m),
            obs: observations.get(&n.station.icao).cloned(),
        })
        .collect();

    let result = compute_corrections(&rows, target_elev_m, now_millis);

    println!("estimate for ({lat:.4}, {lon:.4}), {} neighbours:", rows.len());
    for row in &rows {
        let status = match &row.obs {
            Some(_) => "ok",
            None => "no observation",
        };
        println!(
            "  {:<6} {:>7.1} km  elev {:>6.0} m  [{status}]",
            row.icao, row.distance, row.elev_m.unwrap_or(f64::NAN)
        );
    }

    println!();
    println!("temperature (plain IDW):    {:>8.2} C", opt(result.temperature.plain));
    println!("temperature (corrected):    {:>8.2} C  lapse {:.2} K/km{}",
        opt(result.temperature.corrected),
        result.temperature.lapse.lapse_k_per_km,
        if result.temperature.lapse.fallback { " (fallback)" } else { "" });
    println!("dew point (plain IDW):      {:>8.2} C", opt(result.dewpoint.plain));
    println!("dew point (corrected):      {:>8.2} C", opt(result.dewpoint.corrected));
    println!("pressure QNH (corrected):   {:>8.2} hPa", opt(result.pressure.corrected_qnh));
    println!("pressure station (corr.):   {:>8.2} hPa", opt(result.pressure.corrected_station));
    println!("wind speed (corrected):     {:>8.2} m/s", opt(result.wind.corrected_speed));
    println!("wind dir (corrected):       {:>8.1} deg", opt(result.wind.corrected_dir));

    Ok(())
}

fn opt(v: Option<f64>) -> f64 {
    v.unwrap_or(f64::NAN)
}

fn current_time_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Fetches IEM currents.json for the given ICAO codes, one request for all
/// of them (IEM's `currents.json` batches via repeated `station=` params).
fn fetch_iem_currents(icaos: &[String]) -> Result<HashMap<String, Observation>> {
    let mut request = ureq::get(IEM_CURRENTS_URL);
    for icao in icaos {
        request = request.query("station", icao);
    }
    let response = request
        .call()
        .with_context(|| format!("fetching {IEM_CURRENTS_URL}"))?;
    let body: Value = response.into_json().context("parsing IEM response as JSON")?;
    Ok(parse_iem_currents(&body))
}

#[derive(Debug, Serialize, Deserialize)]
struct Snapshot {
    fetched_at_millis: i64,
    observations: HashMap<String, Observation>,
}

/// IEM rejects a `currents.json` GET whose query string is too long (HTTP
/// 414) somewhere between 8,000 and 9,000 characters; this stays well under
/// that so a full station list still fits in a handful of requests instead
/// of one per station.
const MAX_QUERY_CHARS: usize = 7500;

/// Groups icaos into batches whose `station=...&station=...` query string
/// stays under `MAX_QUERY_CHARS`, so `snapshot` makes a handful of IEM
/// requests instead of one per station.
fn chunk_icaos(icaos: &[String]) -> Vec<Vec<String>> {
    let mut chunks = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_len = 0usize;
    for icao in icaos {
        let part_len = "station=".len() + icao.len();
        let would_be = current_len + if current.is_empty() { 0 } else { 1 } + part_len;
        if would_be > MAX_QUERY_CHARS && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            current_len = 0;
        }
        current_len += if current.is_empty() { 0 } else { 1 } + part_len;
        current.push(icao.clone());
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn run_snapshot(args: impl Iterator<Item = String>) -> Result<()> {
    let flags = parse_flags(args);
    let stations_path = flags.get("stations").context("--stations is required")?;
    let out_path = flags.get("out").context("--out is required")?;

    let stations = load_stations(stations_path)?;
    let icaos: Vec<String> = stations.iter().map(|s| s.icao.clone()).collect();
    let chunks = chunk_icaos(&icaos);

    let mut observations = HashMap::new();
    let mut last_request = None;

    for (i, chunk) in chunks.iter().enumerate() {
        if let Some(last) = last_request {
            let elapsed: Duration = std::time::Instant::now().duration_since(last);
            if elapsed < MIN_REQUEST_GAP {
                std::thread::sleep(MIN_REQUEST_GAP - elapsed);
            }
        }

        let mut backoff = Duration::from_secs(5);
        loop {
            last_request = Some(std::time::Instant::now());
            let mut request = ureq::get(IEM_CURRENTS_URL);
            for icao in chunk {
                request = request.query("station", icao);
            }
            match request.call() {
                Ok(response) => {
                    let body: Value = response
                        .into_json()
                        .with_context(|| format!("parsing IEM response for batch {}", i + 1))?;
                    let obs = parse_iem_currents(&body);
                    eprintln!(
                        "batch {}/{}: {} stations, {} observations",
                        i + 1,
                        chunks.len(),
                        chunk.len(),
                        obs.len()
                    );
                    observations.extend(obs);
                    break;
                }
                Err(ureq::Error::Status(429, _)) => {
                    eprintln!("429 for batch {}, backing off {:?}", i + 1, backoff);
                    std::thread::sleep(backoff);
                    backoff *= 2;
                }
                Err(e) => bail!("fetching batch {}: {e}", i + 1),
            }
        }
    }

    let snapshot = Snapshot {
        fetched_at_millis: current_time_millis(),
        observations,
    };
    let mut file = std::fs::File::create(out_path)
        .with_context(|| format!("creating {out_path}"))?;
    file.write_all(serde_json::to_string_pretty(&snapshot)?.as_bytes())?;
    println!("wrote {} observations to {out_path}", snapshot.observations.len());
    Ok(())
}

#[derive(Debug, Clone, Copy, Default)]
struct VarPair {
    observed: Option<f64>,
    estimated: Option<f64>,
    error: Option<f64>,
}

fn pair(observed: Option<f64>, estimated: Option<f64>) -> VarPair {
    let error = match (observed, estimated) {
        (Some(o), Some(e)) => Some(e - o),
        _ => None,
    };
    VarPair {
        observed,
        estimated,
        error,
    }
}

/// Shortest signed angular difference (deg), estimated - observed, in [-180, 180].
fn circular_error_deg(observed: f64, estimated: f64) -> f64 {
    let raw = (estimated - observed) % 360.0;
    if raw < -180.0 {
        raw + 360.0
    } else if raw > 180.0 {
        raw - 360.0
    } else {
        raw
    }
}

fn pair_circular(observed: Option<f64>, estimated: Option<f64>) -> VarPair {
    match (observed, estimated) {
        (Some(o), Some(e)) => VarPair {
            observed: Some(o),
            estimated: Some(e),
            error: Some(circular_error_deg(o, e)),
        },
        _ => VarPair {
            observed,
            estimated,
            error: None,
        },
    }
}

struct ValidationRow {
    icao: String,
    lat: f64,
    lon: f64,
    elev_m: f64,
    n_neighbours: usize,
    nearest_dist_km: f64,
    nearest_elev_diff_m: f64,
    temp: VarPair,
    dewpoint: VarPair,
    pressure: VarPair,
    wind_speed: VarPair,
    wind_dir: VarPair,
}

fn fmt_opt(v: Option<f64>) -> String {
    match v {
        Some(v) => format!("{v:.4}"),
        None => String::new(),
    }
}

fn write_csv(path: &str, rows: &[ValidationRow]) -> Result<()> {
    let mut file = std::fs::File::create(path).with_context(|| format!("creating {path}"))?;
    writeln!(
        file,
        "icao,lat,lon,elev_m,n_neighbours,nearest_dist_km,nearest_elev_diff_m,\
temp_observed,temp_estimated,temp_error,\
dewpoint_observed,dewpoint_estimated,dewpoint_error,\
pressure_observed,pressure_estimated,pressure_error,\
wind_speed_observed,wind_speed_estimated,wind_speed_error,\
wind_dir_observed,wind_dir_estimated,wind_dir_error"
    )?;
    for r in rows {
        writeln!(
            file,
            "{},{:.4},{:.4},{:.1},{},{:.2},{:.1},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            r.icao,
            r.lat,
            r.lon,
            r.elev_m,
            r.n_neighbours,
            r.nearest_dist_km,
            r.nearest_elev_diff_m,
            fmt_opt(r.temp.observed),
            fmt_opt(r.temp.estimated),
            fmt_opt(r.temp.error),
            fmt_opt(r.dewpoint.observed),
            fmt_opt(r.dewpoint.estimated),
            fmt_opt(r.dewpoint.error),
            fmt_opt(r.pressure.observed),
            fmt_opt(r.pressure.estimated),
            fmt_opt(r.pressure.error),
            fmt_opt(r.wind_speed.observed),
            fmt_opt(r.wind_speed.estimated),
            fmt_opt(r.wind_speed.error),
            fmt_opt(r.wind_dir.observed),
            fmt_opt(r.wind_dir.estimated),
            fmt_opt(r.wind_dir.error),
        )?;
    }
    Ok(())
}

struct ErrorStats {
    count: usize,
    mae: f64,
    rmse: f64,
    bias: f64,
    median_abs: f64,
    p90_abs: f64,
}

fn percentile(sorted_abs: &[f64], p: f64) -> f64 {
    let n = sorted_abs.len();
    if n == 0 {
        return f64::NAN;
    }
    let idx = ((p * n as f64).ceil() as usize).saturating_sub(1).min(n - 1);
    sorted_abs[idx]
}

fn median(sorted_abs: &[f64]) -> f64 {
    let n = sorted_abs.len();
    if n == 0 {
        return f64::NAN;
    }
    if n % 2 == 1 {
        sorted_abs[n / 2]
    } else {
        (sorted_abs[n / 2 - 1] + sorted_abs[n / 2]) / 2.0
    }
}

fn error_stats(errors: &[f64]) -> ErrorStats {
    let n = errors.len();
    let mut abs_sorted: Vec<f64> = errors.iter().map(|e| e.abs()).collect();
    abs_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let bias = errors.iter().sum::<f64>() / n as f64;
    let mae = abs_sorted.iter().sum::<f64>() / n as f64;
    let rmse = (errors.iter().map(|e| e * e).sum::<f64>() / n as f64).sqrt();
    ErrorStats {
        count: n,
        mae,
        rmse,
        bias,
        median_abs: median(&abs_sorted),
        p90_abs: percentile(&abs_sorted, 0.90),
    }
}

fn print_stats(label: &str, errors: &[f64]) {
    if errors.is_empty() {
        println!("  {label}: n=0");
        return;
    }
    let s = error_stats(errors);
    println!(
        "  {label}: n={:<5} MAE={:>6.2}  RMSE={:>6.2}  bias={:>6.2}  median|e|={:>6.2}  p90|e|={:>6.2}",
        s.count, s.mae, s.rmse, s.bias, s.median_abs, s.p90_abs
    );
}

fn run_validate(args: impl Iterator<Item = String>) -> Result<()> {
    let flags = parse_flags(args);
    let stations_path = flags.get("stations").context("--stations is required")?;
    let snapshot_path = flags.get("snapshot").context("--snapshot is required")?;
    let out_path = flags.get("out").context("--out is required")?;

    let stations = load_stations(stations_path)?;
    let snapshot_text = std::fs::read_to_string(snapshot_path)
        .with_context(|| format!("reading {snapshot_path}"))?;
    let snapshot: Snapshot = serde_json::from_str(&snapshot_text)
        .with_context(|| format!("parsing {snapshot_path}"))?;
    let now_millis = snapshot.fetched_at_millis;

    let mut rows_out: Vec<ValidationRow> = Vec::new();
    let mut skipped_no_neighbour = 0usize;

    for station in &stations {
        let Some(obs) = snapshot.observations.get(&station.icao) else {
            continue;
        };
        let age = match obs.obs_time_millis {
            Some(t) => (now_millis - t) as f64 / 60000.0,
            None => continue,
        };
        if age > MAX_AGE_MIN {
            continue;
        }

        let neighbours = nearest_within(
            &stations,
            station.lat,
            station.lon,
            Some(DEFAULT_K),
            MAX_RADIUS_KM,
            Some(&station.icao),
        );
        if neighbours.is_empty() {
            skipped_no_neighbour += 1;
            continue;
        }

        let rows: Vec<NeighborRow> = neighbours
            .iter()
            .map(|n| NeighborRow {
                icao: n.station.icao.clone(),
                distance: n.distance,
                elev_m: Some(n.station.elev_m),
                obs: snapshot.observations.get(&n.station.icao).cloned(),
            })
            .collect();
        let result = compute_corrections(&rows, Some(station.elev_m), now_millis);

        rows_out.push(ValidationRow {
            icao: station.icao.clone(),
            lat: station.lat,
            lon: station.lon,
            elev_m: station.elev_m,
            n_neighbours: neighbours.len(),
            nearest_dist_km: neighbours[0].distance,
            nearest_elev_diff_m: (station.elev_m - neighbours[0].station.elev_m).abs(),
            temp: pair(
                obs.temp_c,
                result.temperature.corrected.or(result.temperature.plain),
            ),
            dewpoint: pair(
                obs.dewpoint_c,
                result.dewpoint.corrected.or(result.dewpoint.plain),
            ),
            pressure: pair(
                obs.pressure_hpa,
                result.pressure.corrected_qnh.or(result.pressure.plain),
            ),
            wind_speed: pair(
                obs.wind_ms,
                result.wind.corrected_speed.or(result.wind.plain_scalar_speed),
            ),
            wind_dir: pair_circular(obs.wind_dir_deg, result.wind.corrected_dir),
        });
    }

    write_csv(out_path, &rows_out)?;

    let temp_errors: Vec<f64> = rows_out.iter().filter_map(|r| r.temp.error).collect();

    println!(
        "validated {} stations against {} candidates ({} skipped: no neighbour within {:.0} km); wrote {out_path}",
        rows_out.len(),
        stations.len(),
        skipped_no_neighbour,
        MAX_RADIUS_KM
    );
    println!();
    println!("temperature error (estimated - observed, C):");
    print_stats("overall", &temp_errors);

    let dist_bins: [(&str, f64, f64); 3] = [("0-25 km", 0.0, 25.0), ("25-50 km", 25.0, 50.0), ("50-100 km", 50.0, 100.0)];
    println!("  by nearest-neighbour distance:");
    for (label, lo, hi) in dist_bins {
        let errs: Vec<f64> = rows_out
            .iter()
            .filter(|r| r.nearest_dist_km >= lo && r.nearest_dist_km < hi)
            .filter_map(|r| r.temp.error)
            .collect();
        print_stats(label, &errs);
    }

    let elev_bins: [(&str, f64, f64); 4] = [
        ("0-100 m", 0.0, 100.0),
        ("100-300 m", 100.0, 300.0),
        ("300-1000 m", 300.0, 1000.0),
        (">1000 m", 1000.0, f64::INFINITY),
    ];
    println!("  by |elevation difference| to nearest neighbour:");
    for (label, lo, hi) in elev_bins {
        let errs: Vec<f64> = rows_out
            .iter()
            .filter(|r| r.nearest_elev_diff_m >= lo && r.nearest_elev_diff_m < hi)
            .filter_map(|r| r.temp.error)
            .collect();
        print_stats(label, &errs);
    }

    println!();
    println!("other variables (estimated - observed), overall:");
    print_stats(
        "dew point (C)",
        &rows_out.iter().filter_map(|r| r.dewpoint.error).collect::<Vec<_>>(),
    );
    print_stats(
        "pressure QNH (hPa)",
        &rows_out.iter().filter_map(|r| r.pressure.error).collect::<Vec<_>>(),
    );
    print_stats(
        "wind speed (m/s)",
        &rows_out.iter().filter_map(|r| r.wind_speed.error).collect::<Vec<_>>(),
    );
    print_stats(
        "wind direction (deg)",
        &rows_out.iter().filter_map(|r| r.wind_dir.error).collect::<Vec<_>>(),
    );

    Ok(())
}
