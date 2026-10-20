//! `css()` rules in the document: each generated class is inserted once, as a
//! `<style data-rmx-style="CLASS">` in `<head>`, the same form the server writes.

use std::cell::RefCell;
use std::collections::HashSet;

use wasm_bindgen::JsCast;

use super::document;
use crate::mixin::rule_css;
use crate::node::StyleRule;

thread_local! {
    static SEEN: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// Make sure a rule's style tag is in the document.
pub(crate) fn insert(rule: &StyleRule) {
    if SEEN.with(|s| s.borrow().contains(&rule.class)) {
        return;
    }
    let doc = document();
    let Ok(style) = doc.create_element("style") else { return };
    let _ = style.set_attribute("data-rmx-style", &rule.class);
    style.set_text_content(Some(&rule_css(rule)));
    if let Some(head) = doc.head() {
        let _ = head.append_child(&style);
    }
    SEEN.with(|s| s.borrow_mut().insert(rule.class.clone()));
}

/// Record (and de-duplicate) server style tags under `root`. Tags that arrive in frame
/// content or a fetched document move to `<head>` so they outlive the content that
/// brought them.
pub(crate) fn adopt_server_styles(root: &web_sys::Node) {
    let doc = document();
    let Some(head) = doc.head() else { return };
    let list = match root.dyn_ref::<web_sys::Document>() {
        Some(d) => d.query_selector_all("style[data-rmx-style]"),
        None => match root.dyn_ref::<web_sys::Element>() {
            Some(e) => e.query_selector_all("style[data-rmx-style]"),
            None => match root.dyn_ref::<web_sys::DocumentFragment>() {
                Some(f) => f.query_selector_all("style[data-rmx-style]"),
                None => return,
            },
        },
    };
    let Ok(list) = list else { return };
    let live_doc = root.is_same_node(Some(doc.as_ref()));
    for i in 0..list.length() {
        let Some(style) = list.item(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else { continue };
        let class = style.get_attribute("data-rmx-style").unwrap_or_default();
        let new = SEEN.with(|s| s.borrow_mut().insert(class));
        if live_doc && new {
            continue;
        }
        if new {
            let _ = head.append_child(&style);
        } else {
            style.remove();
        }
    }
}
