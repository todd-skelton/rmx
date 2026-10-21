//! Remix's `match/limits.test.ts`. Remix's negative-limit `RangeError` has no counterpart:
//! limits are `usize`.

use rmx_route_pattern::{AddError, MatcherLimit, MatcherLimits, MatcherOptions, MatcherResourceError, Matcher};
use rmx_url::Url;

fn matcher(limits: MatcherLimits) -> Matcher<()> {
    Matcher::with_options(MatcherOptions { limits, ..Default::default() })
}

fn pattern_size(max: usize) -> MatcherLimits {
    MatcherLimits { max_pattern_size: max, ..Default::default() }
}

fn match_work(max: usize) -> MatcherLimits {
    MatcherLimits { max_match_work: max, ..Default::default() }
}

fn resource_error(limit: MatcherLimit, maximum: usize, actual: usize) -> MatcherResourceError {
    MatcherResourceError { limit, maximum, actual }
}

fn try_match(matcher: &Matcher<()>, href: &str) -> Result<bool, MatcherResourceError> {
    matcher.match_url(&Url::parse(href).unwrap()).map(|m| m.is_some())
}

fn assert_exceeds_match_work(result: Result<bool, MatcherResourceError>, maximum: usize) {
    let error = result.expect_err("should exceed maxMatchWork");
    assert_eq!(error.limit, MatcherLimit::MaxMatchWork);
    assert_eq!(error.maximum, maximum);
    assert!(error.actual > maximum, "{error}");
}

#[test]
fn enforces_pattern_size_below_at_and_above_the_boundary() {
    assert!(matcher(pattern_size(6)).add_str("/a", ()).is_ok());
    assert!(matcher(pattern_size(6)).add_str("/abcde", ()).is_ok());
    let error = matcher(pattern_size(6)).add_str("/abcdef", ()).unwrap_err();
    assert_eq!(error, AddError::Resource(resource_error(MatcherLimit::MaxPatternSize, 6, 7)));
}

#[test]
fn enforces_pattern_size_before_parsing_string_patterns() {
    let error = matcher(pattern_size(0)).add_str("(", ()).unwrap_err();
    assert_eq!(error, AddError::Resource(resource_error(MatcherLimit::MaxPatternSize, 0, 1)));
}

#[test]
fn measures_utf8_pattern_size() {
    assert!(matcher(pattern_size(5)).add_str("/\u{1f4bf}", ()).is_ok());
    let error = matcher(pattern_size(4)).add_str("/\u{1f4bf}", ()).unwrap_err();
    assert_eq!(error, AddError::Resource(resource_error(MatcherLimit::MaxPatternSize, 4, 5)));
}

#[test]
fn enforces_matcher_size_below_at_and_above_the_boundary() {
    let mut m = matcher(MatcherLimits { max_matcher_size: 4, ..Default::default() });
    assert!(m.add_str("/a", ()).is_ok());
    assert!(m.add_str("/b", ()).is_ok());
    let error = m.add_str("/c", ()).unwrap_err();
    assert_eq!(error, AddError::Resource(resource_error(MatcherLimit::MaxMatcherSize, 4, 6)));
}

#[test]
fn enforces_matcher_size_before_parsing_string_patterns() {
    let mut m = matcher(MatcherLimits { max_pattern_size: 1, max_matcher_size: 0, ..Default::default() });
    let error = m.add_str("(", ()).unwrap_err();
    assert_eq!(error, AddError::Resource(resource_error(MatcherLimit::MaxMatcherSize, 0, 1)));
}

#[test]
fn enforces_match_work_below_at_and_above_the_boundary() {
    let mut below = matcher(match_work(11));
    below.add_str("/a", ()).unwrap();
    assert_eq!(try_match(&below, "https://example.com/a"), Ok(true));

    let mut at = matcher(match_work(10));
    at.add_str("/a", ()).unwrap();
    assert_eq!(try_match(&at, "https://example.com/a"), Ok(true));

    let mut above = matcher(match_work(9));
    above.add_str("/a", ()).unwrap();
    assert_eq!(try_match(&above, "https://example.com/a"), Err(resource_error(MatcherLimit::MaxMatchWork, 9, 10)));
}

#[test]
fn enforces_match_work_across_all_candidates() {
    let mut m = matcher(match_work(17));
    m.add_str("/a", ()).unwrap();
    m.add_str("/a", ()).unwrap();
    assert_eq!(try_match(&m, "https://example.com/a"), Err(resource_error(MatcherLimit::MaxMatchWork, 17, 18)));
}

#[test]
fn enforces_match_work_before_canonicalizing_oversized_url_input() {
    let mut m = matcher(match_work(3));
    m.add_str("/*path", ()).unwrap();
    assert_eq!(try_match(&m, "https://example.com/abcd"), Err(resource_error(MatcherLimit::MaxMatchWork, 3, 4)));
}

#[test]
fn matches_long_deterministic_patterns_within_the_default_match_work_limit() {
    let value = "a".repeat(2_000);
    let mut m = Matcher::new();
    m.add_str(&format!("/{value}"), ()).unwrap();
    assert_eq!(try_match(&m, &format!("https://example.com/{value}")), Ok(true));
}

#[test]
fn enforces_match_work_while_scanning_optional_pathname_variables() {
    let mut m = matcher(match_work(1_000));
    m.add_str("/:name(.:extension)", ()).unwrap();
    assert_exceeds_match_work(try_match(&m, &format!("https://example.com/{}", "a".repeat(128))), 1_000);
}

#[test]
fn enforces_match_work_while_scanning_optional_hostname_variables() {
    let mut m = matcher(match_work(2_000));
    m.add_str("://:tenant(.:region).example.com/", ()).unwrap();
    assert_exceeds_match_work(try_match(&m, &format!("https://{}.example.com/", "a".repeat(64))), 2_000);
}

#[test]
fn enforces_match_work_while_comparing_pathname_wildcard_captures() {
    let mut m = matcher(match_work(1_000));
    m.add_str("/*first(/*rest)", ()).unwrap();
    assert_exceeds_match_work(try_match(&m, &format!("https://example.com/{}a", "a/".repeat(30))), 1_000);
}

#[test]
fn enforces_match_work_while_comparing_hostname_wildcard_captures() {
    let mut m = matcher(match_work(1_000));
    m.add_str("://*tenant(.*region).example.com/", ()).unwrap();
    assert_exceeds_match_work(try_match(&m, &format!("https://{}a.example.com/", "a.".repeat(12))), 1_000);
}

#[test]
fn matches_long_linear_variables_within_a_small_match_work_limit() {
    let mut m = matcher(match_work(1_000));
    m.add_str("/:name", ()).unwrap();
    let name = "a".repeat(128);
    let found = m.match_url(&Url::parse(&format!("https://example.com/{name}")).unwrap()).unwrap().expect("match");
    assert_eq!(found.params.get("name"), Some(name.as_str()));
}

#[test]
fn route_pattern_match_url_reports_resource_errors() {
    let pattern = rmx_route_pattern::RoutePattern::parse("/a").unwrap();
    let options = MatcherOptions { limits: match_work(9), ..Default::default() };
    let error = pattern.match_url_with(&Url::parse("https://example.com/a").unwrap(), options).unwrap_err();
    assert_eq!(error, resource_error(MatcherLimit::MaxMatchWork, 9, 10));
}
