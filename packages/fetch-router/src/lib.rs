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
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use http::{header, HeaderValue, Method, StatusCode};
use rmx_fetch::{request_url, response, AbortError, AbortSignal, Body, Request, RequestExt, Response};
use rmx_route_pattern::{specificity, Matcher, MultiMatcher, Params, RoutePattern};
use rmx_url::Url;

pub mod async_context;
pub mod routes;

pub use routes::{IntoRoute, Route, RouteMap};

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
    /// The context's own copy of the request headers, made on the first
    /// [`Context::headers_mut`] or set with [`Context::set_headers`].
    headers: Option<http::HeaderMap>,
    original_method: Method,
    url: Url,
    params: Params,
    router: Router,
    signal: AbortSignal,
    /// Set by the [`async_context`] middleware; mirrors changes to this context.
    scope: Option<async_context::Scope>,
}

impl Context {
    /// A context for `request` outside a router (Remix's `new RequestContext(request)`), for
    /// calling middleware and handlers directly. [`Context::router`] is then an empty router.
    pub fn new(request: Request) -> Context {
        Context::with_router(request, Router::new())
    }

    fn with_router(request: Request, router: Router) -> Context {
        let (parts, body) = request.into_parts();
        let url = request_url(&parts);
        let original_method = parts.method.clone();
        let request = Request::from_parts(parts, body);
        let signal = request.signal();
        Context { request, headers: None, original_method, url, params: Params::default(), router, signal, scope: None }
    }

    /// The request's abort signal (`request.signal`). It aborts when the client goes away
    /// before the response is finished; it never aborts if the request carries none.
    pub fn signal(&self) -> &AbortSignal {
        &self.signal
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

    /// The request headers as this context sees them: the request's, until middleware
    /// changes them with [`Context::headers_mut`] or [`Context::set_headers`].
    pub fn headers(&self) -> &http::HeaderMap {
        self.headers.as_ref().unwrap_or_else(|| self.request.headers())
    }

    /// The headers for later middleware and handlers to read, changeable without touching the
    /// request (Remix's `context.headers`, a copy of `request.headers` made when first used).
    pub fn headers_mut(&mut self) -> HeadersMut<'_> {
        let Context { request, headers, scope, .. } = self;
        let headers = headers.get_or_insert_with(|| request.headers().clone());
        HeadersMut { headers, scope }
    }

    /// Replace the headers this context sees (`context.headers = headers`).
    pub fn set_headers(&mut self, headers: http::HeaderMap) {
        async_context::update(&self.scope, |s| s.headers = headers.clone());
        self.headers = Some(headers);
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers().get(name).and_then(|v| v.to_str().ok())
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

    /// Read a request-scoped value set by middleware. A value's type is its key; use a
    /// newtype for distinct values of one type, and `.cloned().unwrap_or_default()` (or
    /// `unwrap_or`) where Remix gives a context key a default value.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.request.extensions().get::<T>()
    }

    /// Whether a value of type `T` was set (Remix's `context.has(key)`).
    pub fn has<T: Send + Sync + 'static>(&self) -> bool {
        self.get::<T>().is_some()
    }

    /// # Panics
    ///
    /// When `T` was stored with [`Context::set_sealed`].
    pub fn get_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.check_sealed::<T>();
        self.request.extensions_mut().get_mut::<T>()
    }

    /// Store a request-scoped value (one per type; use a newtype for distinct keys).
    ///
    /// # Panics
    ///
    /// When `T` was stored with [`Context::set_sealed`].
    pub fn set<T: Clone + Send + Sync + 'static>(&mut self, value: T) {
        self.check_sealed::<T>();
        async_context::update(&self.scope, |s| {
            s.values.insert(value.clone());
        });
        self.request.extensions_mut().insert(value);
    }

    /// Store a value that later middleware and handlers can read but not replace: their
    /// [`Context::set`] and [`Context::get_mut`] for `T` panic with `message`. The session
    /// middleware uses this so a handler cannot swap in a session it would never save (Remix
    /// checks the same thing after `next()`).
    pub fn set_sealed<T: Clone + Send + Sync + 'static>(&mut self, value: T, message: &'static str) {
        self.set(value);
        self.request.extensions_mut().insert(Sealed::<T>(message, PhantomData));
    }

    fn check_sealed<T: 'static>(&self) {
        if let Some(sealed) = self.request.extensions().get::<Sealed<T>>() {
            panic!("{}", sealed.0);
        }
    }

    /// The router handling this request, for subrequests (`ctx.router().fetch(req)`).
    pub fn router(&self) -> &Router {
        &self.router
    }
}

/// The headers returned by [`Context::headers_mut`]. Changes reach [`async_context`] when
/// it is dropped.
pub struct HeadersMut<'a> {
    headers: &'a mut http::HeaderMap,
    scope: &'a Option<async_context::Scope>,
}

impl std::ops::Deref for HeadersMut<'_> {
    type Target = http::HeaderMap;

    fn deref(&self) -> &http::HeaderMap {
        self.headers
    }
}

impl std::ops::DerefMut for HeadersMut<'_> {
    fn deref_mut(&mut self) -> &mut http::HeaderMap {
        self.headers
    }
}

impl Drop for HeadersMut<'_> {
    fn drop(&mut self) {
        let headers = &*self.headers;
        async_context::update(self.scope, |s| s.headers = headers.clone());
    }
}

/// Marks `T` as stored by [`Context::set_sealed`]; holds the panic message.
struct Sealed<T>(&'static str, PhantomData<fn() -> T>);

impl<T> Clone for Sealed<T> {
    fn clone(&self) -> Self {
        Sealed(self.0, PhantomData)
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
///
/// [`Next::run`] takes `self`, so calling the rest of the chain twice is a compile error
/// (Remix rejects with "next() called multiple times" at runtime), and a middleware must
/// return a `Response`, so one that neither answers nor calls `next` does not compile either.
///
/// ```compile_fail,E0382
/// use rmx_router::{Context, Next};
/// async fn twice(ctx: Context, next: Next) -> rmx_fetch::Response {
///     let _ = next.run(ctx).await;
///     next.run(todo!()).await
/// }
/// ```
///
/// ```compile_fail,E0277
/// use rmx_router::{action, Context, Next};
/// // error: the middleware's future yields `()`, not a `Response` (Remix rejects with
/// // "Middleware must return a Response or call next()").
/// let _ = action(|_: Context| async { "OK" }).with(|_: Context, _: Next| async {});
/// ```
pub struct Next {
    chain: MiddlewareList,
    index: usize,
    endpoint: Arc<Endpoint>,
}

impl Next {
    /// Run the remaining middleware and the handler.
    ///
    /// Like Remix's `runMiddleware`, each step first checks the request's [`AbortSignal`] and
    /// then races the signal: an aborted request skips the rest of the chain and answers with
    /// [`AbortError::into_response`] (Remix rejects with an `AbortError`), which middleware
    /// can recognise with [`AbortError::of`].
    ///
    /// The next step is called when the returned future is first polled, so a middleware or
    /// handler that panics before returning its future panics inside this one, where
    /// [`catch_panic`] sees it (Remix's `next()` returns a rejected promise).
    pub fn run(self, ctx: Context) -> BoxFuture<Response> {
        Box::pin(self.run_step(ctx))
    }

    async fn run_step(mut self, ctx: Context) -> Response {
        if ctx.signal.aborted() {
            return AbortError.into_response();
        }
        let signal = (!ctx.signal.is_never()).then(|| ctx.signal.clone());
        let fut = if let Some(mw) = self.chain.get(self.index).cloned() {
            self.index += 1;
            mw.call(ctx, self)
        } else {
            match &*self.endpoint {
                Endpoint::Handler(h) => h.call(ctx),
                Endpoint::Dispatch => {
                    let router = ctx.router.clone();
                    Box::pin(async move { router.dispatch(ctx).await })
                }
            }
        };
        match signal {
            None => fut.await,
            Some(signal) => signal.race(fut).await.unwrap_or_else(AbortError::into_response),
        }
    }
}

/// A panic caught by [`catch_panic`], with its message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panic(pub String);

impl Panic {
    /// The panic message (`"unknown panic"` when the payload was not a string).
    pub fn message(&self) -> &str {
        &self.0
    }

    fn from_payload(payload: &(dyn std::any::Any + Send)) -> Panic {
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".into());
        Panic(message)
    }
}

impl fmt::Display for Panic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Panic {}

/// Run `future`, turning a panic while it is polled into `Err(Panic)`. A thrown error in
/// Remix rejects the handler's promise and middleware can `try { await next() } catch`; in
/// Rust a handler that fails unexpectedly panics, and middleware catches it with
/// `catch_panic(next.run(ctx)).await`. Expected failures are values (`Result` responses).
///
/// In the browser a panic aborts the program (wasm32 has no unwinding), so nothing is caught.
pub fn catch_panic<F: Future>(future: F) -> CatchPanic<F> {
    CatchPanic(Box::pin(future))
}

/// The future returned by [`catch_panic`].
pub struct CatchPanic<F>(Pin<Box<F>>);

impl<F: Future> Future for CatchPanic<F> {
    type Output = Result<F::Output, Panic>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        let future = self.0.as_mut();
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| future.poll(cx))) {
            Ok(std::task::Poll::Pending) => std::task::Poll::Pending,
            Ok(std::task::Poll::Ready(output)) => std::task::Poll::Ready(Ok(output)),
            Err(payload) => std::task::Poll::Ready(Err(Panic::from_payload(&*payload))),
        }
    }
}

// ---------------------------------------------------------------------------------------
// Controllers
// ---------------------------------------------------------------------------------------

/// A route's handler plus its own middleware (Remix's action object). Build one with
/// [`action`] and add middleware with [`Action::with`].
pub struct Action {
    handler: Arc<dyn Handler>,
    middleware: Vec<Arc<dyn Middleware>>,
}

impl Action {
    pub fn new(handler: impl Handler) -> Action {
        Action { handler: Arc::new(handler), middleware: Vec::new() }
    }

    /// Add middleware that runs after router and controller middleware, before the handler.
    pub fn with(mut self, middleware: impl Middleware) -> Action {
        self.middleware.push(Arc::new(middleware));
        self
    }
}

impl<H: Handler> From<H> for Action {
    fn from(handler: H) -> Action {
        Action::new(handler)
    }
}

/// An action for a route: `action(handler)` or `action(handler).with(middleware)`.
pub fn action(handler: impl Handler) -> Action {
    Action::new(handler)
}

/// Actions for a route map plus middleware shared by all of them (Remix's
/// `createController(routes, { middleware, actions })`). The actions alone convert into a
/// controller without middleware.
pub struct Controller<A> {
    pub middleware: Vec<Arc<dyn Middleware>>,
    pub actions: A,
}

impl<A> Controller<A> {
    pub fn new(actions: A) -> Controller<A> {
        Controller { middleware: Vec::new(), actions }
    }

    /// Add middleware that runs before every action of this controller.
    pub fn with(mut self, middleware: impl Middleware) -> Controller<A> {
        self.middleware.push(Arc::new(middleware));
        self
    }
}

impl<A> From<A> for Controller<A> {
    fn from(actions: A) -> Controller<A> {
        Controller::new(actions)
    }
}

/// `any`, `get`, `head`, `post`, `put`, `patch`, `delete` and `options` for [`Router`] and
/// [`RouteBuilder`]. Each takes a handler or an [`Action`] with action middleware.
macro_rules! verb_methods {
    () => {
        verb_methods!(@one any, None, "any request method");
        verb_methods!(@one get, Some(Method::GET), "`GET` (which also serves `HEAD`)");
        verb_methods!(@one head, Some(Method::HEAD), "`HEAD`");
        verb_methods!(@one post, Some(Method::POST), "`POST`");
        verb_methods!(@one put, Some(Method::PUT), "`PUT`");
        verb_methods!(@one patch, Some(Method::PATCH), "`PATCH`");
        verb_methods!(@one delete, Some(Method::DELETE), "`DELETE`");
        verb_methods!(@one options, Some(Method::OPTIONS), "`OPTIONS`");
    };
    (@one $name:ident, $method:expr, $doc:literal) => {
        #[doc = concat!("Register an action for ", $doc, " requests to `route`.")]
        pub fn $name(&mut self, route: impl IntoRoute, action: impl Into<Action>) -> &mut Self {
            self.add_verb($method, route, action)
        }
    };
}

// ---------------------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------------------

/// A registered route as the router's matcher stores it (Remix's `RouteEntry` / `MatchData`).
pub struct RouteEntry {
    method: Option<Method>,
    handler: Arc<dyn Handler>,
    middleware: MiddlewareList,
    pattern: RoutePattern,
}

impl RouteEntry {
    /// The pattern the route matches, with any mount prefix applied.
    pub fn pattern(&self) -> &RoutePattern {
        &self.pattern
    }

    /// The method the route handles, or `None` for any method.
    pub fn method(&self) -> Option<&Method> {
        self.method.as_ref()
    }
}

impl fmt::Debug for RouteEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteEntry").field("method", &self.method).field("pattern", &self.pattern.source()).finish()
    }
}

/// A matcher a [`Router`] can use: a [`MultiMatcher`] of [`RouteEntry`]s.
pub trait RouteMatcher: MultiMatcher<Arc<RouteEntry>> + MaybeSend + MaybeSync + 'static {}

impl<M: MultiMatcher<Arc<RouteEntry>> + MaybeSend + MaybeSync + 'static> RouteMatcher for M {}

struct Inner {
    middleware: Vec<Arc<dyn Middleware>>,
    chain: MiddlewareList,
    routes: Box<dyn RouteMatcher>,
    /// `METHOD pattern` for each route in the order added, for [`Router::routes`].
    listing: Vec<String>,
    not_found: Arc<dyn Handler>,
}

/// Maps requests to handlers. Cheap to clone (an `Arc`); register routes before sharing it.
/// Clones are the same router: they compare equal, like one JavaScript router object.
#[derive(Clone)]
pub struct Router {
    inner: Arc<Inner>,
}

impl PartialEq for Router {
    fn eq(&self, other: &Router) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for Router {}

impl fmt::Debug for Router {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Router").field("routes", &self.inner.listing).finish()
    }
}

impl Default for Router {
    fn default() -> Self {
        Router::new()
    }
}

impl Router {
    pub fn new() -> Router {
        Router::with_matcher(Matcher::new())
    }

    /// A router that adds its routes to `matcher` and asks it for matches (Remix's `matcher`
    /// option). Mounted routes are added with their prefixed patterns.
    pub fn with_matcher(matcher: impl RouteMatcher) -> Router {
        // Remix's default handler: `Not Found: <pathname>`.
        let not_found: Arc<dyn Handler> =
            Arc::new(|ctx: Context| async move { response::status(StatusCode::NOT_FOUND, format!("Not Found: {}", ctx.url().pathname())) });
        Router {
            inner: Arc::new(Inner {
                middleware: Vec::new(),
                chain: Arc::from(Vec::new()),
                routes: Box::new(matcher),
                listing: Vec::new(),
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

    /// Register an action for a route, using the route's own method. An action is a
    /// handler, or [`action`]`(handler).with(middleware)` for action middleware.
    pub fn route(&mut self, route: impl IntoRoute, action: impl Into<Action>) -> &mut Self {
        self.builder().route(route, action);
        self
    }

    fn add_verb(&mut self, method: Option<Method>, route: impl IntoRoute, action: impl Into<Action>) -> &mut Self {
        self.builder().add_verb(method, route, action);
        self
    }

    verb_methods!();

    /// Map a route map to a controller (Remix's `router.map(routes, controller)`): each direct
    /// route of `routes` gets its action, behind the controller's and the action's middleware.
    /// Nested route maps are mapped with their own call. A single [`Route`] takes an [`Action`].
    pub fn map<M: RouteMap + ?Sized>(&mut self, routes: &M, controller: impl Into<Controller<M::Actions>>) -> &mut Self {
        self.builder().map(routes, controller);
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
        let ctx = Context::with_router(request, self.clone());
        Next { chain: self.inner.chain.clone(), index: 0, endpoint: Arc::new(Endpoint::Dispatch) }.run(ctx)
    }

    /// `GET` an absolute URL. Handy in tests.
    pub fn fetch_url(&self, url: &str) -> BoxFuture<Response> {
        let req = http::Request::builder().uri(url).body(Body::empty()).expect("valid request URL");
        self.fetch(req)
    }

    /// The registered routes, for debugging.
    pub fn routes(&self) -> Vec<String> {
        self.inner.listing.clone()
    }

    async fn dispatch(&self, mut ctx: Context) -> Response {
        let method = ctx.method().clone();
        let matches = match self.inner.routes.match_all(ctx.url()) {
            Ok(matches) => matches,
            Err(e) => return response::status(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };
        // Remix's dispatch: the first match (most specific first) for this method or ANY wins.
        // A HEAD request falls back to the most specific GET match, unless an equally specific
        // HEAD or ANY route follows it.
        let mut found = None;
        let mut head_fallback: Option<usize> = None;
        let mut allowed: Vec<Method> = Vec::new();
        for (i, m) in matches.iter().enumerate() {
            if let Some(fallback) = head_fallback {
                if !specificity::equal(m, &matches[fallback]) {
                    break;
                }
            }
            match &m.data.method {
                Some(rm) if *rm != method => {
                    if method == Method::HEAD && *rm == Method::GET {
                        head_fallback.get_or_insert(i);
                    } else if !allowed.contains(rm) {
                        allowed.push(rm.clone());
                    }
                }
                _ => {
                    found = Some(i);
                    break;
                }
            }
        }
        let strip_body = found.is_none() && head_fallback.is_some();
        let Some(index) = found.or(head_fallback) else {
            if allowed.is_empty() {
                return self.inner.not_found.call(ctx).await;
            }
            // GET routes also serve HEAD, so HEAD is allowed whenever GET is.
            let allow: Vec<&str> = [Method::GET, Method::HEAD, Method::POST, Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS]
                .iter()
                .filter(|m| allowed.contains(m) || (**m == Method::HEAD && allowed.contains(&Method::GET)))
                .map(Method::as_str)
                .collect();
            let mut res = response::status(StatusCode::METHOD_NOT_ALLOWED, format!("Method Not Allowed: {method}"));
            if let Ok(v) = HeaderValue::from_str(&allow.join(", ")) {
                res.headers_mut().insert(header::ALLOW, v);
            }
            return res;
        };
        let found = &matches[index];
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
    fn add_verb(&mut self, method: Option<Method>, route: impl IntoRoute, action: impl Into<Action>) -> &mut Self {
        let Action { handler, middleware } = action.into();
        let mut chain = self.middleware.clone();
        chain.extend(middleware);
        self.add_entry(route.into_route(method), handler, chain)
    }

    fn add_entry(&mut self, mut route: Route, handler: Arc<dyn Handler>, middleware: Vec<Arc<dyn Middleware>>) -> &mut Self {
        if let Some(p) = &self.prefix {
            route = route.under(p);
        }
        let entry = RouteEntry {
            method: route.method().cloned(),
            handler,
            middleware: Arc::from(middleware),
            pattern: route.pattern().clone(),
        };
        let listed = match &entry.method {
            Some(m) => format!("{m} {}", entry.pattern),
            None => format!("ANY {}", entry.pattern),
        };
        let inner = self.router.inner_mut();
        if let Err(e) = inner.routes.add(route.pattern().clone(), Arc::new(entry)) {
            panic!("route {}: {e}", route.pattern());
        }
        inner.listing.push(listed);
        self
    }

    /// Map a route map to a controller below this builder's prefix (see [`Router::map`]).
    pub fn map<M: RouteMap + ?Sized>(&mut self, routes: &M, controller: impl Into<Controller<M::Actions>>) -> &mut Self {
        let Controller { middleware, actions } = controller.into();
        for (route, action) in routes.pair_actions(actions) {
            let mut chain = self.middleware.clone();
            chain.extend(middleware.iter().cloned());
            chain.extend(action.middleware);
            self.add_entry(route, action.handler, chain);
        }
        self
    }

    /// Register an action for a route, using the route's own method.
    pub fn route(&mut self, route: impl IntoRoute, action: impl Into<Action>) -> &mut Self {
        self.add_verb(None, route, action)
    }

    verb_methods!();

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
