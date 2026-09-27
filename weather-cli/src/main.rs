use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;
use weather::corrections::{compute_corrections, NeighborRow};
use weather::observation::{parse_iem_currents, Observation};
use weather::stations::{load_stations, nearest};

const IEM_CURRENTS_URL: &str = "https://mesonet.agron.iastate.edu/api/1/currents.json";
const MIN_REQUEST_GAP: Duration = Duration::from_secs(3);

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    match command.as_str() {
        "estimate" => run_estimate(args),
        "snapshot" => run_snapshot(args),
        _ => {
            eprintln!("usage: weather-cli <estimate|snapshot> [options]");
            eprintln!("  estimate --lat LAT --lon LON --stations PATH [--k N]");
            eprintln!("  snapshot --stations PATH --out FILE");
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

fn run_snapshot(args: impl Iterator<Item = String>) -> Result<()> {
    let flags = parse_flags(args);
    let stations_path = flags.get("stations").context("--stations is required")?;
    let out_path = flags.get("out").context("--out is required")?;

    let stations = load_stations(stations_path)?;
    let mut observations = HashMap::new();
    let mut last_request = None;

    for station in &stations {
        if let Some(last) = last_request {
            let elapsed: Duration = std::time::Instant::now().duration_since(last);
            if elapsed < MIN_REQUEST_GAP {
                std::thread::sleep(MIN_REQUEST_GAP - elapsed);
            }
        }

        let mut backoff = Duration::from_secs(5);
        loop {
            last_request = Some(std::time::Instant::now());
            let request = ureq::get(IEM_CURRENTS_URL).query("station", &station.icao);
            match request.call() {
                Ok(response) => {
                    let body: Value = response.into_json().with_context(|| {
                        format!("parsing IEM response for {}", station.icao)
                    })?;
                    observations.extend(parse_iem_currents(&body));
                    break;
                }
                Err(ureq::Error::Status(429, _)) => {
                    eprintln!("429 for {}, backing off {:?}", station.icao, backoff);
                    std::thread::sleep(backoff);
                    backoff *= 2;
                }
                Err(e) => bail!("fetching {}: {e}", station.icao),
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
