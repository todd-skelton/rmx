//! Ports of Remix's `vdom.mixins.test.tsx`. A Remix `createMixin` is a [`HostMixin`] here;
//! its render patches the host `Element` in place instead of returning `<handle.element>`.
//! Run with `pwsh ./scripts/test-browser.ps1 -Test browser_mixins`.
#![cfg(target_arch = "wasm32")]

extern crate rsmix_component as rsmix;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rsmix::{
    create_root, mixin, node_ref, on, rsx, unsafe_html, AttrValue, Children, Component, Element, EventHandler, Handle, HostMixin,
    IntoNode, Mixin, MixinHandle, Node,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn container() -> web_sys::HtmlElement {
    web_sys::window().unwrap().document().unwrap().create_element("div").unwrap().unchecked_into()
}

fn find(el: &web_sys::HtmlElement, selector: &str) -> web_sys::HtmlElement {
    el.query_selector(selector).unwrap().unwrap_or_else(|| panic!("no {selector}")).unchecked_into()
}

fn text(v: Option<&AttrValue>) -> String {
    match v {
        Some(AttrValue::Str(s)) => s.to_string(),
        _ => String::new(),
    }
}

thread_local! {
    static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn log(s: impl Into<String>) {
    LOG.with(|l| l.borrow_mut().push(s.into()));
}

fn take_log() -> Vec<String> {
    LOG.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

/// Replace `console.error` while `f` runs and return the messages of the errors it got.
fn capture_errors(f: impl FnOnce()) -> Vec<String> {
    let console = js_sys::Reflect::get(&js_sys::global(), &"console".into()).unwrap();
    let original = js_sys::Reflect::get(&console, &"error".into()).unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    let spy = Closure::<dyn Fn(JsValue)>::new(move |v: JsValue| {
        let message = v.dyn_ref::<js_sys::Error>().map(|e| String::from(e.message())).unwrap_or_else(|| format!("{v:?}"));
        s.borrow_mut().push(message);
    });
    js_sys::Reflect::set(&console, &"error".into(), spy.as_ref()).unwrap();
    f();
    js_sys::Reflect::set(&console, &"error".into(), &original).unwrap();
    let out = seen.borrow().clone();
    out
}

async fn tick() {
    JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await.unwrap();
}

async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms).unwrap();
    });
    JsFuture::from(promise).await.unwrap();
}

/// Records the attribute names and whether children were visible.
struct Inspect;

impl HostMixin for Inspect {
    type Args = ();
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        Inspect
    }
    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        let names: Vec<&str> = host.attrs.iter().map(|(n, _)| *n).collect();
        log(format!("attrs:{}", names.join(",")));
        log(format!("children:{}", host.children.len()));
    }
}

#[wasm_bindgen_test]
fn does_not_add_mix_to_props_seen_by_a_mixin_on_an_empty_host() {
    take_log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <input title="unchanged" mix={mixin::<Inspect>(())}/> });
    root.flush();
    assert_eq!(el.inner_html(), r#"<input title="unchanged">"#);
    assert_eq!(take_log(), ["attrs:title", "children:0"]);
}

/// Appends its argument to `title`, logging the title it saw.
struct Append;

impl HostMixin for Append {
    type Args = &'static str;
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        Append
    }
    fn render(&mut self, suffix: &&'static str, host: &mut Element, _: &MixinHandle<Self>) {
        let title = text(host.attr("title"));
        log(title.clone());
        host.set_attr("title", format!("{title}{suffix}"));
    }
}

#[wasm_bindgen_test]
fn keeps_original_props_and_earlier_mixin_inputs_intact() {
    take_log();
    let element = rsx! { <div title="base" mix={[mixin::<Append>("-one"), mixin::<Append>("-two")]}>"Child"</div> };
    let el = container();
    let root = create_root(&el);
    root.render(element.clone());
    root.flush();
    assert_eq!(el.inner_html(), r#"<div title="base-one-two">Child</div>"#);
    let Node::Element(original) = &element else { panic!("element") };
    assert_eq!(text(original.attr("title")), "base");
    assert_eq!(original.children.len(), 1);
    assert_eq!(take_log(), ["base", "base-one"]);
}

struct WithTitle;

impl HostMixin for WithTitle {
    type Args = &'static str;
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        WithTitle
    }
    fn render(&mut self, title: &&'static str, host: &mut Element, _: &MixinHandle<Self>) {
        host.set_attr("title", *title);
    }
}

#[wasm_bindgen_test]
fn composes_mixins_in_order_and_does_not_leak_mix_to_the_dom() {
    take_log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={[mixin::<WithTitle>("hello"), mixin::<Append>("-world")]}></div> });
    let div = find(&el, "div");
    assert_eq!(div.get_attribute("title").as_deref(), Some("hello-world"));
    assert!(!div.has_attribute("mix"));
}

struct WithData;

impl HostMixin for WithData {
    type Args = &'static str;
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        WithData
    }
    fn render(&mut self, value: &&'static str, host: &mut Element, _: &MixinHandle<Self>) {
        host.set_attr("data-mixed", *value);
    }
}

/// Adds `WithData` from inside its own render (a nested descriptor).
struct WithNested;

impl HostMixin for WithNested {
    type Args = &'static str;
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        WithNested
    }
    fn render(&mut self, value: &&'static str, host: &mut Element, _: &MixinHandle<Self>) {
        host.mix(mixin::<WithData>(value));
    }
}

#[wasm_bindgen_test]
fn supports_nested_mix_descriptors() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={mixin::<WithNested>("nested")}></div> });
    assert_eq!(find(&el, "div").get_attribute("data-mixed").as_deref(), Some("nested"));
}

#[wasm_bindgen_test]
fn patches_the_host_and_keeps_its_children() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={mixin::<WithData>("created")}>"child"</div> });
    root.flush();
    let div = find(&el, "div");
    assert_eq!(div.get_attribute("data-mixed").as_deref(), Some("created"));
    assert_eq!(div.text_content().as_deref(), Some("child"));
}

#[wasm_bindgen_test]
fn strips_children_and_raw_html_props_before_mixins_run() {
    take_log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={mixin::<Inspect>(())}>"child"</div> });
    root.flush();
    root.render(rsx! { <div innerHTML={unsafe_html("<strong>html</strong>")} mix={mixin::<Inspect>(())}></div> });
    root.flush();
    root.render(rsx! { <iframe srcDoc={unsafe_html("<p>html</p>")} mix={mixin::<Inspect>(())}></iframe> });
    root.flush();
    assert_eq!(take_log(), ["attrs:", "children:0", "attrs:", "children:0", "attrs:", "children:0"]);
    // The host's own raw HTML still applies.
    assert_eq!(find(&el, "iframe").get_attribute("srcdoc").as_deref(), Some("<p>html</p>"));
}

/// Tries to give the host children.
struct WithChildren;

impl HostMixin for WithChildren {
    type Args = ();
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        WithChildren
    }
    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        host.set_attr("data-mode", "children");
        host.children.push("blocked".into_node());
    }
}

#[wasm_bindgen_test]
fn ignores_children_added_by_mixins_while_preserving_host_content() {
    let el = container();
    let root = create_root(&el);
    let errors = capture_errors(|| {
        root.render(rsx! { <div mix={mixin::<WithChildren>(())}>"safe"</div> });
        root.flush();
    });
    let div = find(&el, "div");
    assert_eq!(div.get_attribute("data-mode").as_deref(), Some("children"));
    assert_eq!(div.text_content().as_deref(), Some("safe"));
    assert_eq!(errors, ["mixin elements must not receive children"]);
}

/// Tries to replace the host's content with `innerHTML`.
struct WithInnerHtml;

impl HostMixin for WithInnerHtml {
    type Args = ();
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        WithInnerHtml
    }
    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        host.set_attr("data-mode", "innerHTML");
        host.set_attr("innerHTML", unsafe_html("<strong>blocked</strong>"));
    }
}

#[wasm_bindgen_test]
fn ignores_inner_html_added_by_mixins_while_preserving_host_content() {
    let el = container();
    let root = create_root(&el);
    let errors = capture_errors(|| {
        root.render(rsx! { <div mix={mixin::<WithInnerHtml>(())}>"safe"</div> });
        root.flush();
    });
    let div = find(&el, "div");
    assert_eq!(div.get_attribute("data-mode").as_deref(), Some("innerHTML"));
    assert_eq!(div.text_content().as_deref(), Some("safe"));
    assert_eq!(errors, ["mixins must not return children or raw HTML props"]);
}

#[wasm_bindgen_test]
fn ignores_iframe_document_and_outer_html_props_added_by_mixins() {
    // A plain (stateless) mixin gets the same treatment.
    let raw = Mixin::new(|host: &mut Element| {
        host.set_attr("srcDoc", "<p>camel</p>");
        host.set_attr("srcdoc", "<p>lowercase</p>");
        host.set_attr("outerHTML", "<p>replacement</p>");
    });
    let el = container();
    let root = create_root(&el);
    let errors = capture_errors(|| {
        root.render(rsx! { <iframe mix={raw}></iframe> });
        root.flush();
    });
    assert!(!find(&el, "iframe").has_attribute("srcdoc"));
    assert_eq!(el.inner_html(), "<iframe></iframe>");
    assert_eq!(errors, ["mixins must not return children or raw HTML props"]);
}

/// Returns a list of descriptors (with gaps) instead of patching the host itself.
struct WithReturnedMix;

impl HostMixin for WithReturnedMix {
    type Args = &'static str;
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        WithReturnedMix
    }
    fn render(&mut self, value: &&'static str, host: &mut Element, _: &MixinHandle<Self>) {
        host.mix([None, Some(vec![mixin::<WithData>(value)]), None]);
    }
}

#[wasm_bindgen_test]
fn supports_mixins_returning_nested_descriptors_directly() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={mixin::<WithReturnedMix>("returned")}></div> });
    root.flush();
    assert_eq!(find(&el, "div").get_attribute("data-mixed").as_deref(), Some("returned"));
}

#[derive(Clone, Default)]
struct ButtonProps {
    mix: Vec<Mixin>,
    children: Children,
}

struct Button;

impl Component for Button {
    type Props = ButtonProps;
    fn setup(_: &ButtonProps, _: &Handle<Self>) -> Self {
        Button
    }
    fn render(&self, props: &ButtonProps, _: &Handle<Self>) -> Node {
        rsx! { <button mix={[vec![mixin::<WithTitle>("base")], props.mix.clone()]}>{&props.children}</button> }
    }
}

#[wasm_bindgen_test]
fn normalizes_component_mix_props_so_wrapped_hosts_can_compose_them() {
    take_log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <Button mix={mixin::<Append>("-override")}>"Click"</Button> });
    root.flush();
    let button = find(&el, "button");
    assert_eq!(button.get_attribute("title").as_deref(), Some("base-override"));
    assert!(!button.has_attribute("mix"));
    assert_eq!(button.text_content().as_deref(), Some("Click"));
}

/// Logs the shared host id it was set up with.
struct HostId<const N: u8>;

impl<const N: u8> HostMixin for HostId<N> {
    type Args = ();
    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        log(h.id().to_string());
        HostId
    }
}

#[wasm_bindgen_test]
fn shares_one_host_identity_across_mixins_on_the_same_host_node() {
    take_log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={[mixin::<HostId<1>>(()), mixin::<HostId<2>>(()), mixin::<HostId<3>>(())]}></div> });
    root.flush();
    let ids = take_log();
    assert_eq!(ids.len(), 3);
    assert_eq!(ids[0], ids[1]);
    assert_eq!(ids[1], ids[2]);
}

struct ProviderValue(&'static str);

#[derive(Clone, Default)]
struct ProviderProps {
    children: Children,
}

struct Provider;

impl Component for Provider {
    type Props = ProviderProps;
    fn setup(_: &ProviderProps, h: &Handle<Self>) -> Self {
        h.set_context(ProviderValue("from-context"));
        Provider
    }
    fn render(&self, props: &ProviderProps, _: &Handle<Self>) -> Node {
        rsx! { <section>{&props.children}</section> }
    }
}

struct WithContextValue;

impl HostMixin for WithContextValue {
    type Args = ();
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        WithContextValue
    }
    fn render(&mut self, _: &(), host: &mut Element, h: &MixinHandle<Self>) {
        let value = h.context::<ProviderValue>().expect("provider");
        host.set_attr("data-value", value.0);
    }
}

#[wasm_bindgen_test]
fn reads_ancestor_component_context_from_mixins() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <Provider><div mix={mixin::<WithContextValue>(())}></div></Provider> });
    root.flush();
    assert_eq!(find(&el, "div").get_attribute("data-value").as_deref(), Some("from-context"));
}

thread_local! {
    static SIGNALS: RefCell<Vec<web_sys::AbortSignal>> = const { RefCell::new(Vec::new()) };
}

/// Keeps its slot's signal.
struct WithSignal<const N: u8>;

impl<const N: u8> HostMixin for WithSignal<N> {
    type Args = ();
    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        SIGNALS.with(|s| s.borrow_mut().push(h.signal()));
        WithSignal
    }
}

fn take_signals() -> Vec<web_sys::AbortSignal> {
    SIGNALS.with(|s| std::mem::take(&mut *s.borrow_mut()))
}

#[wasm_bindgen_test]
fn aborts_the_signal_when_the_host_node_is_removed() {
    take_signals();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={mixin::<WithSignal<0>>(())}></div> });
    root.flush();
    let signal = take_signals().pop().unwrap();
    assert!(!signal.aborted());
    root.render(());
    root.flush();
    assert!(signal.aborted());
}

#[wasm_bindgen_test]
fn aborts_the_signal_when_a_mixin_slot_is_removed_while_the_host_stays_mounted() {
    take_signals();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={[mixin::<WithSignal<1>>(()), mixin::<WithSignal<2>>(())]}></div> });
    root.flush();
    let signals = take_signals();
    let (kept, removed) = (&signals[0], &signals[1]);
    assert!(!kept.aborted() && !removed.aborted());
    let div = find(&el, "div");
    root.render(rsx! { <div mix={mixin::<WithSignal<1>>(())}></div> });
    root.flush();
    assert!(!kept.aborted());
    assert!(removed.aborted());
    assert!(find(&el, "div").is_same_node(Some(&div)));
    assert!(take_signals().is_empty(), "the kept slot was not set up again");
}

/// Setup only: no render.
struct Passthrough;

impl HostMixin for Passthrough {
    type Args = ();
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        Passthrough
    }
}

#[wasm_bindgen_test]
fn supports_setup_only_passthrough_mixins() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={[mixin::<Passthrough>(()), mixin::<WithTitle>("ok")]}></div> });
    root.flush();
    assert_eq!(find(&el, "div").get_attribute("title").as_deref(), Some("ok"));
}

#[wasm_bindgen_test]
fn does_not_duplicate_on_handlers_for_passthrough_mixins() {
    let clicks = Rc::new(Cell::new(0));
    let c = clicks.clone();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <button mix={[mixin::<Passthrough>(()), on("click", EventHandler::new(move |_| c.set(c.get() + 1)))]}></button> });
    root.flush();
    let init = web_sys::MouseEventInit::new();
    init.set_bubbles(true);
    find(&el, "button").dispatch_event(&web_sys::MouseEvent::new_with_mouse_event_init_dict("click", &init).unwrap()).unwrap();
    root.flush();
    assert_eq!(clicks.get(), 1);
}

/// Sets `id` and logs its `remove` events.
struct RemoveLog<const N: u8>;

impl<const N: u8> HostMixin for RemoveLog<N> {
    type Args = ();
    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        h.on_remove(|_, _| log(format!("remove:{N}")));
        RemoveLog
    }
    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        host.set_attr("id", if N == 0 { "a" } else { "b" });
    }
}

#[wasm_bindgen_test]
fn runs_remove_lifecycle_when_descriptor_type_changes_and_on_unmount() {
    take_log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={mixin::<RemoveLog<0>>(())}></div> });
    root.render(rsx! { <div mix={mixin::<RemoveLog<1>>(())}></div> });
    assert_eq!(find(&el, "div").id(), "b");
    root.render(());
    assert_eq!(take_log(), ["remove:0", "remove:1"]);
}

/// Logs `beforeRemove`; `persist` keeps the node until the stored resolver runs.
struct BeforeRemove {
    persist: bool,
}

thread_local! {
    static RESOLVE: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
}

fn resolve_pending() {
    if let Some(resolve) = RESOLVE.with(|r| r.borrow_mut().take()) {
        resolve.call0(&JsValue::NULL).unwrap();
    }
}

impl HostMixin for BeforeRemove {
    type Args = bool;
    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        h.on_insert(|_, _, _| log("insert"));
        h.on_reclaimed(|_, _, _| log("reclaimed"));
        h.on_remove(|_, _| log("remove"));
        h.on_before_remove(|this, _, event| {
            log("beforeRemove");
            if this.persist {
                event.persist_node(|signal: web_sys::AbortSignal| async move {
                    let pending = js_sys::Promise::new(&mut |resolve, _| {
                        let on_abort = resolve.clone();
                        let listener = Closure::once_into_js(move || {
                            let _ = on_abort.call0(&JsValue::NULL);
                        });
                        signal.add_event_listener_with_callback("abort", listener.unchecked_ref()).unwrap();
                        RESOLVE.with(|r| *r.borrow_mut() = Some(resolve));
                    });
                    let _ = JsFuture::from(pending).await;
                });
            }
        });
        BeforeRemove { persist: false }
    }
    fn render(&mut self, persist: &bool, host: &mut Element, _: &MixinHandle<Self>) {
        self.persist = *persist;
        host.set_attr("id", "target");
    }
}

#[wasm_bindgen_test]
fn dispatches_before_remove_once() {
    take_log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={mixin::<BeforeRemove>(false)}></div> });
    root.flush();
    root.render(());
    root.flush();
    assert_eq!(take_log(), ["insert", "beforeRemove", "remove"]);
    assert_eq!(el.inner_html(), "");
}

/// Logs `insert` with the node it was given.
struct InsertNode;

thread_local! {
    static INSERTED: RefCell<Option<web_sys::Element>> = const { RefCell::new(None) };
}

impl HostMixin for InsertNode {
    type Args = ();
    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        h.on_insert(|_, _, event| {
            INSERTED.with(|n| *n.borrow_mut() = Some(event.node.element().clone()));
            log("insert");
        });
        InsertNode
    }
    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        host.set_attr("id", "inserted");
    }
}

#[wasm_bindgen_test]
fn runs_insert_lifecycle_with_the_bound_host_node() {
    take_log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={mixin::<InsertNode>(())}></div> });
    root.flush();
    root.render(rsx! { <div mix={mixin::<InsertNode>(())}></div> });
    root.flush();
    let div = find(&el, "#inserted");
    assert!(INSERTED.with(|n| n.borrow().as_ref().is_some_and(|n| n.is_same_node(Some(&div)))));
    assert_eq!(take_log(), ["insert"]);
}

struct UpdateLifecycle;

impl HostMixin for UpdateLifecycle {
    type Args = &'static str;
    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        let step = |node: &rsmix::NodeHandle| node.element().get_attribute("data-step").unwrap_or_default();
        h.on_before_update(move |_, _, node| log(format!("before:{}", step(node))));
        h.on_commit(move |_, _, node| log(format!("commit:{}", step(node))));
        UpdateLifecycle
    }
    fn render(&mut self, step: &&'static str, host: &mut Element, _: &MixinHandle<Self>) {
        host.set_attr("data-step", *step);
    }
}

#[wasm_bindgen_test]
fn runs_before_update_and_commit_lifecycle_events_in_update_order() {
    take_log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={mixin::<UpdateLifecycle>("0")}></div> });
    root.flush();
    root.render(rsx! { <div mix={mixin::<UpdateLifecycle>("1")}></div> });
    root.flush();
    assert_eq!(take_log(), ["before:0", "commit:1"]);
}

fn mark(name: &'static str) -> Mixin {
    node_ref(move |node, _| {
        log(name);
        let _ = node.element().set_attribute(&format!("data-{name}"), "1");
    })
}

/// Adds a `node_ref` mixin named by its argument.
struct WithConnect;

impl HostMixin for WithConnect {
    type Args = &'static str;
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        WithConnect
    }
    fn render(&mut self, name: &&'static str, host: &mut Element, _: &MixinHandle<Self>) {
        host.mix(mark(name));
    }
}

#[wasm_bindgen_test]
fn composes_ref_callbacks_across_mixins_and_base_mix() {
    take_log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={[mixin::<WithConnect>("a"), mixin::<WithConnect>("b"), mark("base")]}></div> });
    root.flush();
    let div = find(&el, "div");
    for name in ["data-a", "data-b", "data-base"] {
        assert_eq!(div.get_attribute(name).as_deref(), Some("1"), "{name}");
    }
    let mut calls = take_log();
    calls.sort();
    assert_eq!(calls, ["a", "b", "base"]);
}

/// Adds a click listener that logs its argument.
struct WithOn;

impl HostMixin for WithOn {
    type Args = &'static str;
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        WithOn
    }
    fn render(&mut self, name: &&'static str, host: &mut Element, _: &MixinHandle<Self>) {
        let name = *name;
        host.mix(on("click", EventHandler::new(move |_| log(name))));
    }
}

#[wasm_bindgen_test]
fn composes_on_mixins_across_nested_mixins() {
    take_log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! {
        <button mix={[mixin::<WithOn>("a"), mixin::<WithOn>("b"), on("click", EventHandler::new(|_| log("base")))]}>"click"</button>
    });
    root.flush();
    find(&el, "button").click();
    root.flush();
    assert_eq!(take_log(), ["base", "a", "b"]);
}

#[wasm_bindgen_test]
fn supports_on_mixin_helper_composition_standalone() {
    take_log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! {
        <button mix={[on("click", EventHandler::new(|_| log("first"))), on("click", EventHandler::new(|_| log("second")))]}>"click"</button>
    });
    root.flush();
    find(&el, "button").click();
    root.flush();
    assert_eq!(take_log(), ["first", "second"]);
}

/// Counts clicks in its own state and re-renders only the host.
struct Counter {
    count: u32,
}

impl HostMixin for Counter {
    type Args = ();
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        Counter { count: 0 }
    }
    fn render(&mut self, _: &(), host: &mut Element, h: &MixinHandle<Self>) {
        host.set_attr("data-count", self.count.to_string());
        host.mix(on("click", h.on(|this, h| {
            this.count += 1;
            h.update();
        })));
    }
}

thread_local! {
    static APP_RENDERS: Cell<u32> = const { Cell::new(0) };
}

struct App;

impl Component for App {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        App
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        APP_RENDERS.with(|c| c.set(c.get() + 1));
        rsx! { <button mix={mixin::<Counter>(())}>"click"</button> }
    }
}

#[wasm_bindgen_test]
fn updates_only_host_props_when_a_mixin_calls_update() {
    APP_RENDERS.with(|c| c.set(0));
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <App/> });
    root.flush();
    let button = find(&el, "button");
    assert_eq!(button.get_attribute("data-count").as_deref(), Some("0"));
    assert_eq!(APP_RENDERS.with(Cell::get), 1);
    button.click();
    root.flush();
    assert_eq!(button.get_attribute("data-count").as_deref(), Some("1"));
    assert_eq!(APP_RENDERS.with(Cell::get), 1);
}

#[wasm_bindgen_test]
async fn dispatches_reclaimed_on_persisted_reuse_without_rerunning_insert_or_remove() {
    take_log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div key="reclaimed" mix={mixin::<BeforeRemove>(true)}></div> });
    root.flush();
    assert_eq!(take_log(), ["insert"]);

    root.render(());
    root.flush();
    tick().await;
    assert_eq!(take_log(), ["beforeRemove"]);

    root.render(rsx! { <div key="reclaimed" mix={mixin::<BeforeRemove>(true)}></div> });
    root.flush();
    tick().await;
    assert_eq!(take_log(), ["reclaimed"]);
    resolve_pending();
    sleep(0).await;
    assert_eq!(take_log(), Vec::<String>::new());
    assert!(el.query_selector("#target").unwrap().is_some());
}

#[wasm_bindgen_test]
async fn defers_host_removal_when_before_remove_persists_the_node() {
    take_log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div key="deferred" mix={mixin::<BeforeRemove>(true)}></div> });
    root.flush();
    let target = find(&el, "#target");

    root.render(());
    root.flush();
    tick().await;
    assert_eq!(take_log(), ["insert", "beforeRemove"]);
    assert!(find(&el, "#target").is_same_node(Some(&target)));

    resolve_pending();
    sleep(0).await;
    assert_eq!(take_log(), ["remove"]);
    assert!(el.query_selector("#target").unwrap().is_none());
}
