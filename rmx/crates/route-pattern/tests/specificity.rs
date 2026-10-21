//! Remix's `specificity.test.ts`.

use std::cmp::Ordering;

use rmx_route_pattern::{specificity, Match, Matcher};
use rmx_url::Url;

fn match_pattern(pattern: &str, url: &Url) -> Option<Match<()>> {
    let mut matcher = Matcher::new();
    matcher.add_str(pattern, ()).unwrap();
    let found = matcher.match_url(url).unwrap()?;
    Some(Match { url: found.url, pattern: found.pattern, data: (), params: found.params, params_meta: found.params_meta })
}

fn assert_compare(patterns: [&str; 2], url: &str, expected: i32) {
    let url = Url::parse(url).unwrap();
    let a = match_pattern(patterns[0], &url).unwrap_or_else(|| panic!("pattern A {} should match {url}", patterns[0]));
    let b = match_pattern(patterns[1], &url).unwrap_or_else(|| panic!("pattern B {} should match {url}", patterns[1]));
    let expected = expected.cmp(&0);
    assert_eq!(specificity::compare(&a, &b), expected, "{patterns:?} {url}");
    assert_eq!(specificity::compare(&b, &a), expected.reverse(), "{patterns:?} {url} reversed");
    assert_eq!(specificity::equal(&a, &b), expected == Ordering::Equal);
    assert_eq!(specificity::greater_than(&a, &b), expected == Ordering::Greater);
    assert_eq!(specificity::less_than(&b, &a), expected == Ordering::Greater);
}

#[test]
fn hostname_ranks_static_higher_than_variable() {
    assert_compare(["https://example.com", "https://:subdomain.com"], "https://example.com", 1);
}

#[test]
fn hostname_ranks_variable_higher_than_wildcard() {
    assert_compare(["https://:subdomain.com", "https://*.com"], "https://example.com", 1);
}

#[test]
fn hostname_ties_when_variables_have_same_range() {
    assert_compare(["https://:subdomain.com", "https://:other.com"], "https://example.com", 0);
}

#[test]
fn hostname_ties_when_wildcards_have_same_range() {
    assert_compare(["https://*.com", "https://*.com"], "https://example.com", 0);
}

#[test]
fn hostname_ranks_variable_with_prefix_suffix_higher_than_bare_variable() {
    assert_compare(["https://e:sub.com", "https://:subdomain.com"], "https://example.com", 1);
}

#[test]
fn hostname_ranks_wildcard_with_prefix_suffix_higher_than_bare_wildcard() {
    assert_compare(["https://e*.com", "https://*.com"], "https://example.com", 1);
}

#[test]
fn hostname_breaks_tie_on_variables_and_wildcards_by_subsequent_characters() {
    assert_compare(["https://a*.cd.:ef.*.com", "https://*.cd.:ef.*.com"], "https://ab.cd.ef.gh.com", 1);
}

#[test]
fn hostname_ranks_segments_right_to_left_but_chars_within_segments_left_to_right() {
    assert_compare(["https://a.*b-c.d", "https://a.b-*c.d"], "https://a.b-xxx.yyy-c.d", 1);
}

#[test]
fn hostname_ranks_back_to_back_wildcards_with_static_content_higher() {
    assert_compare(["https://*-*.com", "https://*.com"], "https://ab-cd.com", 1);
}

#[test]
fn pathname_ranks_static_higher_than_variable() {
    assert_compare(["https://example.com/posts/123", "https://example.com/posts/:id"], "https://example.com/posts/123", 1);
}

#[test]
fn pathname_ranks_variable_higher_than_wildcard() {
    assert_compare(["https://example.com/posts/:id", "https://example.com/posts/*"], "https://example.com/posts/123", 1);
}

#[test]
fn pathname_ties_when_variables_have_same_range() {
    assert_compare(["https://example.com/posts/:id", "https://example.com/posts/:other"], "https://example.com/posts/123", 0);
}

#[test]
fn pathname_ties_when_wildcards_have_same_range() {
    assert_compare(["https://example.com/posts/*", "https://example.com/posts/*"], "https://example.com/posts/123", 0);
}

#[test]
fn pathname_ranks_variable_with_prefix_suffix_higher_than_bare_variable() {
    assert_compare(["https://example.com/posts-:id", "https://example.com/:segment"], "https://example.com/posts-123", 1);
}

#[test]
fn pathname_ranks_wildcard_with_prefix_suffix_higher_than_bare_wildcard() {
    assert_compare(["https://example.com/p*", "https://example.com/*/:id"], "https://example.com/posts/123", 1);
}

#[test]
fn pathname_breaks_tie_on_variables_and_wildcards_by_subsequent_characters() {
    assert_compare(["https://example.com/*/123/:id/7*", "https://example.com/*/123/:id/*"], "https://example.com/posts/123/456/789", 1);
}

#[test]
fn pathname_ranks_back_to_back_variables_with_static_content_higher() {
    assert_compare(["https://example.com/:a/:b", "https://example.com/*"], "https://example.com/posts/123", 1);
}

#[test]
fn pathname_ranks_back_to_back_wildcards_with_static_content_higher() {
    assert_compare(["https://example.com/*/*", "https://example.com/*"], "https://example.com/posts/123", 1);
}

#[test]
fn search_ranks_exact_value_higher_than_any_value() {
    assert_compare(["https://example.com/posts?q=hello", "https://example.com/posts?q"], "https://example.com/posts?q=hello", 1);
}

#[test]
fn search_ranks_q_and_q_equally() {
    assert_compare(["https://example.com/posts?q=", "https://example.com/posts?q"], "https://example.com/posts?q=hello", 0);
}

#[test]
fn search_ranks_more_constraints_higher() {
    assert_compare(["https://example.com/posts?q=&a=", "https://example.com/posts?q="], "https://example.com/posts?q=hello&a=1", 1);
}

#[test]
fn search_ties_on_different_keys_with_same_count() {
    assert_compare(["https://example.com/posts?q=", "https://example.com/posts?a="], "https://example.com/posts?q=hello&a=1", 0);
}

#[test]
fn search_ranks_multiple_exact_values_higher_than_single_any_value() {
    assert_compare(["https://example.com/posts?q=hello&q=world", "https://example.com/posts?q"], "https://example.com/posts?q=hello&q=world", 1);
}

#[test]
fn search_ranks_exact_values_higher_than_any_values() {
    assert_compare(["https://example.com/posts?q=hello&a=1", "https://example.com/posts?q&a"], "https://example.com/posts?q=hello&a=1&b=2", 1);
}

#[test]
#[should_panic(expected = "Cannot compare matches for different URLs")]
fn throws_when_comparing_matches_for_different_urls() {
    let a = match_pattern("https://example.com/:path", &Url::parse("https://example.com/foo").unwrap()).unwrap();
    let b = match_pattern("https://example.com/:path", &Url::parse("https://example.com/bar").unwrap()).unwrap();
    specificity::compare(&a, &b);
}

#[test]
fn ascending_and_descending_sort_by_specificity() {
    let url = Url::parse("https://example.com/posts/123").unwrap();
    let mut matches: Vec<Match<()>> = ["https://example.com/*", "https://example.com/posts/123", "https://example.com/posts/:id"]
        .iter()
        .map(|p| match_pattern(p, &url).unwrap())
        .collect();
    matches.sort_by(specificity::descending);
    let sources: Vec<String> = matches.iter().map(|m| m.pattern.source()).collect();
    assert_eq!(sources, ["https://example.com/posts/123", "https://example.com/posts/:id", "https://example.com/*"]);
    matches.sort_by(specificity::ascending);
    assert_eq!(matches[0].pattern.source(), "https://example.com/*");
}
