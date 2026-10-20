//! Browser runtime: client entries, frames, soft navigation and the component tree.
//!
//! - [`vdom`] mounts, hydrates and patches component trees and schedules updates.
//! - [`entries`] finds server-rendered client entries in a region and hydrates them.
//! - [`frames`] tracks the top frame and every `<Frame>`, reloads them from the server and
//!   applies streamed `<template>` content.
//! - [`diff`] reconciles server HTML into the live DOM, keeping hydrated entries and frames.
//! - [`navigation`] routes link clicks and form submissions through frame reloads using the
//!   Navigation API.
//! - [`styles`] inserts `css()` rules once per document.

pub(crate) mod diff;
pub(crate) mod entries;
pub(crate) mod frames;
pub(crate) mod navigation;
mod mixin_host;
mod root;
pub(crate) mod styles;
pub mod test;
mod vdom;

use std::any::{Any, TypeId};
use std::rc::{Rc, Weak};

pub use entries::{entry, Entry};
pub use root::{create_range_root, create_root, Root};
pub use vdom::flush;
pub(crate) use mixin_host::MixinHost;
pub(crate) use vdom::{after_flush, listener_signal};
pub(crate) use vdom::{new_instance, queue_task, rerender, schedule_update, AnyInstance, Live};

/// Where a component sits: its parent instance (for context), its frame, and its depth
/// (parents render before children in a batch).
#[derive(Clone, Default)]
pub(crate) struct Scope {
    pub(crate) parent: Option<Weak<dyn AnyInstance>>,
    pub(crate) frame: Option<Rc<frames::FrameState>>,
    pub(crate) depth: u32,
    /// Error listeners of the standalone root this component renders in (`None` = the app).
    pub(crate) errors: Option<crate::TypedEventTarget<crate::AppError>>,
}

/// The nearest value of type `want` provided by `parent` or one of its ancestors.
pub(crate) fn find_context(mut parent: Option<Weak<dyn AnyInstance>>, want: TypeId) -> Option<Rc<dyn Any>> {
    while let Some(inst) = parent.and_then(|w| w.upgrade()) {
        let shared = inst.shared();
        if let Some(v) = shared.contexts.borrow().iter().find(|(t, _)| *t == want).map(|(_, v)| v.clone()) {
            return Some(v);
        }
        parent = shared.scope.parent.clone();
    }
    None
}

/// Options for [`run`].
#[derive(Default)]
pub struct RunOptions {
    /// Client entries the page may contain, created with [`entry`].
    pub entries: Vec<Entry>,
    /// Resolve frame content in the browser instead of fetching it from the server (Remix's
    /// `resolveFrame`). `rmx-spa` uses it to run a router in the page.
    pub resolve_frame: Option<crate::frame::ResolveFrame>,
}

/// Start the browser runtime: hydrate the client entries in the document, start the
/// frames the server rendered (including streamed ones), and intercept same-origin
/// navigations so links and forms reload frames instead of the whole page.
///
/// The page can load its script with `async`: on a streamed response the runtime starts
/// as soon as the main content is parsed (the server's flush marker), so frames that
/// stream in later appear as they arrive instead of when the response ends. When it is
/// running, the document receives an `rmx:ready` event.
///
/// Returns the [`App`] (Remix's `AppRuntime`): listen for errors, wait until the page is
/// hydrated, or dispose the runtime.
pub fn run(options: RunOptions) -> App {
    install_panic_hook();
    entries::register(options.entries);
    frames::set_resolver(options.resolve_frame);
    let mut resolve_ready = None;
    let ready = js_sys::Promise::new(&mut |resolve, _| resolve_ready = Some(resolve));
    when_content_parsed(Box::new(move || {
        styles::adopt_server_styles(&document().into());
        frames::start();
        navigation::start();
        flush();
        if let Some(resolve) = resolve_ready {
            let _ = resolve.call0(&wasm_bindgen::JsValue::UNDEFINED);
        }
        if let Ok(event) = web_sys::Event::new("rmx:ready") {
            let _ = document().dispatch_event(&event);
        }
    }));
    App { ready }
}

/// The running browser app returned by [`run`] (Remix's `AppRuntime`).
///
/// ```ignore
/// let app = rmx::run(RunOptions { entries, ..Default::default() });
/// app.on_error(move |error| show_error_page(&error.message));
/// ```
#[derive(Clone)]
pub struct App {
    ready: js_sys::Promise,
}

impl App {
    /// Call `f` for each error the runtime reports: a frame load or reload that failed, a
    /// navigation whose content could not be loaded, a panic, or [`crate::report_error`].
    ///
    /// Without listeners a failed navigation falls back to loading the document; with
    /// one, the app decides what to show.
    pub fn on_error(&self, f: impl Fn(&crate::AppError) + 'static) -> crate::ListenerId {
        crate::app::add_listener(f)
    }

    pub fn remove_error_listener(&self, id: crate::ListenerId) {
        crate::app::remove_listener(id);
    }

    /// Resolves once the document is hydrated and its frames have started.
    pub async fn ready(&self) {
        let _ = wasm_bindgen_futures::JsFuture::from(self.ready.clone()).await;
    }

    /// The page's frames (`app.frames`).
    pub fn frames(&self) -> crate::Frames {
        crate::Frames
    }

    /// Apply pending component updates now.
    pub fn flush(&self) {
        flush();
    }

    /// Stop intercepting navigations and tear down every client entry and frame in the
    /// document, leaving its DOM in place for the app to replace.
    pub fn dispose(&self) {
        navigation::stop();
        if let Some(body) = document().body() {
            diff::dispose_subtree(&body.into());
        }
        frames::top().abort_reload();
    }
}

/// Log panics with `console.error` and report them to the app before the module aborts.
/// Any hook installed earlier still runs first.
fn install_panic_hook() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            previous(info);
            let error = crate::AppError { message: info.to_string() };
            web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&error.message));
            crate::app::dispatch(&error);
        }));
    });
}

type Task = Box<dyn FnOnce()>;

/// Run `f` once the document's main content is parsed: right away when parsing is done,
/// otherwise at the streamed-response flush marker or `DOMContentLoaded`, whichever is first.
fn when_content_parsed(f: Task) {
    use std::cell::RefCell;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    let doc = document();
    if doc.ready_state() != "loading" || has_flush_marker() {
        return f();
    }
    let slot: Rc<RefCell<Option<Task>>> = Rc::new(RefCell::new(Some(f)));
    let observer_slot: Rc<RefCell<Option<web_sys::MutationObserver>>> = Rc::default();
    let fire = {
        let slot = slot.clone();
        let observer_slot = observer_slot.clone();
        move || {
            if let Some(o) = observer_slot.borrow_mut().take() {
                o.disconnect();
            }
            let f = slot.borrow_mut().take();
            if let Some(f) = f {
                f();
            }
        }
    };
    let on_mutation = {
        let fire = fire.clone();
        Closure::<dyn FnMut()>::new(move || {
            if has_flush_marker() {
                fire();
            }
        })
        .into_js_value()
    };
    if let Ok(observer) = web_sys::MutationObserver::new(on_mutation.unchecked_ref()) {
        let init = web_sys::MutationObserverInit::new();
        init.set_child_list(true);
        init.set_subtree(true);
        if observer.observe_with_options(&doc, &init).is_ok() {
            *observer_slot.borrow_mut() = Some(observer);
        }
    }
    let on_loaded = Closure::once_into_js(fire);
    let _ = doc.add_event_listener_with_callback("DOMContentLoaded", on_loaded.unchecked_ref());
}

/// Whether the body ends with the server's `<!-- rmx:flush ... -->` marker (possibly
/// followed by streamed templates).
fn has_flush_marker() -> bool {
    use wasm_bindgen::JsCast;
    let Some(body) = document().body() else { return false };
    let mut node = body.last_child();
    while let Some(n) = node {
        if let Some(c) = n.dyn_ref::<web_sys::Comment>() {
            if c.data().trim_start().starts_with("rmx:flush") {
                return true;
            }
        } else if n.dyn_ref::<web_sys::Element>().is_some_and(|e| e.tag_name() != "TEMPLATE") {
            return false;
        }
        node = n.previous_sibling();
    }
    false
}

thread_local! {
    static DOCUMENT: web_sys::Document =
        web_sys::window().and_then(|w| w.document()).expect("rmx needs a browser document");
}

pub(crate) fn document() -> web_sys::Document {
    DOCUMENT.with(Clone::clone)
}

pub(crate) fn window() -> web_sys::Window {
    web_sys::window().expect("window")
}

/// Small `Reflect` helpers for browser APIs that web-sys gates or types awkwardly.
pub(crate) mod js {
    use js_sys::{Object, Reflect};
    use wasm_bindgen::{JsCast, JsValue};

    pub(crate) fn get(target: &JsValue, key: &str) -> JsValue {
        Reflect::get(target, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
    }

    pub(crate) fn call(target: &JsValue, method: &str, args: &[&JsValue]) -> Result<JsValue, JsValue> {
        let f: js_sys::Function = get(target, method).dyn_into()?;
        let array = js_sys::Array::new();
        for a in args {
            array.push(a);
        }
        f.apply(target, &array)
    }

    pub(crate) fn object(entries: &[(&str, JsValue)]) -> JsValue {
        let o = Object::new();
        for (k, v) in entries {
            let _ = Reflect::set(&o, &JsValue::from_str(k), v);
        }
        o.into()
    }
}

pub(crate) fn warn(message: &str) {
    web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(message));
}
