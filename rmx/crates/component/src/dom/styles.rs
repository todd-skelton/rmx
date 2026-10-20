//! `css()` rules in the document (Remix's `createStyleManager`).
//!
//! Rules are content-addressed: a class is a hash of its declarations, so a rule can only
//! become unused, never wrong. That gives two lifetimes in one constructed stylesheet on
//! `document.adoptedStyleSheets`:
//!
//! 1. Server rules (`<style data-rmx-style>` tags) are adopted into the sheet, their tags
//!    removed, and pinned for the life of the manager.
//! 2. Rules inserted on the client are reference counted and deleted when the last element
//!    using them is removed. A server tag adopted later for the same class pins the rule.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use js_sys::{Array, Reflect};
use wasm_bindgen::{JsCast, JsValue};

use super::document;
use crate::mixin::rule_body;
use crate::node::StyleRule;

/// The cascade layer generated rules live in: each rule is `@layer rmx.CLASS { .. }`.
pub const STYLE_LAYER: &str = "rmx";

struct Entry {
    count: u32,
    index: u32,
    pinned: bool,
}

/// Owns one constructed stylesheet of generated rules. The runtime uses a single manager
/// per thread; separate managers (with their own layer name) are independent.
pub struct StyleManager {
    layer: String,
    sheet: RefCell<Option<web_sys::CssStyleSheet>>,
    rules: RefCell<HashMap<String, Entry>>,
    generation: Cell<u32>,
}

impl StyleManager {
    /// A manager whose rules go in `@layer {layer}.CLASS`. The stylesheet is adopted by
    /// the document on the first insert, not before.
    pub fn new(layer: &str) -> StyleManager {
        StyleManager { layer: layer.to_string(), sheet: RefCell::new(None), rules: RefCell::new(HashMap::new()), generation: Cell::new(0) }
    }

    fn sheet(&self) -> web_sys::CssStyleSheet {
        if let Some(sheet) = &*self.sheet.borrow() {
            return sheet.clone();
        }
        let sheet = web_sys::CssStyleSheet::new().expect("CSSStyleSheet");
        let doc = document();
        let key = JsValue::from_str("adoptedStyleSheets");
        let next: Array = Reflect::get(&doc, &key).ok().and_then(|v| v.dyn_into::<Array>().ok()).map(|a| a.slice(0, a.length())).unwrap_or_default();
        next.push(&sheet);
        let _ = Reflect::set(&doc, &key, &next);
        *self.sheet.borrow_mut() = Some(sheet.clone());
        sheet
    }

    fn rule_count(sheet: &web_sys::CssStyleSheet) -> u32 {
        sheet.css_rules().map(|l| l.length()).unwrap_or(0)
    }

    fn remove_sheet(&self) {
        let Some(sheet) = self.sheet.borrow_mut().take() else { return };
        let doc = document();
        let key = JsValue::from_str("adoptedStyleSheets");
        if let Ok(list) = Reflect::get(&doc, &key).and_then(|v| v.dyn_into::<Array>()) {
            let sheet: &JsValue = sheet.as_ref();
            let kept: Array = list.iter().filter(|s| s != sheet).collect();
            let _ = Reflect::set(&doc, &key, &kept);
        }
    }

    /// Add a reference to `class`, inserting `rule` (its `.CLASS { .. }` text) the first time.
    pub fn insert(&self, class: &str, rule: &str) {
        self.insert_with(class, || rule.to_string());
    }

    fn insert_with(&self, class: &str, rule: impl FnOnce() -> String) {
        let mut rules = self.rules.borrow_mut();
        if let Some(entry) = rules.get_mut(class) {
            if !entry.pinned {
                entry.count += 1;
            }
            return;
        }
        let sheet = self.sheet();
        let index = Self::rule_count(&sheet);
        let text = format!("@layer {}.{class} {{ {} }}", self.layer, rule());
        match sheet.insert_rule_with_index(&text, index) {
            Ok(_) => {
                rules.insert(class.to_string(), Entry { count: 1, index, pinned: false });
            }
            Err(e) => web_sys::console::error_2(&JsValue::from_str("Invalid css() rule:"), &e),
        }
    }

    /// Drop a reference to `class`. The rule is deleted when no references remain, unless
    /// it came from the server.
    pub fn remove(&self, class: &str) {
        let mut rules = self.rules.borrow_mut();
        let Some(entry) = rules.get_mut(class) else { return };
        if entry.pinned {
            return;
        }
        entry.count -= 1;
        if entry.count > 0 {
            return;
        }
        let index = entry.index;
        rules.remove(class);
        if let Some(sheet) = &*self.sheet.borrow() {
            let _ = sheet.delete_rule(index);
        }
        for entry in rules.values_mut() {
            if entry.index > index {
                entry.index -= 1;
            }
        }
    }

    /// Whether the rule for `class` is in the stylesheet.
    pub fn has(&self, class: &str) -> bool {
        self.rules.borrow().get(class).is_some_and(|e| e.pinned || e.count > 0)
    }

    /// Bumped whenever the stylesheet is cleared (`reset`, `dispose`).
    pub fn generation(&self) -> u32 {
        self.generation.get()
    }

    /// The tracked classes, in stylesheet order.
    pub fn selectors(&self) -> Vec<String> {
        let rules = self.rules.borrow();
        let mut list: Vec<(&String, u32)> = rules.iter().map(|(k, e)| (k, e.index)).collect();
        list.sort_by_key(|(_, i)| *i);
        list.into_iter().map(|(k, _)| k.clone()).collect()
    }

    /// Delete every rule and stop adopting the stylesheet.
    pub fn reset(&self) {
        if let Some(sheet) = &*self.sheet.borrow() {
            for i in (0..Self::rule_count(sheet)).rev() {
                let _ = sheet.delete_rule(i);
            }
        }
        self.rules.borrow_mut().clear();
        self.remove_sheet();
        self.generation.set(self.generation.get() + 1);
    }

    pub fn dispose(&self) {
        self.remove_sheet();
        self.rules.borrow_mut().clear();
        self.generation.set(self.generation.get() + 1);
    }

    /// Move the server `<style data-rmx-style>` tags in or under `source` into the
    /// stylesheet, in document order, and pin their rules. Returns the classes processed.
    /// A tag whose CSS the browser rejects stays in the document so it still applies.
    pub fn adopt_server_styles(&self, source: &web_sys::Node) -> HashSet<String> {
        let mut adopted = HashSet::new();
        for style in server_style_tags(source) {
            if let Some(class) = self.adopt_tag(&style) {
                adopted.insert(class);
            }
        }
        adopted
    }

    fn adopt_tag(&self, style: &web_sys::Element) -> Option<String> {
        let class = style.get_attribute("data-rmx-style")?.trim().to_string();
        if class.is_empty() {
            return None;
        }
        let mut rules = self.rules.borrow_mut();
        if let Some(entry) = rules.get_mut(&class) {
            entry.pinned = true;
            style.remove();
            return Some(class);
        }
        let text = style.text_content().unwrap_or_default();
        let text = text.trim();
        if text.is_empty() {
            style.remove();
            return None;
        }
        let sheet = self.sheet();
        let index = Self::rule_count(&sheet);
        sheet.insert_rule_with_index(text, index).ok()?;
        rules.insert(class.clone(), Entry { count: 0, index, pinned: true });
        style.remove();
        Some(class)
    }
}

fn server_style_tags(source: &web_sys::Node) -> Vec<web_sys::Element> {
    const SELECTOR: &str = "style[data-rmx-style]";
    let mut out = Vec::new();
    if let Some(el) = source.dyn_ref::<web_sys::Element>() {
        if el.matches(SELECTOR).unwrap_or(false) {
            out.push(el.clone());
            return out;
        }
    }
    let list = if let Some(d) = source.dyn_ref::<web_sys::Document>() {
        d.query_selector_all(SELECTOR)
    } else if let Some(e) = source.dyn_ref::<web_sys::Element>() {
        e.query_selector_all(SELECTOR)
    } else if let Some(f) = source.dyn_ref::<web_sys::DocumentFragment>() {
        f.query_selector_all(SELECTOR)
    } else {
        return out;
    };
    let Ok(list) = list else { return out };
    for i in 0..list.length() {
        if let Some(el) = list.item(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) {
            out.push(el);
        }
    }
    out
}

thread_local! {
    static MANAGER: StyleManager = StyleManager::new(STYLE_LAYER);
}

/// An element started using `rule`.
pub(crate) fn retain(rule: &StyleRule) {
    MANAGER.with(|m| m.insert_with(&rule.class, || rule_body(rule)));
}

/// An element stopped using `class`.
pub(crate) fn release(class: &str) {
    MANAGER.with(|m| m.remove(class));
}

/// Adopt the server style tags in a document, container, frame fragment or fetched page.
pub(crate) fn adopt_server_styles(root: &web_sys::Node) {
    MANAGER.with(|m| {
        m.adopt_server_styles(root);
    });
}
