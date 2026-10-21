//! Ports of the child frame cases of Remix's `frame.test.tsx`: what happens to frames
//! inside a frame that reloads. Remix reloads the top frame (the document) or a server
//! frame; here the parent is a named client-created `<Frame>`, since a top frame reload
//! would replace the test runner's page. The parent loads through `fetch`, stubbed with
//! routes that stream their chunks and can hold one back until the test releases it. The
//! responses are written the way `rmx-render` streams them: frame regions, the
//! `rmx-data` script, the flush marker, then `<template>`s. Remix's client entries that
//! trigger the reloads are replaced by frame handles from `Frames.get(name)`. Run with
//! `pwsh ./test-browser.ps1 -Test browser_frame_reloads`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rmx::ssr::{flush_marker, HydrationData, TEMPLATE_END};
use rmx::{create_root, rsx, Component, Frame, FrameEvent, FrameHandle, Frames, Handle, Node, ReloadOptions, Root, RunOptions};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

thread_local! {
    static APP: RefCell<Option<rmx::App>> = const { RefCell::new(None) };
    static SERVER: RefCell<Option<Server>> = const { RefCell::new(None) };
    static STARTS: Cell<u32> = const { Cell::new(0) };
    static COMPLETES: Cell<u32> = const { Cell::new(0) };
}

/// Start the runtime once for the page, with `fetch` stubbed.
async fn start() -> Server {
    let server = SERVER.with(|s| s.borrow_mut().get_or_insert_with(Server::new).clone());
    if APP.with(|a| a.borrow().is_none()) {
        APP.with(|a| *a.borrow_mut() = Some(rmx::run(RunOptions::default())));
    }
    let app = APP.with(|a| a.borrow_mut().take()).expect("app");
    app.ready().await;
    APP.with(|a| *a.borrow_mut() = Some(app));
    server
}

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn get(target: &JsValue, key: &str) -> JsValue {
    js_sys::Reflect::get(target, &key.into()).unwrap_or(JsValue::UNDEFINED)
}

async fn tick() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tick().await;
    }
    panic!("timed out waiting for {what}");
}

/// A response part: HTML sent right away, or HTML the test releases later.
enum Part<'a> {
    Html(String),
    Gate(&'a str),
}

/// `window.fetch` answering by pathname with a stream of the route's parts.
#[derive(Clone)]
struct Server {
    stub: JsValue,
}

impl Server {
    fn new() -> Server {
        let make = js_sys::Function::new_no_args(
            "const routes = {}; const gates = {}; const calls = []; const cancels = [];
             const gate = (name) => {
               if (!gates[name]) { let release; const promise = new Promise((r) => { release = r }); gates[name] = { promise, release } }
               return gates[name];
             };
             window.fetch = async (input) => {
               const url = new URL(typeof input === 'string' ? input : input.url, document.baseURI);
               calls.push(url.pathname);
               const parts = routes[url.pathname] || [];
               const encoder = new TextEncoder();
               const body = new ReadableStream({
                 async start(controller) {
                   for (const part of parts) {
                     const html = typeof part === 'string' ? part : await gate(part.gate).promise;
                     try { controller.enqueue(encoder.encode(html)) } catch { return }
                   }
                   try { controller.close() } catch {}
                 },
                 cancel() { cancels.push(url.pathname) },
               });
               return new Response(body, { status: 200, headers: { 'Content-Type': 'text/html' } });
             };
             return {
               calls,
               cancels,
               route(path, parts) { routes[path] = parts },
               release(name, html) { gate(name).release(html) },
             };",
        );
        Server { stub: make.call0(&JsValue::NULL).unwrap() }
    }

    fn route(&self, path: &str, parts: Vec<Part>) {
        let list = js_sys::Array::new();
        for part in parts {
            match part {
                Part::Html(html) => list.push(&JsValue::from_str(&html)),
                Part::Gate(name) => {
                    let gate = js_sys::Object::new();
                    js_sys::Reflect::set(&gate, &"gate".into(), &name.into()).unwrap();
                    list.push(&gate)
                }
            };
        }
        let route: js_sys::Function = get(&self.stub, "route").unchecked_into();
        route.call2(&self.stub, &path.into(), &list).unwrap();
    }

    fn release(&self, gate: &str, html: &str) {
        let release: js_sys::Function = get(&self.stub, "release").unchecked_into();
        release.call2(&self.stub, &gate.into(), &html.into()).unwrap();
    }

    fn calls(&self) -> Vec<String> {
        let calls: js_sys::Array = get(&self.stub, "calls").unchecked_into();
        calls.iter().map(|c| c.as_string().unwrap()).collect()
    }

    /// Paths whose response stream the runtime cancelled.
    fn cancels(&self) -> Vec<String> {
        let cancels: js_sys::Array = get(&self.stub, "cancels").unchecked_into();
        cancels.iter().map(|c| c.as_string().unwrap()).collect()
    }
}

/// `<!--rmx:f:ID-->content<!--/rmx:f-->`
fn region(id: &str, content: &str) -> String {
    format!("<!--rmx:f:{id}-->{content}<!--/rmx:f-->")
}

/// The `rmx-data` script for `(id, src, name, pending)` frames.
fn data(frames: &[(&str, &str, &str, bool)]) -> String {
    let mut d = HydrationData::default();
    for (id, src, name, pending) in frames {
        d.frames.push((id.to_string(), src.to_string(), Some(name.to_string()), if *pending { "pending" } else { "resolved" }));
    }
    d.to_script()
}

fn template(id: &str, content: &str) -> String {
    format!("<template id=\"{id}\">{content}<!--{TEMPLATE_END}--></template>")
}

fn flush() -> String {
    flush_marker(false).to_string()
}

fn by_id(id: &str) -> Option<web_sys::Element> {
    document().get_element_by_id(id)
}

fn text(id: &str) -> Option<String> {
    by_id(id).and_then(|e| e.text_content())
}

fn input(id: &str) -> web_sys::HtmlInputElement {
    by_id(id).unwrap_or_else(|| panic!("no #{id}")).unchecked_into()
}

fn frame(name: &str) -> FrameHandle {
    Frames.get(name).unwrap_or_else(|| panic!("no frame named {name}"))
}

/// Reload `frame` in the background; the returned flag is set when it finishes.
fn reload(frame: FrameHandle) -> Rc<Cell<bool>> {
    reload_with(frame, ReloadOptions::default())
}

fn reload_with(frame: FrameHandle, options: ReloadOptions) -> Rc<Cell<bool>> {
    let done = Rc::new(Cell::new(false));
    let flag = done.clone();
    wasm_bindgen_futures::spawn_local(async move {
        frame.reload_async(options).await;
        flag.set(true);
    });
    done
}

/// A client-created parent frame in a container in the page.
struct Mounted {
    el: web_sys::HtmlElement,
    root: Root,
    watch: Option<Root>,
}

impl Drop for Mounted {
    fn drop(&mut self) {
        if let Some(watch) = &self.watch {
            watch.dispose();
        }
        self.root.dispose();
        self.el.remove();
    }
}

/// Mount `<Frame name=parent src=path/>` and wait until `#ready` is in the page.
async fn mount(parent: &str, path: &str, ready: &str) -> Mounted {
    let el: web_sys::HtmlElement = document().create_element("div").unwrap().unchecked_into();
    document().body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    root.render(rsx! { <Frame name={parent.to_string()} src={path.to_string()}/> });
    root.flush();
    until(ready, || by_id(ready).is_some()).await;
    Mounted { el, root, watch: None }
}

#[derive(Clone, Default)]
struct WatchProps {
    name: &'static str,
}

/// Counts a frame's reload events.
struct Watch;

impl Component for Watch {
    type Props = WatchProps;
    fn setup(p: &WatchProps, h: &Handle<Self>) -> Self {
        let frame = h.frames().get(p.name).expect("frame to watch");
        h.on_frame(&frame, FrameEvent::ReloadStart, |_, _| STARTS.with(|c| c.set(c.get() + 1)));
        h.on_frame(&frame, FrameEvent::ReloadComplete, |_, _| COMPLETES.with(|c| c.set(c.get() + 1)));
        Watch
    }
    fn render(&self, _: &WatchProps, _: &Handle<Self>) -> Node {
        Node::Empty
    }
}

/// Count reload events of the frame named `name` from now on.
fn watch(mounted: &mut Mounted, name: &'static str) {
    STARTS.with(|c| c.set(0));
    COMPLETES.with(|c| c.set(0));
    let el = document().create_element("div").unwrap();
    mounted.el.append_child(&el).unwrap();
    let root = create_root(&el);
    root.render(rsx! { <Watch name={name}/> });
    root.flush();
    mounted.watch = Some(root);
}

fn events() -> (u32, u32) {
    (STARTS.with(Cell::get), COMPLETES.with(Cell::get))
}

#[wasm_bindgen_test]
async fn preserves_resolved_child_frame_content_while_a_parent_reload_streams() {
    let server = start().await;
    let fallback = r#"<span id="pr-fallback">Loading child...</span>"#;
    let child = |label: &str| format!(r#"<p id="pr-label">{label}</p><input id="pr-input">"#);
    let parent = |id: &str| format!(r#"{}<p id="pr-sibling">Parent sibling</p>{}"#, region(id, fallback), data(&[(id, "/pr-child", "pr-child", true)]));
    server.route("/pr-parent", vec![Part::Html(parent("pr1") + &flush() + &template("pr1", &child("Initial child")))]);
    let mut m = mount("pr-parent", "/pr-parent", "pr-label").await;
    watch(&mut m, "pr-child");
    let field = input("pr-input");
    assert_eq!(text("pr-label").as_deref(), Some("Initial child"));
    field.set_value("typed child value");

    server.route("/pr-parent", vec![Part::Html(parent("pr2") + &flush()), Part::Gate("pr-child")]);
    let done = reload(frame("pr-parent"));
    for _ in 0..5 {
        tick().await;
    }
    assert!(!done.get(), "the reload waits for the child's template");
    assert_eq!(events(), (1, 0), "the child starts reloading with its parent");
    assert!(by_id("pr-fallback").is_none(), "resolved child content does not flash its fallback");
    assert!(input("pr-input").is_same_node(Some(&field)));
    assert_eq!(text("pr-label").as_deref(), Some("Initial child"));
    assert_eq!(text("pr-sibling").as_deref(), Some("Parent sibling"));

    server.release("pr-child", &template("pr2", &child("Reloaded child")));
    until("the reload", || done.get()).await;
    until("the reloaded child", || text("pr-label").as_deref() == Some("Reloaded child")).await;
    assert!(by_id("pr-fallback").is_none());
    assert!(input("pr-input").is_same_node(Some(&field)), "the child's input is kept");
    assert_eq!(field.value(), "typed child value");
    assert_eq!(text("pr-sibling").as_deref(), Some("Parent sibling"));
    until("the child's reloadComplete", || events() == (1, 1)).await;
}

#[wasm_bindgen_test]
async fn diffs_pending_child_fallbacks_and_ignores_stale_templates_during_parent_reloads() {
    let server = start().await;
    let parent = |id: &str, label: &str| {
        format!(
            r#"<p id="pf-top">Parent</p>{}{}{}"#,
            region(id, &format!(r#"<span id="pf-fallback" data-label="{label}">{label}</span>"#)),
            data(&[(id, "/pf-child", "pf-child", true)]),
            flush(),
        )
    };
    server.route("/pf-parent", vec![Part::Html(parent("pf1", "Initial fallback"))]);
    let mut m = mount("pf-parent", "/pf-parent", "pf-fallback").await;
    let fallback = by_id("pf-fallback").unwrap();
    assert_eq!(fallback.text_content().as_deref(), Some("Initial fallback"));
    watch(&mut m, "pf-child");

    server.route("/pf-parent", vec![Part::Html(parent("pf2", "Reloaded fallback"))]);
    let done = reload(frame("pf-parent"));
    until("the parent reload", || done.get()).await;
    tick().await;
    assert_eq!(events(), (1, 0), "the pending child starts reloading with its parent");
    assert!(by_id("pf-fallback").unwrap().is_same_node(Some(&fallback)), "the fallback is diffed in place");
    assert_eq!(fallback.text_content().as_deref(), Some("Reloaded fallback"));
    assert_eq!(fallback.get_attribute("data-label").as_deref(), Some("Reloaded fallback"));

    let stale = document().create_element("div").unwrap();
    stale.set_inner_html(&template("pf1", r#"<span id="pf-stale">Stale child</span>"#));
    document().body().unwrap().append_child(&stale.first_child().unwrap()).unwrap();
    for _ in 0..3 {
        tick().await;
    }
    assert!(by_id("pf-stale").is_none(), "a template for the old frame id is ignored");
    assert!(by_id("pf-fallback").unwrap().is_same_node(Some(&fallback)));

    let fresh = document().create_element("div").unwrap();
    fresh.set_inner_html(&template("pf2", r#"<span id="pf-fresh">Fresh child</span>"#));
    document().body().unwrap().append_child(&fresh.first_child().unwrap()).unwrap();
    until("the fresh template", || by_id("pf-fresh").is_some()).await;
    assert_eq!(text("pf-fresh").as_deref(), Some("Fresh child"));
    assert!(by_id("pf-fallback").is_none());
    assert!(by_id("pf-stale").is_none());
    until("reloadComplete", || events() == (1, 1)).await;
}

#[wasm_bindgen_test]
async fn keeps_blocking_child_frames_reloadable_after_parent_reloads() {
    let server = start().await;
    let parent = |label: &str| {
        format!(
            "{}{}",
            region("bc1", &format!(r#"<p id="bc-label">{label}</p>"#)),
            data(&[("bc1", "/bc-child", "bc-child", false)]),
        )
    };
    server.route("/bc-parent", vec![Part::Html(parent("Initial blocking child"))]);
    let mut m = mount("bc-parent", "/bc-parent", "bc-label").await;
    assert_eq!(text("bc-label").as_deref(), Some("Initial blocking child"));
    watch(&mut m, "bc-child");

    server.route("/bc-parent", vec![Part::Gate("bc-parent")]);
    let done = reload(frame("bc-parent"));
    until("the child's reloadStart", || events().0 == 1).await;
    assert_eq!(events(), (1, 0), "a blocking child starts reloading with its parent");
    server.release("bc-parent", &parent("Reloaded by parent frame"));
    until("the parent reload", || done.get()).await;
    tick().await;
    assert_eq!(text("bc-label").as_deref(), Some("Reloaded by parent frame"));
    assert_eq!(events(), (1, 1), "and completes with it");

    server.route("/bc-child", vec![Part::Html(r#"<p id="bc-label">Reloaded by child frame 1</p>"#.into())]);
    let done = reload(frame("bc-child"));
    until("the child reload", || done.get()).await;
    assert_eq!(text("bc-label").as_deref(), Some("Reloaded by child frame 1"));
    assert_eq!(server.calls().last().map(String::as_str), Some("/bc-child"));
}

#[wasm_bindgen_test]
async fn removes_child_frame_content_when_a_parent_reload_drops_the_frame() {
    let server = start().await;
    let child = region("rm1", r#"<p id="rm-content">Child content</p><input id="rm-input">"#);
    let sibling = r#"<p id="rm-sibling">Parent sibling</p>"#;
    server.route("/rm-parent", vec![Part::Html(format!("{child}{sibling}{}", data(&[("rm1", "/rm-child", "rm-child", false)])))]);
    let _m = mount("rm-parent", "/rm-parent", "rm-content").await;
    assert_eq!(text("rm-content").as_deref(), Some("Child content"));

    server.route("/rm-parent", vec![Part::Html(sibling.to_string())]);
    let done = reload(frame("rm-parent"));
    until("the parent reload", || done.get()).await;
    assert!(by_id("rm-content").is_none());
    assert!(by_id("rm-input").is_none());
    assert!(Frames.get("rm-child").is_none(), "the removed frame is unregistered");
    assert_eq!(text("rm-sibling").as_deref(), Some("Parent sibling"));
}

/// The parent's markup with the child frame (pending, src `src`) wrapped by `wrap`.
fn src_parent(prefix: &str, id: &str, src: &str, wrap: fn(String) -> String) -> String {
    let fallback = format!(r#"<span id="{prefix}-fallback">Loading replacement...</span>"#);
    format!(
        r#"{}<p id="{prefix}-sibling">Parent sibling</p>{}{}"#,
        wrap(region(id, &fallback)),
        data(&[(id, src, &format!("{prefix}-child"), true)]),
        flush(),
    )
}

async fn replaces_child_content_when_the_src_changes(prefix: &str, wrap: fn(String) -> String) {
    let server = start().await;
    let path = format!("/{prefix}-parent");
    let initial = format!(r#"<p id="{prefix}-content">Initial child</p><input id="{prefix}-input">"#);
    let a = format!("/{prefix}-a");
    server.route(&path, vec![Part::Html(src_parent(prefix, &format!("{prefix}1"), &a, wrap) + &template(&format!("{prefix}1"), &initial))]);
    let _m = mount(&format!("{prefix}-parent"), &path, &format!("{prefix}-content")).await;
    input(&format!("{prefix}-input")).set_value("typed value");
    assert_eq!(text(&format!("{prefix}-content")).as_deref(), Some("Initial child"));

    let b = format!("/{prefix}-b");
    let gate = format!("{prefix}-b");
    server.route(&path, vec![Part::Html(src_parent(prefix, &format!("{prefix}2"), &b, wrap)), Part::Gate(&gate)]);
    let done = reload(frame(&format!("{prefix}-parent")));
    until("the replacement fallback", || by_id(&format!("{prefix}-fallback")).is_some()).await;
    assert!(by_id(&format!("{prefix}-content")).is_none(), "a different src does not keep the old content");
    assert!(by_id(&format!("{prefix}-input")).is_none());
    assert_eq!(text(&format!("{prefix}-fallback")).as_deref(), Some("Loading replacement..."));
    assert_eq!(text(&format!("{prefix}-sibling")).as_deref(), Some("Parent sibling"));

    server.release(&gate, &template(&format!("{prefix}2"), &format!(r#"<p id="{prefix}-content">Replacement child</p>"#)));
    until("the parent reload", || done.get()).await;
    until("the replacement", || text(&format!("{prefix}-content")).as_deref() == Some("Replacement child")).await;
    assert!(by_id(&format!("{prefix}-fallback")).is_none());
    assert!(by_id(&format!("{prefix}-input")).is_none());
    assert_eq!(text(&format!("{prefix}-sibling")).as_deref(), Some("Parent sibling"));
    assert_eq!(frame(&format!("{prefix}-child")).src(), b);
}

#[wasm_bindgen_test]
async fn replaces_direct_child_frame_content_when_its_src_changes_during_a_parent_reload() {
    replaces_child_content_when_the_src_changes("ds", |r| r).await;
}

#[wasm_bindgen_test]
async fn replaces_nested_child_frame_content_when_its_src_changes_during_a_parent_reload() {
    replaces_child_content_when_the_src_changes("ns", |r| format!(r#"<section id="ns-shell">{r}</section>"#)).await;
}

// frame.test.ts: reload commits and supersession

#[wasm_bindgen_test]
async fn commits_streamed_reload_content_before_the_response_finishes() {
    let server = start().await;
    server.route("/cs-parent", vec![Part::Html(r#"<p id="cs-initial">Initial</p>"#.into())]);
    let _m = mount("cs-parent", "/cs-parent", "cs-initial").await;
    server.route("/cs-parent", vec![Part::Html(format!(r#"<p id="cs-next">Next</p>{}"#, flush())), Part::Gate("cs-rest")]);
    let done = reload(frame("cs-parent"));
    until("the committed content", || by_id("cs-next").is_some()).await;
    assert!(by_id("cs-initial").is_none());
    assert!(!done.get(), "the reload is still reading the response");
    server.release("cs-rest", "");
    until("the reload", || done.get()).await;
    assert_eq!(text("cs-next").as_deref(), Some("Next"));
}

#[wasm_bindgen_test]
async fn finishes_a_reload_after_committing_a_pending_child_frame_fallback() {
    let server = start().await;
    server.route("/pc-parent", vec![Part::Html(r#"<p id="pc-initial">Initial</p>"#.into())]);
    let _m = mount("pc-parent", "/pc-parent", "pc-initial").await;
    server.route(
        "/pc-parent",
        vec![Part::Html(region("pc1", r#"<p id="pc-fallback">Loading</p>"#) + &data(&[("pc1", "/pc-pending", "pc-pending", true)]))],
    );
    let done = reload(frame("pc-parent"));
    until("the reload", || done.get()).await;
    assert_eq!(text("pc-fallback").as_deref(), Some("Loading"), "the reload does not wait for the child's template");
    assert!(frame("pc-pending").src().ends_with("/pc-pending"));
}

#[wasm_bindgen_test]
async fn ignores_a_pending_template_after_its_frame_is_disposed() {
    let server = start().await;
    let parent = format!(
        r#"<p id="dt-ready">Parent</p>{}{}{}"#,
        region("dt1", r#"<p id="dt-fallback">Fallback</p>"#),
        data(&[("dt1", "/dt-child", "dt-child", true)]),
        flush(),
    );
    server.route("/dt-parent", vec![Part::Html(parent)]);
    let m = mount("dt-parent", "/dt-parent", "dt-fallback").await;
    drop(m);
    assert!(by_id("dt-fallback").is_none());
    assert!(Frames.get("dt-child").is_none());
    let holder = document().create_element("div").unwrap();
    holder.set_inner_html(&template("dt1", r#"<p id="dt-late">Late</p>"#));
    let late = holder.first_child().unwrap();
    document().body().unwrap().append_child(&late).unwrap();
    for _ in 0..3 {
        tick().await;
    }
    assert!(by_id("dt-late").is_none(), "nothing renders the late template");
    late.unchecked_ref::<web_sys::Element>().remove();
}

#[wasm_bindgen_test]
async fn a_newer_reload_or_submission_supersedes_and_cancels_a_stale_one() {
    let server = start().await;
    server.route("/sr-parent", vec![Part::Html(r#"<p id="sr">Initial</p>"#.into())]);
    let _m = mount("sr-parent", "/sr-parent", "sr").await;

    // A reload superseded by a submission.
    server.route("/sr-parent", vec![Part::Gate("sr-stale-1")]);
    let stale = reload(frame("sr-parent"));
    tick().await;
    server.route("/sr-parent", vec![Part::Html(r#"<p id="sr">Submitted</p>"#.into())]);
    let post = ReloadOptions { method: Some("post".into()), body: Some(vec![("value".into(), "second".into())]), ..Default::default() };
    let fresh = reload_with(frame("sr-parent"), post);
    until("the submission", || fresh.get()).await;
    assert_eq!(text("sr").as_deref(), Some("Submitted"));
    server.release("sr-stale-1", r#"<p id="sr">Stale</p>"#);
    until("the stale reload", || stale.get()).await;
    for _ in 0..3 {
        tick().await;
    }
    assert_eq!(text("sr").as_deref(), Some("Submitted"), "stale content is not applied");
    assert_eq!(server.cancels(), ["/sr-parent"], "the stale response stream is cancelled");

    // A submission superseded by a reload.
    server.route("/sr-parent", vec![Part::Gate("sr-stale-2")]);
    let post = ReloadOptions { method: Some("post".into()), body: Some(vec![("value".into(), "first".into())]), ..Default::default() };
    let stale = reload_with(frame("sr-parent"), post);
    tick().await;
    server.route("/sr-parent", vec![Part::Html(r#"<p id="sr">Reloaded</p>"#.into())]);
    let fresh = reload(frame("sr-parent"));
    until("the reload", || fresh.get()).await;
    server.release("sr-stale-2", r#"<p id="sr">Stale</p>"#);
    until("the stale submission", || stale.get()).await;
    for _ in 0..3 {
        tick().await;
    }
    assert_eq!(text("sr").as_deref(), Some("Reloaded"));
    assert_eq!(server.cancels().len(), 2);
}

// frame.test.tsx: what a reload keeps and replaces

#[wasm_bindgen_test]
async fn reloads_a_frame_region_and_keeps_static_dom_nodes() {
    let server = start().await;
    let activity = |n: u32| {
        format!(r#"<section><h2>Activity</h2><p id="sd-count">Server: {n}</p><ul><li>First</li><li>Second</li></ul><button>Reload</button></section>"#)
    };
    server.route("/sd-parent", vec![Part::Html(activity(1))]);
    let m = mount("sd-parent", "/sd-parent", "sd-count").await;
    let q = |s: &str| m.el.query_selector(s).unwrap().unwrap_or_else(|| panic!("no {s}"));
    let before: Vec<web_sys::Element> = ["section", "h2", "p", "ul", "li:first-child", "li:last-child", "button"].iter().map(|s| q(s)).collect();
    server.route("/sd-parent", vec![Part::Html(activity(2))]);
    let done = reload(frame("sd-parent"));
    until("the reload", || done.get()).await;
    assert_eq!(text("sd-count").as_deref(), Some("Server: 2"));
    let after: Vec<web_sys::Element> = ["section", "h2", "p", "ul", "li:first-child", "li:last-child", "button"].iter().map(|s| q(s)).collect();
    for (a, b) in before.iter().zip(&after) {
        assert!(a.is_same_node(Some(b)), "<{}> was replaced", a.tag_name());
    }
    assert_eq!(q("h2").text_content().as_deref(), Some("Activity"));
}

#[wasm_bindgen_test]
async fn clears_frame_content_when_a_reload_returns_nothing() {
    let server = start().await;
    server.route("/ec-parent", vec![Part::Html(r#"<section id="ec-content"><p id="ec-value">Initial content</p></section>"#.into())]);
    let _m = mount("ec-parent", "/ec-parent", "ec-value").await;
    server.route("/ec-parent", vec![]);
    let done = reload(frame("ec-parent"));
    until("the reload", || done.get()).await;
    assert!(by_id("ec-content").is_none());
    assert!(by_id("ec-value").is_none());
}

#[wasm_bindgen_test]
async fn keeps_head_like_elements_inside_the_frame_when_it_reloads() {
    let server = start().await;
    let content = |n: u32| {
        format!(
            concat!(
                r#"<title>Frame title {n}</title><meta name="hl-description" content="frame-{n}">"#,
                r#"<script type="application/ld+json">{{"count":{n}}}</script>"#,
                r#"<section><p id="hl-body">Frame body {n}</p></section>"#,
            ),
            n = n
        )
    };
    server.route("/hl-parent", vec![Part::Html(content(1))]);
    let m = mount("hl-parent", "/hl-parent", "hl-body").await;
    let head = document().head().unwrap();
    let count = |s: &str| m.el.query_selector_all(s).unwrap().length();
    assert_eq!(m.el.query_selector("title").unwrap().unwrap().text_content().as_deref(), Some("Frame title 1"));
    assert!(head.query_selector(r#"meta[name="hl-description"]"#).unwrap().is_none());
    assert!(head.query_selector(r#"script[type="application/ld+json"]"#).unwrap().is_none());

    server.route("/hl-parent", vec![Part::Html(content(2))]);
    let done = reload(frame("hl-parent"));
    until("the reload", || done.get()).await;
    assert_eq!(count("title"), 1);
    assert_eq!(m.el.query_selector("title").unwrap().unwrap().text_content().as_deref(), Some("Frame title 2"));
    assert_eq!(count(r#"meta[name="hl-description"]"#), 1);
    assert_eq!(
        m.el.query_selector(r#"meta[name="hl-description"]"#).unwrap().unwrap().get_attribute("content").as_deref(),
        Some("frame-2")
    );
    assert_eq!(m.el.query_selector(r#"script[type="application/ld+json"]"#).unwrap().unwrap().text_content().as_deref(), Some(r#"{"count":2}"#));
    assert_eq!(text("hl-body").as_deref(), Some("Frame body 2"));
    assert!(head.query_selector(r#"meta[name="hl-description"]"#).unwrap().is_none());
}

#[wasm_bindgen_test]
async fn restores_the_previous_named_frame_when_the_latest_duplicate_unmounts() {
    let server = start().await;
    server.route("/dup-first", vec![Part::Html(r#"<p id="dup-first">/first</p>"#.into())]);
    server.route("/dup-latest", vec![Part::Html(r#"<p id="dup-latest">/latest</p>"#.into())]);
    assert!(Frames.get("dup-missing").is_none());
    let el: web_sys::HtmlElement = document().create_element("div").unwrap().unchecked_into();
    document().body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    root.render(rsx! { <><Frame name="dup" src="/dup-first"/><Frame name="dup" src="/dup-latest"/></> });
    root.flush();
    until("both frames", || by_id("dup-first").is_some() && by_id("dup-latest").is_some()).await;
    assert!(frame("dup").src().ends_with("/dup-latest"));
    root.render(rsx! { <><Frame name="dup" src="/dup-first"/></> });
    root.flush();
    assert!(frame("dup").src().ends_with("/dup-first"));
    root.dispose();
    el.remove();
    assert!(Frames.get("dup").is_none());
}
