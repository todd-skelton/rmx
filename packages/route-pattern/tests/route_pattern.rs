//! Remix's `route-pattern.test.ts`.

use rsmix_route_pattern::{CaptureKind, HrefError, MatchParamMeta, RoutePattern, RoutePatternCapture, RoutePatternJson, UrlPart};
use rsmix_url::Url;

fn capture(part: UrlPart, kind: CaptureKind, name: &str, optional: bool) -> RoutePatternCapture {
    RoutePatternCapture { part, kind, name: name.to_string(), optional }
}

#[test]
fn exports_public_route_pattern_support_types() {
    let capture = capture(UrlPart::Pathname, CaptureKind::Variable, "id", false);
    let json = RoutePatternJson { pathname: ":id".into(), ..Default::default() };
    let href_error = RoutePattern::parse("https://").unwrap().href(()).unwrap_err();
    let meta = MatchParamMeta { kind: CaptureKind::Variable, name: "id".into(), value: "123".into(), begin: 0, end: 3 };
    assert_eq!(capture.name, "id");
    assert_eq!(json.pathname, ":id");
    assert!(matches!(href_error, HrefError::MissingHostname { .. }));
    assert_eq!(meta.value, "123");
}

#[test]
fn exposes_captures_without_exposing_parsed_tokens() {
    let pattern = RoutePattern::parse("https://:tenant.example.com/:collection(/*path)(.:ext)").unwrap();
    assert_eq!(
        pattern.captures(),
        vec![
            capture(UrlPart::Hostname, CaptureKind::Variable, "tenant", false),
            capture(UrlPart::Pathname, CaptureKind::Variable, "collection", false),
            capture(UrlPart::Pathname, CaptureKind::Wildcard, "path", true),
            capture(UrlPart::Pathname, CaptureKind::Variable, "ext", true),
        ]
    );
}

#[test]
fn exposes_unnamed_wildcards_in_captures() {
    let pattern = RoutePattern::parse("/files/*").unwrap();
    assert_eq!(pattern.captures(), vec![capture(UrlPart::Pathname, CaptureKind::Wildcard, "*", false)]);
}

#[test]
fn creates_hrefs_from_parsed_patterns() {
    let pattern = RoutePattern::parse("/posts/:postId").unwrap();
    assert_eq!(pattern.href([("postId", "123")]).unwrap(), "/posts/123");
    assert!(pattern.href([("userId", "123")]).is_err());
}

#[test]
fn matches_with_parsed_patterns() {
    let pattern = RoutePattern::parse("/posts/:postId").unwrap();
    let found = pattern.match_url(&Url::parse("https://remix.run/posts/123").unwrap()).unwrap().expect("match");
    assert_eq!(found.params.get("postId"), Some("123"));
}

#[test]
fn joins_parsed_patterns_without_reparsing_them() {
    let base = RoutePattern::parse("/orgs/:orgId").unwrap();
    let next = RoutePattern::parse("/repos/:repoId").unwrap();
    let joined = base.join(&next);
    assert_eq!(joined.href([("orgId", "remix-run"), ("repoId", "remix")]).unwrap(), "/orgs/remix-run/repos/remix");
    assert!(joined.href([("orgId", "remix-run")]).is_err());
}

#[test]
fn param_names_lists_named_captures_once() {
    let pattern = RoutePattern::parse("://:id.example.com/:id/*rest/*").unwrap();
    assert_eq!(pattern.param_names(), vec!["id".to_string(), "rest".to_string()]);
}
