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

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use js_sys::Reflect;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use super::form_navigation::{FormNavigationResolver, PendingSubmission};
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
pub(crate) fn navigation() -> Option<JsValue> {
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
    static FORMS: std::cell::RefCell<Option<FormNavigationResolver>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn start() {
    let Some(nav) = navigation() else { return };
    let href = window().location().href().unwrap_or_default();
    let _ = call(&nav, "updateCurrentEntry", &[&object(&[("state", state(None, &href, true, true))])]);
    FORMS.with(|f| *f.borrow_mut() = Some(FormNavigationResolver::new()));
    // `Fn`, not `FnMut`: a navigation can start another one synchronously (re-entry).
    let listener = Closure::<dyn Fn(JsValue)>::new(on_navigate);
    let _ = call(&nav, "addEventListener", &[&JsValue::from_str("navigate"), listener.as_ref()]);
    LISTENER.with(|l| *l.borrow_mut() = Some(listener));
}

/// Stop intercepting navigations (`App::dispose`).
pub(crate) fn stop() {
    FORMS.with(|f| f.borrow_mut().take());
    let Some(listener) = LISTENER.with(|l| l.borrow_mut().take()) else { return };
    if let Some(nav) = navigation() {
        let _ = call(&nav, "removeEventListener", &[&JsValue::from_str("navigate"), listener.as_ref()]);
    }
}

/// Whether `url`, resolved against the document base URL, is on this origin.
fn same_origin(url: &str) -> bool {
    let location = window().location();
    let base = super::document().base_uri().ok().flatten().or_else(|| location.href().ok()).unwrap_or_default();
    web_sys::Url::new_with_base(url, &base).is_ok_and(|u| Some(u.origin()) == location.origin().ok())
}

/// `navigate` events with this `info` load a fresh document (Remix `reloadCurrentDocument`).
const DOCUMENT_RELOAD_INFO: &str = "remix-document-reload";

/// Reload the current document without the runtime intercepting it.
fn reload_document() {
    if let Some(nav) = navigation().filter(|n| get(n, "reload").is_function()) {
        let _ = call(&nav, "reload", &[&object(&[("info", JsValue::from_str(DOCUMENT_RELOAD_INFO))])]);
    } else {
        let _ = window().location().reload();
    }
}

/// A navigation the runtime cannot route to a frame. Pushes and replaces are left to the
/// browser; a traversal between entries the runtime created would only change the URL, so
/// it reloads the destination document instead.
fn fallback_to_document(event: &JsValue, nav_type: &str) {
    if nav_type != "traverse" {
        return;
    }
    let signal = get(event, "signal");
    let handler = Box::new(move || {
        if get(&signal, "aborted").as_bool() != Some(true) {
            reload_document();
        }
        JsValue::UNDEFINED
    });
    intercept_navigation(event, nav_type, "", false, vec![("scroll", behavior(false))], handler, None);
}

thread_local! {
    /// A frame load for a navigation has started and not finished. A hash change that
    /// interrupts it is routed like any navigation, so the destination still loads.
    static NEEDS_FRAME_RELOAD: Cell<bool> = const { Cell::new(false) };
}

/// What a navigation should do, read from its source element or history state.
struct Plan {
    target: Option<String>,
    src: String,
    replace: bool,
    reset_scroll: bool,
    reset_focus: bool,
    /// A non-GET form submission; its data is read when the navigation handler runs.
    submission: Option<PendingSubmission>,
    /// The history state the plan was read from, kept as stored (legacy boolean resets
    /// included) when a redirect rewrites its `src`.
    stored: Option<JsValue>,
}

/// `navigate` events with this `info.type` commit the URL a frame load redirected to.
const FRAME_REDIRECT_INFO: &str = "frame-redirect";
/// `navigate` events with this `info.type` replay a POST submission as a replace navigation.
const FORM_REPLAY_INFO: &str = "rmx-form-submission";

thread_local! {
    /// The submission being replayed, with the frame chosen before its push was cancelled
    /// (browsers without `NavigationPrecommitController`).
    static REPLAY: std::cell::RefCell<Option<(Plan, Rc<frames::FrameState>)>> = const { std::cell::RefCell::new(None) };
}

/// `"after-transition"` or `"manual"`; anything else is not a redirect's reset value.
fn reset_value(v: &JsValue) -> Option<bool> {
    match v.as_string().as_deref() {
        Some("after-transition") => Some(true),
        Some("manual") => Some(false),
        _ => None,
    }
}

fn on_navigate(event: JsValue) {
    let info = get(&event, "info");
    if info.as_string().as_deref() == Some(DOCUMENT_RELOAD_INFO) {
        return;
    }
    let can_intercept = get(&event, "canIntercept").as_bool().unwrap_or(false);
    let hash_change = get(&event, "hashChange").as_bool().unwrap_or(false);
    let download = get(&event, "downloadRequest");
    if !can_intercept || (hash_change && !NEEDS_FRAME_RELOAD.with(Cell::get)) || !(download.is_null() || download.is_undefined()) {
        return;
    }
    let destination = get(&event, "destination");
    let Some(dest) = get(&destination, "url").as_string() else { return };
    if !same_origin(&dest) {
        return;
    }
    let nav_type = get(&event, "navigationType").as_string().unwrap_or_default();
    let info_type = get(&info, "type").as_string();
    if info_type.as_deref() == Some(FRAME_REDIRECT_INFO) {
        if let (Some(reset_scroll), Some(reset_focus)) = (reset_value(&get(&info, "resetScroll")), reset_value(&get(&info, "resetFocus"))) {
            // The content for this URL is already in the page and scrolled: commit the URL.
            let options = vec![("scroll", behavior(false)), ("focusReset", behavior(reset_focus))];
            intercept_navigation(&event, &nav_type, &dest, reset_scroll, options, Box::new(|| JsValue::UNDEFINED), None);
            return;
        }
    }
    let replayed = if info_type.as_deref() == Some(FORM_REPLAY_INFO) { REPLAY.with(|r| r.borrow_mut().take()) } else { None };
    let (plan, chosen) = match replayed {
        Some((plan, frame)) => (plan, Some(frame)),
        None => match plan(&event, &destination, &dest, &nav_type) {
            Some(plan) => (plan, None),
            None => return,
        },
    };
    if !same_origin(&plan.src) {
        return fallback_to_document(&event, &nav_type);
    }

    let top = frames::top();
    let is_replay = chosen.is_some();
    let frame = match (chosen, &plan.target) {
        (Some(frame), _) => frame,
        (None, Some(name)) => match frames::by_name(name) {
            Some(f) => f,
            None => return fallback_to_document(&event, &nav_type),
        },
        (None, None) => top.clone(),
    };

    let Some(nav) = navigation() else { return };
    let cancelable = get(&event, "cancelable").as_bool().unwrap_or(false);
    let entry_state = state(plan.target.as_deref(), &plan.src, plan.reset_scroll, plan.reset_focus);
    let mut precommit: Option<Box<dyn FnOnce(JsValue)>> = None;
    if plan.replace && nav_type == "push" && !is_replay {
        if plan.submission.is_none() {
            // A link or GET form that replaces history: replay the push as a replace.
            if cancelable {
                let _ = call(&event, "preventDefault", &[]);
                let opts = object(&[("state", entry_state), ("history", JsValue::from_str("replace"))]);
                let _ = call(&nav, "navigate", &[&JsValue::from_str(&dest), &opts]);
                return;
            }
        } else if get(&window().into(), "NavigationPrecommitController").is_function() {
            // A form submission that replaces history: rewrite the entry before it commits.
            let url = destination.clone();
            precommit = Some(Box::new(move |controller: JsValue| {
                let _ = call(&controller, "redirect", &[&get(&url, "url"), &object(&[("history", JsValue::from_str("replace"))])]);
            }));
        } else if cancelable {
            // Without precommit support (Safari), replay the submission as a replace
            // navigation into the frame chosen here.
            let _ = call(&event, "preventDefault", &[]);
            let opts = object(&[
                ("state", entry_state),
                ("history", JsValue::from_str("replace")),
                ("info", object(&[("type", JsValue::from_str(FORM_REPLAY_INFO))])),
            ]);
            REPLAY.with(|r| *r.borrow_mut() = Some((plan, frame)));
            let _ = call(&nav, "navigate", &[&JsValue::from_str(&dest), &opts]);
            // The replay's navigate event runs synchronously; drop the plan if it did not.
            REPLAY.with(|r| r.borrow_mut().take());
            return;
        }
    }

    let traverse = nav_type == "traverse";
    let (reset_scroll, reset_focus) = if traverse { (true, true) } else { (plan.reset_scroll, plan.reset_focus) };
    NEEDS_FRAME_RELOAD.with(|n| n.set(true));
    let event_signal: web_sys::AbortSignal = get(&event, "signal").unchecked_into();
    let scroll_event = event.clone();
    let (handler_dest, dest) = (dest.clone(), dest);
    let handler = Box::new(move || -> JsValue {
        let dest = handler_dest;
        wasm_bindgen_futures::future_to_promise(async move {
            if event_signal.aborted() {
                return Ok(JsValue::UNDEFINED);
            }
            let submission = plan.submission.map(|pending| {
                let s = pending.get();
                Submission { method: Some(s.method.to_ascii_uppercase()), form_data: s.form_data, enc_type: Some(s.enc_type) }
            });
            if !traverse {
                let _ = call(&nav, "updateCurrentEntry", &[&object(&[("state", entry_state.clone())])]);
            }
            top.set_src(&dest);
            if !Rc::ptr_eq(&frame, &top) {
                frame.set_src(&plan.src);
            }
            // Scroll once the new content is in, unless this navigation or its load was
            // superseded (Remix calls `event.scroll()` after `reload.committed`).
            let nav_signal = event_signal.clone();
            let on_commit = move |load: &web_sys::AbortSignal| {
                if reset_scroll && !nav_signal.aborted() && !load.aborted() {
                    let _ = call(&scroll_event, "scroll", &[]);
                    webkit_scroll_reset_applied(&scroll_event);
                }
            };
            let result = frame.load_for_navigation(submission.unwrap_or_default(), Some(event_signal.clone()), on_commit).await;
            match result {
                Ok((redirected, load_signal)) => {
                    if event_signal.aborted() || load_signal.aborted() {
                        return Ok(JsValue::UNDEFINED);
                    }
                    NEEDS_FRAME_RELOAD.with(|n| n.set(false));
                    if let Some(redirected) = redirected.filter(|_| Rc::ptr_eq(&frame, &top)) {
                        top.set_src(&redirected);
                        // Keep the stored state (a traversal's legacy settings included) with
                        // the new src; the redirect itself uses this navigation's resets.
                        let next = js_sys::Object::assign(&js_sys::Object::new(), plan.stored.as_ref().unwrap_or(&entry_state).unchecked_ref());
                        let _ = Reflect::set(&next, &JsValue::from_str("src"), &JsValue::from_str(&redirected));
                        let opts = object(&[
                            ("state", next.into()),
                            ("history", JsValue::from_str("replace")),
                            (
                                "info",
                                object(&[
                                    ("type", JsValue::from_str(FRAME_REDIRECT_INFO)),
                                    ("resetScroll", behavior(reset_scroll)),
                                    ("resetFocus", behavior(reset_focus)),
                                ]),
                            ),
                        ]);
                        // Not awaited: this handler must settle before the replacement can finish.
                        let _ = call(&nav, "navigate", &[&JsValue::from_str(&redirected), &opts]);
                    }
                }
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
    let options = vec![("scroll", behavior(reset_scroll)), ("focusReset", behavior(reset_focus))];
    intercept_navigation(&event, &nav_type, &dest, reset_scroll, options, handler, precommit);
}

/// Intercept `event` (Remix `interceptNavigation`): install the scroll style workarounds,
/// and once a handler runs, tie them and the WebKit scroll resync to the transition.
fn intercept_navigation(
    event: &JsValue,
    nav_type: &str,
    dest: &str,
    reset_scroll: bool,
    mut options: Vec<(&'static str, JsValue)>,
    handler: Box<dyn FnOnce() -> JsValue>,
    precommit: Option<Box<dyn FnOnce(JsValue)>>,
) {
    let styles = ScrollStyles::install(event, nav_type, dest, reset_scroll);
    let bound = Rc::new(Cell::new(false));
    let bind = {
        let (event, nav_type, dest, styles) = (event.clone(), nav_type.to_string(), dest.to_string(), styles.clone());
        Rc::new(move || {
            if bound.get() {
                return;
            }
            let aborted = get(&get(&event, "signal"), "aborted").as_bool() == Some(true);
            let transition = navigation().map(|nav| get(&nav, "transition")).filter(JsValue::is_object);
            let (false, Some(transition)) = (aborted, transition) else {
                if let Some(styles) = &styles {
                    styles.remove();
                }
                return;
            };
            bound.set(true);
            resync_webkit_scroll(&event, &nav_type, &dest, reset_scroll, &transition);
            if let Some(styles) = styles.clone() {
                styles.bind(&transition);
            }
        })
    };
    let on_handler = bind.clone();
    options.push(("handler", Closure::once_into_js(move || -> JsValue {
        on_handler();
        handler()
    })));
    if let Some(precommit) = precommit {
        options.push(("precommitHandler", Closure::once_into_js(move |controller: JsValue| {
            bind();
            precommit(controller);
        })));
    }
    if let Err(e) = call(event, "intercept", &[&object(&options)]) {
        if let Some(styles) = styles {
            styles.remove();
        }
        warn(&format!("rmx: could not intercept navigation: {}", frames::js_error(&e)));
    }
}

fn user_agent() -> String {
    get(&get(&window().into(), "navigator"), "userAgent").as_string().unwrap_or_default()
}

fn is_chromium(agent: &str) -> bool {
    agent.contains("Chrome") || agent.contains("Chromium")
}

thread_local! {
    /// NavigateEvent -> function the handler calls once it has reset scroll (Remix
    /// `webKitScrollResets`).
    static WEBKIT_SCROLL_RESETS: js_sys::WeakMap = js_sys::WeakMap::new();
}

/// How long after a navigation finishes iOS restorations are still undone.
const WEBKIT_SCROLL_RESET_MS: i32 = 500;

/// Tell the WebKit workaround for `event` that the navigation has reset scroll.
fn webkit_scroll_reset_applied(event: &JsValue) {
    let notify = WEBKIT_SCROLL_RESETS.with(|m| m.get(event.unchecked_ref()));
    if let Some(notify) = notify.dyn_ref::<js_sys::Function>() {
        let _ = notify.call0(&JsValue::NULL);
    }
}

/// WebKit scroll workarounds for a push or replace that resets scroll (Remix
/// `resyncWebKitScrollAfterNavigation`):
///
/// - iOS Safari can restore the previous position, or the new page's bottom, for several
///   frames after the reset. Those restorations are undone until shortly after the
///   navigation finishes, the user interacts with the page, or another navigation starts.
///   App code that scrolls to one of those positions in that time is undone too.
/// - WebKit can reset its internal scroll position without updating the visual viewport
///   (https://bugs.webkit.org/show_bug.cgi?id=309542): once the navigation finishes at the
///   top, nudge it to 1 and back to 0, unless something else scrolled meanwhile.
fn resync_webkit_scroll(event: &JsValue, nav_type: &str, dest: &str, reset_scroll: bool, transition: &JsValue) {
    let agent = user_agent();
    if !agent.contains("AppleWebKit") || is_chromium(&agent) {
        return;
    }
    if !reset_scroll || (nav_type != "push" && nav_type != "replace") {
        return;
    }
    if web_sys::Url::new(dest).is_ok_and(|u| !u.hash().is_empty()) {
        return;
    }
    let w: JsValue = window().into();
    let scroll_y = {
        let w = w.clone();
        move || get(&w, "scrollY").as_f64().unwrap_or(0.0)
    };
    let scroll_to = |top: f64| {
        let options = object(&[("behavior", JsValue::from_str("instant")), ("left", JsValue::from_f64(0.0)), ("top", JsValue::from_f64(top))]);
        let _ = call(&window().into(), "scrollTo", &[&options]);
    };

    let start_y = scroll_y();
    let Ok(controller) = web_sys::AbortController::new() else { return };
    let signal: JsValue = controller.signal().into();
    let stop: JsValue = Closure::<dyn Fn()>::new(move || controller.abort()).into_js_value();
    let listen = |target: &JsValue, kind: &str, listener: &JsValue, options: JsValue| {
        let _ = call(target, "addEventListener", &[&JsValue::from_str(kind), listener, &options]);
    };
    for kind in ["pointerdown", "wheel", "keydown"] {
        let options = object(&[("capture", JsValue::TRUE), ("passive", JsValue::TRUE), ("signal", signal.clone())]);
        listen(&w, kind, &stop, options);
    }
    if let Some(nav) = navigation() {
        listen(&nav, "navigate", &stop, object(&[("signal", signal.clone())]));
    }

    // Safari clamps the previous position to the page's height when it restores it, and
    // the page can grow before the scroll event runs, so remember every bottom the new page
    // has had.
    let restored = Rc::new(RefCell::new(vec![start_y]));
    let remember_bottom = {
        let restored = restored.clone();
        move || {
            let Some(root) = window().document().and_then(|d| d.document_element()) else { return };
            let inner = window().inner_height().ok().and_then(|h| h.as_f64()).unwrap_or(0.0);
            let max_y = f64::from(root.scroll_height()) - inner;
            if start_y > max_y && !restored.borrow().contains(&max_y) {
                restored.borrow_mut().push(max_y);
            }
        }
    };
    let undo_restoration = {
        let remember_bottom = remember_bottom.clone();
        let scroll_y = scroll_y.clone();
        Rc::new(move || {
            remember_bottom();
            let y = scroll_y();
            if y != 0.0 && restored.borrow().iter().any(|p| (y - p).abs() < 1.0) {
                scroll_to(0.0);
            }
        })
    };

    let reset_applied = Rc::new(Cell::new(false));
    let applied = reset_applied.clone();
    let notify = Closure::<dyn Fn()>::new(move || {
        applied.set(true);
        remember_bottom();
    });
    WEBKIT_SCROLL_RESETS.with(|m| m.set(event.unchecked_ref(), &notify.into_js_value()));
    // Scroll events run before the frame paints, so an undone restoration may never be drawn.
    let on_scroll = {
        let undo = undo_restoration.clone();
        Closure::<dyn Fn()>::new(move || {
            if reset_applied.get() {
                undo();
            }
        })
        .into_js_value()
    };
    listen(&w, "scroll", &on_scroll, object(&[("passive", JsValue::TRUE), ("signal", signal.clone())]));

    let nav_signal = get(event, "signal");
    let aborted = move || get(&nav_signal, "aborted").as_bool() == Some(true);
    let stop_later = stop.clone();
    let on_finished = Closure::once_into_js(move |_: JsValue| {
        if get(&signal, "aborted").as_bool() != Some(true) {
            undo_restoration();
            let _ = window().set_timeout_with_callback_and_timeout_and_arguments_0(stop_later.unchecked_ref(), WEBKIT_SCROLL_RESET_MS);
        }
        let at = move |top: f64| get(&window().into(), "scrollX").as_f64() == Some(0.0) && scroll_y() == top;
        if aborted() || !at(0.0) {
            return;
        }
        scroll_to(1.0);
        let back = Closure::once_into_js(move |_: JsValue| {
            if !aborted() && at(1.0) {
                scroll_to(0.0);
            }
        });
        let _ = window().request_animation_frame(back.unchecked_ref());
    });
    let _ = call(&get(transition, "finished"), "then", &[&on_finished, &stop]);
}

/// Temporary styles that keep reconciliation from fighting the browser's scroll handling
/// (Remix `installNavigationScrollStyleWorkarounds`).
///
/// - Traversals: reconciling a shorter page, or scroll anchoring, can move the scroll
///   position before the Navigation API restores it, and Chromium then treats that as a
///   user scroll and skips restoration. Keep the starting scroll range and turn anchoring
///   off until the navigation finishes. Re-enabling anchoring while the page shrinks back
///   to its own height lets Chromium move the restored position, so anchoring stays off
///   until after the next paint.
/// - Chromium push/replace without a hash: anchoring can keep an offset after the reset to
///   the top, so turn it off through the first paint after the navigation.
#[derive(Clone)]
struct ScrollStyles {
    sheet: JsValue,
    removed: Rc<Cell<bool>>,
    /// Whether the sheet holds more than the anchoring suppression.
    extra: bool,
}

const DISABLE_SCROLL_ANCHORING: &str = "html, body { overflow-anchor: none !important; }";

impl ScrollStyles {
    fn css(nav_type: &str, dest: &str, reset_scroll: bool) -> Option<String> {
        if !reset_scroll {
            return None;
        }
        if nav_type == "traverse" {
            let root = window().document()?.document_element()?;
            let min_height = root.scroll_height() + root.client_height();
            let css = format!(
                "html {{ min-height: {min_height}px !important; overflow-anchor: none !important; }} body {{ overflow-anchor: none !important; }}"
            );
            return Some(css);
        }
        if nav_type != "push" && nav_type != "replace" {
            return None;
        }
        if web_sys::Url::new(dest).is_ok_and(|u| !u.hash().is_empty()) {
            return None;
        }
        if !is_chromium(&user_agent()) {
            return None;
        }
        Some(DISABLE_SCROLL_ANCHORING.into())
    }

    fn install(event: &JsValue, nav_type: &str, dest: &str, reset_scroll: bool) -> Option<Self> {
        let css = Self::css(nav_type, dest, reset_scroll)?;
        let constructor = get(&window().into(), "CSSStyleSheet").dyn_into::<js_sys::Function>().ok()?;
        let sheet = Reflect::construct(&constructor, &js_sys::Array::new()).ok()?;
        call(&sheet, "replaceSync", &[&JsValue::from_str(&css)]).ok()?;
        let document: JsValue = window().document()?.into();
        let sheets = get(&document, "adoptedStyleSheets").dyn_into::<js_sys::Array>().ok()?;
        let next = sheets.slice(0, sheets.length());
        next.push(&sheet);
        Reflect::set(&document, &JsValue::from_str("adoptedStyleSheets"), &next).ok()?;

        let styles = ScrollStyles { sheet, removed: Rc::new(Cell::new(false)), extra: css != DISABLE_SCROLL_ANCHORING };
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

    /// Drop everything but the anchoring suppression once the navigation has finished.
    fn settle(&self) {
        if !self.removed.get() && self.extra {
            let _ = call(&self.sheet, "replaceSync", &[&JsValue::from_str(DISABLE_SCROLL_ANCHORING)]);
        }
    }

    /// Once the navigation's transition finishes, settle the styles and remove them before
    /// the second frame after it; remove them at once if it fails. Called from the
    /// intercept handler, when `navigation.transition` exists.
    fn bind(self, transition: &JsValue) {
        let finished = get(transition, "finished");
        if !finished.is_object() {
            self.remove();
            return;
        }
        let failed = self.clone();
        let on_finished = Closure::once_into_js(move |_: JsValue| {
            self.settle();
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

/// Remix `normalizeResetBehavior`: `false`, `"false"` and `"manual"` turn a reset off.
fn reset_on(v: &JsValue) -> bool {
    !(v.as_bool() == Some(false) || matches!(v.as_string().as_deref(), Some("false" | "manual")))
}

/// The runtime state stored for the destination entry. Safari's `destination.getState()`
/// returns `null` for traversals, so those also look the entry up by key.
fn destination_state(destination: &JsValue, nav_type: &str) -> Option<JsValue> {
    let state = call(destination, "getState", &[]).ok().filter(is_runtime_state);
    if state.is_some() || nav_type != "traverse" {
        return state;
    }
    let nav = navigation()?;
    let key = get(destination, "key");
    let entries: js_sys::Array = call(&nav, "entries", &[]).ok()?.dyn_into().ok()?;
    let entry = entries.iter().find(|e| get(e, "key") == key)?;
    call(&entry, "getState", &[]).ok().filter(is_runtime_state)
}

fn plan(event: &JsValue, destination: &JsValue, dest: &str, nav_type: &str) -> Option<Plan> {
    let from_state = || {
        let s = destination_state(destination, nav_type)?;
        Some(Plan {
            target: get(&s, "target").as_string(),
            src: get(&s, "src").as_string().unwrap_or_else(|| dest.to_string()),
            replace: false,
            reset_scroll: reset_on(&get(&s, "resetScroll")),
            reset_focus: reset_on(&get(&s, "resetFocus")),
            submission: None,
            stored: Some(s),
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
            stored: None,
        });
    }

    // A form submission: the source is the form or its submit button.
    let form = FORMS.with(|f| f.borrow().as_ref().and_then(|r| r.resolve(event)))?;
    let attr = |name: &str| form.get_attribute(name, None);
    if form.has_attribute("data-rmx-document") {
        return None;
    }
    let submission = form.submission();
    // Submitting a form back to the current URL replaces the entry unless told otherwise.
    let same_url = window().location().href().ok().as_deref() == Some(dest);
    let replace = match attr("data-rmx-history").as_deref() {
        Some("replace") => true,
        Some("push") => false,
        _ => submission.is_some() && same_url,
    };
    Some(Plan {
        target: attr("data-rmx-target"),
        src: attr("data-rmx-src").unwrap_or_else(|| dest.to_string()),
        replace,
        reset_scroll: reset_attr(attr("data-rmx-reset-scroll")),
        reset_focus: reset_attr(attr("data-rmx-reset-focus")),
        submission,
        stored: None,
    })
}

