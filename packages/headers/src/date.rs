//! HTTP dates (IMF-fixdate): `Sun, 06 Nov 1994 08:49:37 GMT`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Format a time, truncated to whole seconds.
pub fn format_http_date(t: SystemTime) -> String {
    let secs = match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs_f64().ceil() as i64),
    };
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (year, month, day) = civil_from_days(days);
    let weekday = DAYS[days.rem_euclid(7) as usize];
    let mon = MONTHS[(month - 1) as usize];
    format!("{weekday}, {day:02} {mon} {year} {h:02}:{m:02}:{s:02} GMT")
}

/// Parse an IMF-fixdate; other formats (RFC 850, asctime) are rejected, like Remix.
pub fn parse_http_date(value: &str) -> Option<SystemTime> {
    let b = value.as_bytes();
    if b.len() != 29 || &value[3..5] != ", " || &value[25..] != " GMT" || b[7] != b' ' || b[11] != b' ' || b[16] != b' ' || b[19] != b':' || b[22] != b':' {
        return None;
    }
    let weekday = DAYS.iter().position(|d| *d == &value[..3])?;
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let s = &value[r];
        s.bytes().all(|c| c.is_ascii_digit()).then(|| s.parse().ok()).flatten()
    };
    let day = num(5..7)?;
    let month = MONTHS.iter().position(|m| *m == &value[8..11])? as i64 + 1;
    let year = num(12..16)?;
    let (h, m, s) = (num(17..19)?, num(20..22)?, num(23..25)?);
    if !(1..=31).contains(&day) || h > 23 || m > 59 || s > 60 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    if civil_from_days(days) != (year, month, day) || days.rem_euclid(7) as usize != weekday {
        return None;
    }
    let secs = days * 86_400 + h * 3600 + m * 60 + s;
    Some(if secs >= 0 { UNIX_EPOCH + Duration::from_secs(secs as u64) } else { UNIX_EPOCH - Duration::from_secs((-secs) as u64) })
}

/// A moment as UTC calendar fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UtcDateTime {
    pub year: i64,
    /// 1 to 12.
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub millisecond: u32,
}

impl UtcDateTime {
    pub fn new(t: SystemTime) -> UtcDateTime {
        let millis = match t.duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_millis() as i64,
            Err(e) => -(e.duration().as_millis() as i64),
        };
        let days = millis.div_euclid(86_400_000);
        let rem = millis.rem_euclid(86_400_000);
        let (year, month, day) = civil_from_days(days);
        UtcDateTime {
            year,
            month: month as u32,
            day: day as u32,
            hour: (rem / 3_600_000) as u32,
            minute: (rem % 3_600_000 / 60_000) as u32,
            second: (rem % 60_000 / 1000) as u32,
            millisecond: (rem % 1000) as u32,
        }
    }

    /// `Jan` to `Dec`.
    pub fn month_name(&self) -> &'static str {
        MONTHS[(self.month - 1) as usize]
    }

    /// `Date.prototype.toISOString()`: `2025-09-23T18:34:12.345Z`.
    pub fn to_iso_string(&self) -> String {
        let UtcDateTime { year, month, day, hour, minute, second, millisecond } = *self;
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millisecond:03}Z")
    }
}

/// Parse `Date.prototype.toISOString()` output (`2025-09-23T18:34:12.345Z`); the
/// milliseconds are optional.
pub fn parse_iso_date(value: &str) -> Option<SystemTime> {
    let b = value.as_bytes();
    let with_millis = match b.len() {
        20 => false,
        24 if b[19] == b'.' => true,
        _ => return None,
    };
    if b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[b.len() - 1] != b'Z' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let s = &value[r];
        s.bytes().all(|c| c.is_ascii_digit()).then(|| s.parse().ok()).flatten()
    };
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, m, s) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let ms = if with_millis { num(20..23)? } else { 0 };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || h > 23 || m > 59 || s > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    if civil_from_days(days) != (year, month, day) {
        return None;
    }
    let millis = (days * 86_400 + h * 3600 + m * 60 + s) * 1000 + ms;
    Some(if millis >= 0 { UNIX_EPOCH + Duration::from_millis(millis as u64) } else { UNIX_EPOCH - Duration::from_millis((-millis) as u64) })
}

/// Seconds since the epoch, for comparing dates at HTTP precision.
pub fn unix_seconds(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

// Howard Hinnant's civil calendar algorithms.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_dates_round_trip() {
        let t = UNIX_EPOCH + Duration::from_millis(1_758_652_452_345);
        assert_eq!(UtcDateTime::new(t).to_iso_string(), "2025-09-23T18:34:12.345Z");
        assert_eq!(parse_iso_date("2025-09-23T18:34:12.345Z"), Some(t));
        assert_eq!(parse_iso_date("2025-09-23T18:34:12Z"), Some(UNIX_EPOCH + Duration::from_secs(1_758_652_452)));
        assert_eq!(parse_iso_date("1969-12-31T23:59:59.999Z"), Some(UNIX_EPOCH - Duration::from_millis(1)));
        for bad in ["2025-02-30T00:00:00Z", "2025-13-01T00:00:00Z", "2025-09-23 18:34:12Z", "2025-09-23T18:34:12.345", "2025-09-23T24:00:00Z", "2025-09-23T18:34:12.3Z", ""] {
            assert_eq!(parse_iso_date(bad), None, "{bad}");
        }
    }

    #[test]
    fn format_and_parse() {
        let t = UNIX_EPOCH + Duration::from_secs(784_111_777);
        assert_eq!(format_http_date(t), "Sun, 06 Nov 1994 08:49:37 GMT");
        assert_eq!(parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT"), Some(t));
        assert_eq!(format_http_date(UNIX_EPOCH), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(parse_http_date("Thu, 29 Feb 2024 12:00:00 GMT").map(format_http_date).as_deref(), Some("Thu, 29 Feb 2024 12:00:00 GMT"));
        assert_eq!(parse_http_date("Mon, 06 Nov 1994 08:49:37 GMT"), None, "wrong weekday");
        assert_eq!(parse_http_date("Sunday, 06-Nov-94 08:49:37 GMT"), None);
        assert_eq!(parse_http_date("Wed, 30 Feb 2022 00:00:00 GMT"), None);
    }

    #[test]
    fn utc_fields_and_iso_strings() {
        let t = UNIX_EPOCH + Duration::from_millis(784_111_777_123);
        let utc = UtcDateTime::new(t);
        assert_eq!((utc.year, utc.month, utc.day, utc.hour, utc.minute, utc.second, utc.millisecond), (1994, 11, 6, 8, 49, 37, 123));
        assert_eq!((utc.month_name(), utc.to_iso_string().as_str()), ("Nov", "1994-11-06T08:49:37.123Z"));
        assert_eq!(UtcDateTime::new(UNIX_EPOCH - Duration::from_millis(1)).to_iso_string(), "1969-12-31T23:59:59.999Z");
    }
}
