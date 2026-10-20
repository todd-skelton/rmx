//! Programmatic navigation and the `link()` mixin, usable from shared component code (both
//! are no-ops on the server, where `link` only sets attributes).

use std::rc::Rc;

use crate::mixin::Mixin;
use crate::node::{AttrValue, Element, Event, EventHandler};

/// Options for [`navigate`] and [`link_with`], matching the `data-rmx-*` link attributes.
#[derive(Clone, Debug, Default)]
pub struct NavigateOptions {
    /// A named frame to load into instead of the top frame (`data-rmx-target`).
    pub target: Option<String>,
    /// The URL to load into the target frame; defaults to `href` (`data-rmx-src`).
    pub src: Option<String>,
    /// Replace the current history entry instead of pushing one (`data-rmx-history`).
    pub replace: bool,
    /// `Some(false)` keeps the scroll position (`data-rmx-reset-scroll="false"`).
    pub reset_scroll: Option<bool>,
    /// `Some(false)` keeps focus where it is (`data-rmx-reset-focus="false"`).
    pub reset_focus: Option<bool>,
}

/// Navigate through the runtime, like clicking a link: the target frame (the top frame by
/// default) loads the new content and the URL and history update. Without the Navigation
/// API the browser loads the document. No-op on the server.
pub fn navigate(href: &str, options: NavigateOptions) {
    #[cfg(target_arch = "wasm32")]
    crate::dom::navigation::navigate(href, options);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (href, options);
}

/// Make any element a link (Remix's `link(href)` mixin).
///
/// On `<a>` and `<area>` it sets `href` (and `data-rmx-*` for the options), so the browser
/// and the navigation runtime handle it. On other elements it adds `role="link"`, keyboard
/// focus, Enter to navigate, and Ctrl/Cmd or middle click to open a new tab.
pub fn link(href: impl Into<String>) -> Mixin {
    link_with(href, NavigateOptions::default())
}

pub fn link_with(href: impl Into<String>, options: NavigateOptions) -> Mixin {
    let href: Rc<str> = Rc::from(href.into());
    Mixin::new(move |el: &mut Element| apply_link(el, &href, &options))
}

fn str_attr(v: impl Into<String>) -> AttrValue {
    AttrValue::Str(v.into().into())
}

fn apply_link(el: &mut Element, href: &Rc<str>, options: &NavigateOptions) {
    if matches!(el.tag, "a" | "area") {
        el.set_attr("href", str_attr(&**href));
        if let Some(t) = &options.target {
            el.set_attr("data-rmx-target", str_attr(t.clone()));
        }
        if let Some(s) = &options.src {
            el.set_attr("data-rmx-src", str_attr(s.clone()));
        }
        if options.replace {
            el.set_attr("data-rmx-history", "replace");
        }
        if options.reset_scroll == Some(false) {
            el.set_attr("data-rmx-reset-scroll", "false");
        }
        if options.reset_focus == Some(false) {
            el.set_attr("data-rmx-reset-focus", "false");
        }
        return;
    }
    if el.attr("role").is_none() {
        el.set_attr("role", "link");
    }
    if el.tag == "button" {
        if el.attr("type").is_none() {
            el.set_attr("type", "button");
        }
    } else if el.attr("tabindex").is_none() {
        el.set_attr("tabindex", "0");
    }
    let (h, o) = (href.clone(), options.clone());
    el.add_event("click", EventHandler::new(move |e: &Event| {
        if e.current_attr("disabled").is_some() || e.current_attr("aria-disabled").as_deref() == Some("true") {
            e.prevent_default();
            return;
        }
        if e.prop_f64("button") != Some(0.0) {
            return;
        }
        e.prevent_default();
        if e.prop_bool("metaKey") || e.prop_bool("ctrlKey") {
            open_new_tab(&h);
        } else {
            navigate(&h, o.clone());
        }
    }));
    let h = href.clone();
    el.add_event("auxclick", EventHandler::new(move |e: &Event| {
        if e.prop_f64("button") == Some(1.0) {
            e.prevent_default();
            open_new_tab(&h);
        }
    }));
    let (h, o) = (href.clone(), options.clone());
    let is_button = el.tag == "button";
    el.add_event("keydown", EventHandler::new(move |e: &Event| {
        if e.key() == "Enter" && !e.prop_bool("repeat") {
            e.prevent_default();
            navigate(&h, o.clone());
        } else if is_button && e.key() == " " {
            e.prevent_default();
        }
    }));
}

fn open_new_tab(href: &str) {
    #[cfg(target_arch = "wasm32")]
    if let Some(w) = web_sys::window() {
        let _ = w.open_with_url_and_target(href, "_blank");
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = href;
}
