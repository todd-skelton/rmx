//! Ports of fetch-router's router-abort.test.ts. Remix's `router.fetch` rejects with an
//! `AbortError`; rmx answers with `AbortError::into_response` (status 499, the error in the
//! response extensions), which `AbortError::of` recognises.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;

use rmx::fetch::{block_on, response, AbortController, AbortError, AbortSignal, Body, Request, RequestExt, Response};
use rmx::router::{Context, Next, Router};

/// A timer future backed by a thread, so the tests need no runtime.
struct Sleep {
    done: Arc<Mutex<(bool, Option<std::task::Waker>)>>,
}

fn sleep(ms: u64) -> Sleep {
    let done = Arc::new(Mutex::new((false, None::<std::task::Waker>)));
    let shared = done.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        let mut st = shared.lock().unwrap();
        st.0 = true;
        if let Some(w) = st.1.take() {
            w.wake();
        }
    });
    Sleep { done }
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<()> {
        let mut st = self.done.lock().unwrap();
        if st.0 {
            Poll::Ready(())
        } else {
            st.1 = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

fn request(signal: &AbortSignal) -> Request {
    let mut req = http::Request::builder().uri("https://remix.run/").body(Body::empty()).unwrap();
    req.set_signal(signal.clone());
    req
}

fn home_router() -> Router {
    let mut router = Router::new();
    router.get("/", |_: Context| async { response::text("Home") });
    router
}

fn assert_aborted(res: &Response) {
    assert_eq!(AbortError::of(res), Some(AbortError), "expected an aborted response, got {}", res.status());
    assert_eq!(res.status().as_u16(), 499);
}

#[test]
fn answers_abort_error_when_the_signal_is_already_aborted() {
    let controller = AbortController::new();
    controller.abort();
    assert_aborted(&block_on(home_router().fetch(request(&controller.signal()))));
}

#[test]
fn answers_abort_error_when_the_signal_aborts_while_the_handler_runs() {
    let controller = Arc::new(AbortController::new());
    let mut router = Router::new();
    let c = controller.clone();
    router.get("/", move |_: Context| {
        let c = c.clone();
        async move {
            c.abort();
            sleep(10).await;
            response::text("Home")
        }
    });
    assert_aborted(&block_on(router.fetch(request(&controller.signal()))));
}

#[test]
fn handlers_see_the_request_signal() {
    let controller = AbortController::new();
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move { response::text(format!("aborted: {}", ctx.signal().aborted())) });
    let res = block_on(router.fetch(request(&controller.signal())));
    assert_eq!(block_on(res.into_body().text()).unwrap(), "aborted: false");
}

#[test]
fn middleware_sees_the_abort_from_downstream() {
    let controller = Arc::new(AbortController::new());
    let caught = Arc::new(AtomicBool::new(false));
    let seen = caught.clone();
    let router = Router::new().with(move |ctx: Context, next: Next| {
        let seen = seen.clone();
        async move {
            let res = next.run(ctx).await;
            if AbortError::of(&res).is_some() {
                seen.store(true, Ordering::SeqCst);
            }
            res
        }
    });
    let mut router = router;
    router.get("/", |_: Context| async {
        sleep(50).await;
        response::text("Home")
    });
    let c = controller.clone();
    let aborter = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        c.abort();
    });
    assert_aborted(&block_on(router.fetch(request(&controller.signal()))));
    aborter.join().unwrap();
    assert!(caught.load(Ordering::SeqCst), "middleware should have seen the abort");
}

#[test]
fn completes_normally_when_not_aborted() {
    let controller = AbortController::new();
    let res = block_on(home_router().fetch(request(&controller.signal())));
    assert_eq!(res.status(), 200);
    assert_eq!(AbortError::of(&res), None);
    assert_eq!(block_on(res.into_body().text()).unwrap(), "Home");
}

#[test]
fn answers_abort_error_when_aborted_before_middleware_completes() {
    let controller = Arc::new(AbortController::new());
    let c = controller.clone();
    let mut router = Router::new().with(move |ctx: Context, next: Next| {
        let c = c.clone();
        async move {
            c.abort();
            sleep(10).await;
            next.run(ctx).await
        }
    });
    router.get("/", |_: Context| async { response::text("Home") });
    assert_aborted(&block_on(router.fetch(request(&controller.signal()))));
}

#[test]
fn skips_downstream_middleware_and_the_handler_when_aborted_upstream() {
    let controller = Arc::new(AbortController::new());
    let calls = Arc::new(Mutex::new(Vec::<&str>::new()));
    let c = controller.clone();
    let (downstream, handler) = (calls.clone(), calls.clone());
    let mut router = Router::new()
        .with(move |ctx: Context, next: Next| {
            c.abort();
            next.run(ctx)
        })
        .with(move |ctx: Context, next: Next| {
            downstream.lock().unwrap().push("downstream middleware");
            next.run(ctx)
        });
    router.get("/", move |_: Context| {
        handler.lock().unwrap().push("handler");
        async { response::text("Home") }
    });
    assert_aborted(&block_on(router.fetch(request(&controller.signal()))));
    assert!(calls.lock().unwrap().is_empty(), "ran: {:?}", calls.lock().unwrap());
}

#[test]
fn requests_without_a_signal_never_abort() {
    let res = block_on(home_router().fetch_url("https://remix.run/"));
    assert_eq!(res.status(), 200);
}
