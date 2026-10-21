//! Frames in the browser: the top frame (the document) and every `<Frame>` region.
//!
//! A frame region is the DOM between `<!--rmx:f:ID-->` and `<!--/rmx:f-->`. Reloading a
//! frame fetches its `src` with `X-Remix-Frame: true` and reconciles the returned HTML into
//! the region ([`super::diff`]), keeping hydrated client entries and nested frames that are
//! still present. Non-blocking frames arrive later as `<template id="ID">` elements, which
//! are applied as soon as they are complete.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use super::root::Root;
use super::{diff, document, entries, js, styles, warn, window};
use crate::frame::{FrameContent, FrameRequest, ReloadOptions, ResolveFrame};
use crate::node::Node;
use crate::ssr::{HydrationData, TEMPLATE_END};

// ---------------------------------------------------------------------------------------
// Frame state
// ---------------------------------------------------------------------------------------

pub(crate) struct FrameState {
    src: RefCell<String>,
    name: RefCell<Option<String>>,
    /// Start and end comments; `None` for the top frame (the whole document).
    range: Option<(web_sys::Comment, web_sys::Comment)>,
    reload: RefCell<Option<web_sys::AbortController>>,
    disposed: Cell<bool>,
    /// Showing fallback content while waiting for a streamed template.
    pending: Cell<bool>,
    /// `reloadStart` / `reloadComplete` listeners.
    events: web_sys::EventTarget,
    /// Renders node content from a custom resolver (or `replace`) into the region.
    root: RefCell<Option<Root>>,
    /// Loads of blocking frames created while applying this frame's current load.
    tracker: RefCell<Option<Tracker>>,
    /// Reloading because a frame it is in reloads (Remix's inherited reload): it sent
    /// `reloadStart` and sends `reloadComplete` once its new content is in the page.
    inherited: Cell<bool>,
    /// A load was spawned and has not started yet; it finishes the inherited reload.
    queued: Cell<bool>,
}

/// Loads a frame load waits for before it finishes (Remix's reconciliation tracker): a
/// `<Frame>` without a fallback that a client render creates, or points at a new `src`,
/// while new content is applied. A navigation then finishes (and the browser restores or
/// resets scroll) only once that content is in the page.
type Tracker = Rc<RefCell<Vec<js_sys::Promise>>>;

thread_local! {
    /// Trackers of the frame loads whose content is being applied right now (innermost last).
    static APPLYING: RefCell<Vec<Tracker>> = const { RefCell::new(Vec::new()) };
}

thread_local! {
    /// `RunOptions::resolve_frame`: when set, frame loads go through it instead of `fetch`.
    static RESOLVER: RefCell<Option<ResolveFrame>> = const { RefCell::new(None) };
}

pub(crate) fn set_resolver(resolver: Option<ResolveFrame>) {
    RESOLVER.with(|r| *r.borrow_mut() = resolver);
}

/// A request for new frame content: a reload, a link navigation or a form submission.
#[derive(Default)]
pub(crate) struct Submission {
    /// `POST` for a form submission; `None` (GET) otherwise.
    pub(crate) method: Option<String>,
    pub(crate) form_data: Option<web_sys::FormData>,
    /// `multipart/form-data`, `text/plain` or (anything else) URL encoding.
    pub(crate) enc_type: Option<String>,
}

impl Submission {
    fn is_get(&self) -> bool {
        self.method.as_deref().is_none_or(|m| m.eq_ignore_ascii_case("get") || m.eq_ignore_ascii_case("head"))
    }

    /// The normalized encoding of a body-carrying submission (Remix `getRequestBody`).
    fn enc_type(&self) -> Option<&'static str> {
        if self.form_data.is_none() || self.is_get() {
            return None;
        }
        Some(match self.enc_type.as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("multipart/form-data") => "multipart/form-data",
            Some("text/plain") => "text/plain",
            _ => "application/x-www-form-urlencoded",
        })
    }

    /// The request body and its `Content-Type` (`None` lets fetch set the multipart
    /// boundary). Files contribute their names outside multipart bodies.
    fn body(&self) -> Option<(JsValue, Option<&'static str>)> {
        let enc_type = self.enc_type()?;
        let form_data = self.form_data.as_ref()?;
        Some(match enc_type {
            "multipart/form-data" => (form_data.clone().into(), None),
            "text/plain" => {
                let body: String = entries(form_data).iter().map(|(k, v)| format!("{}={}\r\n", crlf(k), crlf(v))).collect();
                (JsValue::from_str(&body), Some("text/plain"))
            }
            _ => (url_encoded(&entries(form_data)).into(), Some(enc_type)),
        })
    }
}

/// The document base URL that relative frame sources resolve against.
fn base_url() -> String {
    document().base_uri().ok().flatten().or_else(|| window().location().href().ok()).unwrap_or_default()
}

/// Line breaks as CRLF, as browsers encode form entries.
fn crlf(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n")
}

fn url_encoded(fields: &[(String, String)]) -> web_sys::UrlSearchParams {
    let params = web_sys::UrlSearchParams::new().expect("URLSearchParams");
    for (k, v) in fields {
        params.append(&crlf(k), &crlf(v));
    }
    params
}

impl FrameState {
    pub(crate) fn src(&self) -> String {
        self.src.borrow().clone()
    }

    pub(crate) fn set_src(&self, src: &str) {
        *self.src.borrow_mut() = src.to_string();
    }

    pub(crate) fn name(&self) -> Option<String> {
        self.name.borrow().clone()
    }

    pub(crate) fn set_name(&self, name: Option<String>) {
        *self.name.borrow_mut() = name;
    }

    /// Whether the frame shows fallback content (its streamed content has not arrived).
    pub(crate) fn is_pending(&self) -> bool {
        self.pending.get()
    }

    pub(crate) fn is_top(&self) -> bool {
        self.range.is_none()
    }

    pub(crate) fn start(&self) -> web_sys::Comment {
        self.range.as_ref().expect("frame region").0.clone()
    }

    pub(crate) fn end(&self) -> web_sys::Comment {
        self.range.as_ref().expect("frame region").1.clone()
    }

    fn new(src: String, name: Option<String>, range: Option<(web_sys::Comment, web_sys::Comment)>) -> FrameState {
        FrameState {
            src: RefCell::new(src),
            name: RefCell::new(name),
            range,
            reload: RefCell::new(None),
            disposed: Cell::new(false),
            pending: Cell::new(false),
            events: web_sys::EventTarget::new().expect("EventTarget"),
            root: RefCell::new(None),
            tracker: RefCell::new(None),
            inherited: Cell::new(false),
            queued: Cell::new(false),
        }
    }

    /// The live frames directly inside this frame's content (frames inside them excluded).
    fn child_frames(&self) -> Vec<Rc<FrameState>> {
        let doc = document();
        let (root, first, stop): (web_sys::Node, Option<web_sys::Node>, Option<web_sys::Node>) = match &self.range {
            Some((start, end)) => match start.parent_node() {
                Some(parent) => (parent, Some(start.clone().into()), Some(end.clone().into())),
                None => return Vec::new(),
            },
            None => match doc.body() {
                Some(body) => (body.into(), None, None),
                None => return Vec::new(),
            },
        };
        let Ok(walker) = doc.create_tree_walker_with_what_to_show(&root, 0x80 /* SHOW_COMMENT */) else { return Vec::new() };
        if let Some(first) = &first {
            walker.set_current_node(first);
        }
        let mut out = Vec::new();
        while let Ok(Some(node)) = walker.next_node() {
            if stop.as_ref().is_some_and(|s| s.is_same_node(Some(&node))) {
                break;
            }
            if let Some(child) = is_start(&node).then(|| live(&node)).flatten() {
                if let Some(end) = find_end(&node) {
                    walker.set_current_node(&end);
                }
                out.push(child);
            }
        }
        out
    }

    /// An ancestor started reloading: send `reloadStart` once, here and further down.
    fn start_inherited(&self) {
        if self.inherited.replace(true) || self.disposed.get() {
            return;
        }
        self.emit("reloadStart");
        for child in self.child_frames() {
            child.start_inherited();
        }
    }

    /// The new content of an inherited reload is in the page: send `reloadComplete`, and
    /// finish frames inside that are not waiting for a streamed template.
    fn complete_inherited(&self) {
        if self.queued.get() || !self.inherited.replace(false) || self.disposed.get() {
            return;
        }
        for child in self.child_frames() {
            if !child.is_pending() {
                child.complete_inherited();
            }
        }
        self.emit("reloadComplete");
    }

    /// Call `handler` for `event` until `signal` aborts.
    pub(crate) fn add_listener(&self, event: &str, handler: crate::node::EventHandler, signal: &web_sys::AbortSignal) {
        let callback = Closure::<dyn Fn(web_sys::Event)>::new(move |raw: web_sys::Event| {
            (handler.0)(&crate::node::Event { raw, current: None, listener: None });
        })
        .into_js_value();
        let options = web_sys::AddEventListenerOptions::new();
        options.set_signal(signal);
        let _ = self.events.add_event_listener_with_callback_and_add_event_listener_options(
            event,
            callback.unchecked_ref(),
            &options,
        );
    }

    fn emit(&self, event: &str) {
        if let Ok(e) = web_sys::Event::new(event) {
            let _ = self.events.dispatch_event(&e);
        }
    }

    /// `FrameHandle::reload_with`: optional new src, method, encoding and form fields.
    /// Methods other than POST are GET, which puts the fields in the query.
    pub(crate) async fn reload(self: Rc<Self>, options: ReloadOptions) {
        if let Some(src) = &options.src {
            self.set_src(src);
        }
        let post = options.method.as_deref().is_some_and(|m| m.eq_ignore_ascii_case("post"));
        let mut submission = Submission::default();
        if let Some(fields) = &options.body {
            if post {
                let form_data = web_sys::FormData::new().expect("FormData");
                for (k, v) in fields {
                    let _ = form_data.append_with_str(k, v);
                }
                submission.form_data = Some(form_data);
                submission.enc_type = options.enc_type.clone();
            } else if let Ok(url) = web_sys::Url::new_with_base(&self.src(), &base_url()) {
                url.set_search(&url_encoded(fields).to_string().as_string().unwrap_or_default());
                self.set_src(&url.href());
            }
        }
        if post {
            submission.method = Some("POST".into());
        }
        if let Err(e) = self.load(submission).await {
            crate::report_error(js_error(&e));
        }
    }

    /// Fetch the frame's `src` and reconcile the response into the page. Resolves to the
    /// final URL when the server redirected. A newer load of the same frame aborts this one.
    pub(crate) async fn load(self: &Rc<Self>, submission: Submission) -> Result<Option<String>, JsValue> {
        self.queued.set(false);
        if self.disposed.get() {
            return Ok(None);
        }
        let controller = web_sys::AbortController::new()?;
        if let Some(previous) = self.reload.replace(Some(controller.clone())) {
            previous.abort();
        }
        let signal = controller.signal();
        // A load that takes over an inherited reload reuses its `reloadStart`.
        if !self.inherited.replace(false) {
            self.emit("reloadStart");
        }
        for child in self.child_frames() {
            child.start_inherited();
        }
        let tracker = Tracker::default();
        *self.tracker.borrow_mut() = Some(tracker.clone());
        let mut result = self.fetch_and_apply(submission, &signal).await;
        if result.is_ok() {
            // Blocking frames the new content created; their own loads wait for theirs.
            loop {
                let loads = std::mem::take(&mut *tracker.borrow_mut());
                if loads.is_empty() || signal.aborted() {
                    break;
                }
                if let Err(e) = JsFuture::from(js_sys::Promise::all(&loads.into_iter().collect::<js_sys::Array>())).await {
                    result = Err(e);
                    break;
                }
            }
        }
        if self.tracker.borrow().as_ref().is_some_and(|t| Rc::ptr_eq(t, &tracker)) {
            self.tracker.replace(None);
        }
        if signal.aborted() {
            // Superseded by a newer load of this frame (or the frame went away).
            return Ok(None);
        }
        self.reload.replace(None);
        // Frames inside that got their content with this load are done; ones waiting for a
        // streamed template finish when it arrives.
        for child in self.child_frames() {
            if !child.is_pending() {
                child.complete_inherited();
            }
        }
        self.emit("reloadComplete");
        result
    }

    async fn fetch_and_apply(self: &Rc<Self>, submission: Submission, signal: &web_sys::AbortSignal) -> Result<Option<String>, JsValue> {
        if let Some(resolve) = RESOLVER.with(|r| r.borrow().clone()) {
            return self.resolve_and_apply(resolve, submission, signal).await;
        }

        let headers = web_sys::Headers::new()?;
        headers.set("Accept", "text/html")?;
        headers.set("X-Remix-Frame", "true")?;
        if let Some(name) = self.name() {
            headers.set("X-Remix-Target", &name)?;
        }
        let body = submission.body();
        if let Some((_, Some(ct))) = &body {
            headers.set("Content-Type", ct)?;
        }
        let init = web_sys::RequestInit::new();
        init.set_method(submission.method.as_deref().unwrap_or("GET"));
        init.set_headers(&headers);
        init.set_mode(web_sys::RequestMode::SameOrigin);
        init.set_signal(Some(signal));
        if let Some((body, _)) = &body {
            init.set_body(body);
        }
        let response: web_sys::Response = JsFuture::from(window().fetch_with_str_and_init(&self.src(), &init)).await?.dyn_into()?;
        let is_html = response.headers().get("Content-Type")?.is_some_and(|ct| ct.to_ascii_lowercase().contains("text/html"));
        let status = response.status();
        if status >= 500 || (status >= 300 && !is_html) {
            return Err(JsValue::from_str(&format!("Failed to resolve frame: {status} {}", response.status_text())));
        }
        // A redirect renders its final content; the frame keeps the source it requested.
        let redirected = response.redirected().then(|| response.url());
        // A redirect can land a named frame on a page that is not frame content (a login
        // page, say). If it sends a whole document, load it as one.
        let document_fallback = redirected.clone().filter(|_| !self.is_top());
        self.read_stream(&response, signal, document_fallback).await?;
        Ok(redirected)
    }

    /// Apply a response as it streams: the main HTML as soon as the server's flush marker
    /// arrives, then each `<template>` as it completes (the template watcher swaps it into
    /// its frame).
    async fn read_stream(
        self: &Rc<Self>,
        response: &web_sys::Response,
        signal: &web_sys::AbortSignal,
        document_fallback: Option<String>,
    ) -> Result<(), JsValue> {
        let apply = |html: &str| match &document_fallback {
            Some(url) if looks_like_document(html) => {
                let _ = window().location().assign(url);
                false
            }
            _ => {
                self.tracked(|| self.apply_html(html));
                true
            }
        };
        let Some(body) = response.body() else {
            let html = JsFuture::from(response.text()?).await?.as_string().unwrap_or_default();
            if !signal.aborted() && !self.disposed.get() {
                apply(&html);
            }
            return Ok(());
        };
        let reader = js::call(&body, "getReader", &[])?;
        let decoder: JsValue = web_sys::TextDecoder::new()?.into();
        let stream_opts = js::object(&[("stream", JsValue::TRUE)]);
        let mut buffer = String::new();
        let mut main_applied = false;
        loop {
            // A newer load (or disposal) cancels the stream right away, without waiting for
            // its next chunk.
            let read: js_sys::Promise = js::call(&reader, "read", &[])?.unchecked_into();
            let chunk = match until_aborted(JsFuture::from(read.clone()), signal).await {
                Some(chunk) if !signal.aborted() && !self.disposed.get() => chunk?,
                _ => {
                    cancel_quietly(&reader, Some(&read));
                    return Ok(());
                }
            };
            let done = js::get(&chunk, "done").as_bool().unwrap_or(true);
            if !done {
                let text = js::call(&decoder, "decode", &[&js::get(&chunk, "value"), &stream_opts])?;
                buffer.push_str(&text.as_string().unwrap_or_default());
            }
            if !main_applied {
                if let Some((start, end)) = crate::ssr::find_flush_marker(&buffer) {
                    let rest = buffer.split_off(end);
                    buffer.truncate(start);
                    if !apply(&buffer) {
                        cancel_quietly(&reader, None);
                        return Ok(());
                    }
                    buffer = rest.trim_start().to_string();
                    main_applied = true;
                } else if done {
                    apply(&buffer);
                    return Ok(());
                }
            }
            if main_applied {
                while let Some(end) = template_end(&buffer) {
                    let rest = buffer.split_off(end);
                    append_template(&buffer);
                    buffer = rest.trim_start().to_string();
                }
            }
            if done {
                return Ok(());
            }
        }
    }

    /// Load through the app's resolver. The pending resolution is dropped when a newer load
    /// aborts this one.
    async fn resolve_and_apply(
        self: &Rc<Self>,
        resolve: ResolveFrame,
        submission: Submission,
        signal: &web_sys::AbortSignal,
    ) -> Result<Option<String>, JsValue> {
        let base = base_url();
        let url = web_sys::Url::new_with_base(&self.src(), &base).map(|u| u.href()).unwrap_or_else(|_| self.src());
        let request = FrameRequest {
            url,
            method: submission.method.clone().unwrap_or_else(|| "GET".into()),
            enc_type: submission.enc_type().map(String::from),
            form: submission.form_data.as_ref().filter(|_| !submission.is_get()).map(entries),
            target: self.name(),
        };
        let Some(result) = until_aborted(resolve(request), signal).await else { return Ok(None) };
        let response = result.map_err(|e| JsValue::from_str(&e))?;
        match response.content {
            FrameContent::Html(html) => self.tracked(|| self.apply_html(&html)),
            // Node content from a load re-renders like a frame load's server content.
            FrameContent::Node(node) => self.tracked(|| server_reload(|| self.apply_node(node))),
        }
        Ok(response.url)
    }

    /// Render a node tree as the frame's content, reconciling with the previous tree so
    /// component instances that stay in place keep their state.
    pub(crate) fn apply_node(self: &Rc<Self>, node: Node) {
        let existing = self.root.borrow().clone();
        let root = match existing {
            Some(root) => root,
            None => {
                let root = match &self.range {
                    Some((start, end)) => Root::in_range(start.as_ref(), end.as_ref(), Some(self.clone())),
                    None => Root::in_container(document().body().expect("body").into(), Some(self.clone())),
                };
                *self.root.borrow_mut() = Some(root.clone());
                root
            }
        };
        root.render(node);
    }

    /// Apply content for the current load, rendering what it changed right away so the
    /// blocking frames it creates join the load's tracker.
    fn tracked(&self, apply: impl FnOnce()) {
        let Some(tracker) = self.tracker.borrow().clone() else { return apply() };
        APPLYING.with(|a| a.borrow_mut().push(tracker));
        apply();
        super::flush();
        APPLYING.with(|a| a.borrow_mut().pop());
    }

    /// Stop rendering node content (HTML content is taking over the region).
    fn release_root(&self) {
        if let Some(root) = self.root.borrow_mut().take() {
            root.release();
        }
    }

    fn apply_html(self: &Rc<Self>, html: &str) {
        self.release_root();
        if self.is_top() && looks_like_document(html) {
            self.apply_document(html);
        } else {
            let template: web_sys::HtmlTemplateElement = document().create_element("template").expect("template").unchecked_into();
            template.set_inner_html(html);
            self.apply_fragment(&template.content());
        }
    }

    /// Reconcile a fragment into this frame's region (the body, for the top frame).
    fn apply_fragment(self: &Rc<Self>, fragment: &web_sys::DocumentFragment) {
        let resolved = resolve_templates(fragment.as_ref());
        merge_data(fragment.as_ref(), true);
        mark_resolved(&resolved);
        styles::adopt_server_styles(fragment.as_ref());
        let next = child_nodes(fragment.as_ref());
        match &self.range {
            Some((start, end)) => {
                let parent = start.parent_node().expect("frame region is attached");
                diff::diff_nodes(&parent, range_nodes(start, end, false), next, Some(end.clone().into()));
            }
            None => {
                let body: web_sys::Node = document().body().expect("body").into();
                diff::diff_nodes(&body, child_nodes(&body), next, None);
            }
        }
        hydrate_region(self);
    }

    /// Reconcile a full document response into the live document (top frame only).
    fn apply_document(self: &Rc<Self>, html: &str) {
        let parser = web_sys::DomParser::new().expect("DOMParser");
        let Ok(parsed) = parser.parse_from_string(html, web_sys::SupportedType::TextHtml) else { return };
        let doc = document();
        if let (Some(body), Some(next_body)) = (doc.body(), parsed.body()) {
            let resolved = resolve_templates(next_body.as_ref());
            merge_data(parsed.as_ref(), true);
            mark_resolved(&resolved);
            styles::adopt_server_styles(parsed.as_ref());
            if let (Some(html_el), Some(next_html)) = (doc.document_element(), parsed.document_element()) {
                diff::diff_attributes(&html_el, &next_html);
            }
            if let (Some(head), Some(next_head)) = (doc.head(), parsed.head()) {
                diff::diff_head(&head, &next_head);
            }
            diff::diff_attributes(&body, &next_body);
            let body_node: web_sys::Node = body.into();
            diff::diff_nodes(&body_node, child_nodes(&body_node), child_nodes(next_body.as_ref()), None);
        }
        hydrate_region(self);
    }
}

/// Cancel a response body reader. Once an aborted fetch errors the body, its pending read
/// and the cancel both reject; those rejections are expected, so they are handled here
/// instead of reaching the console as unhandled.
fn cancel_quietly(reader: &JsValue, pending_read: Option<&js_sys::Promise>) {
    let ignore: JsValue = js_sys::Function::new_no_args("").into();
    if let Some(read) = pending_read {
        let _ = js::call(read, "catch", &[&ignore]);
    }
    if let Ok(cancelled) = js::call(reader, "cancel", &[]) {
        let _ = js::call(&cancelled, "catch", &[&ignore]);
    }
}

/// Run `fut` until it finishes or `signal` aborts. On abort the future is dropped, which
/// cancels whatever it was waiting on.
async fn until_aborted<T>(fut: impl std::future::Future<Output = T>, signal: &web_sys::AbortSignal) -> Option<T> {
    use std::future::Future;
    use std::task::Poll;
    if signal.aborted() {
        return None;
    }
    let aborted = js_sys::Promise::new(&mut |resolve, _| {
        let options = web_sys::AddEventListenerOptions::new();
        options.set_once(true);
        let _ = signal.add_event_listener_with_callback_and_add_event_listener_options("abort", &resolve, &options);
    });
    let mut aborted = JsFuture::from(aborted);
    let mut fut = std::pin::pin!(fut);
    std::future::poll_fn(|cx| {
        if let Poll::Ready(v) = fut.as_mut().poll(cx) {
            return Poll::Ready(Some(v));
        }
        if std::pin::Pin::new(&mut aborted).poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

/// The entries of a `FormData` (files contribute their name).
fn entries(body: &web_sys::FormData) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Ok(Some(iter)) = js_sys::try_iter(body) {
        for pair in iter.flatten() {
            let pair: js_sys::Array = pair.unchecked_into();
            let key = pair.get(0).as_string().unwrap_or_default();
            let value = pair.get(1);
            let value = value.as_string().unwrap_or_else(|| js::get(&value, "name").as_string().unwrap_or_default());
            out.push((key, value));
        }
    }
    out
}

/// The end of the complete `<template ...>...</template>` at the start of `html`, counting
/// nested templates.
fn template_end(html: &str) -> Option<usize> {
    if !html.starts_with("<template") {
        return None;
    }
    let lower = html.to_ascii_lowercase();
    let mut depth = 0usize;
    let mut i = 0;
    while i < lower.len() {
        let rest = &lower[i..];
        if rest.starts_with("<template") {
            depth += 1;
            i += 9;
        } else if rest.starts_with("</template>") {
            depth -= 1;
            i += 11;
            if depth == 0 {
                return Some(i);
            }
        } else {
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    None
}

/// Put a streamed `<template id>` into the document for the template watcher.
fn append_template(html: &str) {
    let doc = document();
    let holder: web_sys::HtmlTemplateElement = doc.create_element("template").expect("template").unchecked_into();
    holder.set_inner_html(html);
    if let (Some(template), Some(body)) = (holder.content().first_element_child(), doc.body()) {
        let _ = body.append_child(&template);
        check_templates();
    }
}

fn looks_like_document(html: &str) -> bool {
    let head = html.trim_start().get(..20).unwrap_or(html.trim_start()).to_ascii_lowercase();
    head.starts_with("<!doctype") || head.starts_with("<html")
}

pub(crate) fn js_error(e: &JsValue) -> String {
    e.as_string()
        .or_else(|| js_sys::Reflect::get(e, &"message".into()).ok().and_then(|m| m.as_string()))
        .unwrap_or_else(|| format!("{e:?}"))
}

// ---------------------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------------------

thread_local! {
    static TOP: RefCell<Option<Rc<FrameState>>> = const { RefCell::new(None) };
    /// Region frames, found from their start comment through an expando slot.
    static SLOTS: RefCell<Vec<Option<Rc<FrameState>>>> = const { RefCell::new(Vec::new()) };
    static KEY: JsValue = JsValue::from_str("__rmx_frame");
    static DATA: RefCell<Data> = RefCell::new(Data::default());
    /// Frames waiting for a streamed `<template id>`.
    static PENDING: RefCell<Vec<(String, Weak<FrameState>)>> = const { RefCell::new(Vec::new()) };
    static OBSERVER: RefCell<Option<web_sys::MutationObserver>> = const { RefCell::new(None) };
    static RETRY: Cell<bool> = const { Cell::new(false) };
    static NEXT_CLIENT_ID: Cell<u32> = const { Cell::new(0) };
}

/// Hydration data merged from every `rmx-data` script seen so far.
#[derive(Default)]
struct Data {
    entries: HashMap<String, (String, String)>,
    frames: HashMap<String, (String, Option<String>, bool)>,
}

/// The top frame. Created on first use with the document URL.
pub(crate) fn top() -> Rc<FrameState> {
    TOP.with(|t| {
        t.borrow_mut()
            .get_or_insert_with(|| {
                Rc::new(FrameState::new(window().location().href().unwrap_or_default(), None, None))
            })
            .clone()
    })
}

/// The most recently mounted frame with this name.
pub(crate) fn by_name(name: &str) -> Option<Rc<FrameState>> {
    SLOTS.with(|s| {
        s.borrow().iter().rev().flatten().find(|f| !f.disposed.get() && f.name().as_deref() == Some(name)).cloned()
    })
}

fn slot_of(start: &web_sys::Node) -> Option<usize> {
    KEY.with(|k| js_sys::Reflect::get(start, k).ok()?.as_f64()).map(|v| v as usize)
}

/// The live frame whose region starts at `start`.
pub(crate) fn live(start: &web_sys::Node) -> Option<Rc<FrameState>> {
    let slot = slot_of(start)?;
    SLOTS.with(|s| s.borrow().get(slot).cloned().flatten())
}

fn register(state: FrameState) -> Rc<FrameState> {
    let state = Rc::new(state);
    let slot = SLOTS.with(|s| {
        let mut slots = s.borrow_mut();
        match slots.iter().position(Option::is_none) {
            Some(i) => {
                slots[i] = Some(state.clone());
                i
            }
            None => {
                slots.push(Some(state.clone()));
                slots.len() - 1
            }
        }
    });
    let start: web_sys::Node = state.start().into();
    KEY.with(|k| js_sys::Reflect::set(&start, k, &JsValue::from(slot as u32)).ok());
    state
}

/// Forget the frame whose region starts at `start` (its DOM is left to the caller).
impl FrameState {
    /// Cancel an in-flight load, if any.
    pub(crate) fn abort_reload(&self) {
        if let Some(c) = self.reload.take() {
            c.abort();
        }
    }
}

pub(crate) fn dispose(start: &web_sys::Node) {
    let Some(slot) = slot_of(start) else { return };
    KEY.with(|k| js_sys::Reflect::delete_property(start.unchecked_ref::<js_sys::Object>(), k).ok());
    if let Some(frame) = SLOTS.with(|s| s.borrow_mut().get_mut(slot).and_then(Option::take)) {
        frame.disposed.set(true);
        frame.release_root();
        if let Some(c) = frame.reload.take() {
            c.abort();
        }
    }
}

// ---------------------------------------------------------------------------------------
// Hydration data
// ---------------------------------------------------------------------------------------

/// Merge every `<script id="rmx-data">` under `root`, removing the scripts when asked.
fn merge_data(root: &web_sys::Node, remove: bool) {
    for script in query_all(root, r#"script[type="application/json"][id="rmx-data"]"#) {
        let Some(data) = HydrationData::from_json(&script.text_content().unwrap_or_default()) else {
            warn("rmx: bad rmx-data");
            continue;
        };
        DATA.with(|d| {
            let mut d = d.borrow_mut();
            for (id, name, props) in data.entries {
                d.entries.insert(id, (name, props));
            }
            for (id, src, name, status) in data.frames {
                d.frames.insert(id, (src, name, status == "pending"));
            }
        });
        if remove {
            script.remove();
        }
    }
}

fn mark_resolved(ids: &[String]) {
    DATA.with(|d| {
        let mut d = d.borrow_mut();
        for id in ids {
            if let Some(f) = d.frames.get_mut(id) {
                f.2 = false;
            }
        }
    });
}

/// `(name, props JSON)` for a client entry marker id.
pub(crate) fn entry_data(id: &str) -> Option<(String, String)> {
    DATA.with(|d| d.borrow().entries.get(id).cloned())
}

/// `(src, name, pending)` for a frame marker id.
pub(crate) fn frame_data(id: &str) -> Option<(String, Option<String>, bool)> {
    DATA.with(|d| d.borrow().frames.get(id).cloned())
}

pub(crate) fn query_all(root: &web_sys::Node, selector: &str) -> Vec<web_sys::Element> {
    let list = if let Some(d) = root.dyn_ref::<web_sys::Document>() {
        d.query_selector_all(selector)
    } else if let Some(e) = root.dyn_ref::<web_sys::Element>() {
        e.query_selector_all(selector)
    } else if let Some(f) = root.dyn_ref::<web_sys::DocumentFragment>() {
        f.query_selector_all(selector)
    } else {
        return Vec::new();
    };
    let Ok(list) = list else { return Vec::new() };
    (0..list.length()).filter_map(|i| list.item(i)?.dyn_into().ok()).collect()
}

pub(crate) fn child_nodes(parent: &web_sys::Node) -> Vec<web_sys::Node> {
    let list = parent.child_nodes();
    (0..list.length()).filter_map(|i| list.item(i)).collect()
}

/// The nodes between `start` and `end` (siblings), optionally including both.
pub(crate) fn range_nodes(start: &web_sys::Comment, end: &web_sys::Comment, inclusive: bool) -> Vec<web_sys::Node> {
    let end: &web_sys::Node = end.as_ref();
    let mut out = Vec::new();
    if inclusive {
        out.push(start.clone().into());
    }
    let mut node = start.next_sibling();
    while let Some(n) = node {
        if n.is_same_node(Some(end)) {
            break;
        }
        node = n.next_sibling();
        out.push(n);
    }
    if inclusive {
        out.push(end.clone());
    }
    out
}

pub(crate) fn is_start(node: &web_sys::Node) -> bool {
    node.dyn_ref::<web_sys::Comment>().is_some_and(|c| c.data().starts_with("rmx:f:"))
}

pub(crate) fn is_end(node: &web_sys::Node) -> bool {
    node.dyn_ref::<web_sys::Comment>().is_some_and(|c| c.data() == "/rmx:f")
}

/// The matching `<!--/rmx:f-->` after `start`.
pub(crate) fn find_end(start: &web_sys::Node) -> Option<web_sys::Node> {
    let mut depth = 1;
    let mut node = start.next_sibling();
    while let Some(n) = node {
        if is_start(&n) {
            depth += 1;
        } else if is_end(&n) {
            depth -= 1;
            if depth == 0 {
                return Some(n);
            }
        }
        node = n.next_sibling();
    }
    None
}

// ---------------------------------------------------------------------------------------
// Startup and region hydration
// ---------------------------------------------------------------------------------------

/// Start the top frame: read the page's hydration data and hydrate the document.
pub(crate) fn start() {
    let top = top();
    merge_data(document().as_ref(), false);
    hydrate_region(&top);
    check_templates();
}

/// Hydrate new client entries, update live ones, and start new frames in a frame's
/// region. Entries own their DOM, so their insides are skipped.
pub(crate) fn hydrate_region(frame: &Rc<FrameState>) {
    let doc = document();
    let (root, first, stop): (web_sys::Node, Option<web_sys::Node>, Option<web_sys::Node>) = match &frame.range {
        Some((start, end)) => match start.parent_node() {
            Some(parent) => (parent, Some(start.clone().into()), Some(end.clone().into())),
            None => return,
        },
        None => match doc.body() {
            Some(body) => (body.into(), None, None),
            None => return,
        },
    };
    let Ok(walker) = doc.create_tree_walker_with_what_to_show(&root, 0x80 /* SHOW_COMMENT */) else { return };
    if let Some(first) = &first {
        walker.set_current_node(first);
    }
    // Frames we are inside of, with their end comments.
    let mut stack: Vec<(Rc<FrameState>, web_sys::Node)> = Vec::new();
    while let Ok(Some(node)) = walker.next_node() {
        if stop.as_ref().is_some_and(|s| s.is_same_node(Some(&node))) {
            break;
        }
        let current = stack.last().map_or_else(|| frame.clone(), |(f, _)| f.clone());
        if entries::is_start(&node) {
            let comment: web_sys::Comment = node.unchecked_into();
            if let Some(end) = entries::hydrate_or_update(&comment, Some(current)) {
                walker.set_current_node(&end);
            }
        } else if is_start(&node) {
            if let Some(child) = adopt_server_frame(node.unchecked_ref(), Some(current)) {
                stack.push((child.clone(), child.end().into()));
            }
        } else if is_end(&node) && stack.last().is_some_and(|(_, end)| end.is_same_node(Some(&node))) {
            stack.pop();
        }
    }
}

/// The frame for a server-rendered region starting at `start`: the live one if this
/// comment already belongs to a frame, otherwise a new frame from the hydration data.
pub(crate) fn adopt_server_frame(start: &web_sys::Comment, _parent: Option<Rc<FrameState>>) -> Option<Rc<FrameState>> {
    let start_node: &web_sys::Node = start.as_ref();
    let id = start.data().strip_prefix("rmx:f:")?.to_string();
    let data = frame_data(&id);
    if let Some(frame) = live(start_node) {
        if let Some((_, _, true)) = data {
            watch(id, &frame);
        }
        return Some(frame);
    }
    let end: web_sys::Comment = find_end(start_node)?.unchecked_into();
    let (src, name, pending) = data.unwrap_or_default();
    let frame = register(FrameState::new(src, name, Some((start.clone(), end))));
    if pending {
        watch(id, &frame);
    }
    Some(frame)
}

/// A `<Frame>` created by a client component: a detached region holding the fallback,
/// loaded from the server once it is in the document.
pub(crate) fn create_client_frame(
    src: &str,
    name: Option<String>,
    _parent: Option<Rc<FrameState>>,
    fallback_html: &str,
) -> Rc<FrameState> {
    let doc = document();
    let id = NEXT_CLIENT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    let start = doc.create_comment(&format!("rmx:f:c{id}"));
    let end = doc.create_comment("/rmx:f");
    let fragment = doc.create_document_fragment();
    let _ = fragment.append_child(&start);
    if !fallback_html.is_empty() {
        let template: web_sys::HtmlTemplateElement = doc.create_element("template").expect("template").unchecked_into();
        template.set_inner_html(fallback_html);
        let _ = fragment.append_child(&template.content());
    }
    let _ = fragment.append_child(&end);
    let blocking = fallback_html.is_empty();
    let frame = register(FrameState::new(src.to_string(), name, Some((start, end))));
    spawn_reload(frame.clone(), blocking);
    frame
}

thread_local! {
    /// Depth of renders driven by a frame load (Remix's `serverFrameReload`).
    static SERVER_RELOAD: Cell<u32> = const { Cell::new(0) };
}

/// Run `f` as a render driven by a frame load: a client entry getting new server props, or
/// a frame's node content being replaced. Client frames it re-renders reload with it.
pub(crate) fn server_reload<T>(f: impl FnOnce() -> T) -> T {
    SERVER_RELOAD.with(|d| d.set(d.get() + 1));
    let out = f();
    SERVER_RELOAD.with(|d| d.set(d.get() - 1));
    out
}

/// Whether a render driven by a frame load is running (see [`server_reload`]).
pub(crate) fn in_server_reload() -> bool {
    SERVER_RELOAD.with(|d| d.get() > 0)
}

/// Load a frame in the background. A blocking frame (no fallback) created or retargeted
/// while a frame load applies its content joins that load's tracker.
pub(crate) fn spawn_reload(frame: Rc<FrameState>, blocking: bool) {
    frame.queued.set(true);
    let tracker = blocking.then(|| APPLYING.with(|a| a.borrow().last().cloned())).flatten();
    let mut done: Option<js_sys::Function> = None;
    if let Some(tracker) = &tracker {
        let promise = js_sys::Promise::new(&mut |resolve, _| done = Some(resolve));
        tracker.borrow_mut().push(promise);
    }
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = frame.load(Submission::default()).await {
            crate::report_error(js_error(&e));
        }
        if let Some(done) = done {
            let _ = done.call0(&JsValue::UNDEFINED);
        }
    });
}

// ---------------------------------------------------------------------------------------
// Streamed templates
// ---------------------------------------------------------------------------------------

fn template_content(template: &web_sys::HtmlTemplateElement) -> Option<web_sys::DocumentFragment> {
    let content = template.content();
    let last = content.last_child()?;
    let complete = last.dyn_ref::<web_sys::Comment>().is_some_and(|c| c.data() == TEMPLATE_END);
    if !complete {
        return None;
    }
    let _ = content.remove_child(&last);
    Some(content)
}

/// Apply every complete `<template id>` under `root` to the frame region with that id
/// (also under `root`). Returns the ids it resolved.
fn resolve_templates(root: &web_sys::Node) -> Vec<String> {
    let mut resolved = Vec::new();
    // Template content can hold more templates; repeat until nothing changes.
    loop {
        let mut progressed = false;
        for template in query_all(root, "template[id]") {
            let id = template.id();
            let Some(start) = find_comment(root, &format!("rmx:f:{id}")) else { continue };
            let template: web_sys::HtmlTemplateElement = template.unchecked_into();
            let Some(content) = template_content(&template) else { continue };
            let Some(end) = find_end(&start) else { continue };
            let Some(parent) = start.parent_node() else { continue };
            for node in range_nodes(start.unchecked_ref(), end.unchecked_ref(), false) {
                let _ = parent.remove_child(&node);
            }
            let _ = parent.insert_before(&content, Some(&end));
            template.remove();
            resolved.push(id);
            progressed = true;
        }
        if !progressed {
            return resolved;
        }
    }
}

fn find_comment(root: &web_sys::Node, data: &str) -> Option<web_sys::Node> {
    let doc = root.owner_document().unwrap_or_else(document);
    let walker = doc.create_tree_walker_with_what_to_show(root, 0x80).ok()?;
    while let Ok(Some(node)) = walker.next_node() {
        if node.dyn_ref::<web_sys::Comment>().is_some_and(|c| c.data() == data) {
            return Some(node);
        }
    }
    None
}

/// Wait for the `<template id>` holding a non-blocking frame's content. A frame waits for
/// one template: the one for its latest marker, so a late template from content that a
/// reload replaced is ignored.
fn watch(id: String, frame: &Rc<FrameState>) {
    frame.pending.set(true);
    PENDING.with(|p| {
        let mut p = p.borrow_mut();
        p.retain(|(_, f)| !f.upgrade().is_some_and(|f| Rc::ptr_eq(&f, frame)));
        p.push((id, Rc::downgrade(frame)));
    });
    OBSERVER.with(|o| {
        let mut o = o.borrow_mut();
        if o.is_some() {
            return;
        }
        let callback = Closure::<dyn Fn()>::new(check_templates).into_js_value();
        let Ok(observer) = web_sys::MutationObserver::new(callback.unchecked_ref()) else { return };
        let init = web_sys::MutationObserverInit::new();
        init.set_child_list(true);
        init.set_subtree(true);
        if observer.observe_with_options(&document(), &init).is_ok() {
            *o = Some(observer);
        }
    });
    super::queue_task(Box::new(check_templates));
}

/// Apply the templates that have arrived for pending frames.
fn check_templates() {
    let doc = document();
    let pending = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
    let mut waiting = Vec::new();
    for (id, frame) in pending {
        let Some(frame) = frame.upgrade().filter(|f| !f.disposed.get()) else { continue };
        let Some(template) = doc.get_element_by_id(&id) else {
            waiting.push((id, Rc::downgrade(&frame)));
            continue;
        };
        let template: web_sys::HtmlTemplateElement = template.unchecked_into();
        let Some(content) = template_content(&template) else {
            // Still streaming in; look again shortly.
            waiting.push((id, Rc::downgrade(&frame)));
            schedule_retry();
            continue;
        };
        template.remove();
        mark_resolved(std::slice::from_ref(&id));
        frame.pending.set(false);
        frame.apply_fragment(&content);
        frame.complete_inherited();
    }
    let empty = PENDING.with(|p| {
        let mut p = p.borrow_mut();
        p.extend(waiting);
        p.is_empty()
    });
    if empty {
        if let Some(o) = OBSERVER.with(|o| o.borrow_mut().take()) {
            o.disconnect();
        }
    }
}

fn schedule_retry() {
    if RETRY.replace(true) {
        return;
    }
    let callback = Closure::once_into_js(|| {
        RETRY.set(false);
        check_templates();
    });
    let _ = window().set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), 16);
}
