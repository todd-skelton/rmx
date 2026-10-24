//! Remix's `cors-middleware/src/lib/cors.test.ts`. Remix's RegExp and boolean-returning origin
//! resolvers are closures passed to `origin_fn`.

use http::header::VARY;
use http::{HeaderValue, StatusCode};
use rsmix_fetch::{block_on, Body, Response};
use rsmix_headers::Vary;
use rsmix_cors_middleware::{cors, Cors};
use rsmix_fetch_router::{Context, Router};

fn router(cors: Cors) -> Router {
    let mut router = Router::new().with(cors);
    router.get("/", |_ctx: Context| async { "ok" });
    router
}

fn fetch(router: &Router, method: &str, headers: &[(&str, &str)]) -> Response {
    let mut req = http::Request::builder().method(method).uri("https://remix.run/");
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    block_on(router.fetch(req.body(Body::empty()).unwrap()))
}

fn header<'a>(res: &'a Response, name: &str) -> Option<&'a str> {
    res.headers().get(name).map(|v| v.to_str().unwrap())
}

fn vary(res: &Response) -> Vary {
    Vary::parse(header(res, "vary"))
}

fn text(res: Response) -> String {
    block_on(res.into_body().text()).unwrap()
}

const ORIGIN: (&str, &str) = ("origin", "https://example.com");

fn preflight(method: &'static str) -> [(&'static str, &'static str); 2] {
    [ORIGIN, ("access-control-request-method", method)]
}

#[test]
fn adds_wildcard_cors_response_headers_by_default() {
    let res = fetch(&router(cors()), "GET", &[ORIGIN]);
    assert_eq!(res.status(), 200);
    assert_eq!(header(&res, "access-control-allow-origin"), Some("*"));
    assert_eq!(header(&res, "vary"), None);
}

#[test]
fn keeps_the_wildcard_default_when_credentials_are_enabled() {
    let res = fetch(&router(cors().credentials(true)), "GET", &[ORIGIN]);
    assert_eq!(header(&res, "access-control-allow-origin"), Some("*"));
    assert_eq!(header(&res, "access-control-allow-credentials"), Some("true"));
    assert!(!vary(&res).has("Origin"));
}

#[test]
fn reflects_an_explicit_wildcard_origin_when_credentials_are_enabled() {
    let res = fetch(&router(cors().any_origin().credentials(true)), "GET", &[ORIGIN]);
    assert_eq!(header(&res, "access-control-allow-origin"), Some("https://example.com"));
    assert_eq!(header(&res, "access-control-allow-credentials"), Some("true"));
    assert!(vary(&res).has("Origin"));

    // `origin: ['*']` is explicit too.
    let res = fetch(&router(cors().origins(["*"]).credentials(true)), "GET", &[ORIGIN]);
    assert_eq!(header(&res, "access-control-allow-origin"), Some("https://example.com"));
}

#[test]
fn short_circuits_preflight_requests() {
    let res = fetch(
        &router(cors()),
        "OPTIONS",
        &[ORIGIN, ("access-control-request-method", "PATCH"), ("access-control-request-headers", "x-api-key,content-type")],
    );
    assert_eq!(res.status(), 204);
    assert_eq!(header(&res, "access-control-allow-origin"), Some("*"));
    assert_eq!(header(&res, "access-control-allow-methods"), Some("GET, HEAD, PUT, PATCH, POST, DELETE"));
    assert_eq!(header(&res, "access-control-allow-headers"), Some("x-api-key,content-type"));
    let vary = vary(&res);
    assert!(vary.has("Access-Control-Request-Method"));
    assert!(vary.has("Access-Control-Request-Headers"));
}

#[test]
fn reflecting_preflights_vary_on_requested_headers_only_when_sent() {
    // Not in Remix's tests; its resolveAllowedHeaders skips Vary for a missing or blank header.
    for headers in [&preflight("PATCH")[..], &[ORIGIN, ("access-control-request-method", "PATCH"), ("access-control-request-headers", " ")]] {
        let res = fetch(&router(cors()), "OPTIONS", headers);
        assert_eq!(header(&res, "access-control-allow-headers"), None);
        let vary = vary(&res);
        assert!(vary.has("Access-Control-Request-Method"));
        assert!(!vary.has("Access-Control-Request-Headers"));
    }
}

#[test]
fn keeps_the_wildcard_default_for_preflight_requests_with_credentials() {
    let res = fetch(&router(cors().credentials(true)), "OPTIONS", &preflight("PATCH"));
    assert_eq!(res.status(), 204);
    assert_eq!(header(&res, "access-control-allow-origin"), Some("*"));
    assert_eq!(header(&res, "access-control-allow-credentials"), Some("true"));
    let vary = vary(&res);
    assert!(!vary.has("Origin"));
    assert!(vary.has("Access-Control-Request-Method"));
}

#[test]
fn reflects_an_explicit_wildcard_origin_for_preflight_requests_with_credentials() {
    let res = fetch(&router(cors().any_origin().credentials(true)), "OPTIONS", &preflight("PATCH"));
    assert_eq!(res.status(), 204);
    assert_eq!(header(&res, "access-control-allow-origin"), Some("https://example.com"));
    assert_eq!(header(&res, "access-control-allow-credentials"), Some("true"));
    let vary = vary(&res);
    assert!(vary.has("Origin"));
    assert!(vary.has("Access-Control-Request-Method"));
}

#[test]
fn continues_preflight_requests_when_preflight_continue_is_true() {
    let mut router = Router::new().with(cors().preflight_continue(true));
    router.options("/", |_ctx: Context| async { "continued" });
    let res = fetch(&router, "OPTIONS", &preflight("POST"));
    assert_eq!(res.status(), 200);
    assert_eq!(header(&res, "access-control-allow-origin"), Some("*"));
    assert_eq!(header(&res, "access-control-allow-methods"), Some("GET, HEAD, PUT, PATCH, POST, DELETE"));
    assert_eq!(text(res), "continued");
}

#[test]
fn blocks_disallowed_origins_in_preflight_requests() {
    let res = fetch(
        &router(cors().origins(["https://allowed.example"])),
        "OPTIONS",
        &[("origin", "https://blocked.example"), ("access-control-request-method", "POST")],
    );
    assert_eq!(res.status(), 403);
    assert_eq!(header(&res, "access-control-allow-origin"), None);
}

#[test]
fn supports_function_based_dynamic_origin_checks() {
    let router = router(cors().origin_fn(|origin, _| origin.ends_with(".trusted.example").then(|| origin.to_string())));
    let allowed = fetch(&router, "GET", &[("origin", "https://api.trusted.example")]);
    let blocked = fetch(&router, "GET", &[("origin", "https://evil.example")]);
    assert_eq!(header(&allowed, "access-control-allow-origin"), Some("https://api.trusted.example"));
    assert_eq!(header(&blocked, "access-control-allow-origin"), None);
}

#[test]
fn matches_pattern_origins_consistently_across_repeated_requests() {
    // Remix guards against a `/g` RegExp's `lastIndex`; a Rust predicate has no such state.
    let router = router(cors().origin_fn(|origin, _| origin.ends_with(".trusted.example").then(|| origin.to_string())));
    for _ in 0..2 {
        let res = fetch(&router, "GET", &[("origin", "https://api.trusted.example")]);
        assert_eq!(header(&res, "access-control-allow-origin"), Some("https://api.trusted.example"));
    }
}

#[test]
fn origin_resolvers_can_answer_with_a_wildcard_or_another_origin() {
    // Remix's resolver may return '*' or any string; both become the header value.
    let res = fetch(&router(cors().origin_fn(|_, _| Some("*".into()))), "GET", &[ORIGIN]);
    assert_eq!(header(&res, "access-control-allow-origin"), Some("*"));
    assert!(!vary(&res).has("Origin"));
    let res = fetch(&router(cors().origin_fn(|_, _| Some("https://other.example".into()))), "GET", &[ORIGIN]);
    assert_eq!(header(&res, "access-control-allow-origin"), Some("https://other.example"));
    assert!(vary(&res).has("Origin"));
}

#[test]
fn supports_explicit_allowed_headers_and_max_age_for_preflight_requests() {
    let res = fetch(
        &router(cors().allowed_headers(["Authorization", "Content-Type"]).max_age(600)),
        "OPTIONS",
        &[ORIGIN, ("access-control-request-method", "POST"), ("access-control-request-headers", "x-custom-header")],
    );
    assert_eq!(res.status(), 204);
    assert_eq!(header(&res, "access-control-allow-headers"), Some("Authorization, Content-Type"));
    assert_eq!(header(&res, "access-control-max-age"), Some("600"));
    assert!(!vary(&res).has("Access-Control-Request-Headers"));
}

#[test]
fn adds_vary_for_function_based_allowed_header_resolvers() {
    let res = fetch(
        &router(cors().allowed_headers_fn(|_| Some(vec!["Authorization".into()]))),
        "OPTIONS",
        &[ORIGIN, ("access-control-request-method", "POST"), ("access-control-request-headers", "x-api-key")],
    );
    assert_eq!(res.status(), 204);
    assert_eq!(header(&res, "access-control-allow-headers"), Some("Authorization"));
    assert!(vary(&res).has("Access-Control-Request-Headers"));
}

#[test]
fn falls_back_to_requested_headers_for_function_based_allowed_header_resolvers() {
    let res = fetch(
        &router(cors().allowed_headers_fn(|_| None)),
        "OPTIONS",
        &[ORIGIN, ("access-control-request-method", "POST"), ("access-control-request-headers", "x-api-key,content-type")],
    );
    assert_eq!(res.status(), 204);
    assert_eq!(header(&res, "access-control-allow-headers"), Some("x-api-key,content-type"));
    assert!(vary(&res).has("Access-Control-Request-Headers"));
}

#[test]
fn sets_access_control_allow_private_network_when_requested() {
    let res = fetch(
        &router(cors().allow_private_network(true)),
        "OPTIONS",
        &[ORIGIN, ("access-control-request-method", "POST"), ("access-control-request-private-network", "true")],
    );
    assert_eq!(res.status(), 204);
    assert_eq!(header(&res, "access-control-allow-origin"), Some("*"));
    assert_eq!(header(&res, "access-control-allow-private-network"), Some("true"));
    assert!(vary(&res).has("Access-Control-Request-Private-Network"));
}

#[test]
fn does_not_allow_private_network_requests_by_default() {
    let res = fetch(
        &Router::new().with(cors()),
        "OPTIONS",
        &[ORIGIN, ("access-control-request-method", "POST"), ("access-control-request-private-network", "true")],
    );
    assert_eq!(res.status(), 204);
    assert_eq!(header(&res, "access-control-allow-origin"), Some("*"));
    assert_eq!(header(&res, "access-control-allow-private-network"), None);
    assert!(!vary(&res).has("Access-Control-Request-Private-Network"));
}

#[test]
fn varies_on_private_network_requests_when_the_request_header_is_absent() {
    let res = fetch(&Router::new().with(cors().allow_private_network(true)), "OPTIONS", &preflight("POST"));
    assert_eq!(res.status(), 204);
    assert_eq!(header(&res, "access-control-allow-origin"), Some("*"));
    assert_eq!(header(&res, "access-control-allow-private-network"), None);
    assert!(vary(&res).has("Access-Control-Request-Private-Network"));
}

#[test]
fn varies_on_private_network_requests_when_the_request_header_is_false() {
    let res = fetch(
        &Router::new().with(cors().allow_private_network(true)),
        "OPTIONS",
        &[ORIGIN, ("access-control-request-method", "POST"), ("access-control-request-private-network", "false")],
    );
    assert_eq!(res.status(), 204);
    assert_eq!(header(&res, "access-control-allow-private-network"), None);
    assert!(vary(&res).has("Access-Control-Request-Private-Network"));
}

#[test]
fn merges_private_network_vary_values_for_continued_preflight_requests() {
    let mut router = Router::new().with(cors().allow_private_network(true).preflight_continue(true));
    router.options("/", |_ctx: Context| async {
        let mut res = Response::new(Body::from("continued"));
        res.headers_mut().insert(VARY, HeaderValue::from_static("Accept-Encoding"));
        res
    });
    let res = fetch(&router, "OPTIONS", &preflight("POST"));
    assert_eq!(res.status(), 200);
    assert_eq!(header(&res, "access-control-allow-private-network"), None);
    let vary = vary(&res);
    assert!(vary.has("Accept-Encoding"));
    assert!(vary.has("Access-Control-Request-Method"));
    assert!(vary.has("Access-Control-Request-Private-Network"));
    assert_eq!(text(res), "continued");
}

#[test]
fn applies_the_origin_policy_before_allowing_private_network_requests() {
    let router = Router::new().with(cors().origins(["https://allowed.example"]).allow_private_network(true));
    let request = |origin| {
        fetch(
            &router,
            "OPTIONS",
            &[("origin", origin), ("access-control-request-method", "POST"), ("access-control-request-private-network", "true")],
        )
    };
    let allowed = request("https://allowed.example");
    let blocked = request("https://blocked.example");
    assert_eq!(allowed.status(), 204);
    assert_eq!(header(&allowed, "access-control-allow-origin"), Some("https://allowed.example"));
    assert_eq!(header(&allowed, "access-control-allow-private-network"), Some("true"));
    assert_eq!(blocked.status(), StatusCode::FORBIDDEN);
    assert_eq!(header(&blocked, "access-control-allow-origin"), None);
    assert_eq!(header(&blocked, "access-control-allow-private-network"), None);
}

#[test]
fn sets_access_control_expose_headers_for_actual_requests() {
    let res = fetch(&router(cors().exposed_headers(["X-Request-Id", "X-Trace-Id"])), "GET", &[ORIGIN]);
    assert_eq!(res.status(), 200);
    assert_eq!(header(&res, "access-control-expose-headers"), Some("X-Request-Id, X-Trace-Id"));
}

#[test]
fn merges_cors_vary_values_with_an_existing_response_vary_header() {
    let mut router = Router::new().with(cors().any_origin().credentials(true));
    router.get("/", |_ctx: Context| async {
        let mut res = Response::new(Body::from("ok"));
        res.headers_mut().insert(VARY, HeaderValue::from_static("Accept-Encoding"));
        res
    });
    let vary = vary(&fetch(&router, "GET", &[ORIGIN]));
    assert!(vary.has("Accept-Encoding"));
    assert!(vary.has("Origin"));
}
