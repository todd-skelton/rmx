//! Soft navigation through the Navigation API.
//!
//! Same-origin link clicks, form submissions and back/forward become frame reloads: the
//! browser URL and history update as usual, but instead of loading a new document the
//! target frame (the top frame unless `data-rmx-target` names one) fetches its new content
//! and reconciles it into the page. Without the Navigation API the browser navigates
//! normally, so every page still works.
//!
//! Link and form attributes (as in Remix):
//!
//! - `data-rmx-target="name"` reloads the named frame instead of the top frame.
//! - `data-rmx-src="/url"` loads that URL into the target frame (the address bar still
//!   shows `href`).
//! - `data-rmx-history="replace" | "push"` chooses how the history entry is added.
//! - `data-rmx-document` opts out: the browser does a full document navigation.

use std::cell::Cell;
use std::rc::Rc;

use js_sys::Reflect;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use super::frames::{self, Submission};
use super::js::{call, get, object};
use super::{warn, window};
use crate::navigation::{History, Reset};

pub use crate::navigation::NavigateOptions;


fn opt_str(v: Option<&str>) -> JsValue {
    v.map_or(JsValue::UNDEFINED, JsValue::from_str)
}

/// The `navigation` object, when the browser supports intercepting form and link
/// navigations (`NavigateEvent.sourceElement`).
fn navigation() -> Option<JsValue> {
    let w: JsValue = window().into();
    let nav = get(&w, "navigation");
    if nav.is_undefined() || nav.is_null() {
        return None;
    }
    let event_class = get(&w, "NavigateEvent");
    if event_class.is_undefined() {
        return None;
    }
    let proto = get(&event_class, "prototype");
    Reflect::has(&proto, &JsValue::from_str("sourceElement")).ok()?.then_some(nav)
}

fn behavior(reset: bool) -> JsValue {
    JsValue::from_str(if reset { "after-transition" } else { "manual" })
}

fn state(target: Option<&str>, src: &str, reset_scroll: bool, reset_focus: bool) -> JsValue {
    object(&[
        ("$rmx", JsValue::TRUE),
        ("target", opt_str(target)),
        ("src", JsValue::from_str(src)),
        ("resetScroll", behavior(reset_scroll)),
        ("resetFocus", behavior(reset_focus)),
    ])
}

/// `"false"` / `"manual"` turn a reset off; anything else (or nothing) keeps it on.
fn reset_attr(value: Option<String>) -> bool {
    !matches!(value.as_deref(), Some("false" | "manual"))
}

fn is_runtime_state(v: &JsValue) -> bool {
    v.is_object() && Reflect::has(v, &JsValue::from_str("$rmx")).unwrap_or(false)
}

/// Navigate with the runtime: like clicking a link with the matching `data-rmx-*`
/// attributes. Falls back to `location.assign` without the Navigation API.
pub fn navigate(href: &str, options: NavigateOptions) {
    let _ = start_navigation(href, options);
}

/// [`navigate`], resolving once the navigation has finished (its content is in the page)
/// or failed. Without the Navigation API it resolves once the browser starts loading.
pub async fn navigate_async(href: &str, options: NavigateOptions) {
    if let Some(finished) = start_navigation(href, options).and_then(|r| get(&r, "finished").dyn_into::<js_sys::Promise>().ok()) {
        let _ = wasm_bindgen_futures::JsFuture::from(finished).await;
    }
}

/// Start a navigation; the Navigation API's `{ committed, finished }` result, if any.
fn start_navigation(href: &str, options: NavigateOptions) -> Option<JsValue> {
    let Some(nav) = navigation() else {
        let location = window().location();
        let _ = if options.history == Some(History::Replace) { location.replace(href) } else { location.assign(href) };
        return None;
    };
    let src = options.src.as_deref().unwrap_or(href);
    let reset_scroll = options.reset_scroll != Some(Reset::Manual);
    let reset_focus = options.reset_focus != Some(Reset::Manual);
    let history = options.history.map_or(JsValue::UNDEFINED, |h| JsValue::from_str(h.as_str()));
    let opts = object(&[("state", state(options.target.as_deref(), src, reset_scroll, reset_focus)), ("history", history)]);
    call(&nav, "navigate", &[&JsValue::from_str(href), &opts]).ok()
}

type NavigateListener = Closure<dyn Fn(JsValue)>;

thread_local! {
    static LISTENER: std::cell::RefCell<Option<NavigateListener>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn start() {
    let Some(nav) = navigation() else { return };
    let href = window().location().href().unwrap_or_default();
    let _ = call(&nav, "updateCurrentEntry", &[&object(&[("state", state(None, &href, true, true))])]);
    // `Fn`, not `FnMut`: a navigation can start another one synchronously (re-entry).
    let listener = Closure::<dyn Fn(JsValue)>::new(on_navigate);
    let _ = call(&nav, "addEventListener", &[&JsValue::from_str("navigate"), listener.as_ref()]);
    LISTENER.with(|l| *l.borrow_mut() = Some(listener));
}

/// Stop intercepting navigations (`App::dispose`).
pub(crate) fn stop() {
    let Some(listener) = LISTENER.with(|l| l.borrow_mut().take()) else { return };
    if let Some(nav) = navigation() {
        let _ = call(&nav, "removeEventListener", &[&JsValue::from_str("navigate"), listener.as_ref()]);
    }
}

fn same_origin(url: &str) -> bool {
    let location = window().location();
    let base = location.href().unwrap_or_default();
    web_sys::Url::new_with_base(url, &base).is_ok_and(|u| Some(u.origin()) == location.origin().ok())
}

/// What a navigation should do, read from its source element or history state.
struct Plan {
    target: Option<String>,
    src: String,
    replace: bool,
    reset_scroll: bool,
    reset_focus: bool,
    submission: Option<Submission>,
}

fn on_navigate(event: JsValue) {
    let info = get(&event, "info");
    if get(&info, "type").as_string().as_deref() == Some("rmx-redirect") {
        // The content for this URL is already in the page; just commit the URL.
        let reset_scroll = get(&info, "resetScroll").as_bool().unwrap_or(true);
        let nav_type = get(&event, "navigationType").as_string().unwrap_or_default();
        let dest = get(&get(&event, "destination"), "url").as_string().unwrap_or_default();
        let styles = ScrollStyles::install(&event, &nav_type, &dest, reset_scroll);
        let handler = Closure::once_into_js(move || {
            if let (Some(styles), Some(nav)) = (styles, navigation()) {
                styles.bind(&nav);
            }
            JsValue::UNDEFINED
        });
        intercept(&event, handler, false);
        return;
    }
    let can_intercept = get(&event, "canIntercept").as_bool().unwrap_or(false);
    let hash_change = get(&event, "hashChange").as_bool().unwrap_or(false);
    let download = get(&event, "downloadRequest");
    if !can_intercept || hash_change || !(download.is_null() || download.is_undefined()) {
        return;
    }
    let destination = get(&event, "destination");
    let Some(dest) = get(&destination, "url").as_string() else { return };
    if !same_origin(&dest) {
        return;
    }
    let nav_type = get(&event, "navigationType").as_string().unwrap_or_default();
    let Some(plan) = plan(&event, &destination, &dest, &nav_type) else { return };
    if !same_origin(&plan.src) {
        return;
    }

    let top = frames::top();
    let frame = match &plan.target {
        Some(name) => match frames::by_name(name) {
            Some(f) => f,
            None => return, // unknown frame: let the browser load the document
        },
        None => top.clone(),
    };

    let Some(nav) = navigation() else { return };
    let cancelable = get(&event, "cancelable").as_bool().unwrap_or(false);
    let entry_state = state(plan.target.as_deref(), &plan.src, plan.reset_scroll, plan.reset_focus);
    if plan.replace && nav_type == "push" && plan.submission.is_none() && cancelable {
        let _ = call(&event, "preventDefault", &[]);
        let opts = object(&[("state", entry_state), ("history", JsValue::from_str("replace"))]);
        let _ = call(&nav, "navigate", &[&JsValue::from_str(&dest), &opts]);
        return;
    }
    // A form submission that replaces history: rewrite the entry before it commits.
    let precommit_replace = plan.replace
        && plan.submission.is_some()
        && nav_type == "push"
        && !get(&window().into(), "NavigationPrecommitController").is_undefined();

    let traverse = nav_type == "traverse";
    let (reset_scroll, reset_focus) = if traverse { (true, true) } else { (plan.reset_scroll, plan.reset_focus) };
    let styles = ScrollStyles::install(&event, &nav_type, &dest, reset_scroll);
    let handler_styles = styles.clone();
    let handler = Closure::once_into_js(move || -> JsValue {
        if let Some(styles) = handler_styles {
            styles.bind(&nav);
        }
        wasm_bindgen_futures::future_to_promise(async move {
            if !traverse {
                let _ = call(&nav, "updateCurrentEntry", &[&object(&[("state", entry_state)])]);
            }
            top.set_src(&dest);
            if !Rc::ptr_eq(&frame, &top) {
                frame.set_src(&plan.src);
            }
            match frame.load(plan.submission.unwrap_or_default()).await {
                Ok(Some(redirected)) if Rc::ptr_eq(&frame, &top) => {
                    let opts = object(&[
                        ("state", state(None, &redirected, reset_scroll, reset_focus)),
                        ("history", JsValue::from_str("replace")),
                        ("info", object(&[("type", JsValue::from_str("rmx-redirect")), ("resetScroll", JsValue::from_bool(reset_scroll))])),
                    ]);
                    let _ = call(&nav, "navigate", &[&JsValue::from_str(&redirected), &opts]);
                }
                Ok(_) => {}
                Err(e) => {
                    let message = frames::js_error(&e);
                    if crate::app::has_listeners() {
                        crate::report_error(message);
                        return Err(e);
                    }
                    warn(&format!("rmx: navigation failed, loading the document: {message}"));
                    let _ = window().location().assign(&dest);
                }
            }
            Ok(JsValue::UNDEFINED)
        })
        .into()
    });
    let mut opts = vec![("handler", handler), ("scroll", behavior(reset_scroll)), ("focusReset", behavior(reset_focus))];
    if precommit_replace {
        let url = get(&get(&event, "destination"), "url");
        let precommit = Closure::once_into_js(move |controller: JsValue| {
            let _ = call(&controller, "redirect", &[&url, &object(&[("history", JsValue::from_str("replace"))])]);
        });
        opts.push(("precommitHandler", precommit));
    }
    if let Err(e) = call(&event, "intercept", &[&object(&opts)]) {
        if let Some(styles) = styles {
            styles.remove();
        }
        warn(&format!("rmx: could not intercept navigation: {}", frames::js_error(&e)));
    }
}

/// Temporary styles that keep reconciliation from fighting the browser's scroll handling
/// (Remix `installNavigationScrollStyleWorkarounds`).
///
/// - Traversals: reconciling a shorter page, or scroll anchoring, can move the scroll
///   position before the Navigation API restores it, and Chromium then treats that as a
///   user scroll and skips restoration. Keep the starting scroll range and turn anchoring
///   off until the navigation finishes.
/// - Chromium push/replace without a hash: anchoring can keep an offset after the reset to
///   the top, so turn it off through the first paint after the navigation.
#[derive(Clone)]
struct ScrollStyles {
    sheet: JsValue,
    removed: Rc<Cell<bool>>,
    after_paint: bool,
}

impl ScrollStyles {
    fn css(nav_type: &str, dest: &str, reset_scroll: bool) -> Option<(String, bool)> {
        if !reset_scroll {
            return None;
        }
        if nav_type == "traverse" {
            let root = window().document()?.document_element()?;
            let min_height = root.scroll_height() + root.client_height();
            let css = format!(
                "html {{ min-height: {min_height}px !important; overflow-anchor: none !important; }} body {{ overflow-anchor: none !important; }}"
            );
            return Some((css, false));
        }
        if nav_type != "push" && nav_type != "replace" {
            return None;
        }
        if web_sys::Url::new(dest).is_ok_and(|u| !u.hash().is_empty()) {
            return None;
        }
        let agent = get(&get(&window().into(), "navigator"), "userAgent").as_string().unwrap_or_default();
        if !(agent.contains("Chrome") || agent.contains("Chromium")) {
            return None;
        }
        Some(("html, body { overflow-anchor: none !important; }".into(), true))
    }

    fn install(event: &JsValue, nav_type: &str, dest: &str, reset_scroll: bool) -> Option<Self> {
        let (css, after_paint) = Self::css(nav_type, dest, reset_scroll)?;
        let constructor = get(&window().into(), "CSSStyleSheet").dyn_into::<js_sys::Function>().ok()?;
        let sheet = Reflect::construct(&constructor, &js_sys::Array::new()).ok()?;
        call(&sheet, "replaceSync", &[&JsValue::from_str(&css)]).ok()?;
        let document: JsValue = window().document()?.into();
        let sheets = get(&document, "adoptedStyleSheets").dyn_into::<js_sys::Array>().ok()?;
        let next = sheets.slice(0, sheets.length());
        next.push(&sheet);
        Reflect::set(&document, &JsValue::from_str("adoptedStyleSheets"), &next).ok()?;

        let styles = ScrollStyles { sheet, removed: Rc::new(Cell::new(false)), after_paint };
        let signal = get(event, "signal");
        if get(&signal, "aborted").as_bool() == Some(true) {
            styles.remove();
            return None;
        }
        let on_abort = styles.clone();
        let on_abort = Closure::once_into_js(move || on_abort.remove());
        let once = object(&[("once", JsValue::TRUE)]);
        let _ = call(&signal, "addEventListener", &[&JsValue::from_str("abort"), &on_abort, &once]);
        Some(styles)
    }

    fn remove(&self) {
        if self.removed.replace(true) {
            return;
        }
        let Some(document) = window().document() else { return };
        let document: JsValue = document.into();
        let Ok(sheets) = get(&document, "adoptedStyleSheets").dyn_into::<js_sys::Array>() else { return };
        let kept = sheets.filter(&mut |sheet, _, _| sheet != self.sheet);
        let _ = Reflect::set(&document, &JsValue::from_str("adoptedStyleSheets"), &kept);
    }

    /// Remove the styles once the navigation's transition settles. Called from the
    /// intercept handler, when `navigation.transition` exists.
    fn bind(self, nav: &JsValue) {
        let transition = get(nav, "transition");
        let finished = get(&transition, "finished");
        if !finished.is_object() {
            self.remove();
            return;
        }
        let failed = self.clone();
        let on_finished = Closure::once_into_js(move |_: JsValue| {
            if !self.after_paint {
                return self.remove();
            }
            // The first callback runs before the first paint after the navigation; the
            // second removes the styles before the following frame.
            let outer = Closure::once_into_js(move |_: JsValue| {
                let inner = Closure::once_into_js(move |_: JsValue| self.remove());
                let _ = window().request_animation_frame(inner.unchecked_ref());
            });
            let _ = window().request_animation_frame(outer.unchecked_ref());
        });
        let on_failed = Closure::once_into_js(move |_: JsValue| failed.remove());
        let _ = call(&finished, "then", &[&on_finished, &on_failed]);
    }
}

fn intercept(event: &JsValue, handler: JsValue, scroll: bool) {
    let mut opts = vec![("handler", handler)];
    if !scroll {
        opts.push(("scroll", JsValue::from_str("manual")));
    }
    if let Err(e) = call(event, "intercept", &[&object(&opts)]) {
        warn(&format!("rmx: could not intercept navigation: {}", frames::js_error(&e)));
    }
}

fn plan(event: &JsValue, destination: &JsValue, dest: &str, nav_type: &str) -> Option<Plan> {
    let from_state = || {
        let s = call(destination, "getState", &[]).ok()?;
        is_runtime_state(&s).then(|| Plan {
            target: get(&s, "target").as_string(),
            src: get(&s, "src").as_string().unwrap_or_else(|| dest.to_string()),
            replace: false,
            reset_scroll: get(&s, "resetScroll").as_string().as_deref() != Some("manual"),
            reset_focus: get(&s, "resetFocus").as_string().as_deref() != Some("manual"),
            submission: None,
        })
    };
    if nav_type == "traverse" {
        return from_state();
    }
    let source = get(event, "sourceElement");
    let Some(source) = source.dyn_ref::<web_sys::Element>() else {
        // Programmatic: `navigate()` above, or a script calling navigation.navigate.
        return from_state();
    };

    if let Ok(Some(link)) = source.closest("a, area") {
        if link.has_attribute("data-rmx-document") || link.has_attribute("download") {
            return None;
        }
        return Some(Plan {
            target: link.get_attribute("data-rmx-target"),
            src: link.get_attribute("data-rmx-src").unwrap_or_else(|| dest.to_string()),
            replace: link.get_attribute("data-rmx-history").as_deref() == Some("replace"),
            reset_scroll: reset_attr(link.get_attribute("data-rmx-reset-scroll")),
            reset_focus: reset_attr(link.get_attribute("data-rmx-reset-focus")),
            submission: None,
        });
    }

    // A form submission: the source is the form or its submit button.
    let tag = source.tag_name();
    let submitter = (tag == "BUTTON" || tag == "INPUT").then(|| source.clone());
    let form: web_sys::Element = if tag == "FORM" {
        source.clone()
    } else {
        get(source, "form").dyn_into().ok()?
    };
    let attr = |name: &str, submitter_name: &str| {
        submitter
            .as_ref()
            .filter(|s| s.has_attribute(submitter_name))
            .and_then(|s| s.get_attribute(submitter_name))
            .or_else(|| form.get_attribute(name))
    };
    let has = |name: &str| form.has_attribute(name) || submitter.as_ref().is_some_and(|s| s.has_attribute(name));
    if has("data-rmx-document") {
        return None;
    }
    let method = attr("method", "formmethod").unwrap_or_else(|| "get".into()).to_ascii_lowercase();
    let target_attr = attr("target", "formtarget").unwrap_or_default().to_ascii_lowercase();
    if method == "dialog" || target_attr == "_blank" {
        return None;
    }
    let submission = if method == "get" {
        None
    } else {
        let form_data = get(event, "formData");
        let enctype = attr("enctype", "formenctype").unwrap_or_default().to_ascii_lowercase();
        let (body, content_type) = if form_data.is_null() || form_data.is_undefined() {
            (None, None)
        } else if enctype == "multipart/form-data" {
            (Some(form_data), None)
        } else {
            let params = web_sys::UrlSearchParams::new_with_str_sequence_sequence(&form_data).ok()?;
            (Some(params.into()), Some("application/x-www-form-urlencoded".to_string()))
        };
        Some(Submission { method: Some(method.to_ascii_uppercase()), body, content_type })
    };
    // Submitting a form back to the current URL replaces the entry unless told otherwise.
    let same_url = window().location().href().ok().as_deref() == Some(dest);
    let replace = match attr("data-rmx-history", "data-rmx-history").as_deref() {
        Some("replace") => true,
        Some("push") => false,
        _ => submission.is_some() && same_url,
    };
    Some(Plan {
        target: attr("data-rmx-target", "data-rmx-target"),
        src: attr("data-rmx-src", "data-rmx-src").unwrap_or_else(|| dest.to_string()),
        replace,
        reset_scroll: reset_attr(attr("data-rmx-reset-scroll", "data-rmx-reset-scroll")),
        reset_focus: reset_attr(attr("data-rmx-reset-focus", "data-rmx-reset-focus")),
        submission,
    })
}

