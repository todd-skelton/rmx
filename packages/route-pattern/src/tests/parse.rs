//! Remix's `route-pattern/parse.test.ts`.

use std::collections::BTreeMap;

use crate::parse::{parse_part, parse_pattern};
use crate::{ParseError, ParseErrorType, PartPattern, Parts, Protocol, RoutePattern, Token, UrlPart};

use ParseErrorType::*;

/// `t:text`, `/` (separator), `(`, `)`, `:name`, `*name` or `*`.
fn tok(s: &str) -> Token {
    match s {
        "/" => Token::Separator,
        "(" => Token::Open,
        ")" => Token::Close,
        _ if s.starts_with("t:") => Token::Text(s[2..].to_string()),
        _ if s.starts_with(':') => Token::Var(s[1..].to_string()),
        "*" => Token::Wild("*".to_string()),
        _ if s.starts_with('*') => Token::Wild(s[1..].to_string()),
        _ => panic!("bad token notation {s}"),
    }
}

fn part(source: &str, kind: UrlPart) -> PartPattern {
    parse_part(source, (0, source.len()), kind).unwrap()
}

fn assert_parse(source: &str, tokens: &[&str], optionals: &[(usize, usize)]) {
    let expected = PartPattern {
        tokens: tokens.iter().map(|t| tok(t)).collect(),
        optionals: optionals.iter().copied().collect::<BTreeMap<_, _>>(),
        kind: UrlPart::Pathname,
    };
    assert_eq!(part(source, UrlPart::Pathname), expected, "{source}");
}

fn assert_parse_error(source: &str, kind: ParseErrorType, index: usize) {
    let error = parse_part(source, (0, source.len()), UrlPart::Pathname).unwrap_err();
    assert_eq!(error, ParseError::new(kind, source, index), "{source}");
}

fn assert_pattern_error(source: &str, kind: ParseErrorType, index: usize) {
    assert_eq!(RoutePattern::parse(source).unwrap_err(), ParseError::new(kind, source, index), "{source}");
}

fn assert_pattern_error_kind(source: &str, kind: ParseErrorType) {
    assert_eq!(parse_pattern(source).unwrap_err().kind, kind, "{source}");
}

// ParseError

#[test]
fn parse_error_exposes_type_source_and_index() {
    let error = ParseError::new(UnmatchedOpen, "foo(bar", 3);
    assert_eq!(error.kind, UnmatchedOpen);
    assert_eq!(error.kind.as_str(), "unmatched (");
    assert_eq!(error.source, "foo(bar");
    assert_eq!(error.index, 3);
}

#[test]
fn parse_error_underlines_the_error_index() {
    let error = ParseError::new(UnmatchedOpen, "api/(v:major", 4);
    assert_eq!(format!("ParseError: {error}"), "ParseError: unmatched (\n\napi/(v:major\n    ^");
}

// parsePart

#[test]
fn parses_static_text() {
    assert_parse("abc", &["t:abc"], &[]);
}

#[test]
fn parses_a_variable() {
    assert_parse(":abc", &[":abc"], &[]);
    assert_parse(":_hello_WORLD", &[":_hello_WORLD"], &[]);
    assert_parse(":$_hello_WORLD$123$", &[":$_hello_WORLD$123$"], &[]);
}

#[test]
fn parses_a_wildcard() {
    assert_parse("*", &["*"], &[]);
    assert_parse("*abc", &["*abc"], &[]);
    assert_parse("*_hello_WORLD", &["*_hello_WORLD"], &[]);
    assert_parse("*$_hello_WORLD$123$", &["*$_hello_WORLD$123$"], &[]);
}

#[test]
fn parses_an_optional() {
    assert_parse("aa(bb)cc", &["t:aa", "(", "t:bb", ")", "t:cc"], &[(1, 3)]);
    assert_parse("(aa(bb)cc)", &["(", "t:aa", "(", "t:bb", ")", "t:cc", ")"], &[(0, 6), (2, 4)]);
}

#[test]
fn parses_combinations_of_text_variables_wildcards_optionals() {
    assert_parse(
        "api/(v:major(.:minor)/)run",
        &["t:api", "/", "(", "t:v", ":major", "(", "t:.", ":minor", ")", "/", ")", "t:run"],
        &[(2, 10), (5, 8)],
    );
    assert_parse(
        "*/node_modules/(*path/):package/dist/index.:ext",
        &["*", "/", "t:node_modules", "/", "(", "*path", "/", ")", ":package", "/", "t:dist", "/", "t:index.", ":ext"],
        &[(4, 7)],
    );
}

#[test]
fn parses_repeated_param_names() {
    assert_parse(":id/:id", &[":id", "/", ":id"], &[]);
    assert_parse("*id/*id", &["*id", "/", "*id"], &[]);
    assert_parse("*/*", &["*", "/", "*"], &[]);
    assert_parse(":a/*a/:b/*b/:b/*a/:a", &[":a", "/", "*a", "/", ":b", "/", "*b", "/", ":b", "/", "*a", "/", ":a"], &[]);
}

#[test]
fn throws_unmatched_open() {
    assert_parse_error("(", UnmatchedOpen, 0);
    assert_parse_error("(()", UnmatchedOpen, 0);
    assert_parse_error("()(", UnmatchedOpen, 2);
}

#[test]
fn throws_unmatched_close() {
    assert_parse_error(")", UnmatchedClose, 0);
    assert_parse_error(")()", UnmatchedClose, 0);
    assert_parse_error("())", UnmatchedClose, 2);
}

#[test]
fn throws_missing_variable_name() {
    assert_parse_error(":", MissingVariableName, 0);
    assert_parse_error("a:", MissingVariableName, 1);
    assert_parse_error("(a:)", MissingVariableName, 2);
    assert_parse_error(":(a)", MissingVariableName, 0);
    assert_parse_error(":123", MissingVariableName, 0);
    assert_parse_error("::", MissingVariableName, 0);
}

#[test]
fn throws_dangling_escape() {
    assert_parse_error("\\", DanglingEscape, 0);
}

#[test]
fn uses_slash_as_separator_for_pathname_type() {
    let expected = PartPattern { tokens: ["t:a", "/", "t:b", "/", "t:c"].map(tok).to_vec(), optionals: BTreeMap::new(), kind: UrlPart::Pathname };
    assert_eq!(part("a/b/c", UrlPart::Pathname), expected);
}

#[test]
fn uses_dot_as_separator_for_hostname_type() {
    let expected = PartPattern { tokens: ["t:a", "/", "t:b", "/", "t:c"].map(tok).to_vec(), optionals: BTreeMap::new(), kind: UrlPart::Hostname };
    assert_eq!(part("a.b.c", UrlPart::Hostname), expected);
}

// RoutePattern

#[test]
fn derives_source_from_parsed_parts() {
    let pattern = RoutePattern::parse("/posts/:id").unwrap();
    let parts = Parts { protocol: None, hostname: None, port: None, pathname: part("posts/:id", UrlPart::Pathname), search: Vec::new() };
    assert_eq!(*pattern.parts(), parts);
    assert_eq!(pattern.source(), "/posts/:id");
    assert_eq!(pattern.to_string(), "/posts/:id");
}

#[test]
fn accepts_repeated_capture_names() {
    assert!(RoutePattern::parse("://:id.example.com/:id/:id").is_ok());
}

#[test]
fn rejects_adjacent_wildcards() {
    assert_pattern_error("/*left*right", AdjacentWildcards, 6);
}

#[test]
fn rejects_wildcards_that_become_adjacent_when_an_optional_is_omitted() {
    assert_pattern_error("/*left(/middle)*right", AdjacentWildcards, 15);
}

#[test]
fn rejects_params_followed_by_non_delimiter_text() {
    assert_pattern_error("/:year-:month", InvalidParamDelimiter, 6);
}

#[test]
fn rejects_params_followed_by_non_delimiter_text_after_an_optional() {
    assert_pattern_error("/:id(/details)-suffix", InvalidParamDelimiter, 14);
}

#[test]
fn rejects_params_followed_by_non_delimiter_text_after_a_nested_optional() {
    assert_pattern_error("/:id((/details)-suffix)", InvalidParamDelimiter, 15);
}

#[test]
fn rejects_empty_optional_groups() {
    assert_pattern_error("/users()", EmptyOptional, 6);
}

#[test]
fn rejects_optional_branches_with_ambiguous_capture_schemas() {
    assert_pattern_error("/users(/:id)(/:slug)", AmbiguousOptionalCaptures, 12);
}

// parsePattern

fn assert_pattern(source: &str, protocol: Option<Protocol>, hostname: Option<&str>, port: Option<&str>, pathname: &str, search: &[(&str, &[&str])]) {
    let expected = Parts {
        protocol,
        hostname: hostname.map(|h| part(h, UrlPart::Hostname)),
        port: port.map(str::to_string),
        pathname: part(pathname, UrlPart::Pathname),
        search: search.iter().map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect())).collect(),
    };
    assert_eq!(parse_pattern(source).unwrap(), expected, "{source}");
}

#[test]
fn parses_protocol() {
    assert_eq!(parse_pattern("http://").unwrap().protocol, Some(Protocol::Http));
    assert_eq!(parse_pattern("https://").unwrap().protocol, Some(Protocol::Https));
    assert_eq!(parse_pattern("http(s)://").unwrap().protocol, Some(Protocol::HttpOrHttps));
}

#[test]
fn parses_hostname() {
    assert_pattern("://example.com", None, Some("example.com"), None, "", &[]);
}

#[test]
fn parses_port() {
    assert_pattern("://example.com:8000", None, Some("example.com"), Some("8000"), "", &[]);
}

#[test]
fn parses_pathname() {
    assert_pattern("products/:id", None, None, None, "products/:id", &[]);
}

#[test]
fn parses_search() {
    assert_pattern("?q", None, None, None, "", &[("q", &[])]);
    assert_pattern("?q=", None, None, None, "", &[("q", &[])]);
    assert_pattern("?q=1", None, None, None, "", &[("q", &["1"])]);
}

#[test]
fn decodes_search_params_like_url_search_params() {
    assert_pattern("?q=a+b", None, None, None, "", &[("q", &["a b"])]);
    assert_pattern("?q=a%20b", None, None, None, "", &[("q", &["a b"])]);
    assert_pattern("?q=a%2Bb", None, None, None, "", &[("q", &["a+b"])]);
    assert_pattern("?q=caf%C3%A9", None, None, None, "", &[("q", &["caf\u{e9}"])]);
}

#[test]
fn parses_protocol_and_hostname() {
    assert_pattern("https://example.com", Some(Protocol::Https), Some("example.com"), None, "", &[]);
}

#[test]
fn parses_protocol_and_pathname() {
    assert_pattern("http:///dir/file", Some(Protocol::Http), None, None, "dir/file", &[]);
}

#[test]
fn parses_hostname_and_pathname() {
    assert_pattern("://example.com/about", None, Some("example.com"), None, "about", &[]);
}

#[test]
fn parses_protocol_hostname_and_pathname() {
    assert_pattern("https://example.com/about", Some(Protocol::Https), Some("example.com"), None, "about", &[]);
}

#[test]
fn parses_protocol_hostname_and_search() {
    assert_pattern("https://example.com?q=1", Some(Protocol::Https), Some("example.com"), None, "", &[("q", &["1"])]);
}

#[test]
fn parses_protocol_pathname_and_search() {
    assert_pattern("http:///dir/file?q=1", Some(Protocol::Http), None, None, "dir/file", &[("q", &["1"])]);
}

#[test]
fn parses_hostname_pathname_and_search() {
    assert_pattern("://example.com/about?q=1", None, Some("example.com"), None, "about", &[("q", &["1"])]);
}

#[test]
fn parses_protocol_hostname_pathname_and_search() {
    assert_pattern("https://example.com/about?q=1", Some(Protocol::Https), Some("example.com"), None, "about", &[("q", &["1"])]);
}

#[test]
fn parses_search_params_into_constraints_grouped_by_param_name() {
    assert_pattern("?q&q", None, None, None, "", &[("q", &[])]);
    assert_pattern("?q&q=", None, None, None, "", &[("q", &[])]);
    assert_pattern("?q&q=1", None, None, None, "", &[("q", &["1"])]);
    assert_pattern("?q=&q=1", None, None, None, "", &[("q", &["1"])]);
    assert_pattern("?q=1&q=2", None, None, None, "", &[("q", &["1", "2"])]);
    assert_pattern("?q&q=&q=1&q=2", None, None, None, "", &[("q", &["1", "2"])]);
}

#[test]
fn throws_on_invalid_protocol() {
    for source in ["ftp://example.com", "ws://example.com/path", "httpx://example.com", "http(s)x://example.com"] {
        assert_pattern_error_kind(source, InvalidProtocol);
    }
}

#[test]
fn throws_on_port_without_hostname() {
    assert_pattern_error_kind("://:8080/users", MissingHostname);
    assert_pattern_error_kind("http://:80/users", MissingHostname);
}

#[test]
fn parses_an_empty_pattern_as_empty_pathname() {
    assert_pattern("", None, None, None, "", &[]);
    assert_pattern("/", None, None, None, "", &[]);
}

#[test]
fn parses_nameless_hostname_wildcard_as_null() {
    assert_pattern("://*", None, None, None, "", &[]);
}
