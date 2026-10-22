//! Helpers shared by the ports of Remix's auth tests: `test-utils.ts` (`createRequest`,
//! `mockFetch`) plus the router with the session middleware that every test sets up.
//!
//! Remix tests mock the global `fetch`; rmx providers take the fetch they use, so `mock`
//! returns one that records each request. Where a Remix test lets `router.fetch` reject, the
//! rmx routes here answer 400 with `{ "error": message }`, which is how the Remix tests that
//! catch errors report them.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use rmx_auth::{finish_external_auth, start_external_auth, ClientOptions, Fetch, FinishExternalAuthOptions, OAuthProvider, StartExternalAuthOptions};
use rmx_cookie::Cookie;
use rmx_fetch::{block_on, Body, BoxError, Request, Response};
use rmx_router::{Context, Router};
use rmx_session::{MemoryStorage, Session};
use rmx_url::{SearchParams, Url};
use serde_json::{json, Value};

/// A request the mock fetch received.
#[derive(Clone, Debug)]
pub struct Seen {
    pub url: String,
    pub authorization: Option<String>,
    pub user_agent: Option<String>,
    pub accept: Option<String>,
    /// The body as sent.
    pub raw_body: String,
    /// The body read as a form.
    pub body: SearchParams,
}

pub type Log = Arc<Mutex<Vec<Seen>>>;

/// A fetch that records each request and answers with `respond`, which panics on URLs it does
/// not expect (like the Remix mocks throwing `Unexpected fetch`).
pub fn mock(respond: impl Fn(&Seen) -> Response + Send + Sync + 'static) -> (impl Fetch + Clone, Log) {
    let log: Log = Arc::default();
    let seen = log.clone();
    let respond = Arc::new(respond);
    let fetch = move |req: Request| {
        let (seen, respond) = (seen.clone(), respond.clone());
        async move {
            let (parts, body) = req.into_parts();
            let header = |name: &str| parts.headers.get(name).map(|v| v.to_str().unwrap().to_string());
            let raw_body = body.text().await?;
            let request = Seen {
                url: parts.uri.to_string(),
                authorization: header("authorization"),
                user_agent: header("user-agent"),
                accept: header("accept"),
                body: SearchParams::parse(&raw_body),
                raw_body,
            };
            seen.lock().unwrap().push(request.clone());
            Ok::<_, BoxError>(respond(&request))
        }
    };
    (fetch, log)
}

/// `Response.json(value)`.
pub fn json_response(value: Value) -> Response {
    rmx_fetch::response::json(&value)
}

/// `Response.json(value, { status })`.
pub fn status_json(status: u16, value: Value) -> Response {
    let mut res = json_response(value);
    *res.status_mut() = http::StatusCode::from_u16(status).unwrap();
    res
}

pub fn unexpected(seen: &Seen) -> Response {
    panic!("Unexpected fetch: {}", seen.url)
}

pub fn urls(log: &Log) -> Vec<String> {
    log.lock().unwrap().iter().map(|s| s.url.clone()).collect()
}

/// `{ clientId: '<name>-client-id', clientSecret: '<name>-client-secret', redirectUri }`.
pub fn client(name: &str) -> ClientOptions {
    ClientOptions::new(format!("{name}-client-id"), format!("{name}-client-secret"), format!("https://app.example.com/auth/{name}/callback"))
}

/// `createRouter({ middleware: [sessionMiddleware(createCookie('__session', { secrets: ['secret1'] }), createMemorySessionStorage())] })`.
pub fn session_router() -> Router {
    Router::new().with(rmx_middleware::session(Cookie::new("__session").secrets(vec!["secret1".into()]), MemoryStorage::new()))
}

pub fn error_json(message: &str) -> Response {
    status_json(400, json!({ "error": message }))
}

/// A session router with `/login` (`startExternalAuth` with `returnTo` from the query),
/// `/callback` (`Response.json(await finishExternalAuth(provider, context))`) and `/inspect`
/// (the stored transaction or null). Errors answer 400 `{ error }`.
pub fn app<P: OAuthProvider>(provider: P) -> Router
where
    P::Profile: serde::Serialize,
    P::Tokens: serde::Serialize,
{
    let provider = Arc::new(provider);
    let mut router = session_router();
    let login = provider.clone();
    router.get("/login", move |ctx: Context| {
        let provider = login.clone();
        async move {
            let return_to = ctx.url().search_params().get("returnTo").map(str::to_string);
            match start_external_auth(&*provider, &ctx, StartExternalAuthOptions { return_to, ..Default::default() }).await {
                Ok(res) => res,
                Err(e) => error_json(&e.0),
            }
        }
    });
    router.get("/callback", move |ctx: Context| {
        let provider = provider.clone();
        async move {
            match finish_external_auth(&*provider, &ctx, FinishExternalAuthOptions::default()).await {
                Ok(finished) => json_response(serde_json::to_value(finished).unwrap()),
                Err(e) => error_json(&e.0),
            }
        }
    });
    router.get("/inspect", |ctx: Context| async move { json_response(ctx.get::<Session>().unwrap().get::<Value>("__auth").unwrap_or(Value::Null)) });
    router
}

pub struct Reply {
    pub status: u16,
    pub headers: http::HeaderMap,
    pub body: String,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap()
    }

    pub fn location(&self) -> Url {
        Url::parse(self.headers["location"].to_str().unwrap()).unwrap()
    }

    pub fn param(&self, name: &str) -> Option<String> {
        self.location().search_params().get(name).map(str::to_string)
    }

    /// The `name=value` pairs this response set (`createRequest`'s cookie copy).
    pub fn cookie(&self) -> Option<String> {
        let pairs: Vec<&str> = self.headers.get_all("set-cookie").iter().map(|v| v.to_str().unwrap().split(';').next().unwrap()).collect();
        (!pairs.is_empty()).then(|| pairs.join("; "))
    }
}

/// Send `req`, adding the cookies `after` set (`createRequest(url, fromResponse)`).
pub async fn send_async(router: &Router, req: http::request::Builder, body: Body, after: Option<&Reply>) -> Reply {
    let mut req = req;
    if let Some(cookie) = after.and_then(Reply::cookie) {
        req = req.header("cookie", cookie);
    }
    let res = router.fetch(req.body(body).unwrap()).await;
    let (status, headers) = (res.status().as_u16(), res.headers().clone());
    Reply { status, headers, body: res.into_body().text().await.unwrap() }
}

pub fn send(router: &Router, req: http::request::Builder, body: Body, after: Option<&Reply>) -> Reply {
    block_on(send_async(router, req, body, after))
}

/// GET `https://app.example.com<path>` with the cookies `after` set.
pub fn get(router: &Router, path: &str, after: Option<&Reply>) -> Reply {
    send(router, http::Request::get(format!("https://app.example.com{path}")), Body::empty(), after)
}

pub fn state_of(login: &Reply) -> String {
    login.param("state").unwrap()
}

/// Log in, then call back with `code` and the login's state.
pub fn login_and_callback(router: &Router, code: &str) -> Reply {
    let login = get(router, "/login", None);
    get(router, &format!("/callback?code={code}&state={}", state_of(&login)), Some(&login))
}
