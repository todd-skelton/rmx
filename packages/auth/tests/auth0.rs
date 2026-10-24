//! Port of Remix's `providers/auth0.test.ts`.

mod common;

use common::*;
use rsmix_auth::{auth0_provider, Auth0Options, ClientOptions, OAuthProvider};
use serde_json::json;

#[test]
fn derives_the_issuer_from_the_auth0_domain_and_provider_name() {
    let (fetch, _) = mock(|seen| {
        assert_eq!(seen.url, "https://tenant.us.auth0.com/.well-known/openid-configuration");
        json_response(json!({
            "issuer": "https://tenant.us.auth0.com/",
            "authorization_endpoint": "https://tenant.us.auth0.com/authorize",
            "token_endpoint": "https://tenant.us.auth0.com/oauth/token",
            "userinfo_endpoint": "https://tenant.us.auth0.com/userinfo",
        }))
    });
    let client = ClientOptions::new("client-id", "client-secret", "https://app.example.com/auth/auth0/callback");
    let provider = auth0_provider(Auth0Options { domain: "tenant.us.auth0.com".into(), client }, fetch);
    assert_eq!(provider.name(), "auth0");

    let response = get(&app(provider), "/login", None);

    assert_eq!(response.status, 302);
    assert_eq!(response.location().origin(), "https://tenant.us.auth0.com");
}
