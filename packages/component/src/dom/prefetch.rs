//! Prefetching (Remix's `runtime/prefetch.ts`): `data-rmx-prefetch="intent"` links start
//! loading their destination after the pointer or keyboard focus rests on them for 100 ms, or
//! at once when pressed; `data-rmx-prefetch="viewport"` links start when they enter view.
//! [`crate::prefetch`] does the same on demand.
//!
//! By default a prefetch fetches the destination HTML (with the frame headers and low
//! priority), parses it inert in a `<template>`, and adds `<link rel="prefetch">` hints for
//! its external scripts, stylesheets, module preloads and script, style, image and font
//! preloads. The HTML is discarded: navigation fetches as usual and the browser cache decides
//! what is reused. `RunOptions::prefetch` replaces that strategy.
//!
//! Only same-origin URLs that an untargeted top-frame navigation (or a mounted named frame)
//! would load are prefetched: not the current page, downloads, `data-rmx-document` links or
//! links that open another browsing context. Automatic triggers skip Save-Data and 2G
//! connections.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use super::js::{call, get};
use super::{document, window};
use crate::navigation::{PrefetchFrame, PrefetchOptions};

const INTENT_DELAY_MS: i32 = 100;
const VIEWPORT_ATTRIBUTES: [&str; 7] = ["href", "data-rmx-prefetch", "download", "data-rmx-document", "data-rmx-target", "data-rmx-src", "target"];

/// A link resting under the pointer or keyboard focus.
struct Intent {
    anchor: web_sys::Element,
    timer: Cell<Option<i32>>,
    prefetched: Cell<bool>,
    /// The timer callback (dropped with the intent).
    on_timer: RefCell<Option<Closure<dyn FnMut()>>>,
}

#[derive(Default)]
struct State {
    /// `RunOptions::prefetch`, while the runtime runs.
    custom: Option<PrefetchFrame>,
    /// Aborted by [stop]: removes listeners and cancels discovery requests.
    controller: Option<web_sys::AbortController>,
    listeners: Vec<Closure<dyn FnMut(web_sys::Event)>>,
    pointer: Option<Rc<Intent>>,
    focus: Option<Rc<Intent>>,
    viewport: Option<web_sys::IntersectionObserver>,
    mutations: Option<web_sys::MutationObserver>,
    observer_callbacks: Vec<JsValue>,
    /// Discovery work in flight, by source and target.
    discovering: HashSet<String>,
    /// Asset hints in the head and not loaded yet, by asset key.
    hints: Vec<(String, web_sys::Element)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

fn signal() -> Option<web_sys::AbortSignal> {
    STATE.with(|s| s.borrow().controller.as_ref().map(web_sys::AbortController::signal))
}

/// Whether `node` is a pending asset hint, which head reconciliation must leave in place.
pub(crate) fn is_pending_hint(node: &web_sys::Node) -> bool {
    STATE.with(|s| s.borrow().hints.iter().any(|(_, link)| link.is_same_node(Some(node))))
}

/// Start the triggers (from [`crate::run`]); `custom` replaces the default strategy.
pub(crate) fn start(custom: Option<PrefetchFrame>) {
    stop();
    let Ok(controller) = web_sys::AbortController::new() else { return };
    let signal = controller.signal();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.custom = custom;
        s.controller = Some(controller);
    });
    if super::navigation::navigation().is_none() {
        return;
    }
    let doc = document();
    listen(&doc, "pointerover", &signal, false, |event| {
        let Some(anchor) = intent_anchor(&event) else { return };
        if get(&event, "pointerType").as_string().as_deref() != Some("mouse") {
            return;
        }
        set_intent(true, Some(rest(&anchor)));
    });
    listen(&doc, "focusin", &signal, false, |event| {
        let Some(anchor) = intent_anchor(&event) else { return };
        set_intent(false, Some(rest(&anchor)));
    });
    listen(&doc, "pointerout", &signal, false, |event| {
        let next = leave(&event, current(true));
        set_intent(true, next);
    });
    listen(&doc, "focusout", &signal, false, |event| {
        let next = leave(&event, current(false));
        set_intent(false, next);
    });
    listen(&doc, "pointerdown", &signal, true, |event| {
        // Other buttons and modifier keys open new tabs, which can't reuse the prefetch.
        let flag = |name: &str| get(&event, name).as_bool() == Some(true);
        let opens_here = flag("isPrimary") && get(&event, "button").as_f64() == Some(0.0) && !["altKey", "ctrlKey", "metaKey", "shiftKey"].iter().any(|k| flag(k));
        let Some(anchor) = intent_anchor(&event) else { return };
        if !opens_here {
            return;
        }
        let intent = rest(&anchor);
        set_intent(true, Some(intent.clone()));
        trigger(&intent);
    });
    start_viewport();
}

/// Stop the triggers, cancel discovery and remove pending hints (`App::dispose`).
pub(crate) fn stop() {
    let state = STATE.with(|s| std::mem::take(&mut *s.borrow_mut()));
    if let Some(controller) = state.controller {
        controller.abort();
    }
    for intent in [state.pointer, state.focus].into_iter().flatten() {
        clear_timer(&intent);
    }
    if let Some(viewport) = state.viewport {
        viewport.disconnect();
    }
    if let Some(mutations) = state.mutations {
        mutations.disconnect();
    }
    for (_, link) in state.hints {
        link.remove();
    }
}

fn listen(target: &web_sys::Document, kind: &str, signal: &web_sys::AbortSignal, passive: bool, f: impl Fn(web_sys::Event) + 'static) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(f);
    let options = web_sys::AddEventListenerOptions::new();
    options.set_signal(signal);
    if passive {
        options.set_passive(true);
    }
    let _ = target.add_event_listener_with_callback_and_add_event_listener_options(kind, closure.as_ref().unchecked_ref(), &options);
    STATE.with(|s| s.borrow_mut().listeners.push(closure));
}

fn current(pointer: bool) -> Option<Rc<Intent>> {
    STATE.with(|s| {
        let s = s.borrow();
        if pointer { s.pointer.clone() } else { s.focus.clone() }
    })
}

/// Replace the pointer (or focus) intent, cancelling the previous one's timer unless the
/// other input still rests on it.
fn set_intent(pointer: bool, next: Option<Rc<Intent>>) {
    let previous = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let slot = if pointer { &mut s.pointer } else { &mut s.focus };
        std::mem::replace(slot, next)
    });
    let Some(previous) = previous else { return };
    let kept = intents().iter().any(|i| Rc::ptr_eq(i, &previous));
    if !kept {
        clear_timer(&previous);
    }
}

/// The pointer and focus intents.
fn intents() -> Vec<Rc<Intent>> {
    STATE.with(|s| {
        let s = s.borrow();
        let mut all = Vec::with_capacity(2);
        all.extend(s.pointer.clone());
        all.extend(s.focus.clone());
        all
    })
}

/// The intent for `anchor`: the one already resting on it, or a new one with its timer.
fn rest(anchor: &web_sys::Element) -> Rc<Intent> {
    if let Some(intent) = intents().into_iter().find(|i| i.anchor.is_same_node(Some(anchor))) {
        return intent;
    }
    let intent = Rc::new(Intent { anchor: anchor.clone(), timer: Cell::new(None), prefetched: Cell::new(false), on_timer: RefCell::new(None) });
    let weak = Rc::downgrade(&intent);
    let on_timer = Closure::<dyn FnMut()>::new(move || {
        if let Some(intent) = weak.upgrade() {
            trigger(&intent);
        }
    });
    intent.timer.set(window().set_timeout_with_callback_and_timeout_and_arguments_0(on_timer.as_ref().unchecked_ref(), INTENT_DELAY_MS).ok());
    *intent.on_timer.borrow_mut() = Some(on_timer);
    intent
}

fn clear_timer(intent: &Intent) {
    if let Some(timer) = intent.timer.take() {
        window().clear_timeout_with_handle(timer);
    }
}

fn trigger(intent: &Intent) {
    clear_timer(intent);
    if intent.prefetched.get() || intent.anchor.get_attribute("data-rmx-prefetch").as_deref() != Some("intent") || !can_prefetch_automatically() {
        return;
    }
    intent.prefetched.set(true);
    if let Some((src, options)) = anchor_request(&intent.anchor).filter(has_target) {
        prefetch_frame(&src, options);
    }
}

/// The intent still resting after `event` moved the pointer or focus out of an element.
fn leave(event: &web_sys::Event, intent: Option<Rc<Intent>>) -> Option<Rc<Intent>> {
    let intent = intent?;
    let inside = |v: JsValue| v.dyn_into::<web_sys::Node>().ok().is_some_and(|n| intent.anchor.contains(Some(&n)));
    if !inside(event.target().map_or(JsValue::UNDEFINED, Into::into)) {
        return Some(intent);
    }
    inside(get(event, "relatedTarget")).then_some(intent)
}

fn is_html_anchor(el: &web_sys::Element) -> bool {
    el.local_name() == "a" && el.namespace_uri().as_deref() == Some("http://www.w3.org/1999/xhtml")
}

fn intent_anchor(event: &web_sys::Event) -> Option<web_sys::Element> {
    let target = event.target()?.dyn_into::<web_sys::Element>().ok()?;
    target.closest("a[href][data-rmx-prefetch=\"intent\"]").ok().flatten().filter(is_html_anchor)
}

fn can_prefetch_automatically() -> bool {
    let connection = get(&get(&window().into(), "navigator"), "connection");
    if !connection.is_object() {
        return true;
    }
    if get(&connection, "saveData").as_bool() == Some(true) {
        return false;
    }
    !matches!(get(&connection, "effectiveType").as_string().as_deref(), Some("slow-2g" | "2g"))
}

type Request = (String, PrefetchOptions);

fn anchor_request(anchor: &web_sys::Element) -> Option<Request> {
    let target_attr = anchor.get_attribute("target").unwrap_or_default();
    if !anchor.is_connected() || !anchor.has_attribute("href") || anchor.has_attribute("download") || anchor.has_attribute("data-rmx-document") || !(target_attr.is_empty() || target_attr == "_self") {
        return None;
    }
    let raw = anchor.get_attribute("href").unwrap_or_default();
    let base = anchor.base_uri().ok().flatten().unwrap_or_default();
    let href = same_origin_href(&raw, &base)?;
    let src = same_origin_href(&anchor.get_attribute("data-rmx-src").unwrap_or(raw), &base)?;
    match anchor.get_attribute("data-rmx-target").filter(|t| !t.is_empty()) {
        Some(target) => request(&src, PrefetchOptions { target: Some(target) }),
        None => request(&href, PrefetchOptions::default()),
    }
}

fn has_target(request: &Request) -> bool {
    request.1.target.as_deref().is_none_or(|name| super::frames::by_name(name).is_some())
}

/// The request for `href` if the runtime would handle a navigation to it.
fn request(href: &str, options: PrefetchOptions) -> Option<Request> {
    super::navigation::navigation()?;
    let base = document().base_uri().ok().flatten().unwrap_or_default();
    let src = same_origin_href(href, &base)?;
    if options.target.as_deref().is_some_and(|t| !t.is_empty()) {
        return Some((src, options));
    }
    let current = web_sys::Url::new(&window().location().href().ok()?).ok()?;
    current.set_hash("");
    (src != current.href()).then_some((src, PrefetchOptions::default()))
}

fn same_origin_href(href: &str, base: &str) -> Option<String> {
    let url = web_sys::Url::new_with_base(href, base).ok()?;
    url.set_hash("");
    (Some(url.origin()) == window().location().origin().ok()).then(|| url.href())
}

/// [`crate::prefetch`].
pub(crate) fn prefetch(href: &str, options: PrefetchOptions) {
    if let Some((src, options)) = request(href, options) {
        prefetch_frame(&src, options);
    }
}

fn prefetch_frame(src: &str, options: PrefetchOptions) {
    let custom = STATE.with(|s| s.borrow().custom.clone());
    match custom {
        Some(custom) => custom(src, options),
        None => default_prefetch(src, options),
    }
}

// Viewport

fn viewport_request(anchor: &web_sys::Element) -> Option<Request> {
    (anchor.get_attribute("data-rmx-prefetch").as_deref() == Some("viewport")).then(|| anchor_request(anchor)).flatten()
}

fn request_key(request: &Request) -> String {
    format!("{}\n{}", request.0, request.1.target.as_deref().unwrap_or(""))
}

fn start_viewport() {
    let w: JsValue = window().into();
    if get(&w, "IntersectionObserver").is_undefined() {
        return;
    }
    // Each mounted link prefetches once for its current source and target.
    let completed = js_sys::WeakMap::new();
    let on_entries = {
        let completed = completed.clone();
        Closure::<dyn FnMut(js_sys::Array, web_sys::IntersectionObserver)>::new(move |entries: js_sys::Array, viewport: web_sys::IntersectionObserver| {
            if signal().is_none_or(|s| s.aborted()) || !can_prefetch_automatically() {
                return;
            }
            for entry in entries.iter() {
                let entry: web_sys::IntersectionObserverEntry = entry.unchecked_into();
                let anchor = entry.target();
                if !entry.is_intersecting() || !is_html_anchor(&anchor) {
                    continue;
                }
                let Some(request) = viewport_request(&anchor).filter(has_target) else { continue };
                let key = JsValue::from_str(&request_key(&request));
                if completed.get(&anchor) == key {
                    continue;
                }
                completed.set(&anchor, &key);
                viewport.unobserve(&anchor);
                prefetch_frame(&request.0, request.1);
            }
        })
        .into_js_value()
    };
    let Ok(viewport) = web_sys::IntersectionObserver::new(on_entries.unchecked_ref()) else { return };
    let update = {
        let (viewport, completed) = (viewport.clone(), completed.clone());
        Rc::new(move |anchor: &web_sys::Element| {
            viewport.unobserve(anchor);
            if !anchor.is_connected() {
                completed.delete(anchor);
                return;
            }
            // Named frames can still be hydrating; their registration is checked on entry.
            if let Some(request) = viewport_request(anchor) {
                if completed.get(anchor) != JsValue::from_str(&request_key(&request)) {
                    viewport.observe(anchor);
                }
            }
        })
    };
    let scan = {
        let update = update.clone();
        Rc::new(move |node: &web_sys::Node| {
            let Some(el) = node.dyn_ref::<web_sys::Element>() else { return };
            if is_html_anchor(el) {
                update(el);
            }
            if let Ok(anchors) = el.query_selector_all("a") {
                for i in 0..anchors.length() {
                    if let Some(a) = anchors.item(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()).filter(is_html_anchor) {
                        update(&a);
                    }
                }
            }
        })
    };
    let on_mutations = {
        let scan = scan.clone();
        Closure::<dyn FnMut(js_sys::Array)>::new(move |records: js_sys::Array| {
            if signal().is_none_or(|s| s.aborted()) {
                return;
            }
            for record in records.iter() {
                let record: web_sys::MutationRecord = record.unchecked_into();
                if record.type_() == "attributes" {
                    if let Some(a) = record.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).filter(is_html_anchor) {
                        update(&a);
                    }
                }
                for list in [record.removed_nodes(), record.added_nodes()] {
                    for i in 0..list.length() {
                        if let Some(node) = list.item(i) {
                            scan(&node);
                        }
                    }
                }
            }
        })
        .into_js_value()
    };
    let Ok(mutations) = web_sys::MutationObserver::new(on_mutations.unchecked_ref()) else { return };
    let init = web_sys::MutationObserverInit::new();
    init.set_child_list(true);
    init.set_subtree(true);
    init.set_attributes(true);
    init.set_attribute_filter(&VIEWPORT_ATTRIBUTES.iter().map(|a| JsValue::from_str(a)).collect::<js_sys::Array>());
    let _ = mutations.observe_with_options(&document(), &init);
    if let Some(root) = document().document_element() {
        scan(&root.into());
    }
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.viewport = Some(viewport);
        s.mutations = Some(mutations);
        s.observer_callbacks = vec![on_entries, on_mutations];
    });
}

// The default strategy: discover assets in the destination HTML.

fn default_prefetch(src: &str, options: PrefetchOptions) {
    let Some(signal) = signal().filter(|s| !s.aborted()) else { return };
    let key = request_key(&(src.to_string(), options.clone()));
    if !STATE.with(|s| s.borrow_mut().discovering.insert(key.clone())) {
        return;
    }
    let src = src.to_string();
    wasm_bindgen_futures::spawn_local(async move {
        // Speculative failures leave navigation to the default resolver.
        let _ = discover(&src, options.target.as_deref(), &signal).await;
        STATE.with(|s| s.borrow_mut().discovering.remove(&key));
    });
}

async fn discover(src: &str, target: Option<&str>, signal: &web_sys::AbortSignal) -> Result<(), JsValue> {
    use wasm_bindgen_futures::JsFuture;
    let headers = web_sys::Headers::new()?;
    headers.set("Accept", "text/html")?;
    headers.set("X-Remix-Frame", "true")?;
    if let Some(target) = target {
        headers.set("X-Remix-Target", target)?;
    }
    let init = web_sys::RequestInit::new();
    init.set_method("GET");
    init.set_mode(web_sys::RequestMode::SameOrigin);
    init.set_headers(&headers);
    init.set_signal(Some(signal));
    js_sys::Reflect::set(&init, &"priority".into(), &"low".into())?;
    let response: web_sys::Response = JsFuture::from(window().fetch_with_str_and_init(src, &init)).await?.dyn_into()?;
    let is_html = response.headers().get("Content-Type")?.is_some_and(|t| t.to_ascii_lowercase().contains("text/html"));
    if !response.ok() || !is_html {
        if let Some(body) = response.body() {
            let _ = body.cancel();
        }
        return Ok(());
    }
    let html = JsFuture::from(response.text()?).await?.as_string().unwrap_or_default();
    if signal.aborted() {
        return Ok(());
    }
    // Template contents stay inert, including scripts, styles and nested frame templates.
    let template: web_sys::HtmlTemplateElement = document().create_element("template")?.unchecked_into();
    template.set_inner_html(&html);
    let response_url = Some(response.url()).filter(|u| !u.is_empty()).unwrap_or_else(|| src.to_string());
    let base = template.content().query_selector("base[href]")?.and_then(|b| b.get_attribute("href"));
    let base_url = base.and_then(|b| web_sys::Url::new_with_base(&b, &response_url).ok()).map_or(response_url, |u| u.href());
    let mut active = HashSet::new();
    let current = document().query_selector_all("link[href], script[src]")?;
    for i in 0..current.length() {
        let Some(el) = current.item(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else { continue };
        if let Some((key, _)) = asset_prefetch(&el, &el.base_uri().ok().flatten().unwrap_or_default()) {
            active.insert(key);
        }
    }
    discover_assets(&template.content(), &base_url, &active);
    Ok(())
}

fn discover_assets(source: &web_sys::DocumentFragment, base_url: &str, active: &HashSet<String>) {
    let Ok(found) = source.query_selector_all("link[href], script[src], template") else { return };
    for i in 0..found.length() {
        let Some(el) = found.item(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else { continue };
        if let Some(template) = el.dyn_ref::<web_sys::HtmlTemplateElement>() {
            discover_assets(&template.content(), base_url, active);
        } else if let Some((key, link)) = asset_prefetch(&el, base_url) {
            add_hint(key, link, active);
        }
    }
}

fn add_hint(key: String, link: web_sys::Element, active: &HashSet<String>) {
    let pending = STATE.with(|s| s.borrow().hints.iter().any(|(k, _)| *k == key));
    let supported = call(&get(&link, "relList"), "supports", &[&"prefetch".into()]).ok().and_then(|v| v.as_bool()) == Some(true);
    if pending || active.contains(&key) || !supported {
        return;
    }
    let finished = Rc::new(Cell::new(false));
    for kind in ["load", "error"] {
        let (link_for_finish, finished) = (link.clone(), finished.clone());
        let finish = Closure::once_into_js(move || {
            if finished.replace(true) {
                return;
            }
            link_for_finish.remove();
            STATE.with(|s| s.borrow_mut().hints.retain(|(_, l)| !l.is_same_node(Some(&link_for_finish))));
        });
        let _ = link.add_event_listener_with_callback(kind, finish.unchecked_ref());
    }
    STATE.with(|s| s.borrow_mut().hints.push((key, link.clone())));
    if let Some(head) = document().head() {
        let _ = head.append_child(&link);
    }
}

fn rel_has(el: &web_sys::Element, token: &str) -> bool {
    el.get_attribute("rel").is_some_and(|rel| rel.split_ascii_whitespace().any(|t| t.eq_ignore_ascii_case(token)))
}

/// A `<link rel="prefetch">` hint (and its dedupe key) for an asset `el` declares.
fn asset_prefetch(el: &web_sys::Element, base_url: &str) -> Option<(String, web_sys::Element)> {
    let tag = el.local_name();
    let attr_lower = |name: &str| el.get_attribute(name).map(|v| v.trim().to_ascii_lowercase()).unwrap_or_default();
    let (url_attr, is_module, as_value) = match tag.as_str() {
        "link" => {
            let as_attr = attr_lower("as");
            let preload = rel_has(el, "preload") && ["script", "style", "image", "font"].contains(&as_attr.as_str());
            if !rel_has(el, "modulepreload") && !rel_has(el, "stylesheet") && !preload {
                return None;
            }
            // Prefetch hints cannot select a responsive candidate from imagesrcset.
            if as_attr == "image" && el.get_attribute("imagesrcset").is_some_and(|s| !s.is_empty()) {
                return None;
            }
            let media = el.get_attribute("media").unwrap_or_default();
            if (as_attr == "image" || as_attr == "font") && !media.is_empty() && !media_matches(&media) {
                return None;
            }
            let is_module = rel_has(el, "modulepreload");
            let as_value = if is_module {
                "script".to_string()
            } else if rel_has(el, "stylesheet") {
                "style".to_string()
            } else {
                as_attr
            };
            ("href", is_module, as_value)
        }
        "script" => {
            let kind = attr_lower("type");
            if !["", "module", "text/javascript", "application/javascript"].contains(&kind.as_str()) {
                return None;
            }
            ("src", kind == "module", "script".to_string())
        }
        _ => return None,
    };
    let href = el.get_attribute(url_attr).filter(|h| !h.is_empty())?;
    let url = web_sys::Url::new_with_base(&href, base_url).ok()?;
    if url.protocol() != "http:" && url.protocol() != "https:" {
        return None;
    }
    let link = document().create_element("link").ok()?;
    // Ordinary prefetches don't evaluate modules or resolve imports in this page's import map.
    let _ = link.set_attribute("rel", "prefetch");
    let _ = link.set_attribute("fetchpriority", "low");
    let _ = link.set_attribute("as", &as_value);
    let _ = link.set_attribute("href", &url.href());
    for name in ["integrity", "referrerpolicy"] {
        if let Some(value) = el.get_attribute(name) {
            let _ = link.set_attribute(name, &value);
        }
    }
    let cross_origin = (is_module || el.has_attribute("crossorigin")).then(|| if attr_lower("crossorigin") == "use-credentials" { "use-credentials" } else { "anonymous" });
    if let Some(mode) = cross_origin {
        let _ = link.set_attribute("crossorigin", mode);
    }
    let key = [url.href(), as_value, cross_origin.unwrap_or_default().to_string(), el.get_attribute("integrity").unwrap_or_default(), el.get_attribute("referrerpolicy").unwrap_or_default()].join("\n");
    Some((key, link))
}

fn media_matches(media: &str) -> bool {
    call(&window().into(), "matchMedia", &[&media.into()]).ok().is_some_and(|list| get(&list, "matches").as_bool() == Some(true))
}
