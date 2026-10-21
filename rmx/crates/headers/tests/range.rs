//! Ports of Remix's `headers/src/lib/range.test.ts`. `new Range(string)` and `Range.from`
//! are [`Range::parse`]; an object init is a struct literal.

use rmx_headers::Range;

fn parse(value: &str) -> Range {
    Range::parse(Some(value))
}

fn with(unit: &str, ranges: Vec<(Option<u64>, Option<u64>)>) -> Range {
    Range { unit: unit.into(), ranges }
}

#[test]
fn initializes_with_an_empty_string() {
    let range = parse("");
    assert_eq!((range.unit.as_str(), range.ranges.len()), ("", 0));
    assert_eq!(Range::parse(None), Range::default());
}

#[test]
fn parses_ranges() {
    assert_eq!(parse("bytes=0-99"), with("bytes", vec![(Some(0), Some(99))]));
    assert_eq!(parse("bytes=100-"), with("bytes", vec![(Some(100), None)]));
    assert_eq!(parse("bytes=-500"), with("bytes", vec![(None, Some(500))]));
    assert_eq!(parse("bytes=0-99,200-299,400-"), with("bytes", vec![(Some(0), Some(99)), (Some(200), Some(299)), (Some(400), None)]));
    assert_eq!(parse("bytes=0-99, 200-299").ranges, vec![(Some(0), Some(99)), (Some(200), Some(299))], "whitespace");
}

#[test]
fn malformed_ranges_parse_empty() {
    assert!(parse("bytes=-").ranges.is_empty());
    assert!(parse("not-a-range").ranges.is_empty());
}

#[test]
fn constructs_from_parts() {
    let range = with("bytes", vec![(Some(0), Some(99))]);
    assert_eq!((range.unit.as_str(), range.ranges.as_slice()), ("bytes", &[(Some(0), Some(99))][..]));
    assert_eq!(Range { ranges: vec![(Some(0), Some(99))], ..Default::default() }.unit, "", "no unit by default");
    let first = parse("bytes=0-99");
    let copy = with(&first.unit, first.ranges.clone());
    assert_eq!(copy, first);
}

#[test]
fn can_satisfy() {
    assert!(parse("bytes=0-99").can_satisfy(1000));
    assert!(parse("bytes=100-").can_satisfy(1000));
    assert!(parse("bytes=-500").can_satisfy(1000));
    assert!(!parse("bytes=1000-").can_satisfy(500));
    assert!(!with("bytes", vec![]).can_satisfy(1000), "empty");
    assert!(!with("bytes", vec![(Some(100), Some(50))]).can_satisfy(1000), "start > end");
    assert!(!with("bytes", vec![(None, None)]).can_satisfy(1000), "no bounds");
    assert!(!parse("bytes=-").can_satisfy(1000));
    assert!(parse("bytes=1000-,0-99").can_satisfy(500), "one satisfiable range is enough");
}

#[test]
fn normalize() {
    assert_eq!(parse("bytes=0-99").normalize(1000), vec![(0, 99)]);
    assert_eq!(parse("bytes=100-").normalize(1000), vec![(100, 999)]);
    assert_eq!(parse("bytes=-500").normalize(1000), vec![(500, 999)]);
    assert_eq!(parse("bytes=0-5000").normalize(1000), vec![(0, 999)], "clamps the end");
    assert_eq!(parse("bytes=0-99,200-299").normalize(1000), vec![(0, 99), (200, 299)]);
    assert!(parse("bytes=1000-").normalize(500).is_empty());
    assert!(parse("bytes=-").normalize(1000).is_empty());
    assert_eq!(parse("bytes=-5000").normalize(1000), vec![(0, 999)], "suffix larger than the file");
    assert!(with("bytes", vec![(None, None)]).normalize(1000).is_empty(), "hand-built ranges without bounds");
}

#[test]
fn to_string() {
    assert_eq!(parse("bytes=0-99").to_string(), "bytes=0-99");
    assert_eq!(parse("bytes=100-").to_string(), "bytes=100-");
    assert_eq!(parse("bytes=-500").to_string(), "bytes=-500");
    assert_eq!(parse("bytes=0-99,200-299").to_string(), "bytes=0-99,200-299");
    assert_eq!(with("bytes", vec![]).to_string(), "");
    assert_eq!(with("", vec![(Some(0), Some(99))]).to_string(), "", "no unit");
}
