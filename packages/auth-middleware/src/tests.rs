use rsmix_cookie::Cookie;
use rsmix_fetch::{block_on, Body};
use rsmix_fetch_router::{middleware, Context, Router};
use rsmix_session::MemoryStorage;

use super::*;
use rsmix_session_middleware::SessionExt;

#[derive(Clone, Debug, PartialEq)]
struct User {
    id: u32,
}

async fn find_user(id: u32) -> Option<User> {
    (id < 100).then_some(User { id })
}

fn app(schemes: Vec<Arc<dyn AuthScheme<User>>>) -> Router {
    let mut router = Router::new()
        .with(rsmix_session_middleware::session(Cookie::new("sid").secrets(vec!["k".into()]), MemoryStorage::new()))
        .with(auth(schemes));
    router.get("/whoami", |ctx: Context| async move {
        match ctx.auth::<User>() {
            Auth::Authenticated { identity, method } => format!("{} via {method}", identity.id),
            Auth::Anonymous { error: Some(e) } => format!("rejected by {}: {}", e.method, e.message),
            Auth::Anonymous { error: None } => "anonymous".into(),
        }
    });
    router.post("/login/:id", |ctx: Context| async move {
        let session = complete_auth(&ctx);
        session.set("user_id", ctx.param("id").unwrap().parse::<u32>().unwrap());
        "logged in"
    });
    router.get("/has-login", |ctx: Context| async move { ctx.session().has("user_id").to_string() });
    router.group(vec![middleware(require_auth::<User>())], |r| {
        r.get("/private", |_ctx: Context| async { "secret" });
    });
    router.group(
        vec![middleware(require_auth::<User>().on_failure(|_, _| rsmix_response::redirect("/login")))],
        |r| {
            r.get("/page", |_ctx: Context| async { "page" });
        },
    );
    router
}

fn session_auth() -> Arc<dyn AuthScheme<User>> {
    scheme(
        session_scheme(|s| s.get::<u32>("user_id"), find_user).invalidate(|s| s.unset("user_id")),
    )
}

fn send(router: &Router, method: &str, url: &str, headers: &[(&str, &str)]) -> (u16, String, http::HeaderMap) {
    let mut req = http::Request::builder().method(method).uri(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let res = block_on(router.fetch(req.body(Body::empty()).unwrap()));
    let (status, headers) = (res.status().as_u16(), res.headers().clone());
    (status, block_on(res.into_body().text()).unwrap(), headers)
}

fn cookie_of(headers: &http::HeaderMap) -> String {
    headers["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_string()
}

#[test]
fn session_login_flow_rotates_the_id() {
    let router = app(vec![session_auth()]);
    let (_, body, h) = send(&router, "GET", "http://x/whoami", &[]);
    assert_eq!(body, "anonymous");
    assert!(!h.contains_key("set-cookie"), "an untouched session sets no cookie");
    assert_eq!(send(&router, "GET", "http://x/private", &[]).0, 401);

    let (_, _, h) = send(&router, "POST", "http://x/login/7", &[]);
    let first = cookie_of(&h);
    assert_eq!(send(&router, "GET", "http://x/whoami", &[("cookie", &first)]).1, "7 via session");
    assert_eq!(send(&router, "GET", "http://x/private", &[("cookie", &first)]).1, "secret");

    let (_, _, h) = send(&router, "POST", "http://x/login/8", &[("cookie", &first)]);
    let second = cookie_of(&h);
    assert_ne!(second, first, "login regenerates the session id");
    assert_eq!(send(&router, "GET", "http://x/whoami", &[("cookie", &second)]).1, "8 via session");
    assert_eq!(send(&router, "GET", "http://x/whoami", &[("cookie", &first)]).1, "anonymous", "old session deleted");
}

#[test]
fn failed_verification_invalidates_and_reports() {
    let router = app(vec![session_auth()]);
    let (_, _, h) = send(&router, "POST", "http://x/login/500", &[]);
    let c = cookie_of(&h);
    assert_eq!(send(&router, "GET", "http://x/whoami", &[("cookie", &c)]).1, "rejected by session: Invalid session");
    assert_eq!(send(&router, "GET", "http://x/has-login", &[("cookie", &c)]).1, "false", "invalidate cleared it");
}

#[test]
fn require_auth_redirect_and_challenge() {
    let bearer = scheme(bearer_scheme(|token: String| async move { (token == "t0k").then_some(User { id: 1 }) }));
    let router = app(vec![bearer, session_auth()]);
    let (status, _, h) = send(&router, "GET", "http://x/page", &[]);
    assert_eq!((status, h["location"].to_str().unwrap()), (302, "/login"));

    assert_eq!(send(&router, "GET", "http://x/whoami", &[("authorization", "Bearer t0k")]).1, "1 via bearer");
    let (status, _, h) = send(&router, "GET", "http://x/private", &[("authorization", "Bearer bad")]);
    assert_eq!((status, h["www-authenticate"].to_str().unwrap()), (401, "Bearer"));
    assert_eq!(send(&router, "GET", "http://x/whoami", &[("authorization", "Basic abc")]).1, "anonymous", "other schemes skip");
    assert_eq!(send(&router, "GET", "http://x/whoami", &[("authorization", "Bearer  ")]).1, "rejected by bearer: Authorization header is malformed");
    assert_eq!(send(&router, "GET", "http://x/whoami", &[("authorization", "Bearer")]).1, "rejected by bearer: Authorization header is malformed");
}

#[test]
fn api_key_and_closure_schemes() {
    let key = scheme(api_key_scheme(|k: String| async move { (k == "abc").then_some(User { id: 2 }) }));
    let custom = scheme(scheme_fn("query", |ctx: &Context| {
        let id = ctx.url().search_params().get("as").and_then(|v| v.parse::<u32>().ok());
        async move { id.map(|id| SchemeResult::Success(User { id })) }
    }));
    let router = app(vec![key, custom]);
    assert_eq!(send(&router, "GET", "http://x/whoami", &[("x-api-key", "abc")]).1, "2 via api-key");
    assert_eq!(send(&router, "GET", "http://x/whoami", &[("x-api-key", " ")]).1, "rejected by api-key: X-Api-Key header is empty");
    assert_eq!(send(&router, "GET", "http://x/whoami?as=9", &[]).1, "9 via query");
}
