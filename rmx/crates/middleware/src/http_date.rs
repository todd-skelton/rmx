//! IMF-fixdate formatting for `Last-Modified` and `Date`.

use std::time::{SystemTime, UNIX_EPOCH};

/// Format a time as `Sun, 06 Nov 1994 08:49:37 GMT`.
pub fn format(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    let weekday = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    let mon = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(month - 1) as usize];
    format!("{weekday}, {d:02} {mon} {year} {h:02}:{m:02}:{s:02} GMT")
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn formats_known_dates() {
        assert_eq!(super::format(UNIX_EPOCH + Duration::from_secs(784_111_777)), "Sun, 06 Nov 1994 08:49:37 GMT");
        assert_eq!(super::format(UNIX_EPOCH), "Thu, 01 Jan 1970 00:00:00 GMT");
    }
}
