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
use serde::{Deserialize, Serialize};
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
        let entries = vec![
            rmx::entry::<EntryWithFrame>(),
            rmx::entry::<EntryWithBlockingFrame>(),
            rmx::entry::<EntryWithSrc>(),
            rmx::entry::<EntryWithLateFrame>(),
            rmx::entry::<RowAction>(),
            rmx::entry::<CartItems>(),
        ];
        APP.with(|a| *a.borrow_mut() = Some(rmx::run(RunOptions { entries, ..Default::default() })));
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
               if (parts === 'fail') throw new TypeError('Failed to reload entry frame');
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
               fail(path) { routes[path] = 'fail' },
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

    /// Make `fetch` of `path` reject with a `TypeError`, like a network failure.
    fn fail(&self, path: &str) {
        let fail: js_sys::Function = get(&self.stub, "fail").unchecked_into();
        fail.call1(&self.stub, &path.into()).unwrap();
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
    let parent_cancels = || server.cancels().iter().filter(|c| *c == "/sr-parent").count();
    assert_eq!(parent_cancels(), 1, "the stale response stream is cancelled");

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
    assert_eq!(parent_cancels(), 2);
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

// frame.test.tsx: frames rendered by client entries

#[derive(Clone, Default, Serialize, Deserialize)]
struct EntryProps {
    label: String,
}

/// A client entry that renders a named frame.
struct EntryWithFrame;

impl Component for EntryWithFrame {
    type Props = EntryProps;
    rmx::client_entry!("EntryWithFrame");
    fn setup(_: &EntryProps, _: &Handle<Self>) -> Self {
        EntryWithFrame
    }
    fn render(&self, p: &EntryProps, _: &Handle<Self>) -> Node {
        let fallback = rsx! { <span id="ef-frame">"Loading entry frame..."</span> };
        rsx! { <section><p id="ef-label">{p.label.clone()}</p><Frame name="ef-frame" src="/ef-frame" fallback={fallback}/></section> }
    }
}

/// Server markup for the entry with its frame resolved to `frame_html`.
fn entry_page(entry_id: &str, frame_id: &str, label: &str, frame_html: &str) -> String {
    let mut d = HydrationData::default();
    d.entries.push((entry_id.into(), "EntryWithFrame".into(), format!(r#"{{"label":"{label}"}}"#)));
    d.frames.push((frame_id.into(), "/ef-frame".into(), Some("ef-frame".into()), "resolved"));
    format!(
        r#"<!--rmx:h:{entry_id}--><section><p id="ef-label">{label}</p>{}</section><!--/rmx:h-->{}"#,
        region(frame_id, frame_html),
        d.to_script()
    )
}

#[wasm_bindgen_test]
async fn reloads_frames_inside_preserved_client_entries_after_server_driven_rerenders() {
    let server = start().await;
    server.route("/ef-parent", vec![Part::Html(entry_page("efh1", "eff1", "Initial entry", r#"<span id="ef-frame">Initial entry frame</span>"#))]);
    let mut m = mount("ef-parent", "/ef-parent", "ef-label").await;
    assert_eq!(text("ef-frame").as_deref(), Some("Initial entry frame"));
    let label = by_id("ef-label").unwrap();
    watch(&mut m, "ef-frame");
    let frame_loads = || server.calls().iter().filter(|c| *c == "/ef-frame").count();
    assert_eq!(frame_loads(), 0, "the entry's frame came with the server HTML");

    server.route(
        "/ef-parent",
        vec![Part::Html(entry_page("efh2", "eff2", "Reloaded entry", r#"<span id="ef-frame">Unused server entry frame</span>"#))],
    );
    server.route("/ef-frame", vec![Part::Html(r#"<span id="ef-frame">Reloaded entry frame</span>"#.into())]);
    let done = reload(frame("ef-parent"));
    until("the parent reload", || done.get()).await;
    until("the entry frame reload", || text("ef-frame").as_deref() == Some("Reloaded entry frame")).await;
    assert_eq!(text("ef-label").as_deref(), Some("Reloaded entry"), "the entry got its new props");
    assert!(by_id("ef-label").unwrap().is_same_node(Some(&label)), "the entry was kept");
    assert_eq!(frame_loads(), 1, "the entry's frame reloads once, on the client");
    until("the entry frame's events", || events() == (1, 1)).await;
}

#[wasm_bindgen_test]
async fn does_not_reload_client_frames_on_ordinary_client_updates() {
    let server = start().await;
    server.route("/cu-frame", vec![Part::Html(r#"<p id="cu-content">Client frame</p>"#.into())]);
    let el: web_sys::HtmlElement = document().create_element("div").unwrap().unchecked_into();
    document().body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    let page = |label: &str| rsx! { <div><p id="cu-label">{label.to_string()}</p><Frame name="cu-frame" src="/cu-frame"/></div> };
    root.render(page("one"));
    root.flush();
    until("the client frame", || by_id("cu-content").is_some()).await;
    let loads = || server.calls().iter().filter(|c| *c == "/cu-frame").count();
    assert_eq!(loads(), 1);
    root.render(page("two"));
    root.flush();
    for _ in 0..3 {
        tick().await;
    }
    assert_eq!(text("cu-label").as_deref(), Some("two"));
    assert_eq!(loads(), 1, "a client re-render keeps the frame's content");
    root.dispose();
    el.remove();
}

#[wasm_bindgen_test]
async fn does_not_wait_for_non_blocking_frames_inside_preserved_client_entries() {
    let server = start().await;
    server.route("/ew-parent", vec![Part::Html(entry_page("ewh1", "ewf1", "Initial entry", r#"<span id="ef-frame">Initial entry frame</span>"#))]);
    let mut m = mount("ew-parent", "/ew-parent", "ef-label").await;
    watch(&mut m, "ef-frame");

    server.route("/ew-parent", vec![Part::Html(entry_page("ewh2", "ewf2", "Reloaded entry", r#"<span id="ef-frame">Unused</span>"#))]);
    server.route("/ef-frame", vec![Part::Gate("ew-frame")]);
    let done = reload(frame("ew-parent"));
    until("the parent reload", || done.get()).await;
    assert_eq!(text("ef-label").as_deref(), Some("Reloaded entry"));
    assert_eq!(text("ef-frame").as_deref(), Some("Initial entry frame"), "the entry's frame keeps its content while it reloads");
    assert_eq!(events(), (1, 0), "the parent finished without waiting for the entry's frame");

    server.release("ew-frame", r#"<span id="ef-frame">Reloaded entry frame</span>"#);
    until("the entry frame reload", || text("ef-frame").as_deref() == Some("Reloaded entry frame")).await;
    until("reloadComplete", || events() == (1, 1)).await;
}

/// A client entry that renders a blocking frame (no fallback).
struct EntryWithBlockingFrame;

impl Component for EntryWithBlockingFrame {
    type Props = EntryProps;
    rmx::client_entry!("EntryWithBlockingFrame");
    fn setup(_: &EntryProps, _: &Handle<Self>) -> Self {
        EntryWithBlockingFrame
    }
    fn render(&self, p: &EntryProps, _: &Handle<Self>) -> Node {
        rsx! { <section><p id="eb-label">{p.label.clone()}</p><Frame name="eb-frame" src="/eb-frame"/></section> }
    }
}

fn blocking_entry_page(entry_id: &str, frame_id: &str, label: &str, frame_html: &str) -> String {
    let mut d = HydrationData::default();
    d.entries.push((entry_id.into(), "EntryWithBlockingFrame".into(), format!(r#"{{"label":"{label}"}}"#)));
    d.frames.push((frame_id.into(), "/eb-frame".into(), Some("eb-frame".into()), "resolved"));
    format!(
        r#"<!--rmx:h:{entry_id}--><section><p id="eb-label">{label}</p>{}</section><!--/rmx:h-->{}"#,
        region(frame_id, frame_html),
        d.to_script()
    )
}

#[wasm_bindgen_test]
async fn waits_for_blocking_frames_inside_preserved_client_entries() {
    let server = start().await;
    server.route("/eb-parent", vec![Part::Html(blocking_entry_page("ebh1", "ebf1", "Initial entry", r#"<span id="eb-frame">Initial</span>"#))]);
    let _m = mount("eb-parent", "/eb-parent", "eb-label").await;

    server.route("/eb-parent", vec![Part::Html(blocking_entry_page("ebh2", "ebf2", "Reloaded entry", r#"<span id="eb-frame">Unused</span>"#))]);
    server.route("/eb-frame", vec![Part::Gate("eb-frame")]);
    let done = reload(frame("eb-parent"));
    until("the entry update", || text("eb-label").as_deref() == Some("Reloaded entry")).await;
    for _ in 0..3 {
        tick().await;
    }
    assert!(!done.get(), "the parent reload waits for the entry's blocking frame");
    server.release("eb-frame", r#"<span id="eb-frame">Reloaded</span>"#);
    until("the parent reload", || done.get()).await;
    assert_eq!(text("eb-frame").as_deref(), Some("Reloaded"));
}

// frame.test.tsx: client entries whose frames the client updates

thread_local! {
    /// The src `EntryWithSrc` renders its frame with.
    static ENTRY_SRC: RefCell<String> = const { RefCell::new(String::new()) };
    /// Re-renders the latest `EntryWithSrc` (its `handle.update()`).
    static ENTRY_UPDATE: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
    static ENTRY_RENDERS: Cell<u32> = const { Cell::new(0) };
    /// Whether `EntryWithLateFrame` renders its frame; set by its first task.
    static LATE_FRAME: Cell<bool> = const { Cell::new(false) };
}

/// A client entry rendering a named frame whose src comes from `ENTRY_SRC`, like Remix's
/// entries that read a variable outside the component.
struct EntryWithSrc;

impl Component for EntryWithSrc {
    type Props = EntryProps;
    rmx::client_entry!("EntryWithSrc");
    fn setup(_: &EntryProps, h: &Handle<Self>) -> Self {
        let h = h.clone();
        ENTRY_UPDATE.with(|u| *u.borrow_mut() = Some(Rc::new(move || h.update())));
        EntryWithSrc
    }
    fn render(&self, p: &EntryProps, _: &Handle<Self>) -> Node {
        let renders = ENTRY_RENDERS.with(|r| {
            r.set(r.get() + 1);
            r.get()
        });
        let src = ENTRY_SRC.with(|s| s.borrow().clone());
        let fallback = rsx! { <span id="es-frame">"Loading entry frame..."</span> };
        rsx! {
            <section>
                <p id="es-label">{p.label.clone()}</p>
                <p id="es-renders">{renders.to_string()}</p>
                <Frame name="es-frame" src={src} fallback={fallback}/>
            </section>
        }
    }
}

/// Server markup for `EntryWithSrc` with its frame (src `ENTRY_SRC`) resolved to `frame_html`.
fn src_entry_page(entry_id: &str, frame_id: &str, label: &str, frame_html: &str) -> String {
    let src = ENTRY_SRC.with(|s| s.borrow().clone());
    let mut d = HydrationData::default();
    d.entries.push((entry_id.into(), "EntryWithSrc".into(), format!(r#"{{"label":"{label}"}}"#)));
    d.frames.push((frame_id.into(), src, Some("es-frame".into()), "resolved"));
    format!(
        r#"<!--rmx:h:{entry_id}--><section><p id="es-label">{label}</p><p id="es-renders">0</p>{}</section><!--/rmx:h-->{}"#,
        region(frame_id, frame_html),
        d.to_script()
    )
}

fn set_entry_src(src: &str) {
    ENTRY_SRC.with(|s| *s.borrow_mut() = src.to_string());
}

fn update_entry() {
    let update = ENTRY_UPDATE.with(|u| u.borrow().clone()).expect("a mounted EntryWithSrc");
    update();
    rmx::flush();
}

fn loads(server: &Server, path: &str) -> usize {
    server.calls().iter().filter(|c| *c == path).count()
}

fn cancelled(server: &Server, path: &str) -> bool {
    server.cancels().iter().any(|c| c == path)
}

#[wasm_bindgen_test]
async fn does_not_reload_frames_inside_client_entries_on_ordinary_entry_updates() {
    let server = start().await;
    set_entry_src("/eu-frame");
    server.route("/eu-parent", vec![Part::Html(src_entry_page("euh1", "euf1", "Entry", r#"<span id="es-frame">Initial entry frame</span>"#))]);
    let _m = mount("eu-parent", "/eu-parent", "es-label").await;
    let renders: u32 = text("es-renders").unwrap().parse().unwrap();
    assert_eq!(text("es-frame").as_deref(), Some("Initial entry frame"));

    update_entry();
    for _ in 0..3 {
        tick().await;
    }
    assert_eq!(text("es-renders"), Some((renders + 1).to_string()), "the entry re-rendered");
    assert_eq!(text("es-frame").as_deref(), Some("Initial entry frame"));
    assert_eq!(loads(&server, "/eu-frame"), 0, "an ordinary client update keeps the frame's content");
}

#[wasm_bindgen_test]
async fn loads_new_frame_src_values_during_server_driven_entry_rerenders() {
    let server = start().await;
    set_entry_src("/es-a");
    server.route("/es-parent", vec![Part::Html(src_entry_page("esh1", "esf1", "Initial entry", r#"<span id="es-frame">Initial entry frame</span>"#))]);
    let mut m = mount("es-parent", "/es-parent", "es-label").await;
    watch(&mut m, "es-frame");

    set_entry_src("/es-b");
    server.route("/es-parent", vec![Part::Html(src_entry_page("esh2", "esf2", "Reloaded entry", r#"<span id="es-frame">Unused</span>"#))]);
    server.route("/es-b", vec![Part::Html(r#"<span id="es-frame">Reloaded entry frame</span>"#.into())]);
    let done = reload(frame("es-parent"));
    until("the first parent reload", || done.get()).await;
    until("the new frame src", || text("es-frame").as_deref() == Some("Reloaded entry frame")).await;
    assert_eq!(text("es-label").as_deref(), Some("Reloaded entry"));
    assert_eq!((loads(&server, "/es-a"), loads(&server, "/es-b")), (0, 1));
    until("the first reload's events", || events() == (1, 1)).await;

    set_entry_src("/es-c");
    server.route("/es-parent", vec![Part::Html(src_entry_page("esh3", "esf3", "Reloaded again", r#"<span id="es-frame">Unused</span>"#))]);
    server.route("/es-c", vec![Part::Html(r#"<span id="es-frame">Reloaded again frame</span>"#.into())]);
    let done = reload(frame("es-parent"));
    until("the second parent reload", || done.get()).await;
    until("the second frame src", || text("es-frame").as_deref() == Some("Reloaded again frame")).await;
    assert_eq!((loads(&server, "/es-b"), loads(&server, "/es-c")), (1, 1));
    until("the second reload's events", || events() == (2, 2)).await;
}

#[wasm_bindgen_test]
async fn cancels_an_inherited_frame_reload_when_a_client_update_changes_its_src() {
    let server = start().await;
    set_entry_src("/ci-a");
    server.route("/ci-parent", vec![Part::Html(src_entry_page("cih1", "cif1", "Initial entry", r#"<span id="es-frame">Initial entry frame</span>"#))]);
    let _m = mount("ci-parent", "/ci-parent", "es-label").await;

    server.route("/ci-parent", vec![Part::Html(src_entry_page("cih2", "cif2", "Reloaded entry", r#"<span id="es-frame">Unused</span>"#))]);
    server.route("/ci-a", vec![Part::Gate("ci-a")]);
    let done = reload(frame("ci-parent"));
    until("the parent reload", || done.get()).await;
    until("the inherited frame reload", || loads(&server, "/ci-a") == 1).await;
    assert!(!cancelled(&server, "/ci-a"));

    set_entry_src("/ci-b");
    server.route("/ci-b", vec![Part::Html(r#"<span id="es-frame">Updated entry frame</span>"#.into())]);
    update_entry();
    until("the updated frame", || text("es-frame").as_deref() == Some("Updated entry frame")).await;
    until("the inherited reload's cancellation", || cancelled(&server, "/ci-a")).await;

    server.release("ci-a", r#"<span id="es-frame">Stale inherited frame</span>"#);
    for _ in 0..3 {
        tick().await;
    }
    assert_eq!(text("es-frame").as_deref(), Some("Updated entry frame"));
}

#[wasm_bindgen_test]
async fn ignores_stale_frame_reloads_from_superseded_entry_rerenders() {
    let server = start().await;
    set_entry_src("/ss-frame");
    server.route("/ss-parent", vec![Part::Html(src_entry_page("ssh1", "ssf1", "Initial entry", r#"<span id="es-frame">Initial entry frame</span>"#))]);
    let _m = mount("ss-parent", "/ss-parent", "es-label").await;

    server.route("/ss-parent", vec![Part::Html(src_entry_page("ssh2", "ssf2", "First reload", r#"<span id="es-frame">Unused</span>"#))]);
    server.route("/ss-frame", vec![Part::Gate("ss-1")]);
    let done = reload(frame("ss-parent"));
    until("the first parent reload", || done.get()).await;
    assert_eq!(text("es-label").as_deref(), Some("First reload"));
    assert_eq!(text("es-frame").as_deref(), Some("Initial entry frame"));
    until("the entry frame's first reload", || loads(&server, "/ss-frame") == 1).await;

    server.route("/ss-parent", vec![Part::Gate("ss-top")]);
    server.route("/ss-frame", vec![Part::Gate("ss-2")]);
    let second = reload(frame("ss-parent"));
    until("the superseded entry frame reload's cancellation", || cancelled(&server, "/ss-frame")).await;
    server.release("ss-1", r#"<span id="es-frame">Stale entry frame</span>"#);
    for _ in 0..3 {
        tick().await;
    }
    assert_eq!(text("es-label").as_deref(), Some("First reload"));
    assert_eq!(text("es-frame").as_deref(), Some("Initial entry frame"), "the stale reload is ignored");

    server.release("ss-top", &src_entry_page("ssh3", "ssf3", "Second reload", r#"<span id="es-frame">Unused</span>"#));
    until("the second parent reload", || second.get()).await;
    assert_eq!(text("es-label").as_deref(), Some("Second reload"));
    assert_eq!(text("es-frame").as_deref(), Some("Initial entry frame"));
    until("the entry frame's second reload", || loads(&server, "/ss-frame") == 2).await;

    server.release("ss-2", r#"<span id="es-frame">Second entry frame</span>"#);
    until("the second entry frame", || text("es-frame").as_deref() == Some("Second entry frame")).await;
}

#[wasm_bindgen_test]
async fn reports_reload_failures_for_frames_inside_preserved_client_entries() {
    let server = start().await;
    set_entry_src("/rf-frame");
    server.route("/rf-parent", vec![Part::Html(src_entry_page("rfh1", "rff1", "Initial entry", r#"<span id="es-frame">Initial entry frame</span>"#))]);
    let mut m = mount("rf-parent", "/rf-parent", "es-label").await;
    watch(&mut m, "es-frame");
    let errors = Rc::new(RefCell::new(Vec::<String>::new()));
    let seen = errors.clone();
    let app = APP.with(|a| a.borrow_mut().take()).expect("app");
    let listener = app.on_error(move |e| seen.borrow_mut().push(e.message.clone()));

    server.route("/rf-parent", vec![Part::Html(src_entry_page("rfh2", "rff2", "Reloaded entry", r#"<span id="es-frame">Unused</span>"#))]);
    server.fail("/rf-frame");
    let done = reload(frame("rf-parent"));
    until("the parent reload", || done.get()).await;
    until("the reported failure", || !errors.borrow().is_empty()).await;
    app.remove_error_listener(listener);
    APP.with(|a| *a.borrow_mut() = Some(app));

    assert!(errors.borrow()[0].contains("Failed to reload entry frame"), "{:?}", errors.borrow());
    assert_eq!(text("es-label").as_deref(), Some("Reloaded entry"));
    assert_eq!(text("es-frame").as_deref(), Some("Initial entry frame"), "the frame keeps its content");
    until("the entry frame's events", || events() == (1, 1)).await;
}

/// A client entry that renders its frame only after its first task asks for an update, so
/// the frame is created on the client and still loading when its parent reloads.
struct EntryWithLateFrame;

impl Component for EntryWithLateFrame {
    type Props = EntryProps;
    rmx::client_entry!("EntryWithLateFrame");
    fn setup(_: &EntryProps, h: &Handle<Self>) -> Self {
        h.queue_task(|_, h| {
            if !LATE_FRAME.with(|l| l.replace(true)) {
                h.update();
            }
        });
        EntryWithLateFrame
    }
    fn render(&self, p: &EntryProps, _: &Handle<Self>) -> Node {
        let frame = LATE_FRAME.with(Cell::get).then(|| {
            let fallback = rsx! { <span id="lf-frame">"Loading entry frame..."</span> };
            rsx! { <Frame name="lf-frame" src="/lf-frame" fallback={fallback}/> }
        });
        rsx! { <section><p id="lf-label">{p.label.clone()}</p>{frame.unwrap_or(Node::Empty)}</section> }
    }
}

fn late_entry_page(entry_id: &str, label: &str) -> String {
    let mut d = HydrationData::default();
    d.entries.push((entry_id.into(), "EntryWithLateFrame".into(), format!(r#"{{"label":"{label}"}}"#)));
    format!(r#"<!--rmx:h:{entry_id}--><section><p id="lf-label">{label}</p></section><!--/rmx:h-->{}"#, d.to_script())
}

#[wasm_bindgen_test]
async fn restarts_pending_frames_inside_preserved_client_entries_during_ancestor_reloads() {
    let server = start().await;
    LATE_FRAME.with(|l| l.set(false));
    server.route("/lf-parent", vec![Part::Html(late_entry_page("lfh1", "Initial entry"))]);
    server.route("/lf-frame", vec![Part::Gate("lf-1")]);
    let _m = mount("lf-parent", "/lf-parent", "lf-label").await;
    until("the client-created frame's load", || loads(&server, "/lf-frame") == 1).await;
    assert_eq!(text("lf-frame").as_deref(), Some("Loading entry frame..."));

    server.route("/lf-parent", vec![Part::Html(late_entry_page("lfh2", "Reloaded entry"))]);
    server.route("/lf-frame", vec![Part::Gate("lf-2")]);
    let done = reload(frame("lf-parent"));
    until("the parent reload", || done.get()).await;
    until("the restarted frame load", || loads(&server, "/lf-frame") == 2).await;
    assert!(cancelled(&server, "/lf-frame"), "the first load was cancelled");

    server.release("lf-1", r#"<span id="lf-frame">Stale entry frame</span>"#);
    for _ in 0..3 {
        tick().await;
    }
    assert_eq!(text("lf-label").as_deref(), Some("Reloaded entry"));
    assert_eq!(text("lf-frame").as_deref(), Some("Loading entry frame..."));

    server.release("lf-2", r#"<span id="lf-frame">Reloaded entry frame</span>"#);
    until("the restarted frame's content", || text("lf-frame").as_deref() == Some("Reloaded entry frame")).await;
}

// frame.test.tsx: client-created frames re-rendered while they load

/// A container in the page with its own root.
fn container() -> (web_sys::HtmlElement, Root) {
    let el: web_sys::HtmlElement = document().create_element("div").unwrap().unchecked_into();
    document().body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    (el, root)
}

#[wasm_bindgen_test]
async fn keeps_resolved_client_frame_content_after_rerendering_while_resolution_is_pending() {
    let server = start().await;
    server.route("/kr", vec![Part::Gate("kr")]);
    let (el, root) = container();
    let page = |label: &str| {
        let fallback = rsx! { <p id="kr-fallback">{label.to_string()}</p> };
        rsx! { <Frame src="/kr" fallback={fallback}/> }
    };
    root.render(page("Loading first..."));
    root.flush();
    root.render(page("Loading second..."));
    root.flush();

    server.release("kr", r#"<p id="kr-result">Resolved content</p>"#);
    until("the resolved content", || by_id("kr-result").is_some()).await;
    assert_eq!(text("kr-result").as_deref(), Some("Resolved content"));
    assert!(by_id("kr-fallback").is_none());

    root.render(page("Loading third..."));
    root.flush();
    assert_eq!(text("kr-result").as_deref(), Some("Resolved content"));
    assert!(by_id("kr-fallback").is_none());
    assert_eq!(loads(&server, "/kr"), 1);
    root.dispose();
    el.remove();
}

#[wasm_bindgen_test]
async fn cancels_stale_client_frame_streams_when_src_changes() {
    let server = start().await;
    server.route("/cs-slow", vec![Part::Gate("cs-slow")]);
    server.route("/cs-fast", vec![Part::Html(r#"<p id="cs-result">Fast result</p>"#.into())]);
    let (el, root) = container();
    let page = |src: &str| {
        let fallback = rsx! { <p id="cs-fallback">"Loading..."</p> };
        rsx! { <main><Frame src={src.to_string()} fallback={fallback}/></main> }
    };
    root.render(page("/cs-slow"));
    root.flush();
    assert_eq!(text("cs-fallback").as_deref(), Some("Loading..."));
    until("the slow load", || loads(&server, "/cs-slow") == 1).await;

    root.render(page("/cs-fast"));
    root.flush();
    until("the fast result", || text("cs-result").as_deref() == Some("Fast result")).await;
    until("the slow stream's cancellation", || cancelled(&server, "/cs-slow")).await;

    server.release("cs-slow", r#"<p id="cs-result">Slow result</p>"#);
    for _ in 0..3 {
        tick().await;
    }
    assert_eq!(text("cs-result").as_deref(), Some("Fast result"));
    root.dispose();
    el.remove();
}

// frame.test.tsx: a client entry's own frame and the frames next to it

thread_local! {
    static ROW: RefCell<Option<Handle<RowAction>>> = const { RefCell::new(None) };
    static SUMMARY_EVENTS: Cell<(u32, u32)> = const { Cell::new((0, 0)) };
    static ROW_EVENTS: Cell<(u32, u32)> = const { Cell::new((0, 0)) };
}

/// A client entry rendered inside an unnamed frame next to a frame named `ra-summary`.
struct RowAction;

impl Component for RowAction {
    type Props = EntryProps;
    rmx::client_entry!("RowAction");
    fn setup(_: &EntryProps, h: &Handle<Self>) -> Self {
        ROW.with(|r| *r.borrow_mut() = Some(h.clone()));
        RowAction
    }
    fn render(&self, _: &EntryProps, _: &Handle<Self>) -> Node {
        rsx! { <button id="ra-button">"Update"</button> }
    }
}

fn row() -> Handle<RowAction> {
    ROW.with(|r| r.borrow().clone()).expect("a hydrated RowAction")
}

/// Server markup for a named summary frame and an unnamed row frame holding `RowAction`.
fn row_page(prefix: &str, summary: u32) -> String {
    let mut d = HydrationData::default();
    d.entries.push((format!("{prefix}h"), "RowAction".into(), r#"{"label":""}"#.into()));
    d.frames.push((format!("{prefix}s"), "/ra-summary".into(), Some("ra-summary".into()), "resolved"));
    d.frames.push((format!("{prefix}r"), "/ra-row".into(), None, "resolved"));
    format!(
        "<main>{}{}</main>{}",
        region(&format!("{prefix}s"), &format!(r#"<p id="ra-summary">Summary: {summary}</p>"#)),
        region(&format!("{prefix}r"), &format!(r#"<!--rmx:h:{prefix}h--><button id="ra-button">Update</button><!--/rmx:h-->"#)),
        d.to_script()
    )
}

#[wasm_bindgen_test]
async fn looks_up_named_adjacent_frames_from_a_client_entry() {
    let server = start().await;
    server.route("/ra-parent", vec![Part::Html(row_page("ral", 1))]);
    server.route("/ra-summary", vec![Part::Html(r#"<p id="ra-summary">Summary: 2</p>"#.into())]);
    let _m = mount("ra-parent", "/ra-parent", "ra-button").await;
    assert_eq!(text("ra-summary").as_deref(), Some("Summary: 1"));

    let h = row();
    assert!(h.frames().get("ra-missing").is_none());
    h.frames().get("ra-summary").expect("the summary frame").reload_async(ReloadOptions::default()).await;
    assert_eq!(text("ra-summary").as_deref(), Some("Summary: 2"));
}

#[wasm_bindgen_test]
async fn exposes_the_top_frame_apart_from_a_client_entrys_own_frame() {
    let server = start().await;
    server.route("/rt-parent", vec![Part::Html(row_page("rtp", 1))]);
    let _m = mount("rt-parent", "/rt-parent", "ra-button").await;
    let h = row();
    assert!(h.frame() != h.frames().top(), "the entry renders in its frame, not the top frame");
    assert!(h.frames().top() == Frames.top());
    assert!(h.frame().src().ends_with("/ra-row"));
}

#[wasm_bindgen_test]
async fn dispatches_reload_events_for_a_client_entrys_frame_and_a_named_frame() {
    let server = start().await;
    server.route("/re-parent", vec![Part::Html(row_page("rep", 1))]);
    server.route("/ra-summary", vec![Part::Html(r#"<p id="ra-summary">Summary: 2</p>"#.into())]);
    server.route("/ra-row", vec![Part::Html(r#"<!--rmx:h:reh2--><button id="ra-button">Update</button><!--/rmx:h-->"#.to_string() + &{
        let mut d = HydrationData::default();
        d.entries.push(("reh2".into(), "RowAction".into(), r#"{"label":""}"#.into()));
        d.to_script()
    })]);
    let _m = mount("re-parent", "/re-parent", "ra-button").await;
    SUMMARY_EVENTS.with(|e| e.set((0, 0)));
    ROW_EVENTS.with(|e| e.set((0, 0)));

    let h = row();
    let summary = h.frames().get("ra-summary").expect("the summary frame");
    let bump = |cell: &'static std::thread::LocalKey<Cell<(u32, u32)>>, start: bool| {
        cell.with(|e| {
            let (s, c) = e.get();
            e.set(if start { (s + 1, c) } else { (s, c + 1) });
        })
    };
    h.on_frame(&summary, FrameEvent::ReloadStart, move |_, _| bump(&SUMMARY_EVENTS, true));
    h.on_frame(&summary, FrameEvent::ReloadComplete, move |_, _| bump(&SUMMARY_EVENTS, false));
    h.on_frame(&h.frame(), FrameEvent::ReloadStart, move |_, _| bump(&ROW_EVENTS, true));
    h.on_frame(&h.frame(), FrameEvent::ReloadComplete, move |_, _| bump(&ROW_EVENTS, false));

    let summary_done = reload(summary);
    let row_done = reload(h.frame());
    until("both reloads", || summary_done.get() && row_done.get()).await;
    assert_eq!(SUMMARY_EVENTS.with(Cell::get), (1, 1));
    assert_eq!(ROW_EVENTS.with(Cell::get), (1, 1));
    assert_eq!(text("ra-summary").as_deref(), Some("Summary: 2"));
}

thread_local! {
    static CART: RefCell<Option<Handle<CartItems>>> = const { RefCell::new(None) };
    static CART_PENDING: Cell<bool> = const { Cell::new(false) };
}

/// A client entry that is the whole content of its frame, which can reload it away.
struct CartItems;

impl Component for CartItems {
    type Props = EntryProps;
    rmx::client_entry!("CartItems");
    fn setup(_: &EntryProps, h: &Handle<Self>) -> Self {
        CART.with(|c| *c.borrow_mut() = Some(h.clone()));
        CartItems
    }
    fn render(&self, _: &EntryProps, _: &Handle<Self>) -> Node {
        let pending = CART_PENDING.with(Cell::get).then(|| rsx! { <p id="cart-pending">"Updating your cart..."</p> });
        rsx! {
            <>
                {pending.unwrap_or(Node::Empty)}
                <table id="cart-items"><tbody><tr><td>"Book"</td><td><button>"Remove"</button></td></tr></tbody></table>
                <div id="cart-total">"Total: $16.99"</div>
            </>
        }
    }
}

fn cart_markup(entry_id: &str) -> String {
    let mut d = HydrationData::default();
    d.entries.push((entry_id.into(), "CartItems".into(), r#"{"label":""}"#.into()));
    format!(
        r#"<!--rmx:h:{entry_id}--><table id="cart-items"><tbody><tr><td>Book</td><td><button>Remove</button></td></tr></tbody></table><div id="cart-total">Total: $16.99</div><!--/rmx:h-->{}"#,
        d.to_script()
    )
}

#[wasm_bindgen_test]
async fn ignores_updates_from_a_client_entry_removed_by_its_frame_reload() {
    let server = start().await;
    let mut d = HydrationData::default();
    d.frames.push(("cif".into(), "/ci-cart".into(), None, "resolved"));
    server.route("/ci-cart-parent", vec![Part::Html(format!("<main>{}</main>{}", region("cif", &cart_markup("cih")), d.to_script()))]);
    let _m = mount("ci-cart-parent", "/ci-cart-parent", "cart-items").await;
    let errors = Rc::new(RefCell::new(Vec::<String>::new()));
    let seen = errors.clone();
    let app = APP.with(|a| a.borrow().clone()).expect("app");
    let listener = app.on_error(move |e| seen.borrow_mut().push(e.message.clone()));

    let h = CART.with(|c| c.borrow().clone()).expect("a hydrated CartItems");
    CART_PENDING.with(|p| p.set(true));
    h.update();
    rmx::flush();
    assert!(by_id("cart-pending").is_some());
    server.route("/ci-cart", vec![Part::Html(r#"<p id="empty-cart">Your cart is empty.</p>"#.into())]);
    h.frame().reload_async(ReloadOptions::default()).await;
    CART_PENDING.with(|p| p.set(false));
    h.update();
    rmx::flush();
    for _ in 0..3 {
        tick().await;
    }
    app.remove_error_listener(listener);

    assert!(by_id("cart-items").is_none());
    assert_eq!(text("empty-cart").as_deref(), Some("Your cart is empty."));
    assert!(by_id("cart-pending").is_none());
    assert_eq!(*errors.borrow(), Vec::<String>::new());
}
