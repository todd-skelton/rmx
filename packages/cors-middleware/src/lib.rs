//! CORS headers and preflight responses.

use std::sync::Arc;

use http::header::{self, HeaderMap, HeaderName, HeaderValue};
use http::{Method, StatusCode};
use rmx_fetch::{Body, Response};
use rmx_fetch_router::{BoxFuture, Context, Middleware, Next};

type OriginFn = dyn Fn(&str, &Context) -> Option<String> + Send + Sync;
type HeadersFn = dyn Fn(&Context) -> Option<Vec<String>> + Send + Sync;

#[derive(Clone)]
enum Origins {
    Any,
    Reflect,
    Disabled,
    List(Vec<String>),
    Check(Arc<OriginFn>),
}

#[derive(Clone)]
enum AllowedHeaders {
    Reflect,
    List(String),
    Check(Arc<HeadersFn>),
}

/// Adds CORS headers to responses for requests with an `Origin`, and answers preflight
/// (`OPTIONS` with `Access-Control-Request-Method`) requests itself.
#[derive(Clone)]
pub struct Cors {
    origins: Origins,
    origin_configured: bool,
    methods: String,
    allowed_headers: AllowedHeaders,
    exposed_headers: String,
    credentials: bool,
    max_age: Option<u64>,
    preflight_continue: bool,
    preflight_status: StatusCode,
    allow_private_network: bool,
}

/// [`Cors`] with Remix's defaults: any origin, methods GET/HEAD/PUT/PATCH/POST/DELETE,
/// requested headers reflected, no credentials, preflights answered with 204.
pub fn cors() -> Cors {
    Cors {
        origins: Origins::Any,
        origin_configured: false,
        methods: "GET, HEAD, PUT, PATCH, POST, DELETE".into(),
        allowed_headers: AllowedHeaders::Reflect,
        exposed_headers: String::new(),
        credentials: false,
        max_age: None,
        preflight_continue: false,
        preflight_status: StatusCode::NO_CONTENT,
        allow_private_network: false,
    }
}

impl Cors {
    fn set_origins(mut self, origins: Origins) -> Cors {
        self.origins = origins;
        self.origin_configured = true;
        self
    }

    /// Allow every origin (`*`). With credentials, the request origin is echoed instead.
    pub fn any_origin(self) -> Cors {
        self.set_origins(Origins::Any)
    }

    /// Echo back whatever `Origin` the request sent.
    pub fn reflect_origin(self) -> Cors {
        self.set_origins(Origins::Reflect)
    }

    /// Send no CORS headers.
    pub fn disabled(self) -> Cors {
        self.set_origins(Origins::Disabled)
    }

    /// Allow these exact origins (`"*"` in the list allows all).
    pub fn origins<S: Into<String>>(self, origins: impl IntoIterator<Item = S>) -> Cors {
        self.set_origins(Origins::List(origins.into_iter().map(Into::into).collect()))
    }

    /// Decide per request: return the `Access-Control-Allow-Origin` value, or `None` to deny.
    pub fn origin_fn(self, check: impl Fn(&str, &Context) -> Option<String> + Send + Sync + 'static) -> Cors {
        self.set_origins(Origins::Check(Arc::new(check)))
    }

    pub fn methods<S: AsRef<str>>(mut self, methods: impl IntoIterator<Item = S>) -> Cors {
        let mut out: Vec<String> = Vec::new();
        for m in methods {
            let m = m.as_ref().trim().to_ascii_uppercase();
            if !m.is_empty() && !out.contains(&m) {
                out.push(m);
            }
        }
        self.methods = out.join(", ");
        self
    }

    /// Headers allowed in preflights, instead of reflecting the requested ones.
    pub fn allowed_headers<S: AsRef<str>>(mut self, headers: impl IntoIterator<Item = S>) -> Cors {
        self.allowed_headers = AllowedHeaders::List(header_list(headers));
        self
    }

    /// Decide allowed headers per preflight; `None` falls back to reflecting.
    pub fn allowed_headers_fn(mut self, f: impl Fn(&Context) -> Option<Vec<String>> + Send + Sync + 'static) -> Cors {
        self.allowed_headers = AllowedHeaders::Check(Arc::new(f));
        self
    }

    pub fn exposed_headers<S: AsRef<str>>(mut self, headers: impl IntoIterator<Item = S>) -> Cors {
        self.exposed_headers = header_list(headers);
        self
    }

    pub fn credentials(mut self, on: bool) -> Cors {
        self.credentials = on;
        self
    }

    /// `Access-Control-Max-Age` for preflights, in seconds.
    pub fn max_age(mut self, seconds: u64) -> Cors {
        self.max_age = Some(seconds);
        self
    }

    /// Let preflights reach the route after adding headers.
    pub fn preflight_continue(mut self, on: bool) -> Cors {
        self.preflight_continue = on;
        self
    }

    pub fn preflight_status(mut self, status: StatusCode) -> Cors {
        self.preflight_status = status;
        self
    }

    /// Answer Private Network Access preflights.
    pub fn allow_private_network(mut self, on: bool) -> Cors {
        self.allow_private_network = on;
        self
    }

    fn allowed_origin(&self, origin: &str, ctx: &Context) -> Option<String> {
        match &self.origins {
            Origins::Any => Some("*".into()),
            Origins::Reflect => Some(origin.into()),
            Origins::Disabled => None,
            Origins::List(list) => {
                if list.iter().any(|o| o == "*") {
                    Some("*".into())
                } else {
                    list.iter().any(|o| o == origin).then(|| origin.into())
                }
            }
            Origins::Check(check) => check(origin, ctx),
        }
    }
}

fn header_list<S: AsRef<str>>(headers: impl IntoIterator<Item = S>) -> String {
    let mut out: Vec<String> = Vec::new();
    for h in headers {
        let h = h.as_ref().trim();
        if !h.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(h)) {
            out.push(h.to_string());
        }
    }
    out.join(", ")
}

fn empty(status: StatusCode) -> Response {
    let mut res = Response::new(Body::empty());
    *res.status_mut() = status;
    res
}

fn set(headers: &mut HeaderMap, name: &'static str, value: &str) {
    if let Ok(v) = HeaderValue::from_str(value) {
        headers.insert(HeaderName::from_static(name), v);
    }
}

/// Merge names into a `Vary` header, case-insensitively.
fn merge_vary(headers: &mut HeaderMap, add: &[&str]) {
    if add.is_empty() {
        return;
    }
    let mut names: Vec<String> = headers
        .get_all(header::VARY)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect();
    for a in add {
        if !names.iter().any(|n| n.eq_ignore_ascii_case(a)) {
            names.push(a.to_string());
        }
    }
    if let Ok(v) = HeaderValue::from_str(&names.join(", ")) {
        headers.insert(header::VARY, v);
    }
}

impl Middleware for Cors {
    fn call(&self, ctx: Context, next: Next) -> BoxFuture<Response> {
        let preflight = *ctx.method() == Method::OPTIONS && ctx.headers().contains_key("access-control-request-method");
        let Some(origin) = ctx.header("origin").map(str::to_string) else {
            if preflight && !self.preflight_continue {
                let res = empty(self.preflight_status);
                return Box::pin(async move { res });
            }
            return next.run(ctx);
        };
        let Some(allowed) = self.allowed_origin(&origin, &ctx) else {
            if preflight && !self.preflight_continue {
                return Box::pin(async { empty(StatusCode::FORBIDDEN) });
            }
            return next.run(ctx);
        };

        let mut cors = HeaderMap::new();
        let mut vary: Vec<&str> = Vec::new();
        let allow_origin = if self.credentials && allowed == "*" && self.origin_configured { origin } else { allowed };
        set(&mut cors, "access-control-allow-origin", &allow_origin);
        if allow_origin != "*" {
            vary.push("Origin");
        }
        if self.credentials {
            set(&mut cors, "access-control-allow-credentials", "true");
        }

        if preflight {
            set(&mut cors, "access-control-allow-methods", &self.methods);
            vary.push("Access-Control-Request-Method");
            let requested = ctx.header("access-control-request-headers").filter(|h| !h.trim().is_empty()).map(str::to_string);
            let (value, vary_headers) = match &self.allowed_headers {
                AllowedHeaders::List(list) => ((!list.is_empty()).then(|| list.clone()), false),
                AllowedHeaders::Check(f) => match f(&ctx) {
                    Some(list) => (Some(header_list(list)).filter(|l| !l.is_empty()), true),
                    None => (requested, true),
                },
                AllowedHeaders::Reflect => {
                    let vary = requested.is_some();
                    (requested, vary)
                }
            };
            if let Some(v) = value {
                set(&mut cors, "access-control-allow-headers", &v);
            }
            if vary_headers {
                vary.push("Access-Control-Request-Headers");
            }
            if let Some(age) = self.max_age {
                set(&mut cors, "access-control-max-age", &age.to_string());
            }
            if self.allow_private_network {
                vary.push("Access-Control-Request-Private-Network");
                if ctx.header("access-control-request-private-network").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
                    set(&mut cors, "access-control-allow-private-network", "true");
                }
            }
            if !self.preflight_continue {
                let mut res = empty(self.preflight_status);
                res.headers_mut().extend(cors);
                merge_vary(res.headers_mut(), &vary);
                return Box::pin(async move { res });
            }
        } else if !self.exposed_headers.is_empty() {
            set(&mut cors, "access-control-expose-headers", &self.exposed_headers);
        }

        let vary: Vec<&'static str> = vary;
        let fut = next.run(ctx);
        Box::pin(async move {
            let mut res = fut.await;
            for (name, value) in cors.iter() {
                res.headers_mut().insert(name.clone(), value.clone());
            }
            merge_vary(res.headers_mut(), &vary);
            res
        })
    }
}

#[cfg(test)]
mod tests {
    use rmx_fetch::block_on;
    use rmx_fetch_router::Router;

    use super::*;

    fn send(cors: Cors, method: &str, headers: &[(&str, &str)]) -> Response {
        let mut router = Router::new().with(cors);
        router.any("/api", |_ctx: Context| async {
            let mut res = Response::new(Body::from("data"));
            res.headers_mut().insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
            res
        });
        let mut req = http::Request::builder().method(method).uri("http://api.test/api");
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        block_on(router.fetch(req.body(Body::empty()).unwrap()))
    }

    fn h<'a>(res: &'a Response, name: &str) -> Option<&'a str> {
        res.headers().get(name).map(|v| v.to_str().unwrap())
    }

    #[test]
    fn simple_requests_get_allow_origin() {
        let res = send(cors(), "GET", &[("origin", "http://web.test")]);
        assert_eq!(h(&res, "access-control-allow-origin"), Some("*"));
        assert_eq!(h(&res, "vary"), Some("Accept-Encoding"));
        let res = send(cors(), "GET", &[]);
        assert_eq!(h(&res, "access-control-allow-origin"), None);

        let res = send(cors().origins(["http://web.test"]).exposed_headers(["X-Total", "x-total"]), "GET", &[("origin", "http://web.test")]);
        assert_eq!(h(&res, "access-control-allow-origin"), Some("http://web.test"));
        assert_eq!(h(&res, "access-control-expose-headers"), Some("X-Total"));
        assert_eq!(h(&res, "vary"), Some("Accept-Encoding, Origin"));
        let res = send(cors().origins(["http://web.test"]), "GET", &[("origin", "http://evil.test")]);
        assert_eq!(h(&res, "access-control-allow-origin"), None);
    }

    #[test]
    fn preflights_are_answered() {
        let res = send(
            cors().reflect_origin().credentials(true).max_age(600),
            "OPTIONS",
            &[("origin", "http://web.test"), ("access-control-request-method", "PUT"), ("access-control-request-headers", "x-token")],
        );
        assert_eq!(res.status(), 204);
        assert_eq!(h(&res, "access-control-allow-origin"), Some("http://web.test"));
        assert_eq!(h(&res, "access-control-allow-credentials"), Some("true"));
        assert_eq!(h(&res, "access-control-allow-methods"), Some("GET, HEAD, PUT, PATCH, POST, DELETE"));
        assert_eq!(h(&res, "access-control-allow-headers"), Some("x-token"));
        assert_eq!(h(&res, "access-control-max-age"), Some("600"));
        assert_eq!(h(&res, "vary"), Some("Origin, Access-Control-Request-Method, Access-Control-Request-Headers"));

        let denied = send(cors().origins(["http://web.test"]), "OPTIONS", &[("origin", "http://evil.test"), ("access-control-request-method", "PUT")]);
        assert_eq!(denied.status(), 403);
    }

    #[test]
    fn credentials_echo_origin_only_when_configured() {
        let implicit = send(cors().credentials(true), "GET", &[("origin", "http://web.test")]);
        assert_eq!(h(&implicit, "access-control-allow-origin"), Some("*"));
        let explicit = send(cors().any_origin().credentials(true), "GET", &[("origin", "http://web.test")]);
        assert_eq!(h(&explicit, "access-control-allow-origin"), Some("http://web.test"));
    }

    #[test]
    fn private_network_and_disabled() {
        let res = send(
            cors().allow_private_network(true),
            "OPTIONS",
            &[("origin", "http://web.test"), ("access-control-request-method", "GET"), ("access-control-request-private-network", "true")],
        );
        assert_eq!(h(&res, "access-control-allow-private-network"), Some("true"));
        let res = send(cors().disabled(), "GET", &[("origin", "http://web.test")]);
        assert_eq!(h(&res, "access-control-allow-origin"), None);
    }
}
