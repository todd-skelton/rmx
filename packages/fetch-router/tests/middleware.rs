//! Port of Remix's `fetch-router/src/lib/middleware.test.ts` (`runMiddleware`), one test per
//! case. `runMiddleware` is internal to Remix's router; here each chain runs as a route's
//! action middleware through `router.fetch`.
//!
//! Two cases are compile errors in rmx and are `compile_fail` doctests on `Next`
//! (packages/fetch-router/src/lib.rs): "rejects when a middleware neither returns a response nor calls
//! next()" (a middleware returns a `Response`) and "rejects when a middleware calls next()
//! multiple times" (`Next::run` takes `self`).

use std::sync::{Arc, Mutex};

use rmx::fetch::{block_on, Body, Response};
use rmx::response;
use rmx::router::{action, catch_panic, Action, BoxFuture, Context, Next, NextFuture, Router};

/// Run `action`'s middleware and handler for a `GET https://remix.run`.
fn run_middleware(action: Action) -> Response {
    try_run_middleware(action).expect("the chain answers")
}

fn try_run_middleware(action: Action) -> Result<Response, String> {
    let mut router = Router::new();
    router.any("/", action);
    let request = http::Request::builder().uri("https://remix.run").body(Body::empty()).unwrap();
    block_on(catch_panic(router.fetch(request))).map_err(|panic| panic.0)
}

fn text(res: Response) -> (u16, String) {
    let status = res.status().as_u16();
    (status, block_on(res.into_body().text()).unwrap())
}

fn hello_world() -> Action {
    action(|_: Context| async { "Hello, world!" })
}

type Log = Arc<Mutex<Vec<&'static str>>>;

fn record(log: &Log, name: &'static str) -> impl Fn(Context, Next) -> NextFuture + Send + Sync + 'static {
    let log = log.clone();
    move |ctx: Context, next: Next| {
        log.lock().unwrap().push(name);
        next.run(ctx)
    }
}

#[test]
fn runs_middleware_and_returns_a_response() {
    let chain = hello_world().with(|_: Context, _: Next| async { response::text("Hello, world!") });

    assert_eq!(text(run_middleware(chain)), (200, "Hello, world!".into()));
}

#[test]
fn runs_middleware_in_order_from_left_to_right() {
    let log: Log = Log::default();
    let chain = hello_world().with(record(&log, "one")).with(record(&log, "two"));

    assert_eq!(text(run_middleware(chain)), (200, "Hello, world!".into()));
    assert_eq!(*log.lock().unwrap(), ["one", "two"]);
}

#[test]
fn short_circuits_the_chain_when_a_middleware_returns_a_response() {
    let log: Log = Log::default();
    let l = log.clone();
    let chain = hello_world()
        .with(move |_: Context, _: Next| {
            l.lock().unwrap().push("one");
            async { response::text("Hello, middleware!") }
        })
        .with(record(&log, "two"));

    assert_eq!(text(run_middleware(chain)), (200, "Hello, middleware!".into()));
    assert_eq!(*log.lock().unwrap(), ["one"]);
}

#[test]
fn uses_the_downstream_response_when_middleware_calls_next() {
    // A JavaScript middleware may await `next()` and return nothing; a Rust middleware returns
    // the downstream response it awaited.
    let log: Log = Log::default();
    let (l1, l2) = (log.clone(), log.clone());
    let chain = hello_world()
        .with(move |ctx: Context, next: Next| {
            let l1 = l1.clone();
            async move {
                l1.lock().unwrap().push("one");
                next.run(ctx).await
            }
        })
        .with(move |ctx: Context, next: Next| {
            let l2 = l2.clone();
            async move {
                l2.lock().unwrap().push("two");
                next.run(ctx).await
            }
        });

    assert_eq!(text(run_middleware(chain)), (200, "Hello, world!".into()));
    assert_eq!(*log.lock().unwrap(), ["one", "two"]);
}

#[test]
fn rejects_when_a_handler_throws_an_error() {
    let chain = action(|_: Context| -> std::future::Ready<&'static str> { panic!("Handler error!") }).with(|ctx: Context, next: Next| next.run(ctx));

    assert_eq!(try_run_middleware(chain).unwrap_err(), "Handler error!");
}

#[test]
fn rejects_when_a_middleware_throws_an_error() {
    let chain = hello_world().with(|_: Context, _: Next| -> BoxFuture<Response> { panic!("Middleware error!") });

    assert_eq!(try_run_middleware(chain).unwrap_err(), "Middleware error!");
}
