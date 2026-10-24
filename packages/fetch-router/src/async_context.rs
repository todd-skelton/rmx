//! Request-scoped context for code deep in the call stack (Remix's
//! `remix/middleware/async-context`, built on `AsyncLocalStorage`).
//!
//! ```
//! use rmx_fetch_router::{async_context::{async_context, get_context}, Context, Router};
//!
//! fn current_user_id() -> String {
//!     get_context().expect("async_context() is installed").param("id").unwrap_or_default()
//! }
//!
//! let mut router = Router::new().with(async_context());
//! router.get("/users/:id", |_: Context| async { format!("user {}", current_user_id()) });
//!
//! let res = rmx_fetch::block_on(router.fetch_url("https://example.com/users/7"));
//! assert_eq!(rmx_fetch::block_on(res.into_body().text()).unwrap(), "user 7");
//! ```
//!
//! The middleware runs the rest of the request inside a scope that [`get_context`] reads
//! from any function the request's future calls, on any thread. The scope follows the
//! request's context: params set when a route matches, values stored with
//! [`Context::set`], and changes made with [`Context::set_url`] and
//! [`Context::set_method`] are all visible. Tasks spawned onto the runtime do not inherit it.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};

use http::{Extensions, HeaderMap, Method};
use rmx_route_pattern::Params;
use rmx_url::Url;

use crate::{Context, Middleware, Next, NextFuture};
use rmx_fetch::Response;

/// What the scope holds; kept in step with the request's [`Context`].
pub(crate) struct Shared {
    pub(crate) method: Method,
    pub(crate) url: Url,
    pub(crate) headers: HeaderMap,
    pub(crate) params: Params,
    pub(crate) values: Extensions,
}

pub(crate) type Scope = Arc<Mutex<Shared>>;

thread_local! {
    static CURRENT: RefCell<Option<Scope>> = const { RefCell::new(None) };
}

/// Store the request context so [`get_context`] can read it. Install it on the router
/// (`Router::new().with(async_context())`).
pub fn async_context() -> impl Middleware {
    |mut ctx: Context, next: Next| {
        let scope: Scope = Arc::new(Mutex::new(Shared {
            method: ctx.method().clone(),
            url: ctx.url().clone(),
            headers: ctx.headers().clone(),
            params: ctx.params().clone(),
            values: ctx.request().extensions().clone(),
        }));
        ctx.scope = Some(scope.clone());
        Scoped { scope: Some(scope), inner: next.run(ctx) }
    }
}

/// The context of the request being handled, or `None` outside a request or without the
/// [`async_context`] middleware (Remix's `getContext()`, which throws there).
pub fn get_context() -> Option<AsyncContext> {
    CURRENT.with(|c| c.borrow().clone()).map(AsyncContext)
}

/// A read-only view of the current request's context.
#[derive(Clone)]
pub struct AsyncContext(Scope);

impl AsyncContext {
    fn read<T>(&self, f: impl FnOnce(&Shared) -> T) -> T {
        f(&self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn method(&self) -> Method {
        self.read(|s| s.method.clone())
    }

    /// The absolute request URL.
    pub fn url(&self) -> Url {
        self.read(|s| s.url.clone())
    }

    pub fn headers(&self) -> HeaderMap {
        self.read(|s| s.headers.clone())
    }

    pub fn header(&self, name: &str) -> Option<String> {
        self.read(|s| s.headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string))
    }

    /// Params of the matched route (empty before a route matches).
    pub fn params(&self) -> Params {
        self.read(|s| s.params.clone())
    }

    pub fn param(&self, name: &str) -> Option<String> {
        self.read(|s| s.params.get(name).map(str::to_string))
    }

    /// A value stored with [`Context::set`].
    pub fn get<T: Clone + Send + Sync + 'static>(&self) -> Option<T> {
        self.read(|s| s.values.get::<T>().cloned())
    }
}

/// Update the scope a context belongs to.
pub(crate) fn update(scope: &Option<Scope>, f: impl FnOnce(&mut Shared)) {
    if let Some(scope) = scope {
        f(&mut scope.lock().unwrap_or_else(|e| e.into_inner()));
    }
}

/// Makes `scope` current while `inner` is polled, so it travels with the request across
/// threads and awaits (how `tokio::task_local!` works, without depending on tokio).
struct Scoped {
    scope: Option<Scope>,
    inner: NextFuture,
}

impl Future for Scoped {
    type Output = Response;

    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Response> {
        struct Restore(Option<Scope>);
        impl Drop for Restore {
            fn drop(&mut self) {
                CURRENT.with(|c| *c.borrow_mut() = self.0.take());
            }
        }
        let this = &mut *self;
        let _restore = Restore(CURRENT.with(|c| c.replace(this.scope.clone())));
        Pin::new(&mut this.inner).poll(cx)
    }
}
