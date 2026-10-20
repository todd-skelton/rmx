use std::sync::{Arc, Mutex};

use rmx_cookie::Cookie;
use rmx_fetch::{block_on, Body, BoxError, Request, Response};
use rmx_router::{Context, Router};
use rmx_session::{MemoryStorage, Session};
use rmx_url::{SearchParams, Url};
use serde_json::{json, Value};

use super::*;

// sanitize_return_to (utils.test.ts) -----------------------------------------------------

#[test]
fn return_to_drops_paths_that_normalize_to_an_authority() {
    for value in ["/..//other.example", "/.//other.example", "/%2e%2e//other.example", "/account/..//other.example", "/..\\/other.example", "/..//remix.local", "//other.example", "https://evil.example.com", "/\\evil.example.com"] {
        assert_eq!(sanitize_return_to(Some(value)), None, "{value}");
    }
    assert_eq!(sanitize_return_to(None), None);
}

#[test]
fn return_to_keeps_local_paths_queries_fragments_and_encoded_data() {
    assert_eq!(sanitize_return_to(Some("/account/../dashboard?tab=profile#name")).as_deref(), Some("/dashboard?tab=profile#name"));
    assert_eq!(sanitize_return_to(Some("/account//settings")).as_deref(), Some("/account//settings"));
    assert_eq!(sanitize_return_to(Some("/%2Fother.example")).as_deref(), Some("/%2Fother.example"));
    assert_eq!(sanitize_return_to(Some("/?next=//other.example#//section")).as_deref(), Some("/?next=//other.example#//section"));
}

// An in-memory fetch -----------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Seen {
    url: String,
    authorization: Option<String>,
    user_agent: Option<String>,
    body: SearchParams,
}

type Log = Arc<Mutex<Vec<Seen>>>;

/// A fetch that records each request and answers with `respond`, which panics on URLs it
/// does not expect (like Remix's mockFetch throwing).
fn mock(respond: impl Fn(&Seen) -> Response + Send + Sync + 'static) -> (impl Fetch + Clone, Log) {
    let log: Log = Arc::default();
    let seen = log.clone();
    let respond = Arc::new(respond);
    let fetch = move |req: Request| {
        let (seen, respond) = (seen.clone(), respond.clone());
        async move {
            let (parts, body) = req.into_parts();
            let header = |name: &str| parts.headers.get(name).map(|v| v.to_str().unwrap().to_string());
            let request = Seen { url: parts.uri.to_string(), authorization: header("authorization"), user_agent: header("user-agent"), body: SearchParams::parse(&body.text().await?) };
            seen.lock().unwrap().push(request.clone());
            Ok::<_, BoxError>(respond(&request))
        }
    };
    (fetch, log)
}

fn json_response(value: Value) -> Response {
    rmx_fetch::response::json(&value)
}

fn status_json(status: u16, value: Value) -> Response {
    let mut res = json_response(value);
    *res.status_mut() = http::StatusCode::from_u16(status).unwrap();
    res
}

fn unexpected(seen: &Seen) -> Response {
    panic!("Unexpected fetch: {}", seen.url)
}

fn urls(log: &Log) -> Vec<String> {
    log.lock().unwrap().iter().map(|s| s.url.clone()).collect()
}

// A router with the session middleware, like the Remix tests ---------------------------------

fn client(name: &str) -> ClientOptions {
    ClientOptions::new(format!("{name}-client-id"), format!("{name}-client-secret"), format!("https://app.example.com/auth/{name}/callback"))
}

fn app<P: OAuthProvider>(provider: P) -> Router
where
    P::Profile: serde::Serialize,
{
    let provider = Arc::new(provider);
    let mut router = Router::new().with(rmx_middleware::session(Cookie::new("__session").secrets(vec!["secret1".into()]), MemoryStorage::new()));
    let login = provider.clone();
    router.get("/login", move |ctx: Context| {
        let provider = login.clone();
        async move {
            let return_to = ctx.url().search_params().get("returnTo").map(str::to_string);
            match start_external_auth(&*provider, &ctx, StartExternalAuthOptions { return_to, ..Default::default() }).await {
                Ok(res) => res,
                Err(e) => status_json(400, json!({ "error": e.0 })),
            }
        }
    });
    router.get("/callback", move |ctx: Context| {
        let provider = provider.clone();
        async move {
            match finish_external_auth(&*provider, &ctx, FinishExternalAuthOptions::default()).await {
                Ok(finished) => json_response(serde_json::to_value(finished).unwrap()),
                Err(e) => status_json(400, json!({ "error": e.0 })),
            }
        }
    });
    router.get("/inspect", |ctx: Context| async move { json_response(ctx.get::<Session>().unwrap().get::<Value>("__auth").unwrap_or(Value::Null)) });
    router.get("/seed", |ctx: Context| async move {
        let transaction = json!({ "provider": "test", "state": "test-state", "codeVerifier": "test-verifier", "returnTo": "//other.example" });
        ctx.get::<Session>().unwrap().set("__auth", transaction);
        "seeded"
    });
    router
}

struct Reply {
    status: u16,
    headers: http::HeaderMap,
    body: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap()
    }

    fn location(&self) -> Url {
        Url::parse(self.headers["location"].to_str().unwrap()).unwrap()
    }

    fn cookie(&self) -> Option<String> {
        self.headers.get("set-cookie").map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string())
    }
}

/// GET `path`, sending the cookie the previous response set (Remix's createRequest).
fn get(router: &Router, path: &str, after: Option<&Reply>) -> Reply {
    let mut req = http::Request::get(format!("https://app.example.com{path}"));
    if let Some(cookie) = after.and_then(Reply::cookie) {
        req = req.header("cookie", cookie);
    }
    let res = block_on(router.fetch(req.body(Body::empty()).unwrap()));
    let (status, headers) = (res.status().as_u16(), res.headers().clone());
    Reply { status, headers, body: block_on(res.into_body().text()).unwrap() }
}

fn state_of(login: &Reply) -> String {
    login.location().search_params().get("state").unwrap().to_string()
}

// startExternalAuth ----------------------------------------------------------------------------

#[test]
fn start_redirects_to_the_provider_and_stores_a_transaction() {
    let (fetch, log) = mock(unexpected);
    let router = app(google_provider(client("google"), fetch));
    let login = get(&router, "/login?returnTo=/dashboard", None);
    let location = login.location();
    let params = location.search_params();
    let transaction = get(&router, "/inspect", Some(&login)).json();

    assert_eq!(login.status, 302);
    assert_eq!(login.headers.get_all("set-cookie").iter().count(), 1);
    assert_eq!(location.origin(), "https://accounts.google.com");
    assert_eq!(location.pathname(), "/o/oauth2/v2/auth");
    assert_eq!(params.get("client_id"), Some("google-client-id"));
    assert_eq!(params.get("redirect_uri"), Some("https://app.example.com/auth/google/callback"));
    assert_eq!(params.get("response_type"), Some("code"));
    assert_eq!(params.get("scope"), Some("openid email profile"));
    assert_eq!(params.get("code_challenge_method"), Some("S256"));
    assert_eq!(transaction["provider"], "google");
    assert_eq!(transaction["state"].as_str(), params.get("state"));
    let verifier = transaction["codeVerifier"].as_str().unwrap();
    assert_eq!(verifier.len(), 64);
    assert_eq!(params.get("code_challenge"), Some(code_challenge(verifier).as_str()));
    assert_eq!(transaction["returnTo"], "/dashboard");
    assert!(log.lock().unwrap().is_empty(), "fixed Google metadata needs no discovery");
}

#[test]
fn start_drops_unsafe_and_backslash_return_to_values() {
    for return_to in ["https://evil.example.com", "%2F%5Cevil.example.com"] {
        let (fetch, _) = mock(unexpected);
        let router = app(google_provider(client("google"), fetch));
        let login = get(&router, &format!("/login?returnTo={return_to}"), None);
        let transaction = get(&router, "/inspect", Some(&login)).json();
        assert_eq!(login.status, 302);
        assert_eq!(transaction["provider"], "google");
        assert!(transaction["state"].is_string() && transaction["codeVerifier"].is_string());
        assert_eq!(transaction.get("returnTo"), None, "{return_to}");
    }
}

#[test]
fn start_without_the_session_middleware_fails() {
    let (fetch, _) = mock(unexpected);
    let provider = Arc::new(google_provider(client("google"), fetch));
    let mut router = Router::new();
    router.get("/login", move |ctx: Context| {
        let provider = provider.clone();
        async move { start_external_auth(&*provider, &ctx, StartExternalAuthOptions::default()).await.unwrap_err().0 }
    });
    assert_eq!(get(&router, "/login", None).body, "Session not found. Make sure session() middleware runs before startExternalAuth().");
}

// finishExternalAuth ---------------------------------------------------------------------------

fn google_backend(seen: &Seen) -> Response {
    match seen.url.as_str() {
        "https://oauth2.googleapis.com/token" => {
            assert_eq!(seen.body.get("code"), Some("good-code"));
            assert_eq!(seen.body.get("grant_type"), Some("authorization_code"));
            assert_eq!(seen.body.get("client_id"), Some("google-client-id"));
            assert_eq!(seen.body.get("redirect_uri"), Some("https://app.example.com/auth/google/callback"));
            assert_eq!(seen.body.get("code_verifier").map(str::len), Some(64));
            json_response(json!({ "access_token": "access-token", "token_type": "Bearer", "scope": "openid email profile" }))
        }
        "https://openidconnect.googleapis.com/v1/userinfo" => {
            assert_eq!(seen.authorization.as_deref(), Some("Bearer access-token"));
            json_response(json!({ "sub": "u1", "email": "mj@example.com", "name": "Michael Jackson" }))
        }
        _ => unexpected(seen),
    }
}

#[test]
fn finish_completes_a_google_callback_keeps_return_to_and_clears_the_transaction() {
    let (fetch, _) = mock(google_backend);
    let router = app(google_provider(client("google"), fetch));
    let login = get(&router, "/login?returnTo=/dashboard", None);
    let callback = get(&router, &format!("/callback?code=good-code&state={}", state_of(&login)), Some(&login));
    let inspect = get(&router, "/inspect", Some(&callback).filter(|c| c.cookie().is_some()).or(Some(&login)));

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
    assert_eq!(inspect.json(), Value::Null);
}

#[test]
fn finish_rethrows_provider_errors_and_clears_the_transaction() {
    let (fetch, log) = mock(unexpected);
    let router = app(google_provider(client("google"), fetch));
    let login = get(&router, "/login", None);
    let callback = get(&router, &format!("/callback?error=access_denied&error_description=Nope&state={}", state_of(&login)), Some(&login));
    let inspect = get(&router, "/inspect", Some(&login));

    assert_eq!(callback.status, 400);
    assert_eq!(callback.json(), json!({ "error": "Nope" }));
    assert_eq!(inspect.json(), Value::Null);
    assert!(log.lock().unwrap().is_empty());
}

#[test]
fn finish_rejects_invalid_or_missing_state() {
    let (fetch, _) = mock(unexpected);
    let router = app(google_provider(client("google"), fetch));
    let login = get(&router, "/login", None);
    let wrong = get(&router, "/callback?code=good-code&state=wrong", Some(&login));
    assert_eq!((wrong.status, wrong.json()), (400, json!({ "error": "Invalid OAuth state." })));

    let login = get(&router, "/login", None);
    let missing = get(&router, "/callback?code=good-code", Some(&login));
    assert_eq!(missing.json(), json!({ "error": "Missing \"state\" in OAuth callback request." }));

    let no_transaction = get(&router, "/callback?code=good-code&state=x", None);
    assert_eq!(no_transaction.json(), json!({ "error": "Missing OAuth transaction for \"google\"." }));
}

struct TestProvider;

impl OAuthProvider for TestProvider {
    type Profile = Value;

    fn name(&self) -> &str {
        "test"
    }

    async fn authorization_url(&self, _: &OAuthTransaction) -> Result<Url, AuthError> {
        Ok(Url::parse("https://provider.example/authorize").unwrap())
    }

    async fn handle_callback(&self, _: &Url, _: &OAuthTransaction) -> Result<OAuthResult<Value>, AuthError> {
        Ok(OAuthResult {
            provider: "test".into(),
            account: OAuthAccount { provider: "test".into(), provider_account_id: "123".into() },
            profile: json!({}),
            tokens: OAuthTokens { access_token: "test-token".into(), ..Default::default() },
        })
    }
}

#[test]
fn finish_drops_authority_references_from_stored_return_targets() {
    let router = app(TestProvider);
    let seeded = get(&router, "/seed", None);
    let callback = get(&router, "/callback?state=test-state", Some(&seeded));
    assert_eq!(callback.status, 200);
    assert_eq!(callback.json().get("returnTo"), None);
    assert_eq!(callback.json()["result"]["account"]["providerAccountId"], "123");
}

// refreshExternalAuth --------------------------------------------------------------------------

#[test]
fn refresh_google_keeps_the_refresh_token_when_the_provider_omits_one() {
    let (fetch, log) = mock(|seen| {
        assert_eq!(seen.url, "https://oauth2.googleapis.com/token");
        assert_eq!(seen.body.get("grant_type"), Some("refresh_token"));
        assert_eq!(seen.body.get("refresh_token"), Some("refresh-token"));
        assert_eq!(seen.body.get("client_secret"), Some("google-client-secret"));
        json_response(json!({ "access_token": "new-access-token", "expires_in": 3600 }))
    });
    let provider = google_provider(client("google"), fetch);
    let current = OAuthTokens { access_token: "old".into(), refresh_token: Some("refresh-token".into()), token_type: Some("Bearer".into()), ..Default::default() };
    let refreshed = block_on(refresh_external_auth(&provider, &current)).unwrap();
    assert_eq!(refreshed.provider, "google");
    assert_eq!(refreshed.tokens.access_token, "new-access-token");
    assert_eq!(refreshed.tokens.refresh_token.as_deref(), Some("refresh-token"));
    assert_eq!(refreshed.tokens.token_type.as_deref(), Some("Bearer"));
    assert!(refreshed.tokens.expires_at.is_some_and(|at| at > std::time::SystemTime::now()));
    assert_eq!(log.lock().unwrap().len(), 1);
}

#[test]
fn refresh_x_uses_http_basic_auth() {
    let (fetch, _) = mock(|seen| {
        assert_eq!(seen.url, "https://api.x.com/2/oauth2/token");
        assert_eq!(seen.authorization.as_deref(), Some(format!("Basic {}", rmx_crypto::base64_encode(b"x-client-id:x-client-secret")).as_str()));
        assert_eq!(seen.body.get("client_secret"), None);
        json_response(json!({ "access_token": "new-x-token", "refresh_token": "new-refresh" }))
    });
    let provider = x_provider(client("x"), fetch);
    let current = OAuthTokens { access_token: "old".into(), refresh_token: Some("refresh-token".into()), ..Default::default() };
    let refreshed = block_on(refresh_external_auth(&provider, &current)).unwrap();
    assert_eq!((refreshed.tokens.access_token.as_str(), refreshed.tokens.refresh_token.as_deref()), ("new-x-token", Some("new-refresh")));

    let missing = block_on(refresh_external_auth(&provider, &OAuthTokens::default())).unwrap_err();
    assert_eq!(missing.0, "X provider did not receive a refresh token.");
}

#[test]
fn refresh_fails_for_providers_without_refresh_support() {
    let (fetch, _) = mock(unexpected);
    let current = OAuthTokens { access_token: "a".into(), refresh_token: Some("r".into()), ..Default::default() };
    let error = block_on(refresh_external_auth(&github_provider(client("github"), fetch), &current)).unwrap_err();
    assert_eq!(error.0, "OAuth provider \"github\" does not support refresh-token exchange.");
}

// GitHub ---------------------------------------------------------------------------------------

fn github_flow(profile: Value, emails: Option<Value>) -> (Reply, Vec<String>) {
    let (fetch, log) = mock(move |seen| match seen.url.as_str() {
        "https://github.com/login/oauth/access_token" => json_response(json!({ "access_token": "gh-token", "token_type": "bearer", "scope": "read:user,user:email" })),
        "https://api.github.com/user" => {
            assert_eq!(seen.user_agent.as_deref(), Some("rmx-auth"));
            json_response(profile.clone())
        }
        "https://api.github.com/user/emails" => json_response(emails.clone().expect("the email API should not be called")),
        _ => unexpected(seen),
    });
    let router = app(github_provider(client("github"), fetch));
    let login = get(&router, "/login", None);
    let callback = get(&router, &format!("/callback?code=gh-code&state={}", state_of(&login)), Some(&login));
    (callback, urls(&log))
}

#[test]
fn github_redirects_with_configured_scopes() {
    let (fetch, _) = mock(unexpected);
    let router = app(github_provider(client("github").scopes(["repo", "read:org"]).authorization_param("allow_signup", "false"), fetch));
    let location = get(&router, "/login", None).location();
    let params = location.search_params();
    assert_eq!(format!("{}{}", location.origin(), location.pathname()), "https://github.com/login/oauth/authorize");
    assert_eq!(params.get("scope"), Some("repo read:org"));
    assert_eq!(params.get("allow_signup"), Some("false"));
    assert_eq!(params.get("response_type"), None, "GitHub takes no response_type");
    assert_eq!(params.get("code_challenge_method"), Some("S256"));
}

#[test]
fn github_hydrates_a_missing_email_from_the_email_api() {
    let emails = json!([
        { "email": "secondary@example.com", "primary": false, "verified": true },
        { "email": "primary@example.com", "primary": true, "verified": true },
    ]);
    let (callback, urls) = github_flow(json!({ "id": 42, "login": "mj", "name": "MJ", "email": null }), Some(emails));
    let result = &callback.json()["result"];
    assert_eq!(result["profile"]["email"], "primary@example.com");
    assert_eq!(result["profile"]["login"], "mj");
    assert_eq!(result["account"], json!({ "provider": "github", "providerAccountId": "42" }));
    assert_eq!(result["tokens"]["scope"], json!(["read:user", "user:email"]));
    assert_eq!(urls.last().map(String::as_str), Some("https://api.github.com/user/emails"));
}

#[test]
fn github_picks_a_verified_secondary_or_keeps_no_email() {
    let unverified_primary = json!([
        { "email": "primary@example.com", "primary": true, "verified": false },
        { "email": "secondary@example.com", "primary": false, "verified": true },
    ]);
    let (callback, _) = github_flow(json!({ "id": 1, "login": "a" }), Some(unverified_primary));
    assert_eq!(callback.json()["result"]["profile"]["email"], "secondary@example.com");

    let none_verified = json!([{ "email": "primary@example.com", "primary": true, "verified": false }]);
    let (callback, _) = github_flow(json!({ "id": 1, "login": "a", "email": null }), Some(none_verified));
    assert_eq!(callback.json()["result"]["profile"]["email"], Value::Null);

    let (callback, _) = github_flow(json!({ "id": 1, "login": "a" }), Some(json!([])));
    assert_eq!(callback.json()["result"]["profile"]["email"], Value::Null);
}

#[test]
fn github_uses_the_profile_email_without_the_email_api() {
    let (callback, urls) = github_flow(json!({ "id": 7, "login": "a", "email": "public@example.com", "company": "Remix" }), None);
    assert_eq!(callback.json()["result"]["profile"]["email"], "public@example.com");
    assert_eq!(callback.json()["result"]["profile"]["company"], "Remix", "unknown fields are kept");
    assert_eq!(urls, ["https://github.com/login/oauth/access_token", "https://api.github.com/user"]);
}

#[test]
fn github_fails_without_a_valid_id() {
    let (callback, _) = github_flow(json!({ "id": "nope", "login": "a", "email": "a@example.com" }), None);
    assert_eq!((callback.status, callback.json()), (400, json!({ "error": "GitHub profile did not include a valid id." })));
}

#[test]
fn token_errors_carry_the_provider_message() {
    let (fetch, _) = mock(|seen| match seen.url.as_str() {
        "https://github.com/login/oauth/access_token" => json_response(json!({ "error": "bad_verification_code", "error_description": "The code passed is incorrect or expired." })),
        _ => unexpected(seen),
    });
    let router = app(github_provider(client("github"), fetch));
    let login = get(&router, "/login", None);
    let callback = get(&router, &format!("/callback?code=gh-code&state={}", state_of(&login)), Some(&login));
    assert_eq!(callback.json(), json!({ "error": "The code passed is incorrect or expired." }));

    let (fetch, _) = mock(|_| status_json(500, json!({})));
    let router = app(github_provider(client("github"), fetch));
    let login = get(&router, "/login", None);
    let callback = get(&router, &format!("/callback?code=gh-code&state={}", state_of(&login)), Some(&login));
    assert_eq!(callback.json(), json!({ "error": "OAuth token exchange failed." }));

    let login = get(&router, "/login", None);
    let callback = get(&router, &format!("/callback?state={}", state_of(&login)), Some(&login));
    assert_eq!(callback.json(), json!({ "error": "Missing authorization code in OAuth callback request." }));
}

// X --------------------------------------------------------------------------------------------

fn x_flow(data: Value) -> Reply {
    let (fetch, _) = mock(move |seen| match seen.url.as_str() {
        "https://api.x.com/2/oauth2/token" => {
            assert_eq!(seen.authorization.as_deref(), Some(format!("Basic {}", rmx_crypto::base64_encode(b"x-client-id:x-client-secret")).as_str()));
            assert_eq!(seen.body.get("client_id"), None);
            json_response(json!({ "access_token": "x-token", "refresh_token": "x-refresh", "token_type": "bearer", "scope": "tweet.read users.read" }))
        }
        "https://api.x.com/2/users/me?user.fields=profile_image_url,verified,description,url" => json_response(json!({ "data": data.clone() })),
        _ => unexpected(seen),
    });
    let router = app(x_provider(client("x"), fetch));
    let login = get(&router, "/login", None);
    let params = login.location().search_params();
    assert_eq!(params.get("scope"), Some("tweet.read users.read"));
    assert_eq!(params.get("response_type"), Some("code"));
    get(&router, &format!("/callback?code=x-code&state={}", state_of(&login)), Some(&login))
}

#[test]
fn x_normalizes_profiles_and_exchanges_tokens_with_basic_auth() {
    let callback = x_flow(json!({ "id": "12", "name": "Michael", "username": "mjackson", "verified": true }));
    let result = &callback.json()["result"];
    assert_eq!(result["account"], json!({ "provider": "x", "providerAccountId": "12" }));
    assert_eq!(result["profile"]["username"], "mjackson");
    assert_eq!(result["tokens"]["refreshToken"], "x-refresh");
    assert_eq!(result["tokens"]["scope"], json!(["tweet.read", "users.read"]));
}

#[test]
fn x_fails_without_id_username_or_name() {
    for (data, message) in [
        (json!({ "name": "M", "username": "m" }), "X profile did not include a valid id."),
        (json!({ "id": "1", "name": "M" }), "X profile did not include a valid username."),
        (json!({ "id": "1", "username": "m", "name": "" }), "X profile did not include a valid name."),
    ] {
        assert_eq!(x_flow(data).json(), json!({ "error": message }));
    }
}

// Facebook -------------------------------------------------------------------------------------

#[test]
fn facebook_normalizes_profiles_and_rejects_missing_ids() {
    for (profile, expected) in [
        (json!({ "id": "fb1", "name": "M", "email": "m@example.com", "picture": { "data": { "url": "https://cdn.example/p.png" } } }), None),
        (json!({ "name": "M" }), Some("Facebook profile did not include a valid id.")),
    ] {
        let (fetch, _) = mock(move |seen| match seen.url.as_str() {
            "https://graph.facebook.com/oauth/access_token" => json_response(json!({ "access_token": "fb-token" })),
            "https://graph.facebook.com/me?fields=id,name,email,picture" => json_response(profile.clone()),
            _ => unexpected(seen),
        });
        let router = app(facebook_provider(client("facebook"), fetch));
        let login = get(&router, "/login", None);
        assert_eq!(login.location().search_params().get("scope"), Some("public_profile email"));
        let callback = get(&router, &format!("/callback?code=c&state={}", state_of(&login)), Some(&login));
        match expected {
            None => {
                let result = &callback.json()["result"];
                assert_eq!(result["account"]["providerAccountId"], "fb1");
                assert_eq!(result["profile"]["picture"]["data"]["url"], "https://cdn.example/p.png");
            }
            Some(message) => assert_eq!(callback.json(), json!({ "error": message })),
        }
    }
}

// OpenID Connect -------------------------------------------------------------------------------

fn discovery(issuer: &str) -> Value {
    json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/oauth2/v1/authorize"),
        "token_endpoint": format!("{issuer}/oauth2/v1/token"),
        "userinfo_endpoint": format!("{issuer}/oauth2/v1/userinfo"),
    })
}

#[test]
fn oidc_discovers_metadata_once_and_retries_after_failure() {
    let failures = Arc::new(Mutex::new(1));
    let remaining = failures.clone();
    let (fetch, log) = mock(move |seen| match seen.url.as_str() {
        "https://issuer.example.com/.well-known/openid-configuration" => {
            let mut left = remaining.lock().unwrap();
            if *left > 0 {
                *left -= 1;
                return status_json(503, json!({}));
            }
            json_response(discovery("https://issuer.example.com"))
        }
        _ => unexpected(seen),
    });
    let router = app(oidc_provider(OidcOptions { name: "company-sso".into(), ..OidcOptions::new("https://issuer.example.com", client("oidc")) }, fetch));
    let failed = get(&router, "/login", None);
    assert_eq!((failed.status, failed.json()), (400, json!({ "error": "Failed to load OIDC metadata for \"company-sso\"." })));
    for _ in 0..2 {
        let login = get(&router, "/login", None);
        let location = login.location();
        assert_eq!(location.pathname(), "/oauth2/v1/authorize");
        assert_eq!(location.search_params().get("scope"), Some("openid profile email"));
    }
    assert_eq!(urls(&log).len(), 2, "one failed and one cached discovery request");
}

#[test]
fn oidc_completes_the_callback_and_maps_the_profile() {
    let (fetch, _) = mock(|seen| match seen.url.as_str() {
        "https://issuer.example.com/.well-known/openid-configuration" => json_response(discovery("https://issuer.example.com")),
        "https://issuer.example.com/oauth2/v1/token" => {
            assert_eq!(seen.body.get("code"), Some("good-code"));
            json_response(json!({ "access_token": "oidc-access-token", "token_type": "Bearer", "scope": "openid profile email" }))
        }
        "https://issuer.example.com/oauth2/v1/userinfo" => json_response(json!({ "sub": "user-1", "email": "user@example.com", "preferred_username": "user.one" })),
        _ => unexpected(seen),
    });
    let provider = Arc::new(oidc_provider(OidcOptions { name: "company-sso".into(), ..OidcOptions::new("https://issuer.example.com", client("oidc")) }, fetch));
    let mut router = Router::new().with(rmx_middleware::session(Cookie::new("__session").secrets(vec!["secret1".into()]), MemoryStorage::new()));
    let login = provider.clone();
    router.get("/login", move |ctx: Context| {
        let provider = login.clone();
        async move { start_external_auth(&*provider, &ctx, StartExternalAuthOptions::default()).await.unwrap() }
    });
    router.get("/callback", move |ctx: Context| {
        let provider = provider.clone();
        async move {
            let finished = finish_external_auth(&*provider, &ctx, FinishExternalAuthOptions::default()).await.unwrap();
            let result = finished.result.map_profile(|p| json!({ "sub": p.sub, "email": p.email(), "handle": p.preferred_username() }));
            json_response(serde_json::to_value(result).unwrap())
        }
    });
    let login = get(&router, "/login", None);
    let callback = get(&router, &format!("/callback?code=good-code&state={}", state_of(&login)), Some(&login));
    assert_eq!(
        callback.json(),
        json!({
            "provider": "company-sso",
            "account": { "provider": "company-sso", "providerAccountId": "user-1" },
            "profile": { "sub": "user-1", "email": "user@example.com", "handle": "user.one" },
            "tokens": { "accessToken": "oidc-access-token", "tokenType": "Bearer", "scope": ["openid", "profile", "email"] },
        })
    );
}

#[test]
fn oidc_uses_a_metadata_override_and_requires_userinfo_and_sub() {
    for (userinfo_endpoint, userinfo, message) in [
        (None, json!({}), "OIDC provider \"oidc\" did not publish a userinfo_endpoint."),
        (Some("https://idp.example/userinfo".to_string()), json!({ "email": "a@example.com" }), "OIDC provider \"oidc\" did not return a valid \"sub\" claim."),
    ] {
        let (fetch, log) = mock(move |seen| match seen.url.as_str() {
            "https://idp.example/token" => json_response(json!({ "access_token": "t" })),
            "https://idp.example/userinfo" => json_response(userinfo.clone()),
            _ => unexpected(seen),
        });
        let metadata = OidcMetadata {
            issuer: "https://idp.example".into(),
            authorization_endpoint: "https://idp.example/authorize".into(),
            token_endpoint: "https://idp.example/token".into(),
            userinfo_endpoint,
            ..Default::default()
        };
        let options = OidcOptions { metadata: Some(metadata), ..OidcOptions::new("https://idp.example", client("oidc").authorization_param("prompt", "consent")) };
        let router = app(oidc_provider(options, fetch));
        let login = get(&router, "/login", None);
        assert_eq!(login.location().search_params().get("prompt"), Some("consent"));
        let callback = get(&router, &format!("/callback?code=c&state={}", state_of(&login)), Some(&login));
        assert_eq!(callback.json(), json!({ "error": message }));
        assert!(!urls(&log).iter().any(|u| u.contains(".well-known")), "no discovery with a metadata override");
    }
}

#[test]
fn hosted_providers_derive_issuers_and_names() {
    let (fetch, log) = mock(|seen| json_response(discovery(seen.url.trim_end_matches("/.well-known/openid-configuration"))));
    let check = |provider: OidcProvider, name: &str, discovery_url: &str| {
        assert_eq!(provider.name(), name);
        log.lock().unwrap().clear();
        block_on(provider.authorization_url(&create_transaction(name, None))).unwrap();
        assert_eq!(urls(&log), [discovery_url]);
    };
    check(microsoft_provider(MicrosoftOptions { client: client("ms"), tenant: Some("contoso".into()) }, fetch.clone()), "microsoft", "https://login.microsoftonline.com/contoso/v2.0/.well-known/openid-configuration");
    check(microsoft_provider(MicrosoftOptions { client: client("ms"), tenant: None }, fetch.clone()), "microsoft", "https://login.microsoftonline.com/common/v2.0/.well-known/openid-configuration");
    check(okta_provider("https://dev-1.okta.com/oauth2/default", client("okta"), fetch.clone()), "okta", "https://dev-1.okta.com/oauth2/default/.well-known/openid-configuration");
    check(auth0_provider(Auth0Options { domain: "example.us.auth0.com".into(), client: client("auth0") }, fetch.clone()), "auth0", "https://example.us.auth0.com/.well-known/openid-configuration");
    check(auth0_provider(Auth0Options { domain: " https://example.us.auth0.com/ignored ".into(), client: client("auth0") }, fetch), "auth0", "https://example.us.auth0.com/.well-known/openid-configuration");
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
