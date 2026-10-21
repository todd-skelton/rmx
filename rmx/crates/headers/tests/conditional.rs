//! Ports of Remix's `if-match.test.ts`, `if-none-match.test.ts` and `if-range.test.ts`.
//! [`IfMatch::parse`] is `IfMatch.from` (a present header, kept as written, so an empty
//! one is one empty tag); `new IfMatch('')` and `new IfMatch()` are `Default`; an array is
//! `collect()`. [`IfRange::matches`] takes the resource's ETag and modification time.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rmx_headers::{IfMatch, IfNoneMatch, IfRange};

fn s(list: &[&str]) -> Vec<String> {
    list.iter().map(|t| t.to_string()).collect()
}

fn tags_ifmatch(value: &str) -> Vec<String> {
    IfMatch::parse(Some(value)).tags
}

fn tags_ifnonematch(value: &str) -> Vec<String> {
    IfNoneMatch::parse(Some(value)).tags
}

#[test]
fn if_match_initializes() {
    assert!(IfMatch::default().tags.is_empty());
    assert_eq!(tags_ifmatch("67ab43"), s(&["\"67ab43\""]));
    assert_eq!(tags_ifmatch("\"67ab43\""), s(&["\"67ab43\""]));
    assert_eq!(tags_ifmatch("W/\"67ab43\""), s(&["W/\"67ab43\""]));
    assert_eq!(tags_ifmatch("67ab43, 54ed21"), s(&["\"67ab43\"", "\"54ed21\""]));
    assert_eq!(tags_ifmatch("\"67ab43\", \"54ed21\""), s(&["\"67ab43\"", "\"54ed21\""]));
    assert_eq!(tags_ifmatch("W/\"67ab43\", \"54ed21\""), s(&["W/\"67ab43\"", "\"54ed21\""]));
    for list in [["67ab43", "54ed21"], ["\"67ab43\"", "\"54ed21\""]] {
        assert_eq!(list.into_iter().collect::<IfMatch>().tags, s(&["\"67ab43\"", "\"54ed21\""]));
    }
    assert_eq!(["W/\"67ab43\"", "\"54ed21\""].into_iter().collect::<IfMatch>().tags, s(&["W/\"67ab43\"", "\"54ed21\""]));
    let copy = IfMatch::parse(Some("67ab43, 54ed21")).clone();
    assert_eq!(copy.tags, s(&["\"67ab43\"", "\"54ed21\""]));
    assert_eq!(IfMatch::parse(Some("W/\"67ab43\", \"54ed21\"")).to_string(), "W/\"67ab43\", \"54ed21\"");
}

#[test]
fn if_match_has() {
    let h = IfMatch::parse(Some("67ab43, 54ed21"));
    assert!(h.has("\"67ab43\"") && h.has("\"54ed21\""));
    assert!(!h.has("\"7892dd\"") && !h.has("*"));
    let h2 = IfMatch::parse(Some("W/\"67ab43\", \"54ed21\""));
    assert!(h2.has("W/\"67ab43\"") && h2.has("\"54ed21\"") && !h2.has("\"7892dd\""));
}

#[test]
fn if_match_matches_strongly() {
    assert!(IfMatch::default().matches("\"67ab43\""), "no header");
    let h = IfMatch::parse(Some("67ab43, 54ed21"));
    assert!(h.matches("\"67ab43\"") && h.matches("\"54ed21\"") && !h.matches("\"7892dd\""));
    let any = IfMatch::parse(Some("*"));
    assert!(any.matches("\"67ab43\"") && any.matches("\"anything\""));
    assert!(!IfMatch::parse(Some("67ab43")).matches("W/\"67ab43\""), "weak resource");
    assert!(!IfMatch::parse(Some("W/\"67ab43\"")).matches("\"67ab43\""), "weak header");
    assert!(!IfMatch::parse(Some("W/\"67ab43\"")).matches("W/\"67ab43\""), "both weak");
    assert!(IfMatch::parse(Some("\"67ab43\"")).matches("\"67ab43\""), "both strong");
    let mixed = IfMatch::parse(Some("W/\"67ab43\", \"54ed21\""));
    assert!(!mixed.matches("W/\"67ab43\"") && mixed.matches("\"54ed21\""));
}

#[test]
fn list_parsing_keeps_outer_whitespace_and_empty_entries() {
    let expected = s(&["\" first\"", "W/\"second\"", "\"\"", "*", "\"last \""]);
    assert_eq!(tags_ifmatch(" first \t,\t W/\"second\" , , * , last "), expected);
    assert_eq!(tags_ifnonematch(" first \t,\t W/\"second\" , , * , last "), expected);
    for tags in [tags_ifmatch, tags_ifnonematch] {
        assert_eq!(tags(""), s(&["\"\""]));
        assert_eq!(tags(" \t "), s(&["\" \t \""]));
        assert_eq!(tags(" ,\t, "), s(&["\"\"", "\"\"", "\"\""]));
        let tag = format!("\"first{}last\"", " ".repeat(100));
        assert_eq!(tags(&format!("W/\"other\",{tag}")), vec!["W/\"other\"".to_string(), tag]);
        assert_eq!(tags("\"abc\", \"def\"").len(), 2);
    }
    assert!(!IfMatch::parse(Some("")).matches("\"67ab43\""), "a present but empty If-Match fails");
}

#[test]
fn if_none_match() {
    assert!(IfNoneMatch::default().tags.is_empty());
    assert_eq!(tags_ifnonematch("W/\"67ab43\", \"54ed21\""), s(&["W/\"67ab43\"", "\"54ed21\""]));
    assert_eq!(["67ab43", "54ed21"].into_iter().collect::<IfNoneMatch>().tags, s(&["\"67ab43\"", "\"54ed21\""]));
    let h = IfNoneMatch::parse(Some("67ab43, 54ed21"));
    assert!(h.has("\"67ab43\"") && !h.has("\"7892dd\"") && !h.has("*"));
    assert!(h.matches("\"67ab43\"") && h.matches("\"54ed21\"") && !h.matches("\"7892dd\""));
    let weak = IfNoneMatch::parse(Some("W/\"67ab43\", \"54ed21\""));
    assert!(weak.matches("W/\"67ab43\"") && weak.matches("\"54ed21\"") && !weak.matches("\"7892dd\""));
    assert!(IfNoneMatch::parse(Some("*")).matches("\"67ab43\""));
    assert_eq!(weak.to_string(), "W/\"67ab43\", \"54ed21\"");
}

const JAN_2021: u64 = 1_609_459_200_000;

fn at(ms: u64) -> Option<SystemTime> {
    Some(UNIX_EPOCH + Duration::from_millis(ms))
}

#[test]
fn if_range_initializes_and_prints() {
    let date = "Fri, 01 Jan 2021 00:00:00 GMT";
    assert_eq!(IfRange::parse(Some("")).value, "");
    assert_eq!(IfRange::parse(Some(date)).value, date);
    assert_eq!(IfRange::parse(Some("\"67ab43\"")).to_string(), "\"67ab43\"");
    assert_eq!(IfRange::from(at(JAN_2021).unwrap()).value, date);
    assert_eq!(IfRange::from(at(1_704_067_200_000).unwrap()).to_string(), "Mon, 01 Jan 2024 00:00:00 GMT");
}

#[test]
fn if_range_matches_dates_to_the_second() {
    let h = IfRange::parse(Some("Fri, 01 Jan 2021 00:00:00 GMT"));
    assert!(h.matches(None, at(JAN_2021)));
    assert!(!h.matches(None, at(JAN_2021 + 1000)) && !h.matches(None, at(JAN_2021 - 1000)));
    assert!(h.matches(None, at(JAN_2021 + 123)) && h.matches(None, at(JAN_2021 + 999)));
    assert!(!h.matches(None, None), "no modification time");
    assert!(!IfRange::parse(Some("not-a-date")).matches(None, at(JAN_2021)));
    assert!(h.matches(Some("\"67ab43\""), at(JAN_2021)), "a date compares dates");
}

#[test]
fn if_range_matches_strong_etags_only() {
    assert!(IfRange::parse(Some("\"67ab43\"")).matches(Some("\"67ab43\""), None));
    assert!(IfRange::parse(Some("67ab43")).matches(Some("67ab43"), None), "bare tags");
    assert!(!IfRange::parse(Some("\"67ab43\"")).matches(Some("\"54ed21\""), None));
    assert!(!IfRange::parse(Some("\"67ab43\"")).matches(None, None));
    assert!(!IfRange::parse(Some("W/\"67ab43\"")).matches(Some("W/\"67ab43\""), None));
    assert!(!IfRange::parse(Some("\"67ab43\"")).matches(Some("W/\"67ab43\""), None));
    assert!(!IfRange::parse(Some("W/\"67ab43\"")).matches(Some("\"67ab43\""), None));
    assert!(IfRange::parse(Some("\"67ab43\"")).matches(Some("\"67ab43\""), at(JAN_2021)));
}

#[test]
fn an_empty_if_range_always_matches() {
    let h = IfRange::parse(Some(""));
    assert!(h.matches(Some("\"67ab43\""), None) && h.matches(None, at(JAN_2021)) && h.matches(Some("\"67ab43\""), at(JAN_2021)) && h.matches(None, None));
    assert!(IfRange::parse(None).matches(None, None));
}
