//! Remix's `match.test.ts`. Remix matches URL strings with an optional `baseURL`; here URLs
//! are parsed (and relative references joined) with `rmx_url` before matching.

use rmx_route_pattern::{CaptureKind, Match, MatchParamMeta, Matcher, MatcherOptions, Params, RoutePattern};
use rmx_url::Url;

fn url(href: &str) -> Url {
    Url::parse(href).unwrap_or_else(|e| panic!("{href}: {e}"))
}

fn found<'a, T>(matcher: &'a Matcher<T>, href: &str) -> Option<Match<&'a T>> {
    matcher.match_url(&url(href)).unwrap()
}

fn all<'a, T>(matcher: &'a Matcher<T>, href: &str) -> Vec<Match<&'a T>> {
    matcher.match_all(&url(href)).unwrap()
}

fn sources<T>(matches: &[Match<T>]) -> Vec<String> {
    matches.iter().map(|m| m.pattern.to_string()).collect()
}

/// Remix's `deepEqual(match.params, {...})`: `None` is an `undefined` (unmatched optional) param.
fn assert_params(params: &Params, expected: &[(&str, Option<&str>)]) {
    for (name, value) in expected {
        assert_eq!(params.get(name), *value, "param {name} in {params:?}");
    }
    let present = expected.iter().filter(|(_, v)| v.is_some()).count();
    assert_eq!(params.len(), present, "extra params in {params:?}");
}

fn meta(kind: CaptureKind, name: &str, begin: usize, end: usize, value: &str) -> MatchParamMeta {
    MatchParamMeta { kind, name: name.into(), value: value.into(), begin, end }
}

/// Remix's `hostnameCodec` with each value's `new URL(\`https://${value}.example.com/users\`).href`
/// (computed with Node's WHATWG URL, since rmx_url does no IDNA).
const HOSTNAME_CODEC: [(&str, &str); 4] = [
    ("caf\u{e9}", "https://xn--caf-dma.example.com/users"),
    ("\u{5317}\u{4eac}-\u{3068}\u{3046}\u{304d}\u{3087}\u{3046}-\u{c11c}\u{c6b8}", "https://xn-----l73aa8a2ltij00u2kino1zl5ma.example.com/users"),
    ("\u{645}\u{631}\u{62d}\u{628}\u{627}-\u{5e2}\u{5d5}\u{5dc}\u{5dd}", "https://xn----bicvew62bga3b9a8p.example.com/users"),
    ("H\u{e0}-N\u{1ed9}i", "https://xn--h-ni-0na4517b.example.com/users"),
];

/// Remix's `pathnameCodec` with `new URL(\`https://example.com/${value}\`).href` and the `/files/` variant.
const PATHNAME_CODEC: [(&str, &str, &str); 8] = [
    ("caf\u{e9}", "https://example.com/caf%C3%A9", "https://example.com/files/caf%C3%A9"),
    ("\u{5317}\u{4eac}-\u{3068}\u{3046}\u{304d}\u{3087}\u{3046}-\u{c11c}\u{c6b8}", "https://example.com/%E5%8C%97%E4%BA%AC-%E3%81%A8%E3%81%86%E3%81%8D%E3%82%87%E3%81%86-%EC%84%9C%EC%9A%B8", "https://example.com/files/%E5%8C%97%E4%BA%AC-%E3%81%A8%E3%81%86%E3%81%8D%E3%82%87%E3%81%86-%EC%84%9C%EC%9A%B8"),
    ("\u{645}\u{631}\u{62d}\u{628}\u{627}-\u{5e2}\u{5d5}\u{5dc}\u{5dd}", "https://example.com/%D9%85%D8%B1%D8%AD%D8%A8%D8%A7-%D7%A2%D7%95%D7%9C%D7%9D", "https://example.com/files/%D9%85%D8%B1%D8%AD%D8%A8%D8%A7-%D7%A2%D7%95%D7%9C%D7%9D"),
    ("H\u{e0}-N\u{1ed9}i", "https://example.com/H%C3%A0-N%E1%BB%99i", "https://example.com/files/H%C3%A0-N%E1%BB%99i"),
    ("\u{1f4bf}", "https://example.com/%F0%9F%92%BF", "https://example.com/files/%F0%9F%92%BF"),
    ("\u{1f9d1}\u{200d}\u{1f680}", "https://example.com/%F0%9F%A7%91%E2%80%8D%F0%9F%9A%80", "https://example.com/files/%F0%9F%A7%91%E2%80%8D%F0%9F%9A%80"),
    ("acme\u{a0}corp", "https://example.com/acme%C2%A0corp", "https://example.com/files/acme%C2%A0corp"),
    ("\u{ff57}\u{ff49}\u{ff44}\u{ff45}", "https://example.com/%EF%BD%97%EF%BD%89%EF%BD%84%EF%BD%85", "https://example.com/files/%EF%BD%97%EF%BD%89%EF%BD%84%EF%BD%85"),
];

#[test]
fn protocol_matches_any_protocol_when_protocol_is_omitted() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users", ()).unwrap();
    assert!(found(&matcher, "http://example.com/users").is_some());
    assert!(found(&matcher, "https://example.com/users").is_some());
}

#[test]
fn protocol_matches_http_only_when_protocol_is_explicit_http() {
    let mut matcher = Matcher::new();
    matcher.add_str("http://example.com/users", ()).unwrap();
    assert!(found(&matcher, "http://example.com/users").is_some());
}

#[test]
fn protocol_matches_https_only_when_protocol_is_explicit_https() {
    let mut matcher = Matcher::new();
    matcher.add_str("https://example.com/users", ()).unwrap();
    assert!(found(&matcher, "https://example.com/users").is_some());
}

#[test]
fn protocol_matches_both_http_and_https_when_protocol_is_http_s() {
    let mut matcher = Matcher::new();
    matcher.add_str("http(s)://example.com/users", ()).unwrap();
    assert!(found(&matcher, "http://example.com/users").is_some());
    assert!(found(&matcher, "https://example.com/users").is_some());
}

#[test]
fn protocol_returns_null_when_protocol_does_not_match() {
    let mut matcher = Matcher::new();
    matcher.add_str("http://example.com/users", ()).unwrap();
    let found_match = found(&matcher, "https://example.com/users");
    assert!(found_match.is_none());
}

#[test]
fn hostname_matches_any_hostname_when_hostname_is_omitted() {
    let mut matcher = Matcher::new();
    matcher.add_str("users", ()).unwrap();
    assert!(found(&matcher, "https://example.com/users").is_some());
    assert!(found(&matcher, "https://other.com/users").is_some());
    assert!(found(&matcher, "http://localhost/users").is_some());
}

#[test]
fn hostname_matches_exact_static_hostname() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users", ()).unwrap();
    let found_match = found(&matcher, "https://example.com/users");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[]);
}

#[test]
fn hostname_returns_null_when_static_hostname_does_not_match() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users", ()).unwrap();
    let found_match = found(&matcher, "https://other.com/users");
    assert!(found_match.is_none());
}

#[test]
fn hostname_matches_variable_segment_in_hostname() {
    let mut matcher = Matcher::new();
    matcher.add_str("://:subdomain.example.com/api", ()).unwrap();
    let found_match = found(&matcher, "https://api.example.com/api");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("subdomain", Some("api"))]);
}

#[test]
fn hostname_matches_multiple_variables_in_hostname() {
    let mut matcher = Matcher::new();
    matcher.add_str("://:subdomain.:env.example.com/api", ()).unwrap();
    let found_match = found(&matcher, "https://api.prod.example.com/api");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("subdomain", Some("api")), ("env", Some("prod"))]);
}

#[test]
fn hostname_matches_wildcard_segment_in_hostname() {
    let mut matcher = Matcher::new();
    matcher.add_str("://*host.example.com/api", ()).unwrap();
    let found_match = found(&matcher, "https://api.v1.example.com/api");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("host", Some("api.v1"))]);
}

#[test]
fn hostname_excludes_unnamed_wildcard_from_params() {
    let mut matcher = Matcher::new();
    matcher.add_str("://*.example.com/api", ()).unwrap();
    let found_match = found(&matcher, "https://api.v1.example.com/api");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[]);
}

#[test]
fn hostname_matches_optional_hostname_segments_when_present() {
    let mut matcher = Matcher::new();
    matcher.add_str("://api(-:version).example.com/users", ()).unwrap();
    let found_match = found(&matcher, "https://api-v2.example.com/users");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("version", Some("v2"))]);
}

#[test]
fn hostname_matches_optional_hostname_segments_when_absent() {
    let mut matcher = Matcher::new();
    matcher.add_str("://api(-:version).example.com/users", ()).unwrap();
    let found_match = found(&matcher, "https://api.example.com/users");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("version", None)]);
}

#[test]
fn hostname_matches_nested_optionals_in_hostname() {
    let mut matcher = Matcher::new();
    matcher.add_str("://api(.:region(.:zone)).example.com/users", ()).unwrap();
    let all_match = found(&matcher, "https://api.us.east1.example.com/users");
    let all_match = all_match.expect("match");
    assert_params(&all_match.params, &[("region", Some("us")), ("zone", Some("east1"))]);
    let partial_match = found(&matcher, "https://api.us.example.com/users");
    let partial_match = partial_match.expect("match");
    assert_params(&partial_match.params, &[("region", Some("us")), ("zone", None)]);
    let none_match = found(&matcher, "https://api.example.com/users");
    let none_match = none_match.expect("match");
    assert_params(&none_match.params, &[("region", None), ("zone", None)]);
}

#[test]
fn hostname_matches_multiple_optionals_in_hostname() {
    let mut matcher = Matcher::new();
    matcher.add_str("://:sub(.:version).example(.:tld).com/api", ()).unwrap();
    let found_match = found(&matcher, "https://api.v2.example.dev.com/api");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("sub", Some("api")), ("version", Some("v2")), ("tld", Some("dev"))]);
}

#[test]
fn hostname_matches_mixed_static_variable_wildcards() {
    let mut matcher = Matcher::new();
    matcher.add_str("://*prefix.:env.example.com/api", ()).unwrap();
    let found_match = found(&matcher, "https://api.v1.prod.example.com/api");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("prefix", Some("api.v1")), ("env", Some("prod"))]);
}

#[test]
fn hostname_prefers_static_over_variable_hostname() {
    let mut matcher = Matcher::new();
    matcher.add_str("://:subdomain.example.com/api", ()).unwrap();
    matcher.add_str("://api.example.com/api", ()).unwrap();
    let found_match = found(&matcher, "https://api.example.com/api");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "://api.example.com/api");
}

#[test]
fn hostname_prefers_variable_over_wildcard_hostname() {
    let mut matcher = Matcher::new();
    matcher.add_str("://*host.example.com/api", ()).unwrap();
    matcher.add_str("://:subdomain.example.com/api", ()).unwrap();
    let found_match = found(&matcher, "https://api.example.com/api");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "://:subdomain.example.com/api");
}

#[test]
fn hostname_prefers_longer_static_prefix_in_hostname() {
    let mut matcher = Matcher::new();
    matcher.add_str("://:subdomain.com/api", ()).unwrap();
    matcher.add_str("://:subdomain.example.com/api", ()).unwrap();
    let found_match = found(&matcher, "https://api.example.com/api");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "://:subdomain.example.com/api");
}

#[test]
fn port_matches_omitted_port_in_url_when_port_is_omitted_in_pattern() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users", ()).unwrap();
    assert!(found(&matcher, "http://example.com/users").is_some());
    assert!(found(&matcher, "https://example.com/users").is_some());
}

#[test]
fn port_matches_explicit_port() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com:8080/users", ()).unwrap();
    let found_match = found(&matcher, "http://example.com:8080/users");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[]);
}

#[test]
fn port_matches_explicit_http_default_port() {
    let mut matcher = Matcher::new();
    matcher.add_str("http://example.com:80/users", ()).unwrap();
    let found_match = found(&matcher, "http://example.com:80/users");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[]);
}

#[test]
fn port_matches_explicit_https_default_port() {
    let mut matcher = Matcher::new();
    matcher.add_str("https://example.com:443/users", ()).unwrap();
    let found_match = found(&matcher, "https://example.com:443/users");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[]);
}

#[test]
fn port_normalizes_default_ports_per_protocol_variant() {
    let mut matcher = Matcher::new();
    matcher.add_str("http(s)://example.com:443/users", ()).unwrap();
    assert!(found(&matcher, "http://example.com:443/users").is_some());
    assert!(found(&matcher, "https://example.com:443/users").is_some());
    assert!(found(&matcher, "https://example.com/users").is_some());
    assert!(found(&matcher, "http://example.com/users").is_none());
}

#[test]
fn port_returns_null_when_explicit_port_does_not_match() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com:8080/users", ()).unwrap();
    assert!(found(&matcher, "http://example.com:3000/users").is_none());
}

#[test]
fn port_returns_null_when_pattern_has_explicit_port_but_url_omits_port() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com:8080/users", ()).unwrap();
    assert!(found(&matcher, "http://example.com/users").is_none());
}

#[test]
fn port_returns_null_when_pattern_omits_port_but_url_has_explicit_port() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users", ()).unwrap();
    assert!(found(&matcher, "http://example.com:8080/users").is_none());
}

#[test]
fn port_matches_port_with_hostname_variables() {
    let mut matcher = Matcher::new();
    matcher.add_str("://:subdomain.example.com:8080/api", ()).unwrap();
    let found_match = found(&matcher, "https://api.example.com:8080/api");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("subdomain", Some("api"))]);
}

#[test]
fn port_matches_origin_less_pattern_against_url_with_any_port() {
    let mut matcher = Matcher::new();
    matcher.add_str("/", ()).unwrap();
    matcher.add_str("/about", ()).unwrap();
    assert!(found(&matcher, "http://localhost/").is_some());
    assert!(found(&matcher, "http://localhost:44199/").is_some());
    assert!(found(&matcher, "https://example.com:8080/about").is_some());
}

#[test]
fn pathname_matches_root_pathname_when_pathname_is_empty() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com", ()).unwrap();
    assert!(found(&matcher, "http://example.com/").is_some());
}

#[test]
fn pathname_matches_static_segments() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users/list", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/users/list");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[]);
}

#[test]
fn pathname_returns_null_when_static_segment_does_not_match() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users", ()).unwrap();
    assert!(found(&matcher, "http://example.com/posts").is_none());
}

#[test]
fn pathname_returns_null_when_url_is_shorter_than_pattern() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users/list", ()).unwrap();
    assert!(found(&matcher, "http://example.com/users").is_none());
}

#[test]
fn pathname_returns_null_when_trailing_slash_does_not_match() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users", ()).unwrap();
    assert!(found(&matcher, "http://example.com/users/").is_none());
}

#[test]
fn pathname_matches_variables() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users/:id", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/users/123");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("id", Some("123"))]);
}

#[test]
fn pathname_matches_multiple_variables() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users/:userId/posts/:postId", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/users/42/posts/99");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("userId", Some("42")), ("postId", Some("99"))]);
}

#[test]
fn pathname_captures_uuid_hyphens_as_param_data() {
    let mut matcher = Matcher::new();
    matcher.add_str("/users/:id", ()).unwrap();
    let found_match = found(&matcher, "https://example.com/users/550e8400-e29b-41d4-a716-446655440000");
    assert_params(&found_match.as_ref().expect("match").params, &[("id", Some("550e8400-e29b-41d4-a716-446655440000"))]);
}

#[test]
fn pathname_bounds_params_at_raw_dots_but_keeps_encoded_dots_as_data() {
    let mut matcher = Matcher::new();
    matcher.add_str("/files/:name.:ext", ()).unwrap();
    assert_params(&found(&matcher, "https://example.com/files/readme.md").expect("match").params, &[("name", Some("readme")), ("ext", Some("md"))]);
    assert_params(&found(&matcher, "https://example.com/files/a%2Eb.txt").expect("match").params, &[("name", Some("a.b")), ("ext", Some("txt"))]);
}

#[test]
fn pathname_does_not_let_an_omitted_repeated_capture_overwrite_an_earlier_value() {
    let mut matcher = Matcher::new();
    matcher.add_str("/:id(/:id)", ()).unwrap();
    assert_params(&found(&matcher, "https://example.com/first").expect("match").params, &[("id", Some("first"))]);
}

#[test]
fn pathname_matches_many_separated_wildcards_without_regex_backtracking() {
    let mut matcher = Matcher::new();
    matcher.add_str("/*a/a/*b/b/*c/c/*d/d/*e/e/*f/f/*g/g/*h/end", ()).unwrap();
    let found_match = found(&matcher, "https://example.com/1/a/2/b/3/c/4/d/5/e/6/f/7/g/8/end");
    assert_params(&found_match.as_ref().expect("match").params, &[("a", Some("1")), ("b", Some("2")), ("c", Some("3")), ("d", Some("4")), ("e", Some("5")), ("f", Some("6")), ("g", Some("7")), ("h", Some("8"))]);
}

#[test]
fn pathname_uses_the_earliest_static_continuation_when_wildcard_splits_are_ambiguous() {
    let mut matcher = Matcher::new();
    matcher.add_str("/*first/x/*second/end", ()).unwrap();
    let found_match = found(&matcher, "https://example.com/1/x/2/x/3/end");
    assert_params(&found_match.as_ref().expect("match").params, &[("first", Some("1")), ("second", Some("2/x/3"))]);
}

#[test]
fn pathname_matches_patterns_with_many_independent_optionals() {
    let mut matcher = Matcher::new();
    matcher.add_str("/root(/a)(/b)(/c)(/d)(/e)(/f)(/g)(/h)(/i)(/j)(/k)(/l)(/m)(/n)(/o)(/p)", ()).unwrap();
    assert!(found(&matcher, "https://example.com/root/a/c/e/g/i/k/m/o").is_some());
}

#[test]
fn pathname_decodes_percent_encoded_static_pattern_text() {
    let mut matcher = Matcher::new();
    matcher.add_str("/caf%C3%A9/%61", ()).unwrap();
    assert!(found(&matcher, "https://example.com/caf%C3%A9/a").is_some());
    assert!(found(&matcher, "https://example.com/caf\u{e9}/%61").is_some());
}

#[test]
fn pathname_matches_special_characters_in_variable_values() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/:filename", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/files/my-file_v2");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("filename", Some("my-file_v2"))]);
}

#[test]
fn pathname_does_not_partially_match_variables_after_a_static_suffix() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/report-:format.pdf", ()).unwrap();
    assert!(found(&matcher, "http://example.com/files/report-json.pdf.backup").is_none());
}

#[test]
fn pathname_matches_wildcards() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/*path", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/files/docs/readme.md");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("path", Some("docs/readme.md"))]);
}

#[test]
fn pathname_matches_wildcard_with_continuation() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/*path/status", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/files/docs/api/status");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("path", Some("docs/api"))]);
}

#[test]
fn pathname_does_not_partially_match_wildcards_before_a_static_suffix() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/*path/status", ()).unwrap();
    assert!(found(&matcher, "http://example.com/files/docs/status/extra").is_none());
}

#[test]
fn pathname_excludes_unnamed_wildcard_from_params() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/*/download", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/files/docs/download");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[]);
}

#[test]
fn pathname_matches_optional_segments_when_present() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/posts(/:lang)", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/posts/en");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("lang", Some("en"))]);
}

#[test]
fn pathname_matches_optional_segments_when_absent() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/posts(/:lang)", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/posts");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("lang", None)]);
}

#[test]
fn pathname_matches_nested_optionals() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/docs(/:version(/:page))", ()).unwrap();
    let match1 = found(&matcher, "http://example.com/docs/v1/intro");
    let match1 = match1.expect("match");
    assert_params(&match1.params, &[("version", Some("v1")), ("page", Some("intro"))]);
    let match2 = found(&matcher, "http://example.com/docs/v1");
    let match2 = match2.expect("match");
    assert_params(&match2.params, &[("version", Some("v1")), ("page", None)]);
    let match3 = found(&matcher, "http://example.com/docs");
    let match3 = match3.expect("match");
    assert_params(&match3.params, &[("version", None), ("page", None)]);
}

#[test]
fn pathname_matches_multiple_optionals() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/api(/:version)/users(/:id)", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/api/v2/users/123");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("version", Some("v2")), ("id", Some("123"))]);
}

#[test]
fn pathname_matches_complex_optionals_for_file_extensions() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/:id(.:format)", ()).unwrap();
    let match1 = found(&matcher, "http://example.com/files/doc123.pdf");
    let match1 = match1.expect("match");
    assert_params(&match1.params, &[("id", Some("doc123")), ("format", Some("pdf"))]);
    let match2 = found(&matcher, "http://example.com/files/doc123");
    let match2 = match2.expect("match");
    assert_params(&match2.params, &[("id", Some("doc123")), ("format", None)]);
}

#[test]
fn pathname_matches_mixed_static_variable_wildcards() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/api/:version/files/*path", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/api/v1/files/docs/guide.pdf");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[("version", Some("v1")), ("path", Some("docs/guide.pdf"))]);
}

#[test]
fn pathname_matches_deep_nesting() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/products/electronics/computers/laptops/gaming/accessories/keyboards", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/products/electronics/computers/laptops/gaming/accessories/keyboards");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[]);
}

#[test]
fn pathname_prefers_static_over_variable_pathname() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users/new", ()).unwrap();
    matcher.add_str("://example.com/users/:id", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/users/new");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "://example.com/users/new");
}

#[test]
fn pathname_prefers_variable_over_wildcard_pathname() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/*path", ()).unwrap();
    matcher.add_str("://example.com/files/:id", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/files/123");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "://example.com/files/:id");
}

#[test]
fn pathname_prefers_longer_static_prefix_in_pathname() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/api/:id", ()).unwrap();
    matcher.add_str("://example.com/api/v1/:id", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/api/v1/users");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "://example.com/api/v1/:id");
}

#[test]
fn search_matches_any_query_params_when_no_search_constraints() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/search", ()).unwrap();
    assert!(found(&matcher, "http://example.com/search").is_some());
    assert!(found(&matcher, "http://example.com/search?q=test").is_some());
    assert!(found(&matcher, "http://example.com/search?q=test&lang=en").is_some());
}

#[test]
fn search_matches_bare_parameter_for_presence_check() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/search?q", ()).unwrap();
    assert!(found(&matcher, "http://example.com/search?q").is_some());
}

#[test]
fn search_matches_bare_parameter_with_any_value() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/search?q", ()).unwrap();
    assert!(found(&matcher, "http://example.com/search?q=test").is_some());
    assert!(found(&matcher, "http://example.com/search?q=hello").is_some());
}

#[test]
fn search_matches_any_value_constraint_with_non_empty_value() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/search?q=", ()).unwrap();
    assert!(found(&matcher, "http://example.com/search?q=test").is_some());
}

#[test]
fn search_matches_key_only_constraint() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/search?q=", ()).unwrap();
    assert!(found(&matcher, "http://example.com/search?q").is_some());
    assert!(found(&matcher, "http://example.com/search?q=").is_some());
    assert!(found(&matcher, "http://example.com/search?q=test").is_some());
}

#[test]
fn search_matches_specific_value_with_exact_match() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/api?format=json", ()).unwrap();
    assert!(found(&matcher, "http://example.com/api?format=json").is_some());
}

#[test]
fn search_returns_null_when_specific_value_does_not_match() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/api?format=json", ()).unwrap();
    assert!(found(&matcher, "http://example.com/api?format=xml").is_none());
}

#[test]
fn search_matches_multiple_constraints() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/search?q=&lang=en", ()).unwrap();
    assert!(found(&matcher, "http://example.com/search?q=test&lang=en").is_some());
}

#[test]
fn search_matches_constraints_regardless_of_order() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/api?format=json&version=v1", ()).unwrap();
    assert!(found(&matcher, "http://example.com/api?version=v1&format=json").is_some());
}

#[test]
fn search_allows_extra_params_beyond_constraints() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/search?q", ()).unwrap();
    assert!(found(&matcher, "http://example.com/search?q=test&lang=en&page=2").is_some());
}

#[test]
fn search_matches_repeated_parameter_values() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/filter?tags", ()).unwrap();
    assert!(found(&matcher, "http://example.com/filter?tags=a&tags=b").is_some());
}

#[test]
fn search_returns_null_when_constraint_is_not_met() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/api?auth", ()).unwrap();
    assert!(found(&matcher, "http://example.com/api").is_none());
    assert!(found(&matcher, "http://example.com/api?other=value").is_none());
}

#[test]
fn search_prefers_more_constraints_over_fewer() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/search?q", ()).unwrap();
    matcher.add_str("://example.com/search?q&lang", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/search?q=test&lang=en");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "://example.com/search?q=&lang=");
}

#[test]
fn search_prefers_exact_value_over_any_value() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/api?format", ()).unwrap();
    matcher.add_str("://example.com/api?format=json", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/api?format=json");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "://example.com/api?format=json");
}

#[test]
fn search_ties_specificity_for_both_forms_of_key_only_constraints_tiebreaks_using_insertion_order() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/search?q", ()).unwrap();
    matcher.add_str("://example.com/search?q=", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/search?q=test");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "://example.com/search?q=");
}

#[test]
fn ignorecase_uses_case_sensitive_pathname_matching_by_default() {
    let mut matcher = Matcher::new();
    matcher.add_str("/Posts/:id", ()).unwrap();
    assert!(found(&matcher, "https://example.com/posts/123").is_none());
    assert!(found(&matcher, "https://example.com/POSTS/123").is_none());
    assert!(found(&matcher, "https://example.com/Posts/123").is_some());
}

#[test]
fn ignorecase_ignores_pathname_case_when_ignorecase_is_true() {
    let mut matcher = Matcher::with_options(MatcherOptions { ignore_case: true, ..Default::default() });
    matcher.add_str("/Posts/:id", ()).unwrap();
    assert!(found(&matcher, "https://example.com/posts/123").is_some());
    assert!(found(&matcher, "https://example.com/POSTS/123").is_some());
    assert!(found(&matcher, "https://example.com/Posts/123").is_some());
}

#[test]
fn ignorecase_ignores_case_in_dynamic_pathname_segment_regexes_and_wildcard_continuations() {
    let mut matcher = Matcher::with_options(MatcherOptions { ignore_case: true, ..Default::default() });
    matcher.add_str("/files/:name.md", "file").unwrap();
    matcher.add_str("/*path/STATUS", "status").unwrap();
    assert_eq!(found(&matcher, "https://example.com/files/README.MD").map(|m| *m.data), Some("file"));
    assert_eq!(found(&matcher, "https://example.com/files/readme/status").map(|m| *m.data), Some("status"));
}

#[test]
fn ignorecase_ignores_unicode_case_in_static_pathname_text() {
    let mut matcher = Matcher::with_options(MatcherOptions { ignore_case: true, ..Default::default() });
    matcher.add_str("/Caf\u{e9}", ()).unwrap();
    assert!(found(&matcher, "https://example.com/CAF\u{c9}").is_some());
}

#[test]
fn ignorecase_ignores_hostname_case_regardless_of_ignorecase() {
    let mut matcher = Matcher::new();
    matcher.add_str("://Example.COM/users", ()).unwrap();
    assert!(found(&matcher, "https://example.com/users").is_some());
    assert!(found(&matcher, "https://EXAMPLE.COM/users").is_some());
}

#[test]
fn ignorecase_matches_search_params_case_sensitively_regardless_of_ignorecase() {
    let mut matcher = Matcher::with_options(MatcherOptions { ignore_case: true, ..Default::default() });
    matcher.add_str("/api?Sort", ()).unwrap();
    assert!(found(&matcher, "https://example.com/api?Sort").is_some());
    assert!(found(&matcher, "https://example.com/api?sort").is_none());
}

#[test]
fn ignorecase_defaults_to_false() {
    let mut matcher = Matcher::new();
    matcher.add_str("/Posts/:id", ()).unwrap();
    assert!(found(&matcher, "https://example.com/posts/123").is_none());
}

#[test]
fn escaping_hostname_matches_escaped_special_chars_in_static_text() {
    let mut matcher = Matcher::new();
    matcher.add_str("://a\\*b.c\\(d\\).example.com/users", ()).unwrap();
    let found_match = found(&matcher, "https://a*b.c(d).example.com/users");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[]);
}

#[test]
fn escaping_pathname_matches_escaped_special_chars_in_static_text() {
    let mut matcher = Matcher::new();
    matcher.add_str("/a\\:b/c\\*d/e\\(f\\)/g\\\\h", ()).unwrap();
    let found_match = found(&matcher, "https://example.com/a%3Ab/c*d/e(f)/g%5Ch");
    let found_match = found_match.expect("match");
    assert_params(&found_match.params, &[]);
}

#[test]
fn codec_pathname_normalizes_percent_encoded_ascii_in_static_segments() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/a", ()).unwrap();
    let found_match = found(&matcher, "https://example.com/%61");
    assert_params(&found_match.as_ref().expect("match").params, &[]);
}

#[test]
fn codec_pathname_normalizes_percent_encoded_hostname_ascii_in_static_labels() {
    let mut matcher = Matcher::new();
    matcher.add_str("://a.example.com/", ()).unwrap();
    let found_match = found(&matcher, "https://%41.example.com/");
    assert_params(&found_match.as_ref().expect("match").params, &[]);
}

#[test]
fn codec_pathname_decodes_percent_encoded_ascii_in_variables() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/:value", ()).unwrap();
    let found_match = found(&matcher, "https://example.com/%61");
    assert_params(&found_match.as_ref().expect("match").params, &[("value", Some("a"))]);
}

#[test]
fn codec_pathname_decodes_percent_encoded_ascii_in_wildcards() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/*path", ()).unwrap();
    let found_match = found(&matcher, "https://example.com/files/%61");
    assert_params(&found_match.as_ref().expect("match").params, &[("path", Some("a"))]);
}

#[test]
fn codec_pathname_returns_null_for_malformed_percent_encoded_pathnames() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/:name", ()).unwrap();
    assert!(found(&matcher, "https://example.com/files/%E0%A4%A").is_none());
    assert!(found(&matcher, "https://example.com/files/%").is_none());
}

#[test]
fn codec_pathname_returns_no_matches_for_malformed_percent_encoded_pathnames() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/:name", ()).unwrap();
    matcher.add_str("://example.com/files/*path", ()).unwrap();
    assert!(all(&matcher, "https://example.com/files/%E0%A4%A").is_empty());
}

#[test]
fn codec_pathname_does_not_match_encoded_slashes_as_pathname_separators() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/:dir/:name", ()).unwrap();
    assert!(found(&matcher, "http://example.com/files/docs%2Freadme.md").is_none());
}

#[test]
fn codec_pathname_decodes_encoded_slashes_in_variables() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/:name", ()).unwrap();
    let found_match = found(&matcher, "https://example.com/files/docs%2Freadme%2Emd");
    assert_params(&found_match.as_ref().expect("match").params, &[("name", Some("docs/readme.md"))]);
}

#[test]
fn codec_pathname_decodes_encoded_slashes_in_wildcards_with_continuation() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/*path/status", ()).unwrap();
    let found_match = found(&matcher, "https://example.com/files/docs%2Freadme.md/status");
    assert_params(&found_match.as_ref().expect("match").params, &[("path", Some("docs/readme.md"))]);
}

#[test]
fn codec_pathname_decodes_structural_slashes_in_wildcards_with_continuation() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/*path/status", ()).unwrap();
    let found_match = found(&matcher, "https://example.com/files/docs/readme.md/status");
    assert_params(&found_match.as_ref().expect("match").params, &[("path", Some("docs/readme.md"))]);
}

#[test]
fn codec_search_preserves_url_encoding_in_parameter_values() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/search?q=hello%20world", ()).unwrap();
    assert!(found(&matcher, "http://example.com/search?q=hello%20world").is_some());
}

#[test]
fn codec_search_matches_required_values_regardless_of_order_or_duplicate_url_values() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/search?tag=featured&tag=popular", ()).unwrap();
    assert!(found(&matcher, "http://example.com/search?tag=popular&tag=popular&tag=featured").is_some());
    assert!(found(&matcher, "http://example.com/search?tag=featured").is_none());
}

#[test]
fn paramsmeta_returns_empty_arrays_when_no_params() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/users");
    let found_match = found_match.expect("match");
    assert!(found_match.params_meta.hostname.is_empty());
    assert!(found_match.params_meta.pathname.is_empty());
}

#[test]
fn paramsmeta_includes_hostname_params_with_metadata() {
    let mut matcher = Matcher::new();
    matcher.add_str("://:subdomain.example.com/api", ()).unwrap();
    let found_match = found(&matcher, "https://api.example.com/api");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.params_meta.hostname.len(), 1);
    assert_eq!(found_match.params_meta.hostname[0].name, "subdomain");
    assert_eq!(found_match.params_meta.hostname[0].kind, CaptureKind::Variable);
    assert_eq!(found_match.params_meta.hostname[0].value, "api");
    assert!(found_match.params_meta.pathname.is_empty());
}

#[test]
fn paramsmeta_includes_pathname_params_with_metadata() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users/:id", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/users/123");
    let found_match = found_match.expect("match");
    assert!(found_match.params_meta.hostname.is_empty());
    assert_eq!(found_match.params_meta.pathname.len(), 1);
    assert_eq!(found_match.params_meta.pathname[0].name, "id");
    assert_eq!(found_match.params_meta.pathname[0].kind, CaptureKind::Variable);
    assert_eq!(found_match.params_meta.pathname[0].value, "123");
}

#[test]
fn paramsmeta_includes_params_from_both_hostname_and_pathname() {
    let mut matcher = Matcher::new();
    matcher.add_str("://:subdomain.example.com/users/:id", ()).unwrap();
    let found_match = found(&matcher, "https://api.example.com/users/123");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.params_meta.hostname.len(), 1);
    assert_eq!(found_match.params_meta.hostname[0].name, "subdomain");
    assert_eq!(found_match.params_meta.hostname[0].value, "api");
    assert_eq!(found_match.params_meta.pathname.len(), 1);
    assert_eq!(found_match.params_meta.pathname[0].name, "id");
    assert_eq!(found_match.params_meta.pathname[0].value, "123");
}

#[test]
fn paramsmeta_includes_wildcard_params_in_metadata() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/*path", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/files/docs/readme.md");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.params_meta.pathname.len(), 1);
    assert_eq!(found_match.params_meta.pathname[0].name, "path");
    assert_eq!(found_match.params_meta.pathname[0].kind, CaptureKind::Wildcard);
    assert_eq!(found_match.params_meta.pathname[0].value, "docs/readme.md");
}

#[test]
fn paramsmeta_includes_unnamed_wildcards_in_metadata_with_name() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/files/*/download", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/files/docs/download");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.params_meta.pathname.len(), 1);
    assert_eq!(found_match.params_meta.pathname[0].name, "*");
    assert_eq!(found_match.params_meta.pathname[0].kind, CaptureKind::Wildcard);
    assert_eq!(found_match.params_meta.pathname[0].value, "docs");
}

#[test]
fn paramsmeta_excludes_undefined_optional_params_from_metadata() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/posts(/:lang)", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/posts");
    let found_match = found_match.expect("match");
    assert!(found_match.params_meta.pathname.is_empty());
}

#[test]
fn paramsmeta_includes_only_matched_optional_params_in_metadata() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/docs(/:version(/:page))", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/docs/v1");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.params_meta.pathname.len(), 1);
    assert_eq!(found_match.params_meta.pathname[0].name, "version");
    assert_eq!(found_match.params_meta.pathname[0].value, "v1");
}

#[test]
fn specificity_prefers_static_over_variable() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/:segment", ()).unwrap();
    matcher.add_str("://example.com/users", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/users");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "://example.com/users");
}

#[test]
fn specificity_prefers_variable_over_wildcard() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/*path", ()).unwrap();
    matcher.add_str("://example.com/:id", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/123");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "://example.com/:id");
}

#[test]
fn specificity_prefers_longer_static_prefix_over_shorter() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/:id", ()).unwrap();
    matcher.add_str("://example.com/api/:id", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/api/users");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "://example.com/api/:id");
}

#[test]
fn specificity_prefers_hostname_specificity_over_pathname_specificity() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/*path", ()).unwrap();
    matcher.add_str("://:subdomain.example.com/users", ()).unwrap();
    let found_match = found(&matcher, "http://api.example.com/users");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "://:subdomain.example.com/users");
}

#[test]
fn specificity_increases_specificity_with_search_constraints() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/search", ()).unwrap();
    matcher.add_str("://example.com/search?q", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/search?q=test");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "://example.com/search?q=");
}

#[test]
fn specificity_prefers_explicit_protocol_over_omitted_or_http_s_protocol() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users", ()).unwrap();
    matcher.add_str("http(s)://example.com/users", ()).unwrap();
    matcher.add_str("http://example.com/users", ()).unwrap();
    let found_match = found(&matcher, "http://example.com/users");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "http://example.com/users");
}

#[test]
fn specificity_prefers_explicit_port_over_omitted_port() {
    let mut matcher = Matcher::new();
    matcher.add_str("http://example.com/users", ()).unwrap();
    matcher.add_str("http://example.com:8080/users", ()).unwrap();
    let found_match = found(&matcher, "http://example.com:8080/users");
    let found_match = found_match.expect("match");
    assert_eq!(found_match.pattern.to_string(), "http://example.com:8080/users");
}

#[test]
fn specificity_returns_null_when_no_patterns_match() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users", ()).unwrap();
    matcher.add_str("://example.com/posts", ()).unwrap();
    assert!(found(&matcher, "http://example.com/comments").is_none());
}

#[test]
fn returns_all_matches_sorted_by_specificity() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/*path", ()).unwrap();
    matcher.add_str("://example.com/users/:id", ()).unwrap();
    matcher.add_str("://example.com/users/new", ()).unwrap();
    let matches = all(&matcher, "http://example.com/users/new");
    assert_eq!(sources(&matches), ["://example.com/users/new", "://example.com/users/:id", "://example.com/*path"]);
}

#[test]
fn returns_empty_array_when_no_matches() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users", ()).unwrap();
    matcher.add_str("://example.com/posts", ()).unwrap();
    let matches = all(&matcher, "http://example.com/comments");
    assert!(matches.is_empty());
}

#[test]
fn includes_patterns_with_same_specificity() {
    let mut matcher = Matcher::new();
    matcher.add_str("://example.com/users/:id", ()).unwrap();
    matcher.add_str("://example.com/posts/:id", ()).unwrap();
    let matches = all(&matcher, "http://example.com/users/123");
    assert_eq!(sources(&matches), ["://example.com/users/:id"]);
    let matches2 = all(&matcher, "http://example.com/posts/456");
    assert_eq!(sources(&matches2), ["://example.com/posts/:id"]);
}

#[test]
fn deduplicates_optional_variants_that_collapse_to_the_same_concrete_pattern() {
    let mut matcher = Matcher::new();
    matcher.add_str("/(x)(x)", ()).unwrap();
    let matches = all(&matcher, "http://example.com/x");
    assert_eq!(sources(&matches), ["/(x)(x)"]);
}

#[test]
fn relative_url_references_requires_an_absolute_base_url() {
    assert!(Url::parse("/users/123").is_err());
    assert!(Url::parse("/admin/settings").is_err());
}

#[test]
fn relative_url_references_resolves_root_relative_strings_against_a_url_base() {
    let pattern = RoutePattern::parse("/users/:id").unwrap();
    let target = url("https://example.com/admin/settings").join("/users/123").unwrap();
    let found = pattern.match_url(&target).unwrap().expect("match");
    assert_params(&found.params, &[("id", Some("123"))]);
    assert_eq!(found.url.href(), "https://example.com/users/123");
}

#[test]
fn relative_url_references_resolves_path_and_query_relative_strings_with_standard_url_semantics() {
    let mut matcher = Matcher::new();
    matcher.add_str("/users/:id?tab=profile", ()).unwrap();
    let a = url("https://example.com/admin/settings").join("../users/123?tab=profile").unwrap();
    assert_params(&matcher.match_url(&a).unwrap().expect("match").params, &[("id", Some("123"))]);
    let b = url("https://example.com/users/123").join("?tab=profile").unwrap();
    assert_params(&matcher.match_url(&b).unwrap().expect("match").params, &[("id", Some("123"))]);
}

#[test]
fn relative_url_references_resolves_network_path_references_against_the_base_protocol() {
    let mut matcher = Matcher::new();
    matcher.add_str("https://cdn.example.com/assets/*path", ()).unwrap();
    let target = url("https://example.com/docs/").join("//cdn.example.com/assets/logo.svg").unwrap();
    let found = matcher.match_url(&target).unwrap().expect("match");
    assert_params(&found.params, &[("path", Some("logo.svg"))]);
    assert_eq!(found.url.href(), "https://cdn.example.com/assets/logo.svg");
}

#[test]
fn relative_url_references_supports_the_same_base_url_in_match_all() {
    let mut matcher = Matcher::new();
    matcher.add_str("/users/:id", "variable").unwrap();
    matcher.add_str("/users/new", "static").unwrap();
    let target = url("https://example.com/admin/settings").join("../users/new").unwrap();
    let matches = matcher.match_all(&target).unwrap();
    assert_eq!(matches.iter().map(|m| *m.data).collect::<Vec<_>>(), ["static", "variable"]);
    assert_eq!(matches[0].url.href(), "https://example.com/users/new");
}

#[test]
fn port_matches_hrefs_generated_from_patterns_with_explicit_default_ports() {
    let mut matcher = Matcher::new();
    let http = RoutePattern::parse("http://example.com:80/users").unwrap();
    let https = RoutePattern::parse("https://example.com:443/users").unwrap();
    matcher.add(http.clone(), ()).unwrap();
    matcher.add(https.clone(), ()).unwrap();
    assert!(found(&matcher, &http.href(()).unwrap()).is_some());
    assert!(found(&matcher, &https.href(()).unwrap()).is_some());
}

#[test]
fn pathname_lets_the_last_participating_repeated_capture_win() {
    let mut matcher = Matcher::new();
    matcher.add_str("://:id.example.com/:id(/:id)", ()).unwrap();
    let found = found(&matcher, "https://tenant.example.com/first/second").expect("match");
    assert_params(&found.params, &[("id", Some("second"))]);
    let pairs = |metas: &[MatchParamMeta]| metas.iter().map(|m| (m.name.clone(), m.value.clone())).collect::<Vec<_>>();
    assert_eq!(pairs(&found.params_meta.hostname), [("id".to_string(), "tenant".to_string())]);
    assert_eq!(pairs(&found.params_meta.pathname), [("id".to_string(), "first".to_string()), ("id".to_string(), "second".to_string())]);
}

#[test]
fn codec_hostname_matches_punycode_encoded_unicode_static_labels() {
    for (value, href) in HOSTNAME_CODEC {
        let mut matcher = Matcher::new();
        matcher.add_str(&format!("://{value}.example.com/users"), ()).unwrap();
        assert_params(&found(&matcher, href).unwrap_or_else(|| panic!("{value}")).params, &[]);
    }
}

#[test]
fn codec_hostname_decodes_punycode_encoded_unicode_variable_labels() {
    for (value, href) in HOSTNAME_CODEC {
        let mut matcher = Matcher::new();
        matcher.add_str("://:value.example.com/users", ()).unwrap();
        let lower = value.to_lowercase();
        assert_params(&found(&matcher, href).unwrap_or_else(|| panic!("{value}")).params, &[("value", Some(&lower))]);
    }
}

#[test]
fn codec_hostname_decodes_punycode_encoded_unicode_wildcard_labels() {
    for (value, href) in HOSTNAME_CODEC {
        let mut matcher = Matcher::new();
        matcher.add_str("://*host.example.com/users", ()).unwrap();
        let lower = value.to_lowercase();
        assert_params(&found(&matcher, href).unwrap_or_else(|| panic!("{value}")).params, &[("host", Some(&lower))]);
    }
}

#[test]
fn codec_pathname_matches_percent_encoded_unicode_static_segments() {
    for (value, href, _) in PATHNAME_CODEC {
        let mut matcher = Matcher::new();
        matcher.add_str(&format!("://example.com/{value}"), ()).unwrap();
        assert_params(&found(&matcher, href).unwrap_or_else(|| panic!("{value}")).params, &[]);
    }
}

#[test]
fn codec_pathname_decodes_percent_encoded_unicode_variables() {
    for (value, href, _) in PATHNAME_CODEC {
        let mut matcher = Matcher::new();
        matcher.add_str("://example.com/:value", ()).unwrap();
        assert_params(&found(&matcher, href).unwrap_or_else(|| panic!("{value}")).params, &[("value", Some(value))]);
    }
}

#[test]
fn codec_pathname_decodes_percent_encoded_unicode_wildcards() {
    for (value, _, href) in PATHNAME_CODEC {
        let mut matcher = Matcher::new();
        matcher.add_str("://example.com/files/*path", ()).unwrap();
        assert_params(&found(&matcher, href).unwrap_or_else(|| panic!("{value}")).params, &[("path", Some(value))]);
    }
}

#[test]
fn codec_pathname_treats_raw_and_percent_encoded_url_path_safe_static_text_as_equivalent() {
    let mut matcher = Matcher::new();
    matcher.add_str("/packages/@scope+name,semi;equals=/file", ()).unwrap();
    assert_params(&found(&matcher, "https://example.com/packages/@scope+name,semi;equals=/file").expect("raw").params, &[]);
    let encoded = "https://example.com/packages/%40scope%2Bname%2Csemi%3Bequals%3D/file";
    assert_params(&found(&matcher, encoded).expect("encoded").params, &[]);
}

#[test]
fn codec_pathname_decodes_structural_url_chars_in_variables_encoded_by_create_href() {
    let mut matcher = Matcher::new();
    let pattern = RoutePattern::parse("://example.com/posts/:slug").unwrap();
    matcher.add(pattern.clone(), ()).unwrap();
    let slug = "hello/world?draft=true#preview";
    let href = pattern.href([("slug", slug)]).unwrap();
    assert_params(&found(&matcher, &href).expect("match").params, &[("slug", Some(slug))]);
}

#[test]
fn codec_pathname_decodes_structural_url_chars_in_wildcards_encoded_by_create_href() {
    let mut matcher = Matcher::new();
    let pattern = RoutePattern::parse("://example.com/files/*path").unwrap();
    matcher.add(pattern.clone(), ()).unwrap();
    let path = "docs/@remix-run/ui?raw#v1";
    let href = pattern.href([("path", path)]).unwrap();
    assert_params(&found(&matcher, &href).expect("match").params, &[("path", Some(path))]);
}

#[test]
fn params_meta_includes_wildcard_hostname_metadata_for_pathname_only_patterns() {
    let mut matcher = Matcher::new();
    matcher.add_str("/users/:id", ()).unwrap();
    let found = found(&matcher, "http://example.com/users/123").expect("match");
    assert_eq!(found.params_meta.hostname, [meta(CaptureKind::Wildcard, "*", 0, 11, "example.com")]);
    assert_eq!(found.params_meta.pathname, [meta(CaptureKind::Variable, "id", 6, 9, "123")]);
}

#[test]
fn orders_complex_specificity_scenarios_consistently() {
    let mut matcher = Matcher::new();
    // Added out of order so the result is ordered by specificity, not insertion order.
    for source in [
        "/*path",
        "://api.example.com/users/:id",
        "://:subdomain.example.com/*path",
        "://api.example.com/*path",
        "/users/:id",
        "://api.example.com/users/123",
        "://:subdomain.example.com/users/:id",
        "://api.example.com/:resource/:id",
        "://api.example.com/users(/:id)",
        "/users(/:id)",
        "://api(.:region).example.com/users/:id",
    ] {
        matcher.add_str(source, ()).unwrap();
    }
    let matches = all(&matcher, "http://api.example.com/users/123");
    assert_eq!(
        sources(&matches),
        [
            "://api.example.com/users/123",
            "://api.example.com/users/:id",
            "://api.example.com/users(/:id)",
            "://api(.:region).example.com/users/:id",
            "://api.example.com/:resource/:id",
            "://api.example.com/*path",
            "://:subdomain.example.com/users/:id",
            "://:subdomain.example.com/*path",
            "/users/:id",
            "/users(/:id)",
            "/*path",
        ]
    );
}
