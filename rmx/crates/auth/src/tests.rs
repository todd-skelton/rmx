//! Checks without a Remix counterpart. The ports of Remix's auth tests, one test per case,
//! are in `crates/auth/tests/`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use rmx_fetch::{block_on, Body, BoxError, Request, Response};
use rmx_router::{Context, Router};
use serde_json::json;

use super::*;

fn client(name: &str) -> ClientOptions {
    ClientOptions::new(format!("{name}-client-id"), format!("{name}-client-secret"), format!("https://app.example.com/auth/{name}/callback"))
}

/// A fetch that records URLs and answers with `respond`.
fn mock(respond: impl Fn(&str) -> Response + Send + Sync + 'static) -> (impl Fetch + Clone, Arc<Mutex<Vec<String>>>) {
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = log.clone();
    let respond = Arc::new(respond);
    let fetch = move |req: Request| {
        let (seen, respond) = (seen.clone(), respond.clone());
        async move {
            let url = req.uri().to_string();
            seen.lock().unwrap().push(url.clone());
            Ok::<_, BoxError>(respond(&url))
        }
    };
    (fetch, log)
}

fn status_json(status: u16, value: serde_json::Value) -> Response {
    let mut res = rmx_fetch::response::json(&value);
    *res.status_mut() = http::StatusCode::from_u16(status).unwrap();
    res
}

#[test]
fn start_without_the_session_middleware_fails() {
    let (fetch, _) = mock(|url| panic!("Unexpected fetch: {url}"));
    let provider = Arc::new(google_provider(client("google"), fetch));
    let mut router = Router::new();
    router.get("/login", move |ctx: Context| {
        let provider = provider.clone();
        async move { start_external_auth(&*provider, &ctx, StartExternalAuthOptions::default()).await.unwrap_err().0 }
    });
    let res = block_on(router.fetch(http::Request::get("https://app.example.com/login").body(Body::empty()).unwrap()));
    assert_eq!(block_on(res.into_body().text()).unwrap(), "Session not found. Make sure session() middleware runs before startExternalAuth().");
}

#[test]
fn token_errors_carry_the_provider_message_or_the_fallback() {
    let request = |fetch: &dyn Fetch| {
        let tokens = TokenRequest { token_endpoint: "https://provider.example/token", client_id: "id", client_secret: "secret", client_authentication: ClientAuthentication::RequestBody, headers: &[] };
        block_on(exchange_authorization_code(fetch, tokens, "https://app.example/callback", "code", "verifier")).unwrap_err().0
    };
    let (fetch, _) = mock(|_| rmx_fetch::response::json(&json!({ "error": "bad_verification_code", "error_description": "The code passed is incorrect or expired." })));
    assert_eq!(request(&fetch), "The code passed is incorrect or expired.");
    let (fetch, _) = mock(|_| status_json(500, json!({})));
    assert_eq!(request(&fetch), "OAuth token exchange failed.");
    let (fetch, _) = mock(|_| rmx_fetch::response::json(&json!({ "token_type": "bearer" })));
    assert_eq!(request(&fetch), "OAuth token response did not include an access token.");

    let callback = Url::parse("https://app.example.com/callback?state=s").unwrap();
    assert_eq!(authorization_code(&callback).unwrap_err().0, "Missing authorization code in OAuth callback request.");
}

#[test]
fn hosted_providers_derive_discovery_urls() {
    let (fetch, log) = mock(|url| {
        let issuer = url.trim_end_matches("/.well-known/openid-configuration");
        rmx_fetch::response::json(&json!({ "issuer": issuer, "authorization_endpoint": format!("{issuer}/authorize"), "token_endpoint": format!("{issuer}/token") }))
    });
    let check = |provider: OidcProvider, discovery_url: &str| {
        log.lock().unwrap().clear();
        block_on(provider.authorization_url(&mut create_transaction("p", None))).unwrap();
        assert_eq!(*log.lock().unwrap(), [discovery_url]);
    };
    check(microsoft_provider(MicrosoftOptions { client: client("ms"), tenant: None }, fetch.clone()), "https://login.microsoftonline.com/common/v2.0/.well-known/openid-configuration");
    check(auth0_provider(Auth0Options { domain: " https://example.us.auth0.com/ignored ".into(), client: client("auth0") }, fetch.clone()), "https://example.us.auth0.com/.well-known/openid-configuration");
    check(oidc_provider(OidcOptions { discovery_url: Some("https://idp.example/custom".into()), ..OidcOptions::new("https://idp.example", client("oidc")) }, fetch), "https://idp.example/custom");
}

#[test]
fn transactions_and_challenges_follow_pkce() {
    let a = create_transaction("p", Some("/x".into()));
    let b = create_transaction("p", None);
    assert_ne!(a.state, b.state);
    assert_eq!(a.code_verifier.len(), 64);
    // RFC 7636 appendix B.
    assert_eq!(code_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
}

#[test]
fn tokens_serialize_expiry_as_an_iso_string_and_read_it_back() {
    let tokens = OAuthTokens { access_token: "t".into(), expires_at: Some(UNIX_EPOCH + Duration::from_millis(1_758_652_452_345)), ..Default::default() };
    let value = serde_json::to_value(&tokens).unwrap();
    assert_eq!(value, json!({ "accessToken": "t", "expiresAt": "2025-09-23T18:34:12.345Z" }));
    assert_eq!(serde_json::from_value::<OAuthTokens>(value).unwrap(), tokens);
    assert!(serde_json::from_value::<OAuthTokens>(json!({ "accessToken": "t", "expiresAt": "soon" })).is_err());
}
