//! Remix's `route-pattern/serialize.test.ts`.

use crate::parse::parse_pattern;
use crate::serialize::{serialize_part, serialize_pattern, serialize_protocol, serialize_search};
use crate::{Parts, RoutePattern, RoutePatternJson};

fn parts_of(source: &str) -> Parts {
    parse_pattern(source).unwrap()
}

fn assert_round_trip(source: &str, expected: &str) {
    assert_eq!(serialize_pattern(&parts_of(source)), expected, "{source}");
}

fn protocol_of(source: &str) -> &'static str {
    serialize_protocol(parts_of(source).protocol)
}

fn hostname_of(source: &str) -> String {
    parts_of(source).hostname.as_ref().map(serialize_part).unwrap_or_default()
}

fn port_of(source: &str) -> String {
    parts_of(source).port.unwrap_or_default()
}

fn pathname_of(source: &str) -> String {
    serialize_part(&parts_of(source).pathname)
}

fn search_of(source: &str) -> String {
    serialize_search(&parts_of(source).search)
}

#[test]
fn reconstructs_pathname_only() {
    assert_round_trip("/posts/:id", "/posts/:id");
    assert_round_trip("posts/:id", "/posts/:id");
    assert_round_trip("/posts(/:id)", "/posts(/:id)");
    assert_round_trip("/", "/");
    assert_round_trip("", "/");
}

#[test]
fn reconstructs_hostname_only() {
    assert_round_trip("://example.com", "://example.com/");
    assert_round_trip("://:host", "://:host/");
}

#[test]
fn reconstructs_port() {
    assert_round_trip("://example.com:8000", "://example.com:8000/");
    assert_round_trip("://:host:8080", "://:host:8080/");
}

#[test]
fn reconstructs_protocol() {
    assert_round_trip("http://", "http:///");
    assert_round_trip("https://", "https:///");
    assert_round_trip("http(s)://", "http(s):///");
}

#[test]
fn reconstructs_protocol_hostname_and_pathname() {
    for source in ["https://example.com/about", "http://example.com/products/:id", "http(s)://*host/path"] {
        assert_round_trip(source, source);
    }
}

#[test]
fn reconstructs_search() {
    assert_round_trip("/?q", "/?q=");
    assert_round_trip("/?q=1", "/?q=1");
    assert_round_trip("/?q=1&q=2", "/?q=1&q=2");
}

#[test]
fn serialize_protocol_returns_protocol_or_empty_string() {
    assert_eq!(protocol_of("http://example.com"), "http");
    assert_eq!(protocol_of("https://example.com"), "https");
    assert_eq!(protocol_of("http(s)://example.com"), "http(s)");
    assert_eq!(protocol_of("/pathname"), "");
    assert_eq!(protocol_of("://example.com"), "");
}

#[test]
fn serialize_hostname_returns_hostname_or_empty_string() {
    assert_eq!(hostname_of("://example.com"), "example.com");
    assert_eq!(hostname_of("://:host"), ":host");
    assert_eq!(hostname_of("://api.example.com"), "api.example.com");
    assert_eq!(hostname_of("/pathname"), "");
    assert_eq!(hostname_of("http://"), "");
}

#[test]
fn serialize_hostname_preserves_hostname_structure() {
    assert_eq!(hostname_of("://:tenant.example.com"), ":tenant.example.com");
}

#[test]
fn serialize_hostname_escapes_special_chars_in_hostname_text() {
    let pattern = r"a\:b.c\*d.e\(f\).g\\h";
    assert_eq!(hostname_of(&format!("://{pattern}")), pattern);
}

#[test]
fn serialize_port_returns_port_or_empty_string() {
    assert_eq!(port_of("://example.com:8000"), "8000");
    assert_eq!(port_of("://example.com:3000"), "3000");
    assert_eq!(port_of("://example.com"), "");
    assert_eq!(port_of("/pathname"), "");
}

#[test]
fn serialize_pathname_returns_pathname_or_empty_string() {
    assert_eq!(pathname_of("/posts/:id"), "posts/:id");
    assert_eq!(pathname_of("posts/:id"), "posts/:id");
    assert_eq!(pathname_of("/posts(/:id)"), "posts(/:id)");
    assert_eq!(pathname_of("://example.com"), "");
    assert_eq!(pathname_of("/"), "");
    assert_eq!(pathname_of(""), "");
}

#[test]
fn serialize_pathname_preserves_pathname_structure() {
    for source in ["api/(v:major(.:minor)/)run", "*/node_modules/(*path/):package/dist/index.:ext"] {
        assert_eq!(pathname_of(source), source);
    }
}

#[test]
fn serialize_pathname_emits_nameless_wildcard_as_bare_star() {
    assert_eq!(pathname_of("*"), "*");
}

#[test]
fn serialize_pathname_escapes_special_chars_in_pathname_text() {
    let pattern = r"a\:b/c\*d/e\(f\)/g\\h";
    assert_eq!(pathname_of(&format!("/{pattern}")), pattern);
}

#[test]
fn serialize_search_returns_search_or_empty_string() {
    assert_eq!(search_of("?q"), "q=");
    assert_eq!(search_of("?q="), "q=");
    assert_eq!(search_of("?q=1"), "q=1");
    assert_eq!(search_of("?q=1&q=2"), "q=1&q=2");
    assert_eq!(search_of("/posts?filter"), "filter=");
    assert_eq!(search_of("/posts?sort=asc"), "sort=asc");
    assert_eq!(search_of("/posts"), "");
    assert_eq!(search_of(""), "");
}

#[test]
fn serialize_pattern_parts_returns_each_helper_as_a_single_object() {
    let pattern = RoutePattern::parse("https://api.example.com:8000/v1/:resource?filter=active").unwrap();
    let expected = RoutePatternJson {
        protocol: "https".into(),
        hostname: "api.example.com".into(),
        port: "8000".into(),
        pathname: "v1/:resource".into(),
        search: "filter=active".into(),
    };
    assert_eq!(pattern.to_json(), expected);
}

// types/stringify.test.ts: the type-level Stringify<Parse<source>> cases, checked at run time
// as a parse and serialize round trip.
#[test]
fn stringify_type_cases_round_trip() {
    for (source, expected) in [
        ("", "/"),
        ("http", "/http"),
        ("hello/world", "/hello/world"),
        ("hello/world?q=1", "/hello/world?q=1"),
        ("hello/world?q=1&q=2", "/hello/world?q=1&q=2"),
        ("hello/world?q=1&a=2", "/hello/world?q=1&a=2"),
        ("http://example.com", "http://example.com/"),
        ("http:///", "http:///"),
        ("https:///path", "https:///path"),
        ("https://example.com/path", "https://example.com/path"),
        ("https://example.com/path?q=1", "https://example.com/path?q=1"),
        ("https://example.com/path?q=1&q=2", "https://example.com/path?q=1&q=2"),
        ("https://example.com/path?q=1&a=2", "https://example.com/path?q=1&a=2"),
    ] {
        assert_round_trip(source, expected);
    }
}
