//! Errors the browser runtime reports to the app (Remix's `ComponentErrorEvent` on the
//! runtime returned by `run()`).
//!
//! A frame that fails to load or reload, a soft navigation whose content cannot be loaded,
//! and a panic are reported to every listener added with
//! [`App::on_error`](crate::App::on_error) and logged with `console.error`. Rust code
//! does not throw, so a component reports its own failures with [`report_error`] or
//! [`Handle::report_error`](crate::Handle::report_error). Components in a standalone root
//! ([`create_root`](crate::create_root)) report to that root's
//! [`Root::on_error`](crate::Root::on_error) listeners instead.

use std::fmt;

use crate::signal::{ListenerId, TypedEventTarget};

/// An error reported to the app's `error` listeners.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppError {
    pub message: String,
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

thread_local! {
    static ERRORS: TypedEventTarget<AppError> = TypedEventTarget::new();
}

/// Report an error to the app: log it with `console.error` (stderr on the server) and
/// dispatch it to the [`App::on_error`](crate::App::on_error) listeners.
pub fn report_error(error: impl fmt::Display) {
    let error = AppError { message: error.to_string() };
    #[cfg(target_arch = "wasm32")]
    web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&error.message));
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{}", error.message);
    dispatch(&error);
}

/// Report to a root's error listeners, or to the app's when `target` is `None`.
#[cfg(target_arch = "wasm32")]
pub(crate) fn report_to(target: Option<&TypedEventTarget<AppError>>, error: impl fmt::Display) {
    let Some(target) = target else { return report_error(error) };
    let error = AppError { message: error.to_string() };
    web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&error.message));
    target.dispatch(&error);
}

pub(crate) fn dispatch(error: &AppError) {
    let target = ERRORS.with(Clone::clone);
    target.dispatch(error);
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn add_listener(f: impl Fn(&AppError) + 'static) -> ListenerId {
    ERRORS.with(|t| t.add_listener(f))
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn remove_listener(id: ListenerId) {
    ERRORS.with(|t| t.remove_listener(id));
}

#[cfg_attr(not(all(target_arch = "wasm32", feature = "navigation")), allow(dead_code))]
pub(crate) fn has_listeners() -> bool {
    ERRORS.with(|t| t.listener_count() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn reported_errors_reach_listeners() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        let id = add_listener(move |e| s.borrow_mut().push(e.to_string()));
        assert!(has_listeners());
        report_error("Failed to resolve frame: 500 Internal Server Error");
        remove_listener(id);
        report_error("not seen");
        assert_eq!(*seen.borrow(), vec!["Failed to resolve frame: 500 Internal Server Error".to_string()]);
    }
}
