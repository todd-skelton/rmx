//! `actions/schedules/ics.ts`: export a schedule as a weekly-recurring iCalendar feed.

use super::data::ScheduleDocument;

const PRODUCT_ID: &str = "-//Timeboxer//Schedule Export//EN";
const DAY_CODES: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
const DAY_MS: i64 = 86_400_000;

/// `createScheduleIcs(schedule, now)`; `now_ms` is a Unix time in milliseconds.
pub fn create_schedule_ics(schedule: &ScheduleDocument, now_ms: i64) -> String {
    let monday = monday_of_week(now_ms);
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".into(),
        format!("PRODID:{PRODUCT_ID}"),
        "CALSCALE:GREGORIAN".into(),
        "METHOD:PUBLISH".into(),
        format!("X-WR-CALNAME:{}", escape_text(&schedule.name)),
    ];
    for block in &schedule.blocks {
        let Some(day_code) = usize::try_from(block.day_of_week).ok().and_then(|d| DAY_CODES.get(d)) else { continue };
        lines.extend([
            "BEGIN:VEVENT".to_string(),
            format!("UID:{}", escape_text(&format!("{}-{}@timeboxer", schedule.id, block.id))),
            format!("DTSTAMP:{}", format_utc(now_ms)),
            format!("SUMMARY:{}", escape_text(&block.name)),
            format!("DTSTART:{}", format_floating(monday, block.day_of_week, block.start_minute)),
            format!("DTEND:{}", format_floating(monday, block.day_of_week, block.end_minute)),
            format!("RRULE:FREQ=WEEKLY;BYDAY={day_code}"),
            "END:VEVENT".into(),
        ]);
    }
    lines.push("END:VCALENDAR".into());
    let mut out = fold_lines(&lines).join("\r\n");
    out.push_str("\r\n");
    out
}

/// UTC midnight of the Monday of `now`'s week, in milliseconds.
fn monday_of_week(now_ms: i64) -> i64 {
    let midnight = now_ms.div_euclid(DAY_MS) * DAY_MS;
    // 1970-01-01 was a Thursday: day 0 is Thursday, so Monday is 3 days later.
    let days_from_monday = (now_ms.div_euclid(DAY_MS) + 3).rem_euclid(7);
    midnight - days_from_monday * DAY_MS
}

fn format_floating(monday: i64, day_of_week: i64, minute_of_day: i64) -> String {
    let (y, m, d) = civil_from_days((monday + day_of_week * DAY_MS).div_euclid(DAY_MS));
    format!("{y:04}{m:02}{d:02}T{:02}{:02}00", minute_of_day / 60, minute_of_day % 60)
}

fn format_utc(ms: i64) -> String {
    let days = ms.div_euclid(DAY_MS);
    let (y, m, d) = civil_from_days(days);
    let secs = ms.rem_euclid(DAY_MS) / 1000;
    format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z", secs / 3600, secs / 60 % 60, secs % 60)
}

/// Days since 1970-01-01 to a (year, month, day) date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn escape_text(value: &str) -> String {
    value.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace("\r\n", "\\n").replace(['\n', '\r'], "\\n")
}

/// Fold at 75 UTF-16 code units, continuing with a space and 74 more (as `String.slice`).
fn fold_lines(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for line in lines {
        let units: Vec<u16> = line.encode_utf16().collect();
        let mut chunks = vec![String::from_utf16_lossy(&units[..units.len().min(75)])];
        let mut rest = &units[units.len().min(75)..];
        while !rest.is_empty() {
            let take = rest.len().min(74);
            chunks.push(format!(" {}", String::from_utf16_lossy(&rest[..take])));
            rest = &rest[take..];
        }
        out.extend(chunks);
    }
    out
}

#[cfg(test)]
mod tests {
    //! `actions/schedules/ics.test.ts`

    use super::*;
    use crate::layout::Block;

    /// 2026-01-08T12:34:56Z
    const FIXED_NOW: i64 = 1_767_875_696_000;

    fn schedule(name: &str, blocks: Vec<Block>) -> ScheduleDocument {
        ScheduleDocument { blocks, id: 42, name: name.into(), revision: 0, updated_at: 0 }
    }

    fn block(id: &str, name: &str, day: i64, start: i64, end: i64) -> Block {
        Block { color: None, day_of_week: day, end_minute: end, id: id.into(), name: name.into(), start_minute: start }
    }

    fn focus() -> Block {
        block("focus", "Focus", 0, 540, 600)
    }

    #[track_caller]
    fn assert_includes(ics: &str, expected: &str) {
        assert!(ics.contains(expected), "expected ICS to include {expected:?} in {ics:?}");
    }

    fn event_fields(summary: &str, start: &str, end: &str, day: &str) -> String {
        format!("SUMMARY:{summary}\r\nDTSTART:{start}\r\nDTEND:{end}\r\nRRULE:FREQ=WEEKLY;BYDAY={day}\r\n")
    }

    #[test]
    fn escapes_text_values_for_calendar_and_event_fields() {
        let ics = create_schedule_ics(
            &schedule("Planning, review; C\\D\nNext\r\nAgain\rDone", vec![Block { name: "Focus, plan; ship\\code\nNext".into(), ..focus() }]),
            FIXED_NOW,
        );
        assert_includes(&ics, "X-WR-CALNAME:Planning\\, review\\; C\\\\D\\nNext\\nAgain\\nDone\r\n");
        assert_includes(&ics, "SUMMARY:Focus\\, plan\\; ship\\\\code\\nNext\r\n");
    }

    #[test]
    fn folds_long_lines() {
        let ics = create_schedule_ics(&schedule("Weekly schedule", vec![Block { name: "A".repeat(100), ..focus() }]), FIXED_NOW);
        let lines: Vec<&str> = ics.split("\r\n").collect();
        assert!(lines.contains(&format!("SUMMARY:{}", "A".repeat(67)).as_str()));
        assert!(lines.contains(&format!(" {}", "A".repeat(33)).as_str()));
        assert!(lines.iter().all(|l| l.len() <= 75));
    }

    #[test]
    fn maps_every_weekday_to_the_matching_byday_code() {
        let names = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
        let blocks = names.iter().enumerate().map(|(i, n)| block(&n.to_lowercase(), n, i as i64, 540, 600)).collect();
        let ics = create_schedule_ics(&schedule("Weekly schedule", blocks), FIXED_NOW);
        for (i, (name, code)) in names.iter().zip(DAY_CODES).enumerate() {
            let date = format!("202601{:02}", 5 + i);
            assert_includes(&ics, &event_fields(name, &format!("{date}T090000"), &format!("{date}T100000"), code));
        }
    }

    #[test]
    fn renders_an_empty_schedule_as_a_calendar_without_events() {
        let ics = create_schedule_ics(&schedule("Empty schedule", vec![]), FIXED_NOW);
        let expected = [
            "BEGIN:VCALENDAR",
            "VERSION:2.0",
            "PRODID:-//Timeboxer//Schedule Export//EN",
            "CALSCALE:GREGORIAN",
            "METHOD:PUBLISH",
            "X-WR-CALNAME:Empty schedule",
            "END:VCALENDAR",
        ]
        .join("\r\n")
            + "\r\n";
        assert_eq!(ics, expected);
    }

    #[test]
    fn uses_injected_time_for_deterministic_timestamps_and_event_dates() {
        let ics = create_schedule_ics(&schedule("Weekly schedule", vec![block("focus", "Focus", 2, 9 * 60 + 5, 10 * 60 + 15)]), FIXED_NOW);
        assert_includes(&ics, "DTSTAMP:20260108T123456Z\r\n");
        assert_includes(&ics, "DTSTART:20260107T090500\r\n");
        assert_includes(&ics, "DTEND:20260107T101500\r\n");
    }

    #[test]
    fn creates_independent_events_for_duplicate_block_names() {
        let mut s = schedule("Weekly schedule", vec![block("deep-work-a", "Deep work", 0, 540, 600), block("deep-work-b", "Deep work", 0, 660, 600)]);
        s.id = 99;
        let ics = create_schedule_ics(&s, FIXED_NOW);
        assert_eq!(ics.matches("BEGIN:VEVENT\r\n").count(), 2);
        assert_eq!(ics.matches("SUMMARY:Deep work\r\n").count(), 2);
        assert_includes(&ics, "UID:99-deep-work-a@timeboxer\r\n");
        assert_includes(&ics, "UID:99-deep-work-b@timeboxer\r\n");
    }
}
