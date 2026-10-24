//! Remix's `cop-middleware/src/lib/cop.test.ts`.

use http::StatusCode;
use rsmix_fetch::{block_on, Body, Response};
use rsmix_cop_middleware::{cop, Cop};
use rsmix_form_data_middleware::form_data;
use rsmix_method_override_middleware::method_override;
use rsmix_fetch_router::{Context, IntoResponse, Router};

async fn ok(_ctx: Context) -> &'static str {
    "ok"
}

fn create_test_router(cop: Cop) -> Router {
    let mut router = Router::new().with(cop);
    router.get("/", ok);
    router.head("/", ok);
    router.options("/", ok);
    router.post("/", ok);
    router.put("/", ok);
    router.post("/bypass/", ok);
    router.post("/bypass/*path", ok);
    router.post("/only/:foo", ok);
    router.post("/no-trailing", ok);
    router.post("/yes-trailing/", ok);
    router.post("/post-only/", ok);
    router.post("/put-only/", ok);
    router.post("/get-only/", ok);
    router
}

fn fetch(router: &Router, method: &str, pathname: &str, headers: &[(&str, &str)], body: &str) -> Response {
    let mut req = http::Request::builder().method(method).uri(format!("https://example.com{pathname}"));
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    block_on(router.fetch(req.body(Body::from(body.to_string())).unwrap()))
}

fn text(res: Response) -> String {
    block_on(res.into_body().text()).unwrap()
}

const CROSS_SITE: (&str, &str) = ("Sec-Fetch-Site", "cross-site");
const ATTACKER: (&str, &str) = ("Origin", "https://attacker.example");
const FORM: (&str, &str) = ("Content-Type", "application/x-www-form-urlencoded");

#[test]
fn allows_same_origin_requests_for_unsafe_methods() {
    let res = fetch(&create_test_router(cop()), "POST", "/", &[("Sec-Fetch-Site", "same-origin")], "");
    assert_eq!(res.status(), 200);
}

#[test]
fn allows_browser_initiated_top_level_requests_with_sec_fetch_site_none() {
    let res = fetch(&create_test_router(cop()), "POST", "/", &[("Sec-Fetch-Site", "none")], "");
    assert_eq!(res.status(), 200);
}

#[test]
fn rejects_unsafe_cross_site_requests_from_sec_fetch_site() {
    let res = fetch(&create_test_router(cop()), "POST", "/", &[CROSS_SITE], "");
    assert_eq!(res.status(), 403);
    assert_eq!(text(res), "Forbidden: cross-origin request detected from Sec-Fetch-Site header");
}

#[test]
fn rejects_same_site_requests_to_preserve_origin_level_guarantees() {
    let res = fetch(&create_test_router(cop()), "POST", "/", &[("Sec-Fetch-Site", "same-site")], "");
    assert_eq!(res.status(), 403);
}

#[test]
fn falls_back_to_origin_when_sec_fetch_site_is_missing() {
    let router = create_test_router(cop());
    let allowed = fetch(&router, "POST", "/", &[("Origin", "https://example.com")], "");
    let denied = fetch(&router, "POST", "/", &[ATTACKER], "");
    assert_eq!(allowed.status(), 200);
    assert_eq!(denied.status(), 403);
    assert_eq!(
        text(denied),
        "Forbidden: cross-origin request detected, and/or browser is out of date: Sec-Fetch-Site is missing, and Origin does not match Host"
    );
}

#[test]
fn allows_requests_with_no_browser_provenance_headers() {
    assert_eq!(fetch(&create_test_router(cop()), "POST", "/", &[], "").status(), 200);
}

#[test]
fn allows_safe_methods_even_for_cross_site_requests() {
    assert_eq!(fetch(&create_test_router(cop()), "GET", "/", &[CROSS_SITE], "").status(), 200);
}

#[test]
fn checks_the_original_request_method_after_a_safe_method_override() {
    let mut router = Router::new().with(form_data()).with(method_override()).with(cop());
    router.get("/", ok);
    let res = fetch(&router, "POST", "/", &[FORM, CROSS_SITE], "_method=GET");
    assert_eq!(res.status(), 403);
    assert_eq!(text(res), "Forbidden: cross-origin request detected from Sec-Fetch-Site header");
}

#[test]
fn uses_the_original_request_method_for_method_specific_bypass_patterns() {
    let mut router = Router::new().with(form_data()).with(method_override()).with(cop().insecure_bypass("GET /"));
    router.get("/", ok);
    let res = fetch(&router, "POST", "/", &[FORM, CROSS_SITE], "_method=GET");
    assert_eq!(res.status(), 403);
    assert_eq!(text(res), "Forbidden: cross-origin request detected from Sec-Fetch-Site header");
}

#[test]
fn allows_bypass_patterns_matching_the_original_method_after_an_unsafe_method_override() {
    let mut router = Router::new().with(form_data()).with(method_override()).with(cop().insecure_bypass("POST /"));
    router.delete("/", |_ctx: Context| async { "Deleted" });
    let res = fetch(&router, "POST", "/", &[FORM, CROSS_SITE], "_method=DELETE");
    assert_eq!(res.status(), 200);
    assert_eq!(text(res), "Deleted");
}

#[test]
fn supports_trusted_origins() {
    let router = create_test_router(cop().trusted_origin("https://trusted.example"));
    let res = fetch(&router, "POST", "/", &[("Origin", "https://trusted.example"), CROSS_SITE], "");
    assert_eq!(res.status(), 200);
}

#[test]
fn supports_insecure_bypass_patterns() {
    let router =
        create_test_router(cop().insecure_bypass("/bypass/").insecure_bypass("/only/{foo}").insecure_bypass("POST /post-only/"));
    assert_eq!(fetch(&router, "POST", "/bypass/", &[ATTACKER, CROSS_SITE], "").status(), 200);
    assert_eq!(fetch(&router, "POST", "/only/123", &[ATTACKER], "").status(), 200);
    assert_eq!(fetch(&router, "POST", "/post-only/", &[ATTACKER], "").status(), 200);
}

#[test]
fn does_not_bypass_paths_that_only_differ_by_trailing_slash_or_method() {
    let router = create_test_router(
        cop()
            .insecure_bypass("/no-trailing")
            .insecure_bypass("/yes-trailing/")
            .insecure_bypass("PUT /put-only/")
            .insecure_bypass("GET /get-only/"),
    );
    assert_eq!(fetch(&router, "POST", "/no-trailing/", &[ATTACKER], "").status(), 403);
    assert_eq!(fetch(&router, "POST", "/yes-trailing", &[ATTACKER], "").status(), 403);
    assert_eq!(fetch(&router, "POST", "/put-only/", &[ATTACKER], "").status(), 403);
    assert_eq!(fetch(&router, "POST", "/get-only/", &[ATTACKER], "").status(), 403);
}

#[test]
fn supports_custom_deny_handlers() {
    let router = create_test_router(cop().on_deny(|_, _| (StatusCode::IM_A_TEAPOT, "custom deny").into_response()));
    let res = fetch(&router, "POST", "/", &[CROSS_SITE], "");
    assert_eq!(res.status(), 418);
    assert_eq!(text(res), "custom deny");
}

fn panic_message(f: impl FnOnce() -> Cop + std::panic::UnwindSafe) -> String {
    let payload = std::panic::catch_unwind(f).err().expect("expected a panic");
    payload.downcast_ref::<String>().cloned().or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string())).unwrap()
}

#[test]
fn validates_trusted_origins_and_bypass_patterns() {
    assert_eq!(
        panic_message(|| cop().trusted_origin("https://example.com/")),
        r#"invalid origin "https://example.com/": trailing slash is not allowed"#
    );
    assert_eq!(panic_message(|| cop().trusted_origin("null")), r#"invalid origin "null""#);
    assert_eq!(
        panic_message(|| cop().insecure_bypass("POST foo")),
        r#"invalid bypass pattern "POST foo": path must start with "/""#
    );
    assert_eq!(
        panic_message(|| cop().insecure_bypass("/foo/{...}/bar")),
        r#"invalid bypass pattern "/foo/{...}/bar": tail wildcards must be last"#
    );
    // The rest of Remix's messages, which its test only checks by throwing.
    for (pattern, message) in [
        ("  ", "bypass pattern must not be empty"),
        ("PATCHY /x", r#"invalid request method in bypass pattern "PATCHY /x""#),
        ("/x?y", r#"invalid bypass pattern "/x?y": query strings and fragments are not supported"#),
        ("/a//b", r#"invalid bypass pattern "/a//b": empty path segments are not allowed"#),
        ("/a/{}", r#"invalid bypass pattern "/a/{}": empty wildcards are not allowed"#),
        ("/a/{$}", r#"invalid bypass pattern "/a/{$}": "{$}" is not supported in cop-middleware"#),
        ("/a/{...}", r#"invalid bypass pattern "/a/{...}": tail wildcards require a name"#),
    ] {
        assert_eq!(panic_message(move || cop().insecure_bypass(pattern)), message);
    }
    for (origin, message) in [
        (" ", "trusted origin must not be empty"),
        ("https://a.example/x", r#"invalid origin "https://a.example/x": path, query, and fragment are not allowed"#),
        ("https://a.example?q", r#"invalid origin "https://a.example?q": path, query, and fragment are not allowed"#),
        ("https://u:p@a.example", r#"invalid origin "https://u:p@a.example""#),
    ] {
        assert_eq!(panic_message(move || cop().trusted_origin(origin)), message);
    }
}
