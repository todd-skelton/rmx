//! Port of Remix's `providers/google.test.ts`.

mod common;

use common::*;
use rmx_auth::{google_provider, ClientOptions, OAuthProvider};

fn google_client() -> ClientOptions {
    ClientOptions::new("client-id", "client-secret", "https://app.example.com/auth/google/callback")
}

#[test]
fn uses_the_google_oidc_defaults_and_provider_name() {
    let (fetch, _) = mock(unexpected);
    let provider = google_provider(google_client(), fetch);
    assert_eq!(provider.name(), "google");

    let response = get(&app(provider), "/login", None);
    let location = response.location();

    assert_eq!(response.status, 302);
    assert_eq!(location.origin(), "https://accounts.google.com");
    assert_eq!(location.pathname(), "/o/oauth2/v2/auth");
    assert_eq!(location.search_params().get("scope"), Some("openid email profile"));
}

#[test]
fn includes_configured_authorization_params_in_the_google_login_redirect() {
    let (fetch, _) = mock(unexpected);
    let client = google_client().authorization_param("access_type", "offline").authorization_param("prompt", "consent");

    let response = get(&app(google_provider(client, fetch)), "/login", None);

    assert_eq!(response.param("access_type").as_deref(), Some("offline"));
    assert_eq!(response.param("prompt").as_deref(), Some("consent"));
}
