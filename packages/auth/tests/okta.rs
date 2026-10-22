//! Port of Remix's `providers/okta.test.ts`.

mod common;

use common::*;
use rmx_auth::{okta_provider, ClientOptions, OAuthProvider};
use serde_json::json;

#[test]
fn uses_the_provided_okta_issuer_and_provider_name() {
    let (fetch, _) = mock(|seen| {
        assert_eq!(seen.url, "https://example.okta.com/oauth2/default/.well-known/openid-configuration");
        json_response(json!({
            "issuer": "https://example.okta.com/oauth2/default",
            "authorization_endpoint": "https://example.okta.com/oauth2/default/v1/authorize",
            "token_endpoint": "https://example.okta.com/oauth2/default/v1/token",
            "userinfo_endpoint": "https://example.okta.com/oauth2/default/v1/userinfo",
        }))
    });
    let client = ClientOptions::new("client-id", "client-secret", "https://app.example.com/auth/okta/callback");
    let provider = okta_provider("https://example.okta.com/oauth2/default", client, fetch);
    assert_eq!(provider.name(), "okta");

    let response = get(&app(provider), "/login", None);

    assert_eq!(response.status, 302);
    assert_eq!(response.location().origin(), "https://example.okta.com");
}
