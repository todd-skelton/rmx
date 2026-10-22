//! Port of Remix's `start-external-auth.test.ts`.

mod common;

use common::*;
use rmx_auth::google_provider;

fn google_login(query: &str) -> (Reply, serde_json::Value) {
    let (fetch, _) = mock(unexpected);
    let router = app(google_provider(client("google"), fetch));
    let login = get(&router, &format!("/login{query}"), None);
    let transaction = get(&router, "/inspect", Some(&login)).json();
    (login, transaction)
}

#[test]
fn redirects_to_the_provider_authorization_url_and_stores_a_transaction() {
    let (response, transaction) = google_login("?returnTo=/dashboard");
    let location = response.location();
    let params = location.search_params();

    assert_eq!(response.status, 302);
    assert_eq!(response.headers.get_all("set-cookie").iter().count(), 1);
    assert_eq!(location.origin(), "https://accounts.google.com");
    assert_eq!(location.pathname(), "/o/oauth2/v2/auth");
    assert_eq!(params.get("client_id"), Some("google-client-id"));
    assert_eq!(params.get("redirect_uri"), Some("https://app.example.com/auth/google/callback"));
    assert_eq!(params.get("response_type"), Some("code"));
    assert_eq!(params.get("scope"), Some("openid email profile"));
    assert!(params.get("state").is_some());
    assert!(params.get("code_challenge").is_some());
    assert_eq!(params.get("code_challenge_method"), Some("S256"));
    assert_eq!(transaction["provider"], "google");
    assert_eq!(transaction["state"].as_str(), params.get("state"));
    assert!(transaction["codeVerifier"].is_string());
    assert_eq!(transaction["returnTo"], "/dashboard");
}

#[test]
fn drops_unsafe_return_to_values_before_persisting_the_transaction() {
    let (response, transaction) = google_login("?returnTo=https://evil.example.com");

    assert_eq!(response.status, 302);
    assert_eq!(transaction["provider"], "google");
    assert!(transaction["state"].is_string());
    assert!(transaction["codeVerifier"].is_string());
    assert_eq!(transaction.get("returnTo"), None);
}

#[test]
fn drops_backslash_return_to_values_before_persisting_the_transaction() {
    let (response, transaction) = google_login("?returnTo=%2F%5Cevil.example.com");

    assert_eq!(response.status, 302);
    assert_eq!(transaction["provider"], "google");
    assert!(transaction["state"].is_string());
    assert!(transaction["codeVerifier"].is_string());
    assert_eq!(transaction.get("returnTo"), None);
}
