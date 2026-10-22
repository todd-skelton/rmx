//! Port of Remix's `providers/x.test.ts`.

mod common;

use common::*;
use rmx_auth::x_provider;
use serde_json::{json, Value};

const PROFILE_URL: &str = "https://api.x.com/2/users/me?user.fields=profile_image_url,verified,description,url";

/// Log in and call back with X answering `{ data }` for the profile.
fn x_flow(data: Value) -> Reply {
    let (fetch, _) = mock(move |seen| match seen.url.as_str() {
        "https://api.x.com/2/oauth2/token" => json_response(json!({ "access_token": "x-token", "token_type": "bearer", "scope": "tweet.read users.read" })),
        PROFILE_URL => json_response(json!({ "data": data.clone() })),
        _ => unexpected(seen),
    });
    login_and_callback(&app(x_provider(client("x"), fetch)), "x-code")
}

#[test]
fn redirects_to_the_x_authorization_endpoint_with_default_pkce_parameters() {
    let (fetch, _) = mock(unexpected);
    let response = get(&app(x_provider(client("x"), fetch)), "/login", None);
    let location = response.location();
    let params = location.search_params();

    assert_eq!(response.status, 302);
    assert_eq!(location.origin(), "https://x.com");
    assert_eq!(location.pathname(), "/i/oauth2/authorize");
    assert_eq!(params.get("client_id"), Some("x-client-id"));
    assert_eq!(params.get("redirect_uri"), Some("https://app.example.com/auth/x/callback"));
    assert_eq!(params.get("response_type"), Some("code"));
    assert_eq!(params.get("scope"), Some("tweet.read users.read"));
    assert!(params.get("state").is_some());
    assert!(params.get("code_challenge").is_some());
    assert_eq!(params.get("code_challenge_method"), Some("S256"));
}

#[test]
fn uses_configured_scopes_in_the_x_authorization_url() {
    let (fetch, _) = mock(unexpected);
    let response = get(&app(x_provider(client("x").scopes(["users.read"]), fetch)), "/login", None);

    assert_eq!(response.param("scope").as_deref(), Some("users.read"));
}

#[test]
fn normalizes_x_profiles_and_exchanges_tokens_with_http_basic_auth() {
    let (fetch, _) = mock(|seen| match seen.url.as_str() {
        "https://api.x.com/2/oauth2/token" => {
            assert_eq!(seen.authorization.as_deref(), Some(format!("Basic {}", rmx_crypto::base64_encode(b"x-client-id:x-client-secret")).as_str()));
            assert!(seen.raw_body.contains("grant_type=authorization_code"));
            assert!(seen.raw_body.contains("code_verifier="));
            assert!(seen.raw_body.contains("redirect_uri=https%3A%2F%2Fapp.example.com%2Fauth%2Fx%2Fcallback"));
            assert!(!seen.raw_body.contains("client_id="));
            assert!(!seen.raw_body.contains("client_secret="));
            json_response(json!({ "access_token": "x-token", "token_type": "bearer", "scope": "tweet.read users.read" }))
        }
        PROFILE_URL => json_response(json!({
            "data": { "id": "x-user-1", "name": "X Person", "username": "xperson", "profile_image_url": "https://example.com/x-avatar.png", "verified": true },
        })),
        _ => unexpected(seen),
    });

    let body = login_and_callback(&app(x_provider(client("x"), fetch)), "x-code").json();
    let result = &body["result"];

    assert_eq!(result["provider"], "x");
    assert_eq!(result["account"], json!({ "provider": "x", "providerAccountId": "x-user-1" }));
    assert_eq!(result["profile"], json!({ "id": "x-user-1", "name": "X Person", "username": "xperson", "profile_image_url": "https://example.com/x-avatar.png", "verified": true }));
    assert_eq!(result["tokens"]["accessToken"], "x-token");
    assert_eq!(result["tokens"]["tokenType"], "bearer");
    assert_eq!(result["tokens"]["scope"], json!(["tweet.read", "users.read"]));
}

#[test]
fn fails_when_the_x_profile_does_not_include_a_valid_id() {
    let response = x_flow(json!({ "name": "X Person", "username": "xperson" }));

    assert_eq!(response.status, 400);
    assert_eq!(response.json(), json!({ "error": "X profile did not include a valid id." }));
}

#[test]
fn fails_when_the_x_profile_does_not_include_a_valid_username() {
    let response = x_flow(json!({ "id": "x-user-1", "name": "X Person" }));

    assert_eq!(response.status, 400);
    assert_eq!(response.json(), json!({ "error": "X profile did not include a valid username." }));
}

#[test]
fn fails_when_the_x_profile_does_not_include_a_valid_name() {
    let response = x_flow(json!({ "id": "x-user-1", "username": "xperson" }));

    assert_eq!(response.status, 400);
    assert_eq!(response.json(), json!({ "error": "X profile did not include a valid name." }));
}
