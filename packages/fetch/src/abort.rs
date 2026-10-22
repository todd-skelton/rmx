//! Request cancellation: the web's `AbortController` and `AbortSignal`, without a runtime.
//!
//! A server attaches a signal to each request ([`RequestExt::set_signal`]) and aborts it
//! when the client goes away before the response is finished. Handlers read it with
//! [`RequestExt::signal`] (or `Context::signal` in the router) and stop long work with
//! [`AbortSignal::until_aborted`]. Rust futures are also cancelled by dropping them; the
//! signal is for work that outlives the request future, such as spawned tasks.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use http::{HeaderValue, StatusCode};

use crate::{Body, Request, Response};

/// Aborts its [`AbortSignal`].
#[derive(Debug, Default)]
pub struct AbortController {
    signal: AbortSignal,
}

impl AbortController {
    pub fn new() -> AbortController {
        AbortController { signal: AbortSignal { shared: Some(Arc::new(Shared::default())) } }
    }

    /// The signal this controller aborts.
    pub fn signal(&self) -> AbortSignal {
        self.signal.clone()
    }

    /// Abort the signal and wake everything waiting on it. Later calls do nothing.
    pub fn abort(&self) {
        let Some(shared) = &self.signal.shared else { return };
        if shared.aborted.swap(true, Ordering::SeqCst) {
            return;
        }
        let wakers = std::mem::take(&mut shared.wakers.lock().expect("abort lock").wakers);
        for waker in wakers.into_values() {
            waker.wake();
        }
    }
}

/// Whether a request was aborted. Cheap to clone; clones share the state.
/// The default signal never aborts.
#[derive(Clone, Default)]
pub struct AbortSignal {
    shared: Option<Arc<Shared>>,
}

/// The flag is read without locking (the router checks it at every middleware step); the
/// lock guards the wakers, and pollers re-check the flag while holding it.
#[derive(Default)]
struct Shared {
    aborted: AtomicBool,
    wakers: Mutex<Wakers>,
}

#[derive(Default)]
struct Wakers {
    next_id: u64,
    wakers: HashMap<u64, Waker>,
}

impl AbortSignal {
    /// A signal that never aborts.
    pub fn never() -> AbortSignal {
        AbortSignal { shared: None }
    }

    /// Whether this is a signal that can never abort ([`AbortSignal::never`] or the default),
    /// so code can skip watching it.
    pub fn is_never(&self) -> bool {
        self.shared.is_none()
    }

    /// Whether the signal has been aborted.
    pub fn aborted(&self) -> bool {
        self.shared.as_ref().is_some_and(|s| s.aborted.load(Ordering::SeqCst))
    }

    /// Fails with [`AbortError`] if the signal has been aborted (`signal.throwIfAborted()`).
    pub fn check(&self) -> Result<(), AbortError> {
        if self.aborted() {
            Err(AbortError)
        } else {
            Ok(())
        }
    }

    /// Resolves when the signal aborts (never, for [`AbortSignal::never`]).
    pub fn until_aborted(&self) -> UntilAborted {
        UntilAborted { shared: self.shared.clone(), id: None }
    }

    /// Run `fut` unless the signal aborts first (Remix's `raceRequestAbort`). Fails at once
    /// if the signal is already aborted; otherwise `fut` is polled before the signal, so a
    /// result that is ready when the abort lands still wins.
    pub async fn race<F: Future>(&self, fut: F) -> Result<F::Output, AbortError> {
        self.check()?;
        let mut fut = std::pin::pin!(fut);
        let mut aborted = self.until_aborted();
        std::future::poll_fn(|cx| {
            if let Poll::Ready(v) = fut.as_mut().poll(cx) {
                return Poll::Ready(Ok(v));
            }
            Pin::new(&mut aborted).poll(cx).map(|()| Err(AbortError))
        })
        .await
    }
}

impl fmt::Debug for AbortSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AbortSignal").field("aborted", &self.aborted()).finish()
    }
}

/// The future returned by [`AbortSignal::until_aborted`].
pub struct UntilAborted {
    shared: Option<Arc<Shared>>,
    id: Option<u64>,
}

impl Future for UntilAborted {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = &mut *self;
        let Some(shared) = &this.shared else { return Poll::Pending };
        let mut st = shared.wakers.lock().expect("abort lock");
        if shared.aborted.load(Ordering::SeqCst) {
            return Poll::Ready(());
        }
        let id = *this.id.get_or_insert_with(|| {
            st.next_id += 1;
            st.next_id
        });
        st.wakers.insert(id, cx.waker().clone());
        Poll::Pending
    }
}

impl Drop for UntilAborted {
    fn drop(&mut self) {
        if let (Some(shared), Some(id)) = (&self.shared, self.id) {
            if let Ok(mut st) = shared.wakers.lock() {
                st.wakers.remove(&id);
            }
        }
    }
}

/// The request was aborted (the web's `DOMException` named `AbortError`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AbortError;

impl AbortError {
    /// The status of [`AbortError::into_response`]: 499, nginx's "client closed request".
    pub const STATUS: u16 = 499;

    /// A 499 response that carries this error as an extension, so middleware and callers can
    /// tell an aborted request from a real answer ([`AbortError::of`]).
    pub fn into_response(self) -> Response {
        let mut res = Response::new(Body::from(self.to_string()));
        *res.status_mut() = StatusCode::from_u16(Self::STATUS).expect("valid status");
        res.headers_mut().insert(http::header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
        res.extensions_mut().insert(self);
        res
    }

    /// The abort a response stands for, if it is one made by [`AbortError::into_response`].
    pub fn of(response: &Response) -> Option<AbortError> {
        response.extensions().get::<AbortError>().copied()
    }
}

impl fmt::Display for AbortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("The request was aborted.")
    }
}

impl std::error::Error for AbortError {}

/// The request's abort signal (`request.signal`).
pub trait RequestExt {
    /// The signal attached with [`RequestExt::set_signal`], or one that never aborts.
    fn signal(&self) -> AbortSignal;
    /// Attach `signal` to the request.
    fn set_signal(&mut self, signal: AbortSignal);
}

impl RequestExt for Request {
    fn signal(&self) -> AbortSignal {
        self.extensions().get::<AbortSignal>().cloned().unwrap_or_default()
    }

    fn set_signal(&mut self, signal: AbortSignal) {
        self.extensions_mut().insert(signal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_on;

    // Ports of fetch-router's request-abort.test.ts (`raceRequestAbort`).

    #[test]
    fn resolves_when_the_future_finishes_before_an_abort() {
        let controller = AbortController::new();
        assert_eq!(block_on(controller.signal().race(async { "success" })), Ok("success"));
    }

    #[test]
    fn passes_the_futures_own_error_through() {
        let controller = AbortController::new();
        let out = block_on(controller.signal().race(async { Err::<(), _>("failed") }));
        assert_eq!(out, Ok(Err("failed")));
    }

    #[test]
    fn fails_with_abort_error_when_already_aborted() {
        let controller = AbortController::new();
        controller.abort();
        assert_eq!(block_on(controller.signal().race(async { "success" })), Err(AbortError));
        assert_eq!(AbortError.to_string(), "The request was aborted.");
    }

    #[test]
    fn fails_with_abort_error_when_aborted_while_the_future_runs() {
        let controller = AbortController::new();
        let signal = controller.signal();
        let aborter = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            controller.abort();
        });
        assert_eq!(block_on(signal.race(std::future::pending::<()>())), Err(AbortError));
        aborter.join().unwrap();
    }

    #[test]
    fn never_aborts_by_default_and_signals_are_shared_between_clones() {
        let mut req = http::Request::new(Body::empty());
        assert!(!req.signal().aborted());
        let controller = AbortController::new();
        req.set_signal(controller.signal());
        let copy = req.signal();
        controller.abort();
        controller.abort();
        assert!(req.signal().aborted() && copy.aborted());
        assert_eq!(copy.check(), Err(AbortError));
        assert!(!AbortSignal::never().aborted());
    }

    #[test]
    fn abort_responses_are_recognisable() {
        let res = AbortError.into_response();
        assert_eq!(res.status().as_u16(), 499);
        assert_eq!(AbortError::of(&res), Some(AbortError));
        assert_eq!(AbortError::of(&Response::new(Body::empty())), None);
    }
}
