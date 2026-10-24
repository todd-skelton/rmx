//! The browser runtime without the `frames` feature: the document is the only frame.
//!
//! It reads the page's `rmx-data` and hydrates the client entries in the document. Frame
//! reloads load the document (`FrameHandle::reload` reloads or assigns the page, a POST
//! submits a form), `FrameHandle::replace` renders into the body, frame events never fire,
//! and there are no named frames. Streamed `<template>` content is not applied.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use wasm_bindgen::{JsCast, JsValue};

use super::root::Root;
use super::{document, entries, warn, window};
use crate::frame::ReloadOptions;
use crate::node::Node;

/// The top frame (the document).
pub(crate) struct FrameState {
    root: RefCell<Option<Root>>,
}

impl FrameState {
    pub(crate) fn src(&self) -> String {
        window().location().href().unwrap_or_default()
    }

    pub(crate) fn name(&self) -> Option<String> {
        None
    }

    pub(crate) fn owner(&self) -> Option<std::rc::Weak<dyn super::AnyInstance>> {
        None
    }

    /// Frame events need the `frames` feature; the listener is dropped.
    pub(crate) fn add_listener(&self, _event: &str, _handler: crate::node::EventHandler, _signal: &web_sys::AbortSignal) {}

    pub(crate) fn abort_reload(&self) {}

    /// Load the document: GET goes to `src` (or reloads), POST submits a form with the
    /// fields. Resolves to a signal that never aborts; the page unloads soon after.
    pub(crate) async fn reload(self: Rc<Self>, options: ReloadOptions) -> Result<web_sys::AbortSignal, JsValue> {
        let location = window().location();
        let post = options.method.as_deref().is_some_and(|m| m.eq_ignore_ascii_case("post"));
        let explicit_src = options.src.is_some();
        let src = options.src.unwrap_or_else(|| location.href().unwrap_or_default());
        if post {
            let doc = document();
            let form: web_sys::HtmlFormElement = doc.create_element("form")?.unchecked_into();
            form.set_method("post");
            form.set_action(&src);
            if let Some(enc_type) = &options.enc_type {
                form.set_enctype(enc_type);
            }
            for (name, value) in options.body.iter().flatten() {
                let input = doc.create_element("input")?;
                input.set_attribute("type", "hidden")?;
                input.set_attribute("name", name)?;
                input.set_attribute("value", value)?;
                form.append_child(&input)?;
            }
            doc.body().ok_or("no body")?.append_child(&form)?;
            form.submit()?;
        } else if let Some(fields) = &options.body {
            let url = web_sys::Url::new_with_base(&src, &location.href().unwrap_or_default())?;
            let query = web_sys::UrlSearchParams::new()?;
            for (k, v) in fields {
                query.append(k, v);
            }
            url.set_search(&query.to_string().as_string().unwrap_or_default());
            location.assign(&url.href())?;
        } else if explicit_src {
            location.assign(&src)?;
        } else {
            location.reload()?;
        }
        Ok(web_sys::AbortController::new()?.signal())
    }

    /// Render `node` into the body, reconciling with the previous call.
    pub(crate) fn apply_node(self: &Rc<Self>, node: Node) {
        let existing = self.root.borrow().clone();
        let root = existing.unwrap_or_else(|| {
            let root = Root::in_container(document().body().expect("body").into(), Some(self.clone()));
            *self.root.borrow_mut() = Some(root.clone());
            root
        });
        root.render(node);
    }
}

pub(crate) fn js_error(e: &JsValue) -> String {
    e.as_string()
        .or_else(|| js_sys::Reflect::get(e, &"message".into()).ok().and_then(|m| m.as_string()))
        .unwrap_or_else(|| format!("{e:?}"))
}

thread_local! {
    static TOP: Rc<FrameState> = Rc::new(FrameState { root: RefCell::new(None) });
    /// Client entries from `rmx-data`: marker id to (name, props JSON).
    static ENTRIES: RefCell<HashMap<String, (String, String)>> = RefCell::new(HashMap::new());
}

pub(crate) fn top() -> Rc<FrameState> {
    TOP.with(Rc::clone)
}

/// Named frames need the `frames` feature.
pub(crate) fn by_name(_name: &str) -> Option<Rc<FrameState>> {
    None
}

/// A render driven by a frame load; without frames there is nothing to coordinate.
pub(crate) fn server_reload<T>(f: impl FnOnce() -> T) -> T {
    f()
}

/// The parts of `rmx-data` this runtime reads (`ssr::HydrationData::to_json`): client
/// entries, with their props kept as raw JSON. Frames (`"f"`) are ignored.
#[derive(serde::Deserialize)]
struct Data<'a> {
    #[serde(default, borrow)]
    h: HashMap<String, EntryData<'a>>,
}

#[derive(serde::Deserialize)]
struct EntryData<'a> {
    n: String,
    #[serde(borrow)]
    p: &'a serde_json::value::RawValue,
}

fn merge_data() {
    let Ok(scripts) = document().query_selector_all(r#"script[type="application/json"][id="rmx-data"]"#) else { return };
    for i in 0..scripts.length() {
        let json = scripts.item(i).and_then(|s| s.text_content()).unwrap_or_default();
        let Ok(data) = serde_json::from_str::<Data>(&json) else {
            warn("rsmix: bad rmx-data");
            continue;
        };
        ENTRIES.with(|e| {
            let mut e = e.borrow_mut();
            for (id, entry) in data.h {
                e.insert(id, (entry.n, entry.p.get().to_string()));
            }
        });
    }
}

/// Register hydration data for tests: client entries as `(id, name, props JSON)`. Frames
/// are ignored.
pub(crate) fn set_test_data(entries: &[(&str, &str, &str)], _frames: &[(&str, &str, Option<&str>, bool)]) {
    ENTRIES.with(|e| {
        let mut e = e.borrow_mut();
        for (id, name, props) in entries {
            e.insert(id.to_string(), (name.to_string(), props.to_string()));
        }
    });
}

/// `(name, props JSON)` for a client entry marker id.
pub(crate) fn entry_data(id: &str) -> Option<(String, String)> {
    ENTRIES.with(|e| e.borrow().get(id).cloned())
}

/// Read the page's hydration data and hydrate its client entries. Entries own their DOM, so
/// their insides are skipped.
pub(crate) fn start() {
    merge_data();
    let doc = document();
    let Some(body) = doc.body() else { return };
    let Ok(walker) = doc.create_tree_walker_with_what_to_show(&body, 0x80 /* SHOW_COMMENT */) else { return };
    while let Ok(Some(node)) = walker.next_node() {
        if entries::is_start(&node) {
            if let Some(end) = entries::hydrate_or_update(node.unchecked_ref(), None) {
                walker.set_current_node(&end);
            }
        }
    }
}

/// Unmount the client entries under `node` (`App::dispose`). The DOM stays.
pub(crate) fn dispose_subtree(node: &web_sys::Node) {
    let Ok(walker) = document().create_tree_walker_with_what_to_show(node, 0x80) else { return };
    while let Ok(Some(c)) = walker.next_node() {
        if entries::is_start(&c) {
            entries::dispose(&c);
        }
    }
}
