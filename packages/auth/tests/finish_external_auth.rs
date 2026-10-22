//! Port of Remix's `finish-external-auth.test.ts`.

mod common;

use common::*;
use rmx_auth::{finish_external_auth, google_provider, AuthError, FinishExternalAuthOptions, OAuthAccount, OAuthProvider, OAuthResult, OAuthTokens, OAuthTransaction};
use rmx_router::Context;
use rmx_session::Session;
use rmx_url::Url;
use serde_json::json;

/// `createOAuthProvider('test', ...)` whose callback always succeeds.
struct TestProvider;

impl OAuthProvider for TestProvider {
    type Profile = serde_json::Value;
    type Tokens = OAuthTokens;

    fn name(&self) -> &str {
        "test"
    }

    async fn authorization_url(&self, _transaction: &mut OAuthTransaction) -> Result<Url, AuthError> {
        Ok(Url::parse("https://provider.example/authorize").unwrap())
    }

    async fn handle_callback(&self, _callback: &Url, _transaction: &OAuthTransaction) -> Result<OAuthResult<serde_json::Value>, AuthError> {
        Ok(OAuthResult {
            provider: "test".into(),
            account: OAuthAccount { provider: "test".into(), provider_account_id: "123".into() },
            profile: json!({}),
            tokens: OAuthTokens { access_token: "test-token".into(), ..Default::default() },
        })
    }
}

#[test]
fn drops_authority_references_from_previously_stored_return_targets() {
    let mut router = session_router();
    router.get("/", |ctx: Context| async move {
        let transaction = json!({ "provider": "test", "state": "test-state", "codeVerifier": "test-verifier", "returnTo": "//other.example" });
        ctx.get::<Session>().unwrap().set("__auth", transaction);
        let finished = finish_external_auth(&TestProvider, &ctx, FinishExternalAuthOptions::default()).await.unwrap();
        rmx_fetch::response::redirect_with(finished.return_to.as_deref().unwrap_or("/"), http::StatusCode::FOUND)
    });

    let response = get(&router, "/?state=test-state", None);
    assert_eq!(response.headers["location"], "/");
}

fn google_backend(seen: &Seen) -> rmx_fetch::Response {
    match seen.url.as_str() {
        "https://oauth2.googleapis.com/token" => {
            assert_eq!(seen.body.get("code"), Some("good-code"));
            assert_eq!(seen.body.get("grant_type"), Some("authorization_code"));
            assert!(seen.body.get("code_verifier").is_some());
            json_response(json!({ "access_token": "access-token", "token_type": "Bearer", "scope": "openid email profile" }))
        }
        "https://openidconnect.googleapis.com/v1/userinfo" => json_response(json!({ "sub": "u1", "email": "mj@example.com", "name": "Michael Jackson" })),
        _ => unexpected(seen),
    }
}

#[test]
fn completes_a_google_callback_preserves_return_to_and_clears_the_transaction() {
    let (fetch, _) = mock(google_backend);
    let router = app(google_provider(client("google"), fetch));

    let login = get(&router, "/login?returnTo=/dashboard", None);
    let callback = get(&router, &format!("/callback?code=good-code&state={}", state_of(&login)), Some(&login));
    let inspect = get(&router, "/inspect", Some(&callback));

    assert_eq!(
        callback.json(),
        json!({
            "result": {
                "provider": "google",
                "account": { "provider": "google", "providerAccountId": "u1" },
                "profile": { "sub": "u1", "email": "mj@example.com", "name": "Michael Jackson" },
                "tokens": { "accessToken": "access-token", "tokenType": "Bearer", "scope": ["openid", "email", "profile"] },
            },
            "returnTo": "/dashboard",
        })
    );
    assert_eq!(inspect.json(), serde_json::Value::Null);
}

#[test]
fn rethrows_callback_errors_and_clears_the_transaction() {
    let (fetch, _) = mock(unexpected);
    let router = app(google_provider(client("google"), fetch));

    let login = get(&router, "/login", None);
    let callback = get(&router, &format!("/callback?error=access_denied&error_description=Nope&state={}", state_of(&login)), Some(&login));
    let inspect = get(&router, "/inspect", Some(&callback));

    assert_eq!(callback.status, 400);
    assert_eq!(callback.json(), json!({ "error": "Nope" }));
    assert_eq!(inspect.json(), serde_json::Value::Null);
}

#[test]
fn rejects_invalid_oauth_state() {
    let (fetch, _) = mock(unexpected);
    let router = app(google_provider(client("google"), fetch));

    let login = get(&router, "/login", None);
    let callback = get(&router, "/callback?code=good-code&state=wrong", Some(&login));

    assert_eq!(callback.status, 400);
    assert_eq!(callback.json(), json!({ "error": "Invalid OAuth state." }));
}
