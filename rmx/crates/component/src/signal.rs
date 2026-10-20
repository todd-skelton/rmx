//! Cancellation and typed events that compile on both targets.
//!
//! - [`Signal`] wraps the browser's `AbortSignal`: event handlers get one that aborts when the
//!   same handler fires again ([`Event::signal`](crate::Event::signal)), refs get one that
//!   aborts when the element is removed.
//! - [`TypedEventTarget`] is Remix's `TypedEventTarget` as plain Rust: share one through
//!   context and subscribe with [`Handle::listen`](crate::Handle::listen) so a component
//!   updates when it changes, without its parent re-rendering.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// An `AbortSignal`. On the server it is never aborted.
#[derive(Clone)]
pub struct Signal {
    #[cfg(target_arch = "wasm32")]
    pub(crate) inner: web_sys::AbortSignal,
}

impl Signal {
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn from_raw(inner: web_sys::AbortSignal) -> Signal {
        Signal { inner }
    }

    /// A signal that never aborts.
    pub fn never() -> Signal {
        #[cfg(target_arch = "wasm32")]
        {
            Signal { inner: web_sys::AbortController::new().expect("AbortController").signal() }
        }
        #[cfg(not(target_arch = "wasm32"))]
        Signal {}
    }

    pub fn aborted(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.aborted()
        }
        #[cfg(not(target_arch = "wasm32"))]
        false
    }

    /// Run `f` once when the signal aborts (at once if it already has).
    pub fn on_abort(&self, f: impl FnOnce() + 'static) {
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            if self.inner.aborted() {
                return f();
            }
            let callback = wasm_bindgen::closure::Closure::once_into_js(f);
            let options = web_sys::AddEventListenerOptions::new();
            options.set_once(true);
            let _ = self.inner.add_event_listener_with_callback_and_add_event_listener_options(
                "abort",
                callback.unchecked_ref(),
                &options,
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        drop(f);
    }

    /// The browser `AbortSignal`, to pass to `fetch` or `addEventListener`.
    #[cfg(target_arch = "wasm32")]
    pub fn raw(&self) -> &web_sys::AbortSignal {
        &self.inner
    }
}

/// Wrap a browser `AbortSignal`, for example [`Handle::signal`](crate::Handle::signal).
#[cfg(target_arch = "wasm32")]
impl From<web_sys::AbortSignal> for Signal {
    fn from(inner: web_sys::AbortSignal) -> Signal {
        Signal { inner }
    }
}

type Listener<E> = Rc<dyn Fn(&E)>;

/// A typed event target (Remix's `TypedEventTarget`). Clones share one listener list, so
/// it is cheap to hand out through context.
///
/// ```ignore
/// struct Theme { value: Cell<&'static str>, changes: TypedEventTarget<()> }
/// // provider:  theme.value.set("dark"); theme.changes.dispatch(&());
/// // consumer:  h.listen(&theme.changes, |_, h, _| h.update());
/// ```
pub struct TypedEventTarget<E>(Rc<Inner<E>>);

struct Inner<E> {
    listeners: RefCell<Vec<(u64, Listener<E>)>>,
    next: Cell<u64>,
}

impl<E> Clone for TypedEventTarget<E> {
    fn clone(&self) -> Self {
        TypedEventTarget(self.0.clone())
    }
}

impl<E> Default for TypedEventTarget<E> {
    fn default() -> Self {
        TypedEventTarget(Rc::new(Inner { listeners: RefCell::new(Vec::new()), next: Cell::new(0) }))
    }
}

/// Identifies a listener for [`TypedEventTarget::remove_listener`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ListenerId(u64);

impl<E: 'static> TypedEventTarget<E> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_listener(&self, f: impl Fn(&E) + 'static) -> ListenerId {
        let id = self.0.next.get();
        self.0.next.set(id + 1);
        self.0.listeners.borrow_mut().push((id, Rc::new(f)));
        ListenerId(id)
    }

    pub fn remove_listener(&self, id: ListenerId) {
        self.0.listeners.borrow_mut().retain(|(i, _)| *i != id.0);
    }

    /// Call every listener. Listeners added or removed while dispatching take effect on
    /// the next dispatch.
    pub fn dispatch(&self, event: &E) {
        let listeners: Vec<Listener<E>> = self.0.listeners.borrow().iter().map(|(_, f)| f.clone()).collect();
        for f in listeners {
            f(event);
        }
    }

    pub fn listener_count(&self) -> usize {
        self.0.listeners.borrow().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_event_targets_dispatch_to_listeners() {
        let target = TypedEventTarget::<u32>::new();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        let id = target.add_listener(move |n| s.borrow_mut().push(*n));
        target.dispatch(&1);
        target.remove_listener(id);
        target.dispatch(&2);
        assert_eq!(*seen.borrow(), vec![1]);
        assert_eq!(target.listener_count(), 0);
        assert!(!Signal::never().aborted());
    }
}
