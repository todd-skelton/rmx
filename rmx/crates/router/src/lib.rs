//! A router built on [`rmx_fetch::Request`] / [`rmx_fetch::Response`] (Remix's `fetch-router`).
//!
//! ```
//! use rmx_router::{Router, Context};
//! use rmx_fetch::response;
//!
//! let mut router = Router::new();
//! router.get("/", |_ctx: Context| async { response::text("Home") });
//! router.get("/blog/:slug", |ctx: Context| async move {
//!     response::text(format!("Post {}", ctx.param("slug").unwrap()))
//! });
//!
//! let res = rmx_fetch::block_on(router.fetch_url("https://remix.run/blog/hello-remix"));
//! assert_eq!(rmx_fetch::block_on(res.into_body().text()).unwrap(), "Post hello-remix");
//! ```
//!
//! Middleware runs at two levels: router middleware (every request, before matching) and
//! route middleware (attached with [`Router::group`], after a route matched). Middleware
//! and handlers share typed, request-scoped values through [`Context::set`] / [`Context::get`].
//!
//! The router also runs in the browser (see `rmx-spa`). There, handlers and middleware need
//! not be `Send` ([`MaybeSend`]); the shared `Arc`s are then only used on one thread.
#![cfg_attr(target_arch = "wasm32", allow(clippy::arc_with_non_send_sync))]

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use http::{header, HeaderValue, Method, StatusCode};
use rmx_fetch::{request_url, response, Body, Request, Response};
use rmx_route_pattern::{Matcher, Params, RoutePattern};
use rmx_url::Url;

pub mod async_context;
pub mod routes;

pub use routes::{IntoRoute, Route};

/// A boxed future: `Send` on servers, where requests run on a multi-threaded runtime.
#[cfg(not(target_arch = "wasm32"))]
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// A boxed future. In the browser everything runs on one thread, and futures that wait on
/// browser APIs (timers, `fetch`) are not `Send`, so handlers need not be either.
#[cfg(target_arch = "wasm32")]
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + 'static>>;

/// `Send` on servers; implemented by every type in the browser (see [`BoxFuture`]).
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> MaybeSend for T {}
/// `Send` on servers; implemented by every type in the browser (see [`BoxFuture`]).
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}

/// `Sync` on servers; implemented by every type in the browser.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSync: Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync> MaybeSync for T {}
/// `Sync` on servers; implemented by every type in the browser.
#[cfg(target_arch = "wasm32")]
pub trait MaybeSync {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSync for T {}

// ---------------------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------------------

/// Everything a handler or middleware gets for one request.
pub struct Context {
    request: Request,
    original_method: Method,
    url: Url,
    params: Params,
    router: Router,
    /// Set by the [`async_context`] middleware; mirrors changes to this context.
    scope: Option<async_context::Scope>,
}

impl Context {
    fn new(request: Request, router: Router) -> Context {
        let (parts, body) = request.into_parts();
        let url = request_url(&parts);
        let original_method = parts.method.clone();
        Context { request: Request::from_parts(parts, body), original_method, url, params: Params::default(), router, scope: None }
    }

    pub fn request(&self) -> &Request {
        &self.request
    }

    pub fn request_mut(&mut self) -> &mut Request {
        &mut self.request
    }

    /// Take the request body, leaving an empty one. Read it with `.text()`, `.form()`, etc.
    pub fn take_body(&mut self) -> Body {
        std::mem::take(self.request.body_mut())
    }

    pub fn into_request(self) -> Request {
        self.request
    }

    pub fn method(&self) -> &Method {
        self.request.method()
    }

    /// Change the method used for routing (see method-override middleware).
    pub fn set_method(&mut self, method: Method) {
        async_context::update(&self.scope, |s| s.method = method.clone());
        *self.request.method_mut() = method;
    }

    /// The method the client sent, before any [`Context::set_method`] override.
    pub fn original_method(&self) -> &Method {
        &self.original_method
    }

    pub fn headers(&self) -> &http::HeaderMap {
        self.request.headers()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.request.headers().get(name).and_then(|v| v.to_str().ok())
    }

    /// The absolute request URL.
    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn set_url(&mut self, url: Url) {
        async_context::update(&self.scope, |s| s.url = url.clone());
        self.url = url;
    }

    /// Params captured by the matched route (empty in router middleware).
    pub fn params(&self) -> &Params {
        &self.params
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name)
    }

    /// Read a request-scoped value set by middleware.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.request.extensions().get::<T>()
    }

    pub fn get_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.request.extensions_mut().get_mut::<T>()
    }

    /// Store a request-scoped value (one per type; use a newtype for distinct keys).
    pub fn set<T: Clone + Send + Sync + 'static>(&mut self, value: T) {
        async_context::update(&self.scope, |s| {
            s.values.insert(value.clone());
        });
        self.request.extensions_mut().insert(value);
    }

    /// The router handling this request, for subrequests (`ctx.router().fetch(req)`).
    pub fn router(&self) -> &Router {
        &self.router
    }
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context").field("method", self.method()).field("url", &self.url.href()).finish()
    }
}

// ---------------------------------------------------------------------------------------
// Responses and errors
// ---------------------------------------------------------------------------------------

/// Values a handler may return.
pub trait IntoResponse {
    fn into_response(self) -> Response;
}

impl IntoResponse for Response {
    fn into_response(self) -> Response {
        self
    }
}

impl IntoResponse for &'static str {
    fn into_response(self) -> Response {
        response::text(self)
    }
}

impl IntoResponse for String {
    fn into_response(self) -> Response {
        response::text(self)
    }
}

impl IntoResponse for StatusCode {
    fn into_response(self) -> Response {
        let mut res = Response::new(Body::empty());
        *res.status_mut() = self;
        res
    }
}

impl<S: Into<String>> IntoResponse for (StatusCode, S) {
    fn into_response(self) -> Response {
        response::status(self.0, self.1)
    }
}

impl<T: IntoResponse, E: IntoResponse> IntoResponse for Result<T, E> {
    fn into_response(self) -> Response {
        match self {
            Ok(v) => v.into_response(),
            Err(e) => e.into_response(),
        }
    }
}

impl<T: IntoResponse> IntoResponse for Option<T> {
    fn into_response(self) -> Response {
        self.map_or_else(response::not_found, IntoResponse::into_response)
    }
}

/// A handler error: a status plus a message. Any `std::error::Error` converts into a 500,
/// so handlers can use `?`.
#[derive(Debug)]
pub struct Error {
    pub status: StatusCode,
    pub message: String,
}

impl Error {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Error {
        Error { status, message: message.into() }
    }

    pub fn bad_request(message: impl Into<String>) -> Error {
        Error::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn not_found() -> Error {
        Error::new(StatusCode::NOT_FOUND, "Not Found")
    }
}

impl<E: std::error::Error + Send + Sync + 'static> From<E> for Error {
    fn from(e: E) -> Error {
        Error::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        if self.status.is_server_error() {
            eprintln!("rmx: {} {}", self.status, self.message);
        }
        response::status(self.status, self.message)
    }
}

/// `Result<Response, Error>`, the usual handler return type.
pub type Result<T = Response, E = Error> = std::result::Result<T, E>;

// ---------------------------------------------------------------------------------------
// Handlers and middleware
// ---------------------------------------------------------------------------------------

/// A request handler (Remix's "action"): any `Fn(Context) -> impl Future<Output = impl IntoResponse>`.
pub trait Handler: MaybeSend + MaybeSync + 'static {
    fn call(&self, ctx: Context) -> BoxFuture<Response>;
}

impl<F, Fut, R> Handler for F
where
    F: Fn(Context) -> Fut + MaybeSend + MaybeSync + 'static,
    Fut: Future<Output = R> + MaybeSend + 'static,
    R: IntoResponse,
{
    fn call(&self, ctx: Context) -> BoxFuture<Response> {
        let fut = self(ctx);
        Box::pin(async move { fut.await.into_response() })
    }
}

/// Middleware: runs code before and/or after the rest of the chain.
///
/// ```
/// use rmx_router::{Context, Next};
/// async fn timing(ctx: Context, next: Next) -> rmx_fetch::Response {
///     let start = std::time::Instant::now();
///     let mut res = next.run(ctx).await;
///     let ms = start.elapsed().as_millis().to_string();
///     res.headers_mut().insert("x-time-ms", ms.parse().unwrap());
///     res
/// }
/// ```
pub trait Middleware: MaybeSend + MaybeSync + 'static {
    fn call(&self, ctx: Context, next: Next) -> BoxFuture<Response>;
}

impl<F, Fut> Middleware for F
where
    F: Fn(Context, Next) -> Fut + MaybeSend + MaybeSync + 'static,
    Fut: Future<Output = Response> + MaybeSend + 'static,
{
    fn call(&self, ctx: Context, next: Next) -> BoxFuture<Response> {
        Box::pin(self(ctx, next))
    }
}

type MiddlewareList = Arc<[Arc<dyn Middleware>]>;

enum Endpoint {
    /// End of the router-level chain: match a route and dispatch.
    Dispatch,
    /// End of a route-level chain: the matched handler.
    Handler(Arc<dyn Handler>),
}

/// The rest of the middleware chain.
pub struct Next {
    chain: MiddlewareList,
    index: usize,
    endpoint: Arc<Endpoint>,
}

impl Next {
    /// Run the remaining middleware and the handler.
    pub fn run(mut self, ctx: Context) -> BoxFuture<Response> {
        if let Some(mw) = self.chain.get(self.index).cloned() {
            self.index += 1;
            return mw.call(ctx, self);
        }
        match &*self.endpoint {
            Endpoint::Handler(h) => h.call(ctx),
            Endpoint::Dispatch => {
                let router = ctx.router.clone();
                Box::pin(async move { router.dispatch(ctx).await })
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------------------

struct RouteEntry {
    method: Option<Method>,
    handler: Arc<dyn Handler>,
    middleware: MiddlewareList,
    pattern: RoutePattern,
}

struct Inner {
    middleware: Vec<Arc<dyn Middleware>>,
    chain: MiddlewareList,
    routes: Matcher<Arc<RouteEntry>>,
    not_found: Arc<dyn Handler>,
}

/// Maps requests to handlers. Cheap to clone (an `Arc`); register routes before sharing it.
#[derive(Clone)]
pub struct Router {
    inner: Arc<Inner>,
}

impl Default for Router {
    fn default() -> Self {
        Router::new()
    }
}

impl Router {
    pub fn new() -> Router {
        // Remix's default handler: `Not Found: <pathname>`.
        let not_found: Arc<dyn Handler> =
            Arc::new(|ctx: Context| async move { response::status(StatusCode::NOT_FOUND, format!("Not Found: {}", ctx.url().pathname())) });
        Router {
            inner: Arc::new(Inner {
                middleware: Vec::new(),
                chain: Arc::from(Vec::new()),
                routes: Matcher::new(),
                not_found,
            }),
        }
    }

    fn inner_mut(&mut self) -> &mut Inner {
        Arc::get_mut(&mut self.inner).expect("rmx-router: register routes and middleware before the router is cloned or serving")
    }

    /// Add router middleware (runs for every request, in the order added).
    pub fn with(mut self, middleware: impl Middleware) -> Router {
        self.use_middleware(middleware);
        self
    }

    pub fn use_middleware(&mut self, middleware: impl Middleware) {
        let inner = self.inner_mut();
        inner.middleware.push(Arc::new(middleware));
        inner.chain = Arc::from(inner.middleware.clone());
    }

    /// The handler for requests no route matches (default: plain 404).
    pub fn not_found(&mut self, handler: impl Handler) {
        self.inner_mut().not_found = Arc::new(handler);
    }

    fn builder(&mut self) -> RouteBuilder<'_> {
        RouteBuilder { router: self, prefix: None, middleware: Vec::new() }
    }

    /// Register a handler for a route, using the route's own method.
    pub fn route(&mut self, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        self.builder().route(route, handler);
        self
    }

    pub fn any(&mut self, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        self.builder().add(None, route, handler);
        self
    }

    pub fn get(&mut self, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        self.builder().add(Some(Method::GET), route, handler);
        self
    }

    pub fn post(&mut self, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        self.builder().add(Some(Method::POST), route, handler);
        self
    }

    pub fn put(&mut self, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        self.builder().add(Some(Method::PUT), route, handler);
        self
    }

    pub fn patch(&mut self, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        self.builder().add(Some(Method::PATCH), route, handler);
        self
    }

    pub fn delete(&mut self, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        self.builder().add(Some(Method::DELETE), route, handler);
        self
    }

    pub fn head(&mut self, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        self.builder().add(Some(Method::HEAD), route, handler);
        self
    }

    /// Register routes that share route middleware (Remix's controller middleware).
    pub fn group(&mut self, middleware: Vec<Arc<dyn Middleware>>, install: impl FnOnce(&mut RouteBuilder<'_>)) {
        let mut b = RouteBuilder { router: self, prefix: None, middleware };
        install(&mut b);
    }

    /// Register a group of routes below a pattern prefix (`router.mount('/admin', install)`).
    pub fn mount(&mut self, prefix: &str, install: impl FnOnce(&mut RouteBuilder<'_>)) {
        self.builder().mount(prefix, install);
    }

    /// Handle a request.
    pub fn fetch(&self, request: Request) -> BoxFuture<Response> {
        let ctx = Context::new(request, self.clone());
        Next { chain: self.inner.chain.clone(), index: 0, endpoint: Arc::new(Endpoint::Dispatch) }.run(ctx)
    }

    /// `GET` an absolute URL. Handy in tests.
    pub fn fetch_url(&self, url: &str) -> BoxFuture<Response> {
        let req = http::Request::builder().uri(url).body(Body::empty()).expect("valid request URL");
        self.fetch(req)
    }

    /// The registered routes, for debugging.
    pub fn routes(&self) -> Vec<String> {
        self.inner
            .routes
            .entries()
            .map(|(_, e)| match &e.method {
                Some(m) => format!("{m} {}", e.pattern),
                None => format!("ANY {}", e.pattern),
            })
            .collect()
    }

    async fn dispatch(&self, mut ctx: Context) -> Response {
        let method = ctx.method().clone();
        let matches = self.inner.routes.match_all(ctx.url());
        if matches.is_empty() {
            return self.inner.not_found.call(ctx).await;
        }
        let pick = |want: &Method| {
            matches.iter().find(|m| m.data.method.as_ref().is_none_or(|rm| rm == want))
        };
        let mut strip_body = false;
        let found = pick(&method).or_else(|| {
            if method == Method::HEAD {
                strip_body = true;
                pick(&Method::GET)
            } else {
                None
            }
        });
        let Some(found) = found else {
            let mut allow: Vec<String> = Vec::new();
            for m in &matches {
                if let Some(rm) = &m.data.method {
                    allow.push(rm.to_string());
                    if *rm == Method::GET {
                        allow.push("HEAD".into());
                    }
                }
            }
            allow.sort();
            allow.dedup();
            let mut res = response::status(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed");
            if let Ok(v) = HeaderValue::from_str(&allow.join(", ")) {
                res.headers_mut().insert(header::ALLOW, v);
            }
            return res;
        };
        let entry = found.data.clone();
        ctx.params = found.params.clone();
        async_context::update(&ctx.scope, |s| s.params = ctx.params.clone());
        drop(matches);
        let res = Next { chain: entry.middleware.clone(), index: 0, endpoint: Arc::new(Endpoint::Handler(entry.handler.clone())) }
            .run(ctx)
            .await;
        if strip_body {
            let (parts, _) = res.into_parts();
            return Response::from_parts(parts, Body::empty());
        }
        res
    }
}

/// Registers routes, possibly below a prefix and with shared route middleware.
pub struct RouteBuilder<'a> {
    router: &'a mut Router,
    prefix: Option<RoutePattern>,
    middleware: Vec<Arc<dyn Middleware>>,
}

impl RouteBuilder<'_> {
    fn add(&mut self, method: Option<Method>, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        let mut route = route.into_route(method);
        if let Some(p) = &self.prefix {
            route = route.under(p);
        }
        let entry = RouteEntry {
            method: route.method().cloned(),
            handler: Arc::new(handler),
            middleware: Arc::from(self.middleware.clone()),
            pattern: route.pattern().clone(),
        };
        self.router.inner_mut().routes.add(route.pattern().clone(), Arc::new(entry));
        self
    }

    /// Register a handler for a route, using the route's own method.
    pub fn route(&mut self, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        self.add(None, route, handler)
    }

    pub fn any(&mut self, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        self.add(None, route, handler)
    }

    pub fn get(&mut self, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        self.add(Some(Method::GET), route, handler)
    }

    pub fn post(&mut self, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        self.add(Some(Method::POST), route, handler)
    }

    pub fn put(&mut self, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        self.add(Some(Method::PUT), route, handler)
    }

    pub fn patch(&mut self, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        self.add(Some(Method::PATCH), route, handler)
    }

    pub fn delete(&mut self, route: impl IntoRoute, handler: impl Handler) -> &mut Self {
        self.add(Some(Method::DELETE), route, handler)
    }

    /// Nested group with extra route middleware.
    pub fn group(&mut self, middleware: Vec<Arc<dyn Middleware>>, install: impl FnOnce(&mut RouteBuilder<'_>)) {
        let mut all = self.middleware.clone();
        all.extend(middleware);
        let mut b = RouteBuilder { router: self.router, prefix: self.prefix.clone(), middleware: all };
        install(&mut b);
    }

    /// Nested group below a prefix.
    pub fn mount(&mut self, prefix: &str, install: impl FnOnce(&mut RouteBuilder<'_>)) {
        let p = RoutePattern::parse(prefix).unwrap_or_else(|e| panic!("{e}"));
        let prefix = Some(match &self.prefix {
            Some(base) => base.join(&p),
            None => p,
        });
        let mut b = RouteBuilder { router: self.router, prefix, middleware: self.middleware.clone() };
        install(&mut b);
    }
}

/// Wrap middleware for [`Router::group`]'s list.
pub fn middleware(m: impl Middleware) -> Arc<dyn Middleware> {
    Arc::new(m)
}

#[cfg(test)]
mod tests;
