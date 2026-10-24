//! Port of Remix's `providers/github.test.ts`.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::*;
use rsmix_auth::github_provider;
use serde_json::{json, Value};

fn token(seen: &Seen) -> Option<rsmix_fetch::Response> {
    (seen.url == "https://github.com/login/oauth/access_token").then(|| json_response(json!({ "access_token": "github-token", "token_type": "bearer", "scope": "read:user,user:email" })))
}

/// Remix's `finishGitHubLogin(profile, emails)`: log in and call back, returning the result.
fn finish_github_login(profile: Value, emails: Value) -> Reply {
    let (fetch, _) = mock(move |seen| {
        token(seen).unwrap_or_else(|| match seen.url.as_str() {
            "https://api.github.com/user" => json_response(profile.clone()),
            "https://api.github.com/user/emails" => json_response(emails.clone()),
            _ => unexpected(seen),
        })
    });
    login_and_callback(&app(github_provider(client("github"), fetch)), "github-code")
}

#[test]
fn redirects_to_the_github_authorization_endpoint_with_configured_scopes() {
    let (fetch, _) = mock(unexpected);
    let router = app(github_provider(client("github").scopes(["read:user", "repo"]), fetch));

    let response = get(&router, "/login", None);
    let location = response.location();
    let params = location.search_params();

    assert_eq!(response.status, 302);
    assert_eq!(location.origin(), "https://github.com");
    assert_eq!(location.pathname(), "/login/oauth/authorize");
    assert_eq!(params.get("client_id"), Some("github-client-id"));
    assert_eq!(params.get("redirect_uri"), Some("https://app.example.com/auth/github/callback"));
    assert_eq!(params.get("scope"), Some("read:user repo"));
    assert!(params.get("state").is_some());
    assert!(params.get("code_challenge").is_some());
    assert_eq!(params.get("code_challenge_method"), Some("S256"));
}

#[test]
fn hydrates_missing_email_addresses_from_the_github_email_api() {
    let response = finish_github_login(
        json!({ "id": 123, "login": "mjackson", "email": null, "name": "Michael Jackson" }),
        json!([
            { "email": "unverified@example.com", "primary": false, "verified": false },
            { "email": "secondary@example.com", "primary": false, "verified": true },
            { "email": "mj@example.com", "primary": true, "verified": true },
        ]),
    );

    assert_eq!(
        response.json()["result"],
        json!({
            "provider": "github",
            "account": { "provider": "github", "providerAccountId": "123" },
            "profile": { "id": 123, "login": "mjackson", "email": "mj@example.com", "name": "Michael Jackson" },
            "tokens": { "accessToken": "github-token", "tokenType": "bearer", "scope": ["read:user", "user:email"] },
        })
    );
}

#[test]
fn uses_a_verified_secondary_email_when_the_primary_email_is_unverified() {
    let response = finish_github_login(
        json!({ "id": 123, "login": "mjackson", "email": null }),
        json!([
            { "email": "primary@example.com", "primary": true, "verified": false },
            { "email": "secondary@example.com", "primary": false, "verified": true },
            { "email": "another@example.com", "primary": false, "verified": true },
        ]),
    );
    let body = &response.json()["result"];

    assert_eq!(response.status, 200);
    assert_eq!(body["profile"]["email"], "secondary@example.com");
    assert_eq!(body["account"], json!({ "provider": "github", "providerAccountId": "123" }));
}

#[test]
fn keeps_a_null_profile_email_when_no_github_email_addresses_are_verified() {
    let profile = json!({ "id": 123, "login": "mjackson", "email": null });
    let response = finish_github_login(
        profile.clone(),
        json!([
            { "email": "primary@example.com", "primary": true, "verified": false },
            { "email": "secondary@example.com", "primary": false, "verified": false },
        ]),
    );
    let body = &response.json()["result"];

    assert_eq!(response.status, 200);
    assert_eq!(body["profile"], profile);
    assert_eq!(body["account"], json!({ "provider": "github", "providerAccountId": "123" }));
}

#[test]
fn keeps_an_omitted_profile_email_when_no_github_email_addresses_are_verified() {
    let profile = json!({ "id": 123, "login": "mjackson" });
    let response = finish_github_login(profile.clone(), json!([{ "email": "secondary@example.com", "primary": false, "verified": false }]));
    let body = &response.json()["result"];

    assert_eq!(response.status, 200);
    assert_eq!(body["profile"], profile);
    assert_eq!(body["account"], json!({ "provider": "github", "providerAccountId": "123" }));
}

#[test]
fn uses_the_primary_profile_email_without_calling_the_github_email_api() {
    let email_requests = Arc::new(AtomicUsize::new(0));
    let counter = email_requests.clone();
    let (fetch, _) = mock(move |seen| {
        token(seen).unwrap_or_else(|| match seen.url.as_str() {
            "https://api.github.com/user" => json_response(json!({ "id": 123, "login": "mjackson", "email": "mj@example.com", "name": "Michael Jackson" })),
            "https://api.github.com/user/emails" => {
                counter.fetch_add(1, Ordering::SeqCst);
                json_response(json!([]))
            }
            _ => unexpected(seen),
        })
    });

    let body = login_and_callback(&app(github_provider(client("github"), fetch)), "github-code").json();

    assert_eq!(body["result"]["profile"]["email"], "mj@example.com");
    assert_eq!(email_requests.load(Ordering::SeqCst), 0);
}

#[test]
fn keeps_the_github_profile_email_unchanged_when_the_email_api_returns_no_addresses() {
    let response = finish_github_login(json!({ "id": 123, "login": "mjackson", "email": null, "name": "Michael Jackson" }), json!([]));
    let body = &response.json()["result"];

    assert_eq!(body["profile"]["email"], Value::Null);
    assert_eq!(body["account"]["providerAccountId"], "123");
}

#[test]
fn fails_when_the_github_profile_does_not_include_a_valid_id() {
    let (fetch, _) = mock(|seen| {
        token(seen).unwrap_or_else(|| match seen.url.as_str() {
            "https://api.github.com/user" => json_response(json!({ "login": "mjackson", "email": "mj@example.com", "name": "Michael Jackson" })),
            _ => unexpected(seen),
        })
    });

    let response = login_and_callback(&app(github_provider(client("github"), fetch)), "github-code");

    assert_eq!(response.status, 400);
    assert_eq!(response.json(), json!({ "error": "GitHub profile did not include a valid id." }));
}
