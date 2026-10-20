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
pub(crate) mod styles;
mod vdom;

use std::any::{Any, TypeId};
use std::rc::{Rc, Weak};

pub use entries::{entry, Entry};
pub use navigation::{navigate, NavigateOptions};
pub use vdom::flush;
pub(crate) use vdom::{new_instance, queue_task, rerender, schedule_update, AnyInstance, Live};

/// Where a component sits: its parent instance (for context), its frame, and its depth
/// (parents render before children in a batch).
#[derive(Clone, Default)]
pub(crate) struct Scope {
    pub(crate) parent: Option<Weak<dyn AnyInstance>>,
    pub(crate) frame: Option<Rc<frames::FrameState>>,
    pub(crate) depth: u32,
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
}

/// Start the browser runtime: hydrate the client entries in the document, start the
/// frames the server rendered (including streamed ones), and intercept same-origin
/// navigations so links and forms reload frames instead of the whole page.
pub fn run(options: RunOptions) {
    entries::register(options.entries);
    styles::adopt_server_styles(&document().into());
    frames::start();
    navigation::start();
    flush();
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

pub(crate) fn warn(message: &str) {
    web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(message));
}
