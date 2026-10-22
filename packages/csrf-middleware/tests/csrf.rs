//! Remix's `csrf-middleware/src/lib/csrf.test.ts`.

use rmx_cookie::Cookie;
use rmx_fetch::{block_on, Body, Response};
use rmx_csrf_middleware::{csrf, csrf_token, Csrf};
use rmx_form_data_middleware::form_data;
use rmx_method_override_middleware::method_override;
use rmx_session_middleware::session;
use rmx_router::{Context, Middleware, Router};
use rmx_session::CookieStorage;

fn session_mw() -> impl Middleware {
    session(Cookie::new("__session").secrets(vec!["secret1".into()]), CookieStorage::new())
}

/// The token page (`GET /`) and an `ok` POST behind session, the given middleware and csrf.
fn token_router(csrf: Csrf, with_form_data: bool) -> Router {
    let mut router = Router::new().with(session_mw());
    if with_form_data {
        router = router.with(form_data());
    }
    let mut router = router.with(csrf);
    router.get("/", |ctx: Context| async move { csrf_token(&ctx) });
    router.post("/", |_ctx: Context| async { "ok" });
    router
}

/// Remix's `createRequest`: carry the response's cookies into a new request.
fn create_request(from: &Response, method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> http::Request<Body> {
    let cookies: Vec<&str> =
        from.headers().get_all("set-cookie").iter().map(|v| v.to_str().unwrap().split(';').next().unwrap()).collect();
    let mut req = http::Request::builder().method(method).uri(url);
    if !cookies.is_empty() {
        req = req.header("cookie", cookies.join("; "));
    }
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    req.body(Body::from(body.to_string())).unwrap()
}

fn get(router: &Router, url: &str) -> Response {
    block_on(router.fetch(http::Request::get(url).body(Body::empty()).unwrap()))
}

/// The status and body of a response.
fn read(res: Response) -> (u16, String) {
    (res.status().as_u16(), block_on(res.into_body().text()).unwrap())
}

/// Fetch the token page, returning the response (for its cookie) and the token.
fn token(router: &Router, url: &str) -> (Response, String) {
    let res = get(router, url);
    let headers = res.headers().clone();
    let token = block_on(res.into_body().text()).unwrap();
    let mut carrier = Response::new(Body::empty());
    *carrier.headers_mut() = headers;
    (carrier, token)
}

#[test]
fn creates_and_stores_a_token_on_safe_requests() {
    let router = token_router(csrf(), false);
    let (status, token) = read(get(&router, "https://remix.run/"));
    assert_eq!(status, 200);
    assert_eq!(token.len(), 64);
}

#[test]
fn accepts_a_valid_token_from_request_headers() {
    let router = token_router(csrf(), false);
    let (from, token) = token(&router, "https://remix.run/");
    let req = create_request(&from, "POST", "https://remix.run/", &[("X-Csrf-Token", &token)], "");
    assert_eq!(read(block_on(router.fetch(req))), (200, "ok".into()));
}

#[test]
fn rejects_unsafe_requests_with_a_missing_token() {
    let router = token_router(csrf(), false);
    let (from, _) = token(&router, "https://remix.run/");
    let req = create_request(&from, "POST", "https://remix.run/", &[], "");
    assert_eq!(read(block_on(router.fetch(req))), (403, "Forbidden: missing CSRF token".into()));
}

#[test]
fn rejects_tokens_supplied_only_in_the_query_string_by_default() {
    let router = token_router(csrf(), false);
    let (from, token) = token(&router, "https://remix.run/");
    let req = create_request(&from, "POST", &format!("https://remix.run/?_csrf={token}"), &[], "");
    assert_eq!(read(block_on(router.fetch(req))), (403, "Forbidden: missing CSRF token".into()));
}

#[test]
fn does_not_fall_back_to_the_query_string_when_configured_headers_and_form_fields_are_empty() {
    let router = token_router(csrf().field_name("token").header_names(["X-Custom-Csrf"]), true);
    let (from, token) = token(&router, "https://remix.run/");
    let req = create_request(
        &from,
        "POST",
        &format!("https://remix.run/?token={token}"),
        &[("Origin", "https://remix.run"), ("Content-Type", "application/x-www-form-urlencoded"), ("X-Custom-Csrf", " ")],
        "token=+",
    );
    assert_eq!(read(block_on(router.fetch(req))), (403, "Forbidden: missing CSRF token".into()));
}

#[test]
fn checks_the_original_request_method_after_a_safe_method_override() {
    let mut router = Router::new().with(session_mw()).with(form_data()).with(method_override()).with(csrf());
    router.get("/", |_ctx: Context| async { "ok" });
    let req = http::Request::post("https://remix.run/")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from("_method=GET"))
        .unwrap();
    assert_eq!(read(block_on(router.fetch(req))), (403, "Forbidden: missing CSRF token".into()));
}

#[test]
fn supports_unsafe_method_overrides_with_a_valid_token() {
    let mut router = Router::new().with(session_mw()).with(form_data()).with(method_override()).with(csrf());
    router.get("/token", |ctx: Context| async move { csrf_token(&ctx) });
    router.delete("/", |_ctx: Context| async { "Deleted" });
    let (from, token) = token(&router, "https://remix.run/token");
    let req = create_request(
        &from,
        "POST",
        "https://remix.run/",
        &[("Content-Type", "application/x-www-form-urlencoded")],
        &format!("_method=DELETE&_csrf={token}"),
    );
    assert_eq!(read(block_on(router.fetch(req))), (200, "Deleted".into()));
}

#[test]
fn rejects_unsafe_requests_with_an_invalid_token() {
    let router = token_router(csrf(), false);
    let (from, _) = token(&router, "https://remix.run/");
    let req = create_request(&from, "POST", "https://remix.run/", &[("X-Csrf-Token", "invalid-token")], "");
    assert_eq!(read(block_on(router.fetch(req))), (403, "Forbidden: invalid CSRF token".into()));
}

#[test]
fn validates_form_field_tokens_when_form_data_middleware_is_enabled() {
    let router = token_router(csrf(), true);
    let (from, token) = token(&router, "https://remix.run/");
    let req = create_request(
        &from,
        "POST",
        "https://remix.run/",
        &[("Content-Type", "application/x-www-form-urlencoded")],
        &format!("_csrf={token}"),
    );
    assert_eq!(read(block_on(router.fetch(req))), (200, "ok".into()));
}

#[test]
fn rejects_unsafe_requests_with_invalid_origin_by_default() {
    let router = token_router(csrf(), false);
    let (from, token) = token(&router, "https://remix.run/");
    let req =
        create_request(&from, "POST", "https://remix.run/", &[("Origin", "https://evil.example"), ("X-Csrf-Token", &token)], "");
    assert_eq!(read(block_on(router.fetch(req))), (403, "Forbidden: invalid CSRF origin".into()));
}

#[test]
fn supports_custom_origin_matching() {
    let router = token_router(csrf().origins(["https://admin.example.com"]), false);
    let (from, token) = token(&router, "https://remix.run/");
    let req = create_request(
        &from,
        "POST",
        "https://remix.run/",
        &[("Origin", "https://admin.example.com"), ("X-Csrf-Token", &token)],
        "",
    );
    assert_eq!(read(block_on(router.fetch(req))), (200, "ok".into()));
}

#[test]
fn supports_custom_token_value_extractors() {
    let router = token_router(csrf().value(|ctx| ctx.header("X-Custom-Csrf").map(str::to_string)), false);
    let (from, token) = token(&router, "https://remix.run/");
    let req = create_request(&from, "POST", "https://remix.run/", &[("X-Custom-Csrf", &token)], "");
    assert_eq!(read(block_on(router.fetch(req))), (200, "ok".into()));
}

#[test]
fn supports_query_string_tokens_through_a_custom_token_resolver() {
    let router = token_router(csrf().value(|ctx| ctx.url().search_params().get("_csrf").map(str::to_string)), false);
    let (from, token) = token(&router, "https://remix.run/");
    let req = create_request(
        &from,
        "POST",
        &format!("https://remix.run/?_csrf=%20{token}%20"),
        &[("X-Csrf-Token", "ignored-token")],
        "",
    );
    assert_eq!(read(block_on(router.fetch(req))), (200, "ok".into()));
}

#[test]
#[should_panic(expected = "csrf middleware requires session() middleware to run before it")]
fn throws_when_session_middleware_is_not_registered() {
    let mut router = Router::new().with(csrf());
    router.post("/", |_ctx: Context| async { "ok" });
    block_on(router.fetch(http::Request::post("https://remix.run/").body(Body::empty()).unwrap()));
}

#[test]
#[should_panic(expected = "Session is not started. Use session() middleware before csrf().")]
fn get_csrf_token_requires_a_session() {
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move { csrf_token(&ctx) });
    get(&router, "https://remix.run/");
}
