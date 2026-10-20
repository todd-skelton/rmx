//! Focus and text selection across an update (Remix's `createDocumentState`, adapted from
//! React's `ReactInputSelection`). Moving a focused element with `insertBefore` blurs it;
//! the scheduler captures the focused element and its selection before rendering and puts
//! them back afterwards, keeping the scroll position of every ancestor.

use js_sys::Reflect;
use wasm_bindgen::{JsCast, JsValue};

use super::document;

/// The focused element and its text selection at the time of [`DocumentState::capture`].
pub struct DocumentState {
    focused: Option<web_sys::Element>,
    selection: Option<(f64, f64)>,
}

fn active_element() -> Option<web_sys::Element> {
    let doc = document();
    doc.active_element().or_else(|| doc.body().map(Into::into))
}

/// Text inputs, textareas and editable elements keep a selection worth restoring.
fn has_selection_capabilities(el: &web_sys::Element) -> bool {
    match el.tag_name().as_str() {
        "INPUT" => {
            let kind = Reflect::get(el, &JsValue::from_str("type")).ok().and_then(|v| v.as_string()).unwrap_or_default();
            matches!(kind.as_str(), "text" | "search" | "tel" | "url" | "password")
        }
        "TEXTAREA" => true,
        _ => Reflect::get(el, &JsValue::from_str("contentEditable")).ok().and_then(|v| v.as_string()).as_deref() == Some("true"),
    }
}

fn get_selection(el: &web_sys::Element) -> Option<(f64, f64)> {
    let start = Reflect::get(el, &JsValue::from_str("selectionStart")).ok()?.as_f64()?;
    let end = Reflect::get(el, &JsValue::from_str("selectionEnd")).ok().and_then(|v| v.as_f64()).unwrap_or(start);
    Some((start, end))
}

fn set_selection(el: &web_sys::Element, (start, end): (f64, f64)) {
    let len = Reflect::get(el, &JsValue::from_str("value")).ok().and_then(|v| v.as_string()).map_or(0, |v| v.encode_utf16().count());
    let _ = Reflect::set(el, &JsValue::from_str("selectionStart"), &JsValue::from_f64(start));
    let _ = Reflect::set(el, &JsValue::from_str("selectionEnd"), &JsValue::from_f64(end.min(len as f64)));
}

impl DocumentState {
    /// Remember the focused element and, for text controls, its selection.
    pub fn capture() -> DocumentState {
        let focused = active_element();
        let selection = focused.as_ref().filter(|el| has_selection_capabilities(el)).and_then(get_selection);
        DocumentState { focused, selection }
    }

    /// Focus the captured element again if it lost focus but is still in the document,
    /// restoring its selection and the scroll positions focusing would change.
    pub fn restore(self) {
        let Some(prior) = self.focused else { return };
        if active_element().is_some_and(|now| now.is_same_node(Some(&prior))) {
            return;
        }
        let doc = document();
        if !doc.document_element().is_some_and(|root| root.contains(Some(&prior))) {
            return;
        }
        let mut ancestors = Vec::new();
        let mut at: Option<web_sys::Node> = Some(prior.clone().into());
        while let Some(node) = at {
            if let Some(el) = node.dyn_ref::<web_sys::Element>() {
                ancestors.push((el.clone(), el.scroll_left(), el.scroll_top()));
            }
            at = node.parent_node();
        }
        if let Some(range) = self.selection {
            if has_selection_capabilities(&prior) {
                set_selection(&prior, range);
            }
        }
        if let Some(el) = prior.dyn_ref::<web_sys::HtmlElement>() {
            let _ = el.focus();
        }
        for (el, left, top) in ancestors {
            el.set_scroll_left(left);
            el.set_scroll_top(top);
        }
    }
}
