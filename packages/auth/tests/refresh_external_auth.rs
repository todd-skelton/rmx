//! Port of Remix's `refresh-external-auth.test.ts`.

mod common;

use common::*;
use rmx_auth::{github_provider, google_provider, refresh_external_auth, x_provider, OAuthTokens};
use rmx_fetch::block_on;
use serde_json::json;

fn scopes(list: &[&str]) -> Option<Vec<String>> {
    Some(list.iter().map(|s| s.to_string()).collect())
}

#[test]
fn refreshes_google_tokens_and_preserves_the_existing_refresh_token_when_the_provider_omits_one() {
    let (fetch, _) = mock(|seen| match seen.url.as_str() {
        "https://oauth2.googleapis.com/token" => {
            assert_eq!(seen.body.get("grant_type"), Some("refresh_token"));
            assert_eq!(seen.body.get("refresh_token"), Some("google-refresh-token"));
            assert_eq!(seen.body.get("client_id"), Some("google-client-id"));
            assert_eq!(seen.body.get("client_secret"), Some("google-client-secret"));
            json_response(json!({ "access_token": "google-access-token-2", "token_type": "Bearer", "expires_in": 7200, "scope": "openid email profile" }))
        }
        _ => unexpected(seen),
    });
    let provider = google_provider(client("google"), fetch);
    let current = OAuthTokens {
        access_token: "google-access-token-1".into(),
        refresh_token: Some("google-refresh-token".into()),
        token_type: Some("Bearer".into()),
        scope: scopes(&["openid", "email"]),
        ..Default::default()
    };

    let result = block_on(refresh_external_auth(&provider, &current)).unwrap();

    assert_eq!(result.provider, "google");
    assert_eq!(result.tokens.access_token, "google-access-token-2");
    assert_eq!(result.tokens.refresh_token.as_deref(), Some("google-refresh-token"));
    assert_eq!(result.tokens.token_type.as_deref(), Some("Bearer"));
    assert_eq!(result.tokens.scope, scopes(&["openid", "email", "profile"]));
    assert!(result.tokens.expires_at.is_some());
}

#[test]
fn refreshes_x_tokens_with_http_basic_auth() {
    let (fetch, _) = mock(|seen| match seen.url.as_str() {
        "https://api.x.com/2/oauth2/token" => {
            assert_eq!(seen.authorization.as_deref(), Some(format!("Basic {}", rmx_crypto::base64_encode(b"x-client-id:x-client-secret")).as_str()));
            assert_eq!(seen.body.get("grant_type"), Some("refresh_token"));
            assert_eq!(seen.body.get("refresh_token"), Some("x-refresh-token"));
            assert_eq!(seen.body.get("client_id"), None);
            assert_eq!(seen.body.get("client_secret"), None);
            json_response(json!({ "access_token": "x-access-token-2", "refresh_token": "x-refresh-token-2", "token_type": "bearer", "scope": "tweet.read users.read offline.access" }))
        }
        _ => unexpected(seen),
    });
    let provider = x_provider(client("x"), fetch);
    let current = OAuthTokens {
        access_token: "x-access-token-1".into(),
        refresh_token: Some("x-refresh-token".into()),
        token_type: Some("bearer".into()),
        scope: scopes(&["tweet.read", "users.read", "offline.access"]),
        ..Default::default()
    };

    let result = block_on(refresh_external_auth(&provider, &current)).unwrap();

    assert_eq!(result.provider, "x");
    assert_eq!(result.tokens.access_token, "x-access-token-2");
    assert_eq!(result.tokens.refresh_token.as_deref(), Some("x-refresh-token-2"));
    assert_eq!(result.tokens.token_type.as_deref(), Some("bearer"));
    assert_eq!(result.tokens.scope, scopes(&["tweet.read", "users.read", "offline.access"]));
}

#[test]
fn fails_for_providers_that_do_not_implement_refresh_token_exchange() {
    let (fetch, _) = mock(unexpected);
    let provider = github_provider(client("github"), fetch);
    let tokens = OAuthTokens { access_token: "github-access-token".into(), refresh_token: Some("github-refresh-token".into()), ..Default::default() };

    let error = block_on(refresh_external_auth(&provider, &tokens)).unwrap_err();
    assert!(error.0.contains("does not support refresh-token exchange"), "{error}");
}
