//! Port of Remix's `oauth-flow.integration.test.ts`: the whole browser OIDC flow against a
//! fake provider listening on a loopback port (`startFakeOAuthServer` in `test-utils.ts`). The
//! app's OIDC provider talks to it over HTTP through `rmx_node_fetch_server::fetch`, and the "browser"
//! follows the authorize redirect by hand (`redirect: 'manual'`).

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use rmx_auth::{code_challenge, finish_external_auth, oidc_provider, start_external_auth, ClientOptions, FinishExternalAuthOptions, OidcOptions, StartExternalAuthOptions};
use rmx_fetch::Body;
use rmx_auth_middleware::{auth, complete_auth, require_auth, scheme, session_scheme, Auth, AuthExt};
use rmx_fetch_router::{middleware, Context, Router};
use rmx_url::{SearchParams, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::net::TcpListener;

const ACCESS_TOKEN: &str = "fake-access-token";

/// The fake provider: discovery, `/authorize` (redirects back with a code), `/token` (checks
/// the PKCE verifier against the challenge it saw) and `/userinfo` (checks the bearer token).
fn fake_oauth_server(origin: String, profile: Value) -> Router {
    let expected_challenge: Arc<Mutex<Option<String>>> = Arc::default();
    let mut router = Router::new();
    router.get("/.well-known/openid-configuration", move |_ctx: Context| {
        let origin = origin.clone();
        async move {
            json_response(json!({
                "issuer": origin,
                "authorization_endpoint": format!("{origin}/authorize"),
                "token_endpoint": format!("{origin}/token"),
                "userinfo_endpoint": format!("{origin}/userinfo"),
                "code_challenge_methods_supported": ["S256"],
                "scopes_supported": ["openid", "email", "profile"],
            }))
        }
    });
    let seen_challenge = expected_challenge.clone();
    router.get("/authorize", move |ctx: Context| {
        let seen_challenge = seen_challenge.clone();
        async move {
            let params = ctx.url().search_params();
            *seen_challenge.lock().unwrap() = params.get("code_challenge").map(str::to_string);
            let (Some(redirect_uri), Some(state), Some(_)) = (params.get("redirect_uri"), params.get("state"), params.get("code_challenge")) else {
                return status_json(400, json!({ "error": "invalid_request", "error_description": "Missing redirect_uri, state, or code_challenge" }));
            };
            let mut url = Url::parse(redirect_uri).unwrap();
            let mut query = url.search_params();
            query.set("code", "fake-authorization-code");
            query.set("state", state);
            url.set_search_params(&query);
            rmx_response::redirect_with(&url.href(), http::StatusCode::FOUND)
        }
    });
    router.post("/token", move |mut ctx: Context| {
        let expected_challenge = expected_challenge.clone();
        async move {
            let params = SearchParams::parse(&ctx.take_body().text().await.unwrap());
            let expected = expected_challenge.lock().unwrap().clone();
            let verified = params.get("code") == Some("fake-authorization-code") && expected.is_some() && params.get("code_verifier").map(code_challenge) == expected;
            if !verified {
                return status_json(400, json!({ "error": "invalid_grant", "error_description": "Code verifier validation failed" }));
            }
            json_response(json!({ "access_token": ACCESS_TOKEN, "token_type": "Bearer", "scope": "openid email profile" }))
        }
    });
    router.get("/userinfo", move |ctx: Context| {
        let profile = profile.clone();
        async move {
            if ctx.header("authorization") != Some(format!("Bearer {ACCESS_TOKEN}").as_str()) {
                return status_json(401, json!({ "error": "invalid_token" }));
            }
            json_response(profile)
        }
    });
    router
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct User {
    id: String,
    email: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthSession {
    user_id: String,
}

/// `Response.json(get(Auth))`: the authenticated state as Remix serializes it.
fn auth_json(state: &Auth<User>) -> Value {
    match state {
        Auth::Authenticated { identity, method } => json!({ "ok": true, "identity": identity, "method": method }),
        Auth::Anonymous { .. } => json!({ "ok": false }),
    }
}

#[tokio::test]
async fn completes_the_full_browser_oidc_flow_against_a_local_fake_provider() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(rmx_node_fetch_server::serve(listener, fake_oauth_server(origin.clone(), json!({ "sub": "oauth-user-1", "email": "oauth@example.com", "name": "OAuth User" }))));

    let provider = Arc::new(oidc_provider(
        OidcOptions { name: "fake".into(), ..OidcOptions::new(origin.clone(), ClientOptions::new("fake-client-id", "fake-client-secret", "https://app.example.com/auth/fake/callback")) },
        rmx_node_fetch_server::fetch,
    ));
    let users = Arc::new(vec![User { id: "oauth-user-1".into(), email: "oauth@example.com".into() }]);
    let session_auth = session_scheme(
        |session| session.get::<AuthSession>("auth"),
        move |value: AuthSession| {
            let user = users.iter().find(|u| u.id == value.user_id).cloned();
            async move { user }
        },
    )
    .invalidate(|session| session.unset("auth"));
    let mut router = session_router().with(auth(vec![scheme(session_auth)]));

    let login = provider.clone();
    router.get("/login/fake", move |ctx: Context| {
        let provider = login.clone();
        async move {
            let return_to = ctx.url().search_params().get("returnTo").map(str::to_string);
            start_external_auth(&*provider, &ctx, StartExternalAuthOptions { return_to, ..Default::default() }).await.unwrap()
        }
    });
    router.get("/auth/fake/callback", move |ctx: Context| {
        let provider = provider.clone();
        async move {
            let finished = finish_external_auth(&*provider, &ctx, FinishExternalAuthOptions::default()).await.unwrap();
            complete_auth(&ctx).set("auth", AuthSession { user_id: finished.result.profile.sub.clone() });
            rmx_response::redirect_with(finished.return_to.as_deref().unwrap_or("/"), http::StatusCode::FOUND)
        }
    });
    router.group(vec![middleware(require_auth::<User>())], |r| {
        r.get("/dashboard", |ctx: Context| async move { json_response(auth_json(ctx.auth::<User>())) });
    });

    let login = send_async(&router, http::Request::get("https://app.example.com/login/fake?returnTo=/dashboard"), Body::empty(), None).await;
    let authorize_url = login.location();
    let params = authorize_url.search_params();

    assert_eq!(authorize_url.origin(), origin);
    assert_eq!(authorize_url.pathname(), "/authorize");
    assert_eq!(params.get("response_type"), Some("code"));
    assert_eq!(params.get("scope"), Some("openid profile email"));

    let provider_authorize = rmx_node_fetch_server::fetch(http::Request::get(authorize_url.href()).body(Body::empty()).unwrap()).await.unwrap();
    let callback_url = provider_authorize.headers().get("location").map(|v| v.to_str().unwrap().to_string());

    assert_eq!(provider_authorize.status(), 302);
    assert!(callback_url.is_some());

    let callback = send_async(&router, http::Request::get(callback_url.unwrap()), Body::empty(), Some(&login)).await;

    assert_eq!(callback.status, 302);
    assert_eq!(callback.headers["location"], "/dashboard");

    let dashboard = send_async(&router, http::Request::get("https://app.example.com/dashboard"), Body::empty(), Some(&callback)).await;

    assert_eq!(dashboard.json(), json!({ "ok": true, "identity": { "id": "oauth-user-1", "email": "oauth@example.com" }, "method": "session" }));
    server.abort();
}
