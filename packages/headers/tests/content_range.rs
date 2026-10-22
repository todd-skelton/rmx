//! Ports of Remix's `headers/src/lib/content-range.test.ts`. `new ContentRange(string)` and
//! `ContentRange.from` are [`ContentRange::parse`], which returns `None` for an empty or
//! malformed value (Remix's unset fields). A range is both bounds or `*`, so Remix's
//! start-only and end-only objects, which print as `*`, have no separate form; and the size
//! is a number or `*`, so "size not set" has no form either.

use rmx_headers::ContentRange;

fn parse(value: &str) -> Option<ContentRange> {
    ContentRange::parse(Some(value))
}

fn range(unit: &str, range: Option<(u64, u64)>, size: Option<u64>) -> ContentRange {
    ContentRange { unit: unit.into(), range, size }
}

#[test]
fn empty_values_parse_to_none() {
    assert_eq!(parse(""), None);
    assert_eq!(ContentRange::parse(None), None);
}

#[test]
fn parses_satisfied_unsatisfied_and_unknown_size() {
    assert_eq!(parse("bytes 200-1000/67589"), Some(range("bytes", Some((200, 1000)), Some(67589))));
    assert_eq!(parse("bytes */67589"), Some(range("bytes", None, Some(67589))));
    assert_eq!(parse("bytes 0-999/*"), Some(range("bytes", Some((0, 999)), None)));
    assert_eq!(parse("bytes 0-499/1234"), Some(ContentRange::bytes(0, 499, 1234)));
}

#[test]
fn builds_and_copies_values() {
    let built = range("bytes", Some((200, 1000)), Some(67589));
    let copy = built.clone();
    assert_eq!(copy, ContentRange::bytes(200, 1000, 67589));
    let mut edited = ContentRange::unsatisfied(1);
    edited.unit = "items".into();
    edited.range = Some((100, 500));
    edited.size = Some(1000);
    assert_eq!((edited.unit.as_str(), edited.range, edited.size), ("items", Some((100, 500)), Some(1000)));
}

#[test]
fn to_string() {
    assert_eq!(ContentRange::bytes(200, 1000, 67589).to_string(), "bytes 200-1000/67589");
    assert_eq!(ContentRange::unsatisfied(67589).to_string(), "bytes */67589");
    assert_eq!(range("bytes", Some((0, 999)), None).to_string(), "bytes 0-999/*");
    assert_eq!(ContentRange::default().to_string(), "", "no unit");
    assert_eq!(range("", Some((0, 999)), Some(1000)).to_string(), "");
}

#[test]
fn zero_based_ranges_round_trip() {
    let one = parse("bytes 0-0/1").unwrap();
    assert_eq!((one.range, one.size), (Some((0, 0)), Some(1)));
    assert_eq!(one.to_string(), "bytes 0-0/1");
}
