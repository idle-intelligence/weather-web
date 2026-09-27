//! Minimal UTC timestamp parsing, so the library core has no date/time
//! dependency. Supports the subset of ISO 8601 used by IEM's currents.json
//! (`utc_valid`): "YYYY-MM-DDTHH:MM[:SS[.fff]]Z".

/// Days since the Unix epoch for a given proleptic Gregorian civil date.
/// Howard Hinnant's `days_from_civil` algorithm.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (m as i64 + 9) % 12; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// Parses a timestamp of the form "YYYY-MM-DDTHH:MM[:SS[.fff]](Z|+HH:MM|-HH:MM)"
/// into epoch milliseconds. Returns None if the string does not match.
/// The offset form covers api.weather.gov timestamps
/// ("2026-09-27T21:15:00+00:00"); IEM's currents.json always uses "Z".
pub fn parse_utc_millis(s: &str) -> Option<i64> {
    let s = s.trim();
    let (s, offset_minutes) = match s.strip_suffix('Z') {
        Some(rest) => (rest, 0i64),
        None => {
            let t_pos = s.find('T')?;
            let sign_pos = s[t_pos..].rfind(['+', '-'])? + t_pos;
            let sign: i64 = if s.as_bytes()[sign_pos] == b'+' { 1 } else { -1 };
            let (oh, om) = s[sign_pos + 1..].split_once(':')?;
            let oh: i64 = oh.parse().ok()?;
            let om: i64 = om.parse().ok()?;
            (&s[..sign_pos], sign * (oh * 60 + om))
        }
    };
    let (date, time) = s.split_once('T')?;

    let mut date_parts = date.splitn(3, '-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;

    let mut time_parts = time.split(':');
    let hour: i64 = time_parts.next()?.parse().ok()?;
    let minute: i64 = time_parts.next()?.parse().ok()?;
    let (second, millis) = match time_parts.next() {
        Some(sec_str) => match sec_str.split_once('.') {
            Some((sec, frac)) => {
                let sec: i64 = sec.parse().ok()?;
                let frac_millis: i64 = format!("{:0<3}", frac).get(0..3)?.parse().ok()?;
                (sec, frac_millis)
            }
            None => (sec_str.parse().ok()?, 0),
        },
        None => (0, 0),
    };

    let days = days_from_civil(year, month, day);
    let millis = days * 86_400_000
        + hour * 3_600_000
        + minute * 60_000
        + second * 1_000
        + millis
        - offset_minutes * 60_000;
    Some(millis)
}
