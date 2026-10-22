//! Remix's auth-middleware tests: `auth.test.ts`, `require-auth.test.ts` and the
//! `schemes/{bearer,api-key,session}.test.ts` files. `auth-types.test.ts` checks TypeScript
//! inference; here `Auth<I>` carries the identity type, so the compiler does that work.
//!
//! Remix's legacy `{ status: 'skip' }` result and `undefined` return have no counterpart:
//! a scheme returns `Option<SchemeResult<I>>`, so skipping is `None` and nothing else
//! type-checks.

use std::sync::{Arc, Mutex};

use http::header::WWW_AUTHENTICATE;
use http::StatusCode;
use rmx_cookie::Cookie;
use rmx_fetch::{block_on, Body, Response};
use rmx_auth_middleware::{
    api_key_scheme, auth, bearer_scheme, require_auth, scheme, scheme_fn, session_scheme, Auth, AuthExt, AuthFailure, AuthScheme,
    FailureCode, SchemeResult,
};
use rmx_session_middleware::session;
use rmx_router::{middleware, Context, IntoResponse, Middleware, Next, Router};
use rmx_session::{MemoryStorage, Session};

#[derive(Clone, Debug, PartialEq)]
struct Id(u32);

type Log = Arc<Mutex<Vec<&'static str>>>;

/// A scheme that records its name in `log` and returns `result`.
fn fixed(name: &'static str, log: &Log, result: Option<SchemeResult<Id>>) -> Arc<dyn AuthScheme<Id>> {
    let log = log.clone();
    scheme(scheme_fn(name, move |_ctx: &Context| {
        log.lock().unwrap().push(name);
        let result = result.clone();
        async move { result }
    }))
}

fn success(id: u32) -> Option<SchemeResult<Id>> {
    Some(SchemeResult::Success(Id(id)))
}

fn fetch(router: &Router, url: &str, headers: &[(&str, &str)]) -> Response {
    let mut req = http::Request::get(url);
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    block_on(router.fetch(req.body(Body::empty()).unwrap()))
}

fn text(res: Response) -> String {
    block_on(res.into_body().text()).unwrap()
}

/// The auth state the handler saw, for a router with `mw` before it.
fn state_after(mw: impl Middleware) -> Auth<Id> {
    let seen: Arc<Mutex<Option<Auth<Id>>>> = Arc::default();
    let sink = seen.clone();
    let mut router = Router::new().with(mw);
    router.get("/", move |ctx: Context| {
        *sink.lock().unwrap() = Some(ctx.auth::<Id>().clone());
        async { "ok" }
    });
    assert_eq!(fetch(&router, "https://remix.run/", &[]).status(), 200);
    let state = seen.lock().unwrap().take();
    state.unwrap()
}

fn authenticated(id: u32, method: &str) -> Auth<Id> {
    Auth::Authenticated { identity: Id(id), method: method.into() }
}

fn assert_auth(actual: &Auth<Id>, expected: &Auth<Id>) {
    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
}

// --- auth.test.ts ------------------------------------------------------------------------

#[test]
#[should_panic(expected = "auth() requires at least one authentication scheme")]
fn throws_when_no_schemes_are_configured() {
    let _ = auth::<Id>(vec![]);
}

#[test]
fn provides_auth_state_on_request_context() {
    let log = Log::default();
    let mut router = Router::new().with(auth(vec![fixed("bearer", &log, success(1))]));
    router.get("/", |ctx: Context| async move {
        // `context.auth` and `context.get(Auth)` are the same value.
        let direct = ctx.auth::<Id>();
        let keyed = ctx.get::<Auth<Id>>().unwrap();
        format!("{direct:?} {keyed:?}")
    });
    let res = fetch(&router, "https://remix.run/", &[]);
    assert_eq!(res.status(), 200);
    let expected = format!("{:?}", authenticated(1, "bearer"));
    assert_eq!(text(res), format!("{expected} {expected}"));
}

#[test]
fn authenticates_with_the_first_successful_scheme() {
    let log = Log::default();
    let state = state_after(auth(vec![fixed("bearer", &log, success(1)), fixed("api-key", &log, success(2))]));
    assert_auth(&state, &authenticated(1, "bearer"));
    assert_eq!(*log.lock().unwrap(), ["bearer"]);
}

#[test]
fn continues_through_skip_results_until_one_succeeds() {
    let log = Log::default();
    let state = state_after(auth(vec![fixed("bearer", &log, None), fixed("api-key", &log, success(123))]));
    assert_auth(&state, &authenticated(123, "api-key"));
    assert_eq!(*log.lock().unwrap(), ["bearer", "api-key"]);
}

#[test]
fn sets_unauthenticated_state_with_no_error_when_all_schemes_skip() {
    let log = Log::default();
    let state = state_after(auth(vec![fixed("bearer", &log, None), fixed("api-key", &log, None)]));
    assert_auth(&state, &Auth::Anonymous { error: None });
    assert!(!state.is_ok());
}

#[test]
fn stops_on_failure_and_records_auth_error_details() {
    let log = Log::default();
    let failure = SchemeResult::Failure {
        code: FailureCode::InvalidCredentials,
        message: "Token is invalid".into(),
        challenge: Some(r#"Bearer realm="api""#.into()),
    };
    let state = state_after(auth(vec![fixed("bearer", &log, Some(failure)), fixed("api-key", &log, success(1))]));
    let error = AuthFailure {
        method: "bearer".into(),
        code: FailureCode::InvalidCredentials,
        message: "Token is invalid".into(),
        challenge: Some(r#"Bearer realm="api""#.into()),
    };
    assert_auth(&state, &Auth::Anonymous { error: Some(error) });
    assert_eq!(*log.lock().unwrap(), ["bearer"]);
}

#[test]
#[should_panic(expected = "Verifier failed")]
fn bubbles_thrown_errors_from_schemes() {
    let failing = scheme(scheme_fn("bearer", |_ctx: &Context| async { panic!("Verifier failed") }));
    let mut router = Router::new().with(auth::<Id>(vec![failing]));
    router.get("/", |_ctx: Context| async { "OK" });
    fetch(&router, "https://remix.run/", &[]);
}

#[test]
fn uses_default_failure_message_and_code_when_failure_payload_is_empty() {
    let log = Log::default();
    let state = state_after(auth(vec![fixed("bearer", &log, Some(SchemeResult::failure()))]));
    let error = AuthFailure {
        method: "bearer".into(),
        code: FailureCode::InvalidCredentials,
        message: "Invalid credentials".into(),
        challenge: None,
    };
    assert_auth(&state, &Auth::Anonymous { error: Some(error) });
}

// --- require-auth.test.ts ----------------------------------------------------------------

fn failing_with(message: &str, challenge: Option<&str>) -> Option<SchemeResult<Id>> {
    Some(SchemeResult::Failure {
        code: FailureCode::InvalidCredentials,
        message: message.into(),
        challenge: challenge.map(str::to_string),
    })
}

#[test]
fn passes_through_when_request_is_authenticated() {
    let log = Log::default();
    let mut router = Router::new().with(auth(vec![fixed("bearer", &log, success(123))])).with(require_auth::<Id>());
    router.get("/", |_ctx: Context| async { "OK" });
    let res = fetch(&router, "https://remix.run/", &[]);
    assert_eq!(res.status(), 200);
    assert_eq!(text(res), "OK");
}

#[test]
fn installs_context_auth_when_auth_state_was_set_by_other_middleware() {
    let load_auth = |mut ctx: Context, next: Next| async move {
        ctx.set(authenticated(123, "custom"));
        next.run(ctx).await
    };
    let mut router = Router::new().with(load_auth).with(require_auth::<Id>());
    router.get("/", |ctx: Context| async move { format!("{:?}", ctx.auth::<Id>()) });
    let res = fetch(&router, "https://remix.run/", &[]);
    assert_eq!(res.status(), 200);
    assert_eq!(text(res), format!("{:?}", authenticated(123, "custom")));
}

#[test]
fn returns_default_401_response_when_unauthenticated() {
    let log = Log::default();
    let mut router = Router::new().with(auth(vec![fixed("bearer", &log, None)])).with(require_auth::<Id>());
    router.get("/", |_ctx: Context| async { "OK" });
    let res = fetch(&router, "https://remix.run/", &[]);
    assert_eq!(res.status(), 401);
    assert_eq!(text(res), "Unauthorized");
}

#[test]
fn supports_a_custom_failure_response_callback() {
    let log = Log::default();
    let on_failure = |ctx: &Context, auth: &Auth<Id>| {
        let error = auth.error().map(|e| e.message.as_str()).unwrap_or_default();
        (StatusCode::UNAUTHORIZED, format!("path={} error={error}", ctx.url().pathname())).into_response()
    };
    let mut router = Router::new()
        .with(auth(vec![fixed("bearer", &log, failing_with("Bad token", None))]))
        .with(require_auth::<Id>().on_failure(on_failure));
    router.get("/private", |_ctx: Context| async { "OK" });
    let res = fetch(&router, "https://remix.run/private", &[]);
    assert_eq!(res.status(), 401);
    assert_eq!(text(res), "path=/private error=Bad token");
}

#[test]
fn adds_www_authenticate_header_when_challenge_is_present() {
    let log = Log::default();
    let mut router = Router::new()
        .with(auth(vec![fixed("bearer", &log, failing_with("Invalid token", Some(r#"Bearer realm="api""#)))]))
        .with(require_auth::<Id>());
    router.get("/", |_ctx: Context| async { "OK" });
    let res = fetch(&router, "https://remix.run/", &[]);
    assert_eq!(res.status(), 401);
    assert_eq!(res.headers()[WWW_AUTHENTICATE], r#"Bearer realm="api""#);
}

#[test]
fn does_not_overwrite_existing_www_authenticate_header() {
    let log = Log::default();
    let on_failure = |_: &Context, _: &Auth<Id>| {
        let mut res = (StatusCode::FORBIDDEN, "Forbidden").into_response();
        res.headers_mut().insert(WWW_AUTHENTICATE, r#"Custom realm="admin""#.parse().unwrap());
        res
    };
    let mut router = Router::new()
        .with(auth(vec![fixed("bearer", &log, failing_with("Invalid token", Some(r#"Bearer realm="api""#)))]))
        .with(require_auth::<Id>().on_failure(on_failure));
    router.get("/", |_ctx: Context| async { "OK" });
    let res = fetch(&router, "https://remix.run/", &[]);
    assert_eq!(res.status(), 403);
    let challenges: Vec<_> = res.headers().get_all(WWW_AUTHENTICATE).iter().collect();
    assert_eq!(challenges, [r#"Custom realm="admin""#]);
}

#[test]
#[should_panic(expected = "Auth state not found. Make sure auth() middleware runs before requireAuth().")]
fn throws_a_clear_error_when_auth_middleware_did_not_run_first() {
    let mut router = Router::new().with(require_auth::<Id>());
    router.get("/", |_ctx: Context| async { "OK" });
    fetch(&router, "https://remix.run/", &[]);
}

// --- schemes/bearer.test.ts and schemes/api-key.test.ts ----------------------------------

/// Run `s` against a request to `https://remix.run/private?mode=test` with `headers`.
fn authenticate(s: Arc<dyn AuthScheme<Id>>, headers: &[(&str, &str)]) -> Option<SchemeResult<Id>> {
    let seen: Arc<Mutex<Option<Option<SchemeResult<Id>>>>> = Arc::default();
    let sink = seen.clone();
    let mut router = Router::new();
    router.get("/private", move |ctx: Context| {
        let (s, sink) = (s.clone(), sink.clone());
        async move {
            *sink.lock().unwrap() = Some(s.authenticate(&ctx).await);
            "ok"
        }
    });
    fetch(&router, "https://remix.run/private?mode=test", headers);
    let result = seen.lock().unwrap().take();
    result.expect("the handler ran")
}

fn failure(code: FailureCode, message: &str, challenge: Option<&str>) -> Option<SchemeResult<Id>> {
    Some(SchemeResult::Failure { code, message: message.into(), challenge: challenge.map(str::to_string) })
}

#[test]
fn bearer_authenticates_valid_bearer_tokens() {
    let observed = Arc::new(Mutex::new(String::new()));
    let sink = observed.clone();
    let bearer = scheme(bearer_scheme(move |token: String| {
        *sink.lock().unwrap() = token;
        async { Some(Id(1)) }
    }));
    assert_eq!(authenticate(bearer, &[("Authorization", "Bearer abc123")]), success(1));
    assert_eq!(*observed.lock().unwrap(), "abc123");
}

fn always(id: u32) -> Arc<dyn AuthScheme<Id>> {
    scheme(bearer_scheme(move |_: String| async move { Some(Id(id)) }))
}

#[test]
fn bearer_skips_when_the_auth_header_is_missing() {
    assert_eq!(authenticate(always(1), &[]), None);
}

#[test]
fn bearer_skips_when_a_different_auth_scheme_is_provided() {
    assert_eq!(authenticate(always(1), &[("Authorization", "Basic abc123")]), None);
}

#[test]
fn bearer_fails_when_the_auth_header_format_is_malformed() {
    let expected = failure(FailureCode::InvalidCredentials, "Authorization header is malformed", Some("Bearer"));
    assert_eq!(authenticate(always(1), &[("Authorization", "Bearer")]), expected);
    // Header values lose surrounding whitespace first (fetch `Headers`), so these are malformed
    // too, and a leading space does not matter.
    assert_eq!(authenticate(always(1), &[("Authorization", "Bearer ")]), expected);
    assert_eq!(authenticate(always(1), &[("Authorization", "Bearer \t ")]), expected);
    assert_eq!(authenticate(always(1), &[("Authorization", " Bearer abc ")]), success(1));
}

#[test]
fn bearer_fails_when_token_verification_fails() {
    let bearer = scheme(bearer_scheme(|_: String| async { None::<Id> }));
    let expected = failure(FailureCode::InvalidCredentials, "Invalid credentials", Some("Bearer"));
    assert_eq!(authenticate(bearer, &[("Authorization", "Bearer bad-token")]), expected);
}

#[test]
fn bearer_supports_custom_header_scheme_and_challenge_settings() {
    let custom = || {
        scheme(
            bearer_scheme(|token: String| async move { (token == "good").then_some(Id(2)) })
                .header_name("X-Auth")
                .scheme("Token")
                .challenge(r#"Token realm="internal""#),
        )
    };
    assert_eq!(authenticate(custom(), &[("X-Auth", "Token good")]), success(2));
    assert_eq!(authenticate(custom(), &[("X-Auth", "Bearer good")]), None);
    let expected = failure(FailureCode::InvalidCredentials, "Invalid credentials", Some(r#"Token realm="internal""#));
    assert_eq!(authenticate(custom(), &[("X-Auth", "Token bad")]), expected);
}

#[test]
fn api_key_authenticates_valid_api_keys() {
    let seen = Arc::new(Mutex::new(String::new()));
    let sink = seen.clone();
    let key = scheme(api_key_scheme(move |key: String| {
        *sink.lock().unwrap() = key;
        async { Some(Id(1)) }
    }));
    assert_eq!(authenticate(key, &[("X-Api-Key", "k_live_123")]), success(1));
    assert_eq!(*seen.lock().unwrap(), "k_live_123");
}

fn any_key() -> Arc<dyn AuthScheme<Id>> {
    scheme(api_key_scheme(|_: String| async { Some(Id(1)) }))
}

#[test]
fn api_key_skips_when_api_key_header_is_missing() {
    assert_eq!(authenticate(any_key(), &[]), None);
}

#[test]
fn api_key_fails_when_api_key_header_is_empty() {
    let expected = failure(FailureCode::MissingCredentials, "X-Api-Key header is empty", None);
    assert_eq!(authenticate(any_key(), &[("X-Api-Key", "   ")]), expected);
}

#[test]
fn api_key_fails_when_key_verification_fails() {
    let key = scheme(api_key_scheme(|_: String| async { None::<Id> }));
    let expected = failure(FailureCode::InvalidCredentials, "Invalid credentials", None);
    assert_eq!(authenticate(key, &[("X-Api-Key", "bad-key")]), expected);
}

#[test]
fn api_key_supports_custom_header_names() {
    let key = scheme(api_key_scheme(|key: String| async move { (key == "svc_123").then_some(Id(123)) }).header_name("X-Service-Key"));
    assert_eq!(authenticate(key, &[("X-Service-Key", "svc_123")]), success(123));
}

// --- schemes/session.test.ts -------------------------------------------------------------

/// Middleware that puts a fresh session (with `key` = `user_id` when given) in the context.
fn set_session(entry: Option<(&'static str, u32)>) -> impl Middleware {
    move |mut ctx: Context, next: Next| async move {
        let session = Session::new();
        if let Some((key, user_id)) = entry {
            session.set(key, user_id);
        }
        ctx.set(session);
        next.run(ctx).await
    }
}

#[test]
#[should_panic(expected = "Session not found. Make sure session() middleware runs before createSessionAuthScheme().")]
fn session_scheme_throws_when_session_is_not_available_in_request_context() {
    let s = scheme(session_scheme(|_| None::<u32>, |_| async { Some(Id(1)) }));
    let mut router = Router::new().with(auth(vec![s]));
    router.get("/", |_ctx: Context| async { "OK" });
    fetch(&router, "https://remix.run/", &[]);
}

#[test]
fn session_scheme_skips_when_read_returns_none() {
    let s = scheme(session_scheme(|_| None::<u32>, |_| async { Some(Id(1)) }));
    let mut router = Router::new().with(set_session(None)).with(auth(vec![s]));
    router.get("/", |ctx: Context| async move { format!("{:?}", ctx.auth::<Id>()) });
    assert_eq!(text(fetch(&router, "https://remix.run/", &[])), format!("{:?}", Auth::<Id>::Anonymous { error: None }));
}

#[test]
fn session_scheme_authenticates_when_verify_returns_an_identity() {
    let s = scheme(session_scheme(|session| session.get::<u32>("auth"), |user_id| async move { Some(Id(user_id)) }));
    let mut router = Router::new().with(set_session(Some(("auth", 1)))).with(auth(vec![s]));
    router.get("/", |ctx: Context| async move { format!("{:?}", ctx.auth::<Id>()) });
    assert_eq!(text(fetch(&router, "https://remix.run/", &[])), format!("{:?}", authenticated(1, "session")));
}

#[test]
fn session_scheme_fails_and_invalidates_when_verify_returns_none() {
    let invalidated = Arc::new(Mutex::new(false));
    let flag = invalidated.clone();
    let s = scheme(
        session_scheme(|session| session.get::<u32>("auth"), |_| async { None::<Id> })
            .invalidate(move |session| {
                *flag.lock().unwrap() = true;
                session.unset("auth");
            })
            .failure(FailureCode::InvalidCredentials, "Session is no longer valid"),
    );
    let mut router = Router::new().with(set_session(Some(("auth", 1)))).with(auth(vec![s]));
    router.get("/", |ctx: Context| async move {
        format!("{:?} {:?}", ctx.auth::<Id>(), ctx.get::<Session>().unwrap().get::<u32>("auth"))
    });
    let error = AuthFailure {
        method: "session".into(),
        code: FailureCode::InvalidCredentials,
        message: "Session is no longer valid".into(),
        challenge: None,
    };
    assert_eq!(
        text(fetch(&router, "https://remix.run/", &[])),
        format!("{:?} None", Auth::<Id>::Anonymous { error: Some(error) })
    );
    assert!(*invalidated.lock().unwrap());
}

#[test]
fn session_scheme_uses_a_custom_scheme_name() {
    let s = scheme(session_scheme(|session| session.get::<u32>("member"), |user_id| async move { Some(Id(user_id)) }).name("member-session"));
    let mut router = Router::new().with(set_session(Some(("member", 1)))).with(auth(vec![s]));
    router.get("/", |ctx: Context| async move { format!("{:?}", ctx.auth::<Id>()) });
    assert_eq!(text(fetch(&router, "https://remix.run/", &[])), format!("{:?}", authenticated(1, "member-session")));
}

#[test]
fn session_scheme_invalidates_stale_session_auth_before_require_auth_returns_401() {
    let invalidated = Arc::new(Mutex::new(false));
    let flag = invalidated.clone();
    let s = scheme(
        session_scheme(|session| session.get::<u32>("auth"), |_| async { None::<Id> })
            .invalidate(move |session| {
                *flag.lock().unwrap() = true;
                session.unset("auth");
            })
            .failure(FailureCode::InvalidCredentials, "Session expired"),
    );
    let mut router = Router::new()
        .with(session(Cookie::new("__session").secrets(vec!["secret1".into()]), MemoryStorage::new()))
        .with(auth(vec![s]));
    router.get("/seed", |ctx: Context| async move {
        ctx.get::<Session>().unwrap().set("auth", 1);
        "Seeded"
    });
    router.group(vec![middleware(require_auth::<Id>())], |r| {
        r.get("/protected", |_ctx: Context| async { "OK" });
    });
    router.get("/inspect", |ctx: Context| async move {
        format!("{:?} {:?}", ctx.auth::<Id>(), ctx.get::<Session>().unwrap().get::<u32>("auth"))
    });
    let cookie = |res: &Response| res.headers()["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_string();

    let seeded = fetch(&router, "https://remix.run/seed", &[]);
    let protected = fetch(&router, "https://remix.run/protected", &[("cookie", &cookie(&seeded))]);
    assert_eq!(protected.status(), 401);
    let next_cookie = cookie(&protected);
    assert_eq!(text(protected), "Unauthorized");
    assert!(*invalidated.lock().unwrap());

    let inspected = fetch(&router, "https://remix.run/inspect", &[("cookie", &next_cookie)]);
    assert_eq!(text(inspected), format!("{:?} None", Auth::<Id>::Anonymous { error: None }));
}
