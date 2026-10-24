//! Cross-origin protection: reject unsafe requests that browsers mark as cross-site, with
//! no tokens needed (Remix's `cop`, after Go's `CrossOriginProtection`).

use std::collections::HashSet;
use std::sync::Arc;

use http::{Method, StatusCode};
use rmx_fetch::Response;
use rmx_fetch_router::{BoxFuture, Context, IntoResponse, Middleware, Next};

/// Why [`Cop`] rejected a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopFailure {
    /// `Sec-Fetch-Site` says the request came from another site.
    CrossOriginRequest,
    /// No `Sec-Fetch-Site` (an old browser) and `Origin` does not match the host.
    CrossOriginRequestFromOldBrowser,
}

type DenyFn = dyn Fn(CopFailure, &Context) -> Response + Send + Sync;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Segment {
    Static(String),
    Wildcard,
    Rest,
}

#[derive(Clone, Debug)]
struct BypassPattern {
    method: Option<Method>,
    pathname: String,
    segments: Vec<Segment>,
    subtree: bool,
}

/// Rejects non-GET/HEAD/OPTIONS requests whose `Sec-Fetch-Site` is not `same-origin` or
/// `none`; for browsers without that header, compares `Origin` with the request host.
#[derive(Clone, Default)]
pub struct Cop {
    trusted: HashSet<String>,
    bypass: Vec<BypassPattern>,
    on_deny: Option<Arc<DenyFn>>,
}

pub fn cop() -> Cop {
    Cop::default()
}

impl Cop {
    /// Allow an exact origin such as `https://partner.example`.
    ///
    /// # Panics
    ///
    /// When `origin` is not a bare origin (no path, query, fragment or trailing slash).
    pub fn trusted_origin(mut self, origin: &str) -> Cop {
        let trimmed = origin.trim();
        assert!(!trimmed.is_empty(), "trusted origin must not be empty");
        assert!(!trimmed.ends_with('/'), "invalid origin {origin:?}: trailing slash is not allowed");
        let url = parse_origin(trimmed).unwrap_or_else(|| panic!("invalid origin {origin:?}"));
        assert!(
            matches!(url.pathname(), "" | "/") && url.query().is_none() && url.fragment().is_none(),
            "invalid origin {origin:?}: path, query, and fragment are not allowed"
        );
        self.trusted.insert(url.origin());
        self
    }

    /// Skip protection for matching requests, using Go `ServeMux` patterns: `/webhooks/`
    /// (a subtree), `POST /hooks/{id}` (one segment), `/files/{path...}` (the rest).
    ///
    /// # Panics
    ///
    /// When the pattern is malformed.
    pub fn insecure_bypass(mut self, pattern: &str) -> Cop {
        self.bypass.push(parse_bypass(pattern));
        self
    }

    /// The response for rejected requests (default: 403 with a short message).
    pub fn on_deny(mut self, respond: impl Fn(CopFailure, &Context) -> Response + Send + Sync + 'static) -> Cop {
        self.on_deny = Some(Arc::new(respond));
        self
    }

    /// Why the request would be rejected, if it would be.
    pub fn check(&self, ctx: &Context) -> Option<CopFailure> {
        if matches!(*ctx.original_method(), Method::GET | Method::HEAD | Method::OPTIONS) {
            return None;
        }
        match header(ctx, "sec-fetch-site").map(str::to_ascii_lowercase).as_deref() {
            None => {}
            Some("same-origin" | "none") => return None,
            Some(_) => return (!self.exempt(ctx)).then_some(CopFailure::CrossOriginRequest),
        }
        let origin = header(ctx, "origin")?;
        if parse_origin(origin).is_some_and(|u| u.host() == ctx.url().host()) {
            return None;
        }
        (!self.exempt(ctx)).then_some(CopFailure::CrossOriginRequestFromOldBrowser)
    }

    fn exempt(&self, ctx: &Context) -> bool {
        if self.bypass.iter().any(|p| matches_bypass(p, ctx)) {
            return true;
        }
        header(ctx, "origin").and_then(parse_origin).is_some_and(|u| self.trusted.contains(&u.origin()))
    }
}

impl Middleware for Cop {
    fn call(&self, ctx: Context, next: Next) -> BoxFuture<Response> {
        let Some(failure) = self.check(&ctx) else {
            return Box::pin(next.run(ctx));
        };
        let res = match &self.on_deny {
            Some(respond) => respond(failure, &ctx),
            None => {
                let message = match failure {
                    CopFailure::CrossOriginRequest => "Forbidden: cross-origin request detected from Sec-Fetch-Site header",
                    CopFailure::CrossOriginRequestFromOldBrowser => {
                        "Forbidden: cross-origin request detected, and/or browser is out of date: Sec-Fetch-Site is missing, and Origin does not match Host"
                    }
                };
                (StatusCode::FORBIDDEN, message).into_response()
            }
        };
        Box::pin(async move { res })
    }
}

fn header<'a>(ctx: &'a Context, name: &str) -> Option<&'a str> {
    ctx.header(name).map(str::trim).filter(|v| !v.is_empty())
}

/// A URL with a host and no credentials.
fn parse_origin(origin: &str) -> Option<rmx_url::Url> {
    let authority = origin.split_once("://")?.1.split(['/', '?', '#']).next()?;
    if authority.contains('@') {
        return None;
    }
    rmx_url::Url::parse(origin).ok().filter(|u| !u.host().is_empty())
}

fn is_request_method(m: &str) -> Option<Method> {
    matches!(m, "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS").then(|| Method::from_bytes(m.as_bytes()).ok()).flatten()
}

fn parse_bypass(pattern: &str) -> BypassPattern {
    let trimmed = pattern.trim();
    assert!(!trimmed.is_empty(), "bypass pattern must not be empty");
    let mut method = None;
    let mut pathname = trimmed;
    if let Some((m, rest)) = trimmed.split_once(char::is_whitespace) {
        let rest = rest.trim_start();
        if !m.is_empty() && m.bytes().all(|b| b.is_ascii_uppercase()) && rest.starts_with('/') {
            method = Some(is_request_method(m).unwrap_or_else(|| panic!("invalid request method in bypass pattern {pattern:?}")));
            pathname = rest;
        }
    }
    assert!(pathname.starts_with('/'), "invalid bypass pattern {pattern:?}: path must start with \"/\"");
    assert!(
        !pathname.contains(['?', '#']),
        "invalid bypass pattern {pattern:?}: query strings and fragments are not supported"
    );
    let subtree = pathname.ends_with('/');
    let normalized = if pathname.len() > 1 && subtree { &pathname[..pathname.len() - 1] } else { pathname };
    let raw: Vec<&str> = if normalized == "/" { Vec::new() } else { normalized[1..].split('/').collect() };
    let last = raw.len().saturating_sub(1);
    let segments = raw
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            assert!(!seg.is_empty(), "invalid bypass pattern {pattern:?}: empty path segments are not allowed");
            let Some(name) = seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')) else {
                return Segment::Static(seg.to_string());
            };
            assert!(!name.is_empty(), "invalid bypass pattern {pattern:?}: empty wildcards are not allowed");
            assert!(name != "$", "invalid bypass pattern {pattern:?}: \"{{$}}\" is not supported in cop-middleware");
            match name.strip_suffix("...") {
                Some(rest_name) => {
                    assert!(i == last, "invalid bypass pattern {pattern:?}: tail wildcards must be last");
                    assert!(!rest_name.is_empty(), "invalid bypass pattern {pattern:?}: tail wildcards require a name");
                    Segment::Rest
                }
                None => Segment::Wildcard,
            }
        })
        .collect();
    BypassPattern { method, pathname: pathname.to_string(), segments, subtree }
}

fn matches_bypass(p: &BypassPattern, ctx: &Context) -> bool {
    if p.method.as_ref().is_some_and(|m| m != ctx.original_method()) {
        return false;
    }
    let path = ctx.url().pathname();
    let trailing = path.len() > 1 && path.ends_with('/');
    let normalized = if trailing { &path[..path.len() - 1] } else { path };
    let segments: Vec<&str> = if normalized == "/" || normalized.is_empty() { Vec::new() } else { normalized[1..].split('/').collect() };
    for (i, seg) in p.segments.iter().enumerate() {
        match (seg, segments.get(i)) {
            (Segment::Rest, _) => return true,
            (_, None) => return false,
            (Segment::Static(s), Some(actual)) if s != actual => return false,
            _ => {}
        }
    }
    if p.subtree {
        if segments.len() == p.segments.len() {
            return p.pathname == "/" || trailing;
        }
        return segments.len() > p.segments.len();
    }
    segments.len() == p.segments.len() && !trailing
}

#[cfg(test)]
mod tests {
    use rmx_fetch::{block_on, Body};
    use rmx_fetch_router::Router;

    use super::*;

    fn status(cop: Cop, method: &str, url: &str, headers: &[(&str, &str)]) -> u16 {
        let mut router = Router::new().with(cop);
        router.any("/*path", |_ctx: Context| async { "ok" });
        router.any("/", |_ctx: Context| async { "ok" });
        let mut req = http::Request::builder().method(method).uri(url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        block_on(router.fetch(req.body(Body::empty()).unwrap())).status().as_u16()
    }

    #[test]
    fn sec_fetch_site_decides_for_modern_browsers() {
        assert_eq!(status(cop(), "POST", "http://app.test/x", &[("sec-fetch-site", "same-origin")]), 200);
        assert_eq!(status(cop(), "POST", "http://app.test/x", &[("sec-fetch-site", "none")]), 200);
        assert_eq!(status(cop(), "POST", "http://app.test/x", &[("sec-fetch-site", "cross-site")]), 403);
        assert_eq!(status(cop(), "POST", "http://app.test/x", &[("sec-fetch-site", "same-site")]), 403);
        assert_eq!(status(cop(), "GET", "http://app.test/x", &[("sec-fetch-site", "cross-site")]), 200);
    }

    #[test]
    fn origin_fallback_for_old_browsers() {
        assert_eq!(status(cop(), "POST", "http://app.test/x", &[]), 200, "non-browser clients pass");
        assert_eq!(status(cop(), "POST", "http://app.test:8080/x", &[("origin", "http://app.test:8080")]), 200);
        assert_eq!(status(cop(), "POST", "http://app.test/x", &[("origin", "http://evil.test")]), 403);
    }

    #[test]
    fn trusted_origins_and_bypass_patterns() {
        let c = || cop().trusted_origin("https://partner.test").insecure_bypass("POST /hooks/{id}").insecure_bypass("/public/").insecure_bypass("/files/{path...}");
        let cross = [("sec-fetch-site", "cross-site")];
        assert_eq!(status(c(), "POST", "http://app.test/x", &[("sec-fetch-site", "cross-site"), ("origin", "https://partner.test")]), 200);
        assert_eq!(status(c(), "POST", "http://app.test/hooks/1", &cross), 200);
        assert_eq!(status(c(), "PUT", "http://app.test/hooks/1", &cross), 403, "method-specific");
        assert_eq!(status(c(), "POST", "http://app.test/hooks/1/x", &cross), 403, "exact segment count");
        assert_eq!(status(c(), "POST", "http://app.test/public/a/b", &cross), 200, "subtree");
        assert_eq!(status(c(), "POST", "http://app.test/public/", &cross), 200);
        assert_eq!(status(c(), "POST", "http://app.test/public", &cross), 403);
        assert_eq!(status(c(), "POST", "http://app.test/files/a/b/c", &cross), 200);
    }

    #[test]
    #[should_panic(expected = "trailing slash")]
    fn rejects_bad_trusted_origins() {
        let _ = cop().trusted_origin("https://a.test/");
    }

    #[test]
    #[should_panic(expected = "tail wildcards must be last")]
    fn rejects_bad_patterns() {
        let _ = cop().insecure_bypass("/a/{rest...}/b");
    }
}
