//! Port of Remix's `providers/facebook.test.ts`.

mod common;

use common::*;
use rmx_auth::facebook_provider;
use serde_json::{json, Value};

/// Log in and call back with Facebook answering `profile`.
fn facebook_flow(profile: Value) -> Reply {
    let (fetch, _) = mock(move |seen| match seen.url.as_str() {
        "https://graph.facebook.com/oauth/access_token" => json_response(json!({ "access_token": "facebook-token", "token_type": "bearer", "expires_in": 3600 })),
        "https://graph.facebook.com/me?fields=id,name,email,picture" => json_response(profile.clone()),
        _ => unexpected(seen),
    });
    login_and_callback(&app(facebook_provider(client("facebook"), fetch)), "facebook-code")
}

#[test]
fn redirects_to_the_facebook_authorization_endpoint_with_default_pkce_parameters() {
    let (fetch, _) = mock(unexpected);
    let response = get(&app(facebook_provider(client("facebook"), fetch)), "/login", None);
    let location = response.location();
    let params = location.search_params();

    assert_eq!(response.status, 302);
    assert_eq!(location.origin(), "https://www.facebook.com");
    assert_eq!(location.pathname(), "/dialog/oauth");
    assert_eq!(params.get("client_id"), Some("facebook-client-id"));
    assert_eq!(params.get("redirect_uri"), Some("https://app.example.com/auth/facebook/callback"));
    assert_eq!(params.get("response_type"), Some("code"));
    assert_eq!(params.get("scope"), Some("public_profile email"));
    assert!(params.get("state").is_some());
    assert!(params.get("code_challenge").is_some());
    assert_eq!(params.get("code_challenge_method"), Some("S256"));
}

#[test]
fn uses_configured_scopes_in_the_facebook_authorization_url() {
    let (fetch, _) = mock(unexpected);
    let response = get(&app(facebook_provider(client("facebook").scopes(["email"]), fetch)), "/login", None);

    assert_eq!(response.param("scope").as_deref(), Some("email"));
}

#[test]
fn normalizes_facebook_profiles_and_tokens() {
    let body = facebook_flow(json!({
        "id": "fb_123",
        "name": "Michael Jackson",
        "email": "mj@example.com",
        "picture": { "data": { "url": "https://example.com/avatar.png" } },
    }))
    .json();
    let result = &body["result"];

    assert_eq!(result["provider"], "facebook");
    assert_eq!(result["account"], json!({ "provider": "facebook", "providerAccountId": "fb_123" }));
    assert_eq!(
        result["profile"],
        json!({ "id": "fb_123", "name": "Michael Jackson", "email": "mj@example.com", "picture": { "data": { "url": "https://example.com/avatar.png" } } })
    );
    assert_eq!(result["tokens"]["accessToken"], "facebook-token");
    assert_eq!(result["tokens"]["tokenType"], "bearer");
    let expires_at = result["tokens"]["expiresAt"].as_str().expect("expiresAt is a string");
    assert!(rmx_headers::parse_iso_date(expires_at).is_some(), "{expires_at}");
}

#[test]
fn fails_when_the_facebook_profile_does_not_include_a_valid_id() {
    let response = facebook_flow(json!({ "name": "Michael Jackson", "email": "mj@example.com" }));

    assert_eq!(response.status, 400);
    assert_eq!(response.json(), json!({ "error": "Facebook profile did not include a valid id." }));
}
