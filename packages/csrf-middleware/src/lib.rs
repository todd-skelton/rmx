//! Session-backed CSRF tokens plus an Origin/Referer check for unsafe requests.

use std::sync::Arc;

use http::{Method, StatusCode};
use rmx_fetch::Response;
use rmx_fetch_router::{BoxFuture, Context, IntoResponse, Middleware, Next};
use rmx_session::Session;

use rmx_form_data_middleware::FormDataExt;

/// Why [`Csrf`] rejected a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsrfFailure {
    InvalidOrigin,
    MissingToken,
    InvalidToken,
}

type OriginFn = dyn Fn(&str, &Context) -> bool + Send + Sync;
type TokenFn = dyn Fn(&Context) -> Option<String> + Send + Sync;
type ErrorFn = dyn Fn(CsrfFailure, &Context) -> Response + Send + Sync;

#[derive(Clone)]
enum Origins {
    SameOrigin,
    List(Vec<String>),
    Check(Arc<OriginFn>),
}

/// CSRF protection: every request gets a token in its session (read it with
/// [`csrf_token`] and put it in forms as `_csrf`), and unsafe requests must send it back
/// in a header or form field and come from an allowed origin.
///
/// Needs the session middleware (`rmx_session_middleware::session()`) before it, and
/// [`rmx_form_data_middleware::form_data()`] before it for form tokens.
#[derive(Clone)]
pub struct Csrf {
    token_key: String,
    field_name: String,
    header_names: Vec<String>,
    safe_methods: Vec<Method>,
    origins: Origins,
    allow_missing_origin: bool,
    value: Option<Arc<TokenFn>>,
    on_error: Option<Arc<ErrorFn>>,
}

/// [`Csrf`] with Remix's defaults: token key and field `_csrf`, headers `X-Csrf-Token`,
/// `X-Xsrf-Token`, `Csrf-Token`, safe methods GET/HEAD/OPTIONS, same-origin only, and
/// requests without Origin or Referer allowed.
pub fn csrf() -> Csrf {
    Csrf {
        token_key: "_csrf".into(),
        field_name: "_csrf".into(),
        header_names: ["X-Csrf-Token", "X-Xsrf-Token", "Csrf-Token"].map(String::from).to_vec(),
        safe_methods: vec![Method::GET, Method::HEAD, Method::OPTIONS],
        origins: Origins::SameOrigin,
        allow_missing_origin: true,
        value: None,
        on_error: None,
    }
}

impl Csrf {
    /// Session key holding the token.
    pub fn token_key(mut self, key: impl Into<String>) -> Csrf {
        self.token_key = key.into();
        self
    }

    /// Form field read for the submitted token.
    pub fn field_name(mut self, name: impl Into<String>) -> Csrf {
        self.field_name = name.into();
        self
    }

    /// Headers checked, in order, for the submitted token.
    pub fn header_names<S: Into<String>>(mut self, names: impl IntoIterator<Item = S>) -> Csrf {
        self.header_names = names.into_iter().map(Into::into).collect();
        self
    }

    /// Methods (as sent by the client, before method override) that skip validation.
    pub fn safe_methods(mut self, methods: impl IntoIterator<Item = Method>) -> Csrf {
        self.safe_methods = methods.into_iter().collect();
        self
    }

    /// Exact origins (`https://app.example`) allowed to send unsafe requests, replacing the
    /// same-origin default.
    pub fn origins<S: Into<String>>(mut self, origins: impl IntoIterator<Item = S>) -> Csrf {
        self.origins = Origins::List(origins.into_iter().map(Into::into).collect());
        self
    }

    /// Decide per request whether an origin is allowed.
    pub fn origin_fn(mut self, check: impl Fn(&str, &Context) -> bool + Send + Sync + 'static) -> Csrf {
        self.origins = Origins::Check(Arc::new(check));
        self
    }

    /// Whether requests with neither Origin nor Referer pass the origin check.
    pub fn allow_missing_origin(mut self, allow: bool) -> Csrf {
        self.allow_missing_origin = allow;
        self
    }

    /// Read the submitted token yourself instead of from headers and the form.
    pub fn value(mut self, read: impl Fn(&Context) -> Option<String> + Send + Sync + 'static) -> Csrf {
        self.value = Some(Arc::new(read));
        self
    }

    /// The response for rejected requests (default: 403 with a short message).
    pub fn on_error(mut self, respond: impl Fn(CsrfFailure, &Context) -> Response + Send + Sync + 'static) -> Csrf {
        self.on_error = Some(Arc::new(respond));
        self
    }

    fn check(&self, ctx: &Context) -> Result<(), CsrfFailure> {
        let session = ctx.get::<Session>().expect("csrf middleware requires session() middleware to run before it");
        let expected = token_in(session, &self.token_key);
        if self.safe_methods.contains(ctx.original_method()) {
            return Ok(());
        }
        let allowed = match request_origin(ctx) {
            None => self.allow_missing_origin,
            Some(origin) => match &self.origins {
                Origins::SameOrigin => origin == ctx.url().origin(),
                Origins::List(list) => list.contains(&origin),
                Origins::Check(check) => check(&origin, ctx),
            },
        };
        if !allowed {
            return Err(CsrfFailure::InvalidOrigin);
        }
        let submitted = match &self.value {
            Some(read) => read(ctx),
            None => self
                .header_names
                .iter()
                .find_map(|h| ctx.header(h).map(str::trim).filter(|v| !v.is_empty()))
                .or_else(|| ctx.form_data().get(&self.field_name).map(str::trim))
                .map(str::to_string),
        };
        let submitted = submitted.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).ok_or(CsrfFailure::MissingToken)?;
        if rmx_crypto::constant_time_eq(submitted.as_bytes(), expected.as_bytes()) {
            Ok(())
        } else {
            Err(CsrfFailure::InvalidToken)
        }
    }
}

impl Middleware for Csrf {
    fn call(&self, ctx: Context, next: Next) -> BoxFuture<Response> {
        match self.check(&ctx) {
            Ok(()) => next.run(ctx),
            Err(failure) => {
                let res = match &self.on_error {
                    Some(respond) => respond(failure, &ctx),
                    None => {
                        let message = match failure {
                            CsrfFailure::InvalidOrigin => "Forbidden: invalid CSRF origin",
                            CsrfFailure::MissingToken => "Forbidden: missing CSRF token",
                            CsrfFailure::InvalidToken => "Forbidden: invalid CSRF token",
                        };
                        (StatusCode::FORBIDDEN, message).into_response()
                    }
                };
                Box::pin(async move { res })
            }
        }
    }
}

/// The CSRF token for this request's session (stored under `_csrf`), created if missing.
///
/// # Panics
///
/// When the session middleware did not run.
pub fn csrf_token(ctx: &Context) -> String {
    csrf_token_with_key(ctx, "_csrf")
}

/// [`csrf_token`] for a custom [`Csrf::token_key`].
pub fn csrf_token_with_key(ctx: &Context, key: &str) -> String {
    token_in(ctx.get::<Session>().expect("Session is not started. Use session() middleware before csrf()."), key)
}

fn token_in(session: &Session, key: &str) -> String {
    if let Some(token) = session.get::<String>(key).filter(|t| !t.is_empty()) {
        return token;
    }
    let token = rmx_crypto::random_hex(32);
    session.set(key, &token);
    token
}

fn request_origin(ctx: &Context) -> Option<String> {
    if let Some(origin) = ctx.header("origin").filter(|o| !o.trim().is_empty()) {
        return Some(origin.to_string());
    }
    let referer = ctx.header("referer").filter(|r| !r.trim().is_empty())?;
    rmx_url::Url::parse(referer).ok().map(|u| u.origin())
}

#[cfg(test)]
mod tests {
    use rmx_cookie::Cookie;
    use rmx_fetch::{block_on, Body};
    use rmx_fetch_router::{Context, Router};
    use rmx_session::MemoryStorage;

    use super::*;

    struct Client {
        router: Router,
        cookie: String,
        token: String,
    }

    impl Client {
        fn new(csrf: Csrf) -> Client {
            let mut router = Router::new()
                .with(rmx_session_middleware::session(Cookie::new("sid").secrets(vec!["k".into()]), MemoryStorage::new()))
                .with(rmx_form_data_middleware::form_data())
                .with(rmx_method_override_middleware::method_override())
                .with(csrf);
            router.get("/form", |ctx: Context| async move { csrf_token(&ctx) });
            router.post("/submit", |_ctx: Context| async { "ok" });
            router.put("/submit", |_ctx: Context| async { "put ok" });
            router.get("/submit", |_ctx: Context| async { "get ok" });
            let res = block_on(router.fetch_url("http://app.test/form"));
            let cookie = res.headers()["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_string();
            let token = block_on(res.into_body().text()).unwrap();
            Client { router, cookie, token }
        }

        fn post(&self, body: &str, headers: &[(&str, &str)]) -> (u16, String) {
            let mut req = http::Request::builder()
                .method("POST")
                .uri("http://app.test/submit")
                .header("cookie", &self.cookie)
                .header("content-type", "application/x-www-form-urlencoded");
            for (k, v) in headers {
                req = req.header(*k, *v);
            }
            let res = block_on(self.router.fetch(req.body(Body::from(body.to_string())).unwrap()));
            (res.status().as_u16(), block_on(res.into_body().text()).unwrap())
        }
    }

    #[test]
    fn token_is_stable_per_session_and_required_for_unsafe_methods() {
        let c = Client::new(csrf());
        assert_eq!(c.token.len(), 64);
        let res = block_on(c.router.fetch(http::Request::get("http://app.test/form").header("cookie", &c.cookie).body(Body::empty()).unwrap()));
        assert_eq!(block_on(res.into_body().text()).unwrap(), c.token, "same session, same token");

        assert_eq!(c.post("", &[]), (403, "Forbidden: missing CSRF token".into()));
        assert_eq!(c.post("_csrf=nope", &[]), (403, "Forbidden: invalid CSRF token".into()));
        assert_eq!(c.post(&format!("_csrf={}", c.token), &[]), (200, "ok".into()));
        assert_eq!(c.post("", &[("x-csrf-token", &c.token)]).0, 200);
        assert_eq!(c.post("", &[("csrf-token", &c.token)]).0, 200);
    }

    #[test]
    fn checks_origin_and_referer() {
        let c = Client::new(csrf());
        let body = format!("_csrf={}", c.token);
        assert_eq!(c.post(&body, &[("origin", "http://app.test")]).0, 200);
        assert_eq!(c.post(&body, &[("origin", "http://evil.test")]), (403, "Forbidden: invalid CSRF origin".into()));
        assert_eq!(c.post(&body, &[("referer", "http://evil.test/page")]).0, 403);
        assert_eq!(c.post(&body, &[("referer", "http://app.test/page")]).0, 200);

        let strict = Client::new(csrf().allow_missing_origin(false));
        assert_eq!(strict.post(&format!("_csrf={}", strict.token), &[]).0, 403);

        let listed = Client::new(csrf().origins(["https://admin.test"]));
        let body = format!("_csrf={}", listed.token);
        assert_eq!(listed.post(&body, &[("origin", "https://admin.test")]).0, 200);
        assert_eq!(listed.post(&body, &[("origin", "http://app.test")]).0, 403);
    }

    #[test]
    fn method_override_cannot_skip_validation() {
        let c = Client::new(csrf());
        assert_eq!(c.post("_method=GET", &[]).0, 403, "the original method was POST");
        assert_eq!(c.post(&format!("_method=PUT&_csrf={}", c.token), &[]), (200, "put ok".into()));
    }

    #[test]
    fn custom_error_response() {
        let c = Client::new(csrf().on_error(|failure, _| (StatusCode::BAD_REQUEST, format!("{failure:?}")).into_response()));
        assert_eq!(c.post("", &[]), (400, "MissingToken".into()));
    }
}
