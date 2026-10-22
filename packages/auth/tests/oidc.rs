//! Port of Remix's `providers/oidc.test.ts`. Where Remix expects `router.fetch` to reject
//! (a failed discovery), the login route here answers 400 with the error message.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::*;
use rmx_auth::{oidc_provider, ClientOptions, OidcMetadata, OidcOptions, OidcProfileInput};
use serde::Serialize;
use serde_json::{json, Value};

const ISSUER: &str = "https://issuer.example.com";
const DISCOVERY_URL: &str = "https://issuer.example.com/.well-known/openid-configuration";

fn options() -> OidcOptions {
    OidcOptions::new(ISSUER, ClientOptions::new("client-id", "client-secret", "https://app.example.com/auth/oidc/callback"))
}

fn discovery() -> Value {
    json!({
        "issuer": ISSUER,
        "authorization_endpoint": "https://issuer.example.com/oauth2/v1/authorize",
        "token_endpoint": "https://issuer.example.com/oauth2/v1/token",
        "userinfo_endpoint": "https://issuer.example.com/oauth2/v1/userinfo",
    })
}

fn metadata(userinfo: bool) -> OidcMetadata {
    OidcMetadata {
        issuer: ISSUER.into(),
        authorization_endpoint: "https://issuer.example.com/oauth2/v1/authorize".into(),
        token_endpoint: "https://issuer.example.com/oauth2/v1/token".into(),
        userinfo_endpoint: userinfo.then(|| "https://issuer.example.com/oauth2/v1/userinfo".into()),
        ..Default::default()
    }
}

#[test]
fn discovers_metadata_and_caches_it_across_login_requests() {
    let discovery_requests = Arc::new(AtomicUsize::new(0));
    let counter = discovery_requests.clone();
    let (fetch, _) = mock(move |seen| match seen.url.as_str() {
        DISCOVERY_URL => {
            counter.fetch_add(1, Ordering::SeqCst);
            json_response(discovery())
        }
        _ => unexpected(seen),
    });
    let router = app(oidc_provider(options(), fetch));

    let location1 = get(&router, "/login", None).location();
    let location2 = get(&router, "/login", None).location();

    assert_eq!(discovery_requests.load(Ordering::SeqCst), 1);
    assert_eq!(location1.origin(), ISSUER);
    assert_eq!(location1.pathname(), "/oauth2/v1/authorize");
    assert_eq!(location1.origin(), location2.origin());
    assert_eq!(location1.pathname(), location2.pathname());
    assert_eq!(location1.search_params().get("scope"), Some("openid profile email"));
    assert_eq!(location2.search_params().get("scope"), Some("openid profile email"));
}

#[test]
fn retries_discovery_after_a_failed_metadata_request() {
    let discovery_requests = Arc::new(AtomicUsize::new(0));
    let counter = discovery_requests.clone();
    let (fetch, _) = mock(move |seen| match seen.url.as_str() {
        DISCOVERY_URL if counter.fetch_add(1, Ordering::SeqCst) == 0 => status_json(500, json!({})),
        DISCOVERY_URL => json_response(discovery()),
        _ => unexpected(seen),
    });
    let router = app(oidc_provider(options(), fetch));

    let failed = get(&router, "/login", None);
    assert_eq!(failed.status, 400);
    assert_eq!(failed.json(), json!({ "error": "Failed to load OIDC metadata for \"oidc\"." }));

    let response = get(&router, "/login", None);
    let location = response.location();

    assert_eq!(response.status, 302);
    assert_eq!(discovery_requests.load(Ordering::SeqCst), 2);
    assert_eq!(location.origin(), ISSUER);
    assert_eq!(location.pathname(), "/oauth2/v1/authorize");
}

#[test]
fn includes_authorization_params_in_the_authorization_url() {
    let (fetch, _) = mock(|seen| match seen.url.as_str() {
        DISCOVERY_URL => json_response(discovery()),
        _ => unexpected(seen),
    });
    let mut options = options();
    options.client = options.client.authorization_param("prompt", "login").authorization_param("audience", "https://api.example.com");

    let response = get(&app(oidc_provider(options, fetch)), "/login", None);

    assert_eq!(response.param("prompt").as_deref(), Some("login"));
    assert_eq!(response.param("audience").as_deref(), Some("https://api.example.com"));
    assert_eq!(response.param("client_id").as_deref(), Some("client-id"));
}

#[test]
fn uses_metadata_override_without_discovery() {
    let (fetch, log) = mock(unexpected);
    let options = OidcOptions { metadata: Some(metadata(true)), ..options() };

    let response = get(&app(oidc_provider(options, fetch)), "/login", None);

    assert_eq!(response.status, 302);
    assert!(urls(&log).is_empty());
    assert_eq!(response.location().origin(), ISSUER);
}

/// The app's profile shape from `mapProfile`.
#[derive(Serialize)]
struct CompanyProfile {
    sub: String,
    email: Option<String>,
    handle: Option<String>,
}

#[test]
fn completes_callback_normalizes_claims_and_applies_map_profile() {
    let (fetch, _) = mock(|seen| match seen.url.as_str() {
        DISCOVERY_URL => json_response(discovery()),
        "https://issuer.example.com/oauth2/v1/token" => {
            assert_eq!(seen.body.get("code"), Some("good-code"));
            assert!(seen.body.get("code_verifier").is_some());
            json_response(json!({ "access_token": "oidc-access-token", "token_type": "Bearer", "scope": "openid profile email" }))
        }
        "https://issuer.example.com/oauth2/v1/userinfo" => json_response(json!({ "sub": "user-1", "email": "user@example.com", "preferred_username": "user.one" })),
        _ => unexpected(seen),
    });
    let provider = oidc_provider(OidcOptions { name: "company-sso".into(), ..options() }, fetch).map_profile(|OidcProfileInput { claims, .. }| async move {
        CompanyProfile { email: claims.email().map(str::to_string), handle: claims.preferred_username().map(str::to_string), sub: claims.sub }
    });

    let response = login_and_callback(&app(provider), "good-code");

    assert_eq!(
        response.json()["result"],
        json!({
            "provider": "company-sso",
            "account": { "provider": "company-sso", "providerAccountId": "user-1" },
            "profile": { "sub": "user-1", "email": "user@example.com", "handle": "user.one" },
            "tokens": { "accessToken": "oidc-access-token", "tokenType": "Bearer", "scope": ["openid", "profile", "email"] },
        })
    );
}

#[test]
fn fails_when_metadata_does_not_include_a_userinfo_endpoint() {
    let (fetch, _) = mock(|seen| match seen.url.as_str() {
        "https://issuer.example.com/oauth2/v1/token" => {
            assert_eq!(seen.body.get("code"), Some("good-code"));
            json_response(json!({ "access_token": "oidc-access-token", "token_type": "Bearer" }))
        }
        _ => unexpected(seen),
    });
    let options = OidcOptions { metadata: Some(metadata(false)), ..options() };

    let response = login_and_callback(&app(oidc_provider(options, fetch)), "good-code");

    assert_eq!(response.status, 400);
    assert_eq!(response.json(), json!({ "error": "OIDC provider \"oidc\" did not publish a userinfo_endpoint." }));
}
