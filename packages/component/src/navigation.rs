//! Programmatic navigation and the `link()` mixin, usable from shared component code (both
//! are no-ops on the server, where `link` only sets attributes).

use std::cell::Cell;
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
    /// How the history entry is added (`data-rmx-history`). Unset lets the browser choose
    /// (`"auto"`: a navigation to the current URL replaces).
    pub history: Option<History>,
    /// Scroll behavior (`data-rmx-reset-scroll`); unset means [`Reset::AfterTransition`].
    pub reset_scroll: Option<Reset>,
    /// Focus reset behavior (`data-rmx-reset-focus`); unset means [`Reset::AfterTransition`].
    pub reset_focus: Option<Reset>,
}

/// How a navigation adds its history entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum History {
    Push,
    Replace,
}

impl History {
    pub fn as_str(self) -> &'static str {
        match self {
            History::Push => "push",
            History::Replace => "replace",
        }
    }
}

/// Whether a navigation resets scroll (or focus) once the new content is in. `true` and
/// `false` convert to `AfterTransition` and `Manual`, as in Remix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reset {
    /// The runtime resets it after the transition (the default).
    AfterTransition,
    /// Left alone: the page manages it.
    Manual,
}

impl Reset {
    pub fn as_str(self) -> &'static str {
        match self {
            Reset::AfterTransition => "after-transition",
            Reset::Manual => "manual",
        }
    }
}

impl From<bool> for Reset {
    fn from(reset: bool) -> Reset {
        if reset {
            Reset::AfterTransition
        } else {
            Reset::Manual
        }
    }
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

/// [`navigate`], resolving once the new content is in the page (Remix's
/// `await navigate(href)`). Resolves at once on the server.
pub async fn navigate_async(href: &str, options: NavigateOptions) {
    #[cfg(target_arch = "wasm32")]
    crate::dom::navigation::navigate_async(href, options).await;
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (href, options);
}

/// Make any element a link (Remix's `link(href)` mixin).
///
/// On `<a>` and `<area>` it sets `href` (and `data-rmx-*` for the options), so the browser
/// and the navigation runtime handle it. On other elements it adds `role="link"`, keyboard
/// focus (except on buttons and editable elements), Enter to navigate, and Ctrl/Cmd or
/// middle click to open a new tab. A `disabled` or `aria-disabled="true"` host does not
/// navigate.
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
        if let Some(history) = options.history {
            el.set_attr("data-rmx-history", history.as_str());
        }
        if let Some(reset) = options.reset_scroll {
            el.set_attr("data-rmx-reset-scroll", reset.as_str());
        }
        if let Some(reset) = options.reset_focus {
            el.set_attr("data-rmx-reset-focus", reset.as_str());
        }
        return;
    }
    let is_button = el.tag == "button";
    if el.attr("role").is_none() {
        el.set_attr("role", "link");
    }
    if matches!(el.attr("disabled"), Some(AttrValue::Bool(true))) && el.attr("aria-disabled").is_none() {
        el.set_attr("aria-disabled", "true");
    }
    if is_button {
        if el.attr("type").is_none() {
            el.set_attr("type", "button");
        }
    } else if el.attr("tabindex").is_none() && el.attr("contenteditable").is_none() {
        el.set_attr("tabindex", "0");
    }
    // A button activated from the keyboard also gets a `click` (detail 0); Enter has
    // navigated already and Space must not, so that click is swallowed.
    let suppress_click = Rc::new(Cell::new(false));
    let (h, o, suppress) = (href.clone(), options.clone(), suppress_click.clone());
    el.add_event("click", EventHandler::new(move |e: &Event| {
        if e.prop_f64("detail") == Some(0.0) && suppress.replace(false) {
            e.prevent_default();
            return;
        }
        suppress.set(false);
        if is_disabled(e) {
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
    let (h, suppress) = (href.clone(), suppress_click.clone());
    el.add_event("auxclick", EventHandler::new(move |e: &Event| {
        suppress.set(false);
        if is_disabled(e) {
            e.prevent_default();
            return;
        }
        if e.prop_f64("button") == Some(1.0) {
            e.prevent_default();
            open_new_tab(&h);
        }
    }));
    let (h, o, suppress) = (href.clone(), options.clone(), suppress_click);
    el.add_event("keydown", EventHandler::new(move |e: &Event| {
        if e.key() == "Enter" {
            if e.prop_bool("repeat") {
                return;
            }
            e.prevent_default();
            if is_disabled(e) {
                return;
            }
            suppress.set(is_button);
            navigate(&h, o.clone());
        } else if is_button && e.key() == " " {
            suppress.set(true);
            e.prevent_default();
        }
    }));
    if is_button {
        el.add_event("keyup", EventHandler::new(move |e: &Event| {
            if e.key() == " " {
                e.prevent_default();
            }
        }));
    }
}

/// `disabled` (on elements that have it) or `aria-disabled="true"` on the link host.
fn is_disabled(e: &Event) -> bool {
    e.current_prop_bool("disabled") || e.current_attr("aria-disabled").as_deref() == Some("true")
}

fn open_new_tab(href: &str) {
    #[cfg(target_arch = "wasm32")]
    if let Some(w) = web_sys::window() {
        let _ = w.open_with_url_and_target(href, "_blank");
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = href;
}
