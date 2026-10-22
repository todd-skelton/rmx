//! Port of Remix's `response/src/lib/redirect.test.ts` (`createRedirectResponse` is
//! `response::redirect` and `redirect_with`).

use http::{header, HeaderMap, HeaderValue, StatusCode};
use rmx_response::{redirect, redirect_with, ResponseInit};
use rmx_fetch::Url;

#[test]
fn creates_a_redirect_with_default_302_status() {
    let response = redirect("/home");

    assert_eq!(response.status(), 302);
    assert_eq!(response.headers()["location"], "/home");
}

#[test]
fn creates_a_redirect_with_custom_status_code() {
    let response = redirect_with("/login", StatusCode::MOVED_PERMANENTLY);

    assert_eq!(response.status(), 301);
    assert_eq!(response.headers()["location"], "/login");
}

#[test]
fn accepts_response_init_object_as_second_parameter() {
    let response = redirect_with("/dashboard", ResponseInit::status(StatusCode::TEMPORARY_REDIRECT).header("X-Redirect-Reason", "authentication"));

    assert_eq!(response.status(), 307);
    assert_eq!(response.headers()["location"], "/dashboard");
    assert_eq!(response.headers()["x-redirect-reason"], "authentication");
}

#[test]
fn handles_relative_urls() {
    let response = redirect_with("../parent", StatusCode::SEE_OTHER);

    assert_eq!(response.status(), 303);
    assert_eq!(response.headers()["location"], "../parent");
}

#[test]
fn allows_overriding_location_header() {
    let response = redirect_with("/default", ResponseInit::default().header("Location", "/override"));

    assert_eq!(response.status(), 302);
    assert_eq!(response.headers()["location"], "/override");
}

#[test]
fn allows_overriding_location_header_with_headers_object() {
    let mut headers = HeaderMap::new();
    headers.insert(header::LOCATION, HeaderValue::from_static("/custom-location"));
    headers.insert("x-custom", HeaderValue::from_static("value"));

    let response = redirect_with("/default", ResponseInit { status: None, headers });

    assert_eq!(response.status(), 302);
    assert_eq!(response.headers().get_all(header::LOCATION).iter().count(), 1);
    assert_eq!(response.headers()["location"], "/custom-location");
    assert_eq!(response.headers()["x-custom"], "value");
}

#[test]
fn accepts_a_url_object() {
    let url = Url::parse("https://example.com/login").unwrap();
    let response = redirect_with(&url.href(), StatusCode::MOVED_PERMANENTLY);

    assert_eq!(response.status(), 301);
    assert_eq!(response.headers()["location"], "https://example.com/login");
}
