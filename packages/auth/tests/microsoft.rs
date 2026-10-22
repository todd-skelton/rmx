//! Port of Remix's `providers/microsoft.test.ts`.

mod common;

use common::*;
use rmx_auth::{microsoft_provider, MicrosoftOptions, ClientOptions, OAuthProvider};
use serde_json::json;

#[test]
fn uses_the_tenant_specific_microsoft_issuer_and_provider_name() {
    let (fetch, _) = mock(|seen| {
        assert_eq!(seen.url, "https://login.microsoftonline.com/organizations/v2.0/.well-known/openid-configuration");
        json_response(json!({
            "issuer": "https://login.microsoftonline.com/organizations/v2.0",
            "authorization_endpoint": "https://login.microsoftonline.com/organizations/oauth2/v2.0/authorize",
            "token_endpoint": "https://login.microsoftonline.com/organizations/oauth2/v2.0/token",
            "userinfo_endpoint": "https://graph.microsoft.com/oidc/userinfo",
        }))
    });
    let client = ClientOptions::new("client-id", "client-secret", "https://app.example.com/auth/microsoft/callback");
    let provider = microsoft_provider(MicrosoftOptions { client, tenant: Some("organizations".into()) }, fetch);
    assert_eq!(provider.name(), "microsoft");

    let response = get(&app(provider), "/login", None);

    assert_eq!(response.status, 302);
    assert_eq!(response.location().origin(), "https://login.microsoftonline.com");
}
