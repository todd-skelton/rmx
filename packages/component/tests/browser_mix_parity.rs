//! Ports of Remix's `mix-ssr-parity.test.tsx`: the same `mix` list gives the same host
//! attributes from a client render and from server HTML. Remix cases that Rust rules out at
//! compile time are not ported: a mixin returning an element of another host type or a
//! non-element (rmx mixins patch the host in place), and a throwing mixin (a Rust panic is
//! not caught). Run with `pwsh ./scripts/test-browser.ps1 -Test browser_mix_parity`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::cell::Cell;
use std::collections::BTreeMap;

use rmx::{create_root, mixin, on, render_to_string, rsx, unsafe_html, AttrValue, Element, EventHandler, HostMixin, Mixin, MixinHandle, Node};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

type Attributes = BTreeMap<String, String>;

fn container() -> web_sys::HtmlElement {
    web_sys::window().unwrap().document().unwrap().create_element("div").unwrap().unchecked_into()
}

fn collect(el: &web_sys::Element) -> Attributes {
    el.get_attribute_names()
        .iter()
        .filter_map(|name| name.as_string())
        .map(|name| {
            let value = el.get_attribute(&name).unwrap_or_default();
            (name, value)
        })
        .collect()
}

fn client_attributes(node: Node) -> Attributes {
    let el = container();
    let root = create_root(&el);
    root.render(node);
    root.flush();
    let attrs = collect(&el.query_selector("div").unwrap().expect("a div"));
    root.dispose();
    attrs
}

fn server_attributes(node: &Node) -> Attributes {
    let el = container();
    el.set_inner_html(&render_to_string(node));
    collect(&el.query_selector("div").unwrap().expect("a div"))
}

fn attributes(list: &[(&str, &str)]) -> Attributes {
    list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn expect_parity(make: impl Fn() -> Node, expected: &[(&str, &str)]) {
    let expected = attributes(expected);
    assert_eq!(client_attributes(make()), expected, "client");
    assert_eq!(server_attributes(&make()), expected, "server");
}

fn noop() -> EventHandler {
    EventHandler::new(|_| {})
}

fn title_of(host: &Element) -> String {
    match host.attr("title") {
        Some(AttrValue::Str(s)) => s.to_string(),
        _ => String::new(),
    }
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

struct AppendTitle;

impl HostMixin for AppendTitle {
    type Args = &'static str;
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        AppendTitle
    }
    fn render(&mut self, suffix: &&'static str, host: &mut Element, _: &MixinHandle<Self>) {
        host.set_attr("title", format!("{}{suffix}", title_of(host)));
    }
}

fn with_title(title: &'static str) -> Mixin {
    mixin::<WithTitle>(title)
}

fn append_title(suffix: &'static str) -> Mixin {
    mixin::<AppendTitle>(suffix)
}

#[wasm_bindgen_test]
fn preserves_host_props_when_every_mixin_is_a_dom_event_listener() {
    expect_parity(
        || rsx! { <div id="host" title="unchanged" mix={[on("click", noop()), on("keydown", noop())]}>"Child content"</div> },
        &[("id", "host"), ("title", "unchanged")],
    );
    assert_eq!(render_to_string(&rsx! { <div mix={on("click", noop())}>"Child content"</div> }), "<div>Child content</div>");
}

#[wasm_bindgen_test]
fn preserves_props_with_empty_and_conditional_event_mixins() {
    expect_parity(|| rsx! { <div id="empty" mix={Vec::<Mixin>::new()}></div> }, &[("id", "empty")]);
    expect_parity(|| rsx! { <div id="conditional" mix={[None, Some(on("click", noop())), None]}></div> }, &[("id", "conditional")]);
}

#[wasm_bindgen_test]
fn composes_prop_mixins_around_dom_event_listeners() {
    expect_parity(
        || rsx! { <div mix={[with_title("hello"), on("click", noop()), append_title("-world")]}></div> },
        &[("title", "hello-world")],
    );
}

struct WithEvent;

impl HostMixin for WithEvent {
    type Args = ();
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        WithEvent
    }
    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        host.mix(on("click", noop()));
    }
}

#[wasm_bindgen_test]
fn preserves_props_when_a_mixin_adds_dom_event_listeners() {
    expect_parity(|| rsx! { <div id="host" mix={mixin::<WithEvent>(())}></div> }, &[("id", "host")]);
}

#[wasm_bindgen_test]
fn composes_mixins_in_order_with_the_same_result_on_both_sides() {
    expect_parity(
        || rsx! { <div id="host" mix={[with_title("hello"), append_title("-world")]}></div> },
        &[("id", "host"), ("title", "hello-world")],
    );
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

/// Adds `WithData` to the host (Remix: `<handle.element mix={[withData(value)]}>` or
/// returning `[withData(value)]`; both are `host.mix(..)` here).
struct WithNested;

impl HostMixin for WithNested {
    type Args = &'static str;
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        WithNested
    }
    fn render(&mut self, value: &&'static str, host: &mut Element, _: &MixinHandle<Self>) {
        host.mix(vec![mixin::<WithData>(*value)]);
    }
}

#[wasm_bindgen_test]
fn expands_nested_mix_descriptors_identically() {
    expect_parity(|| rsx! { <div mix={[mixin::<WithNested>("nested")]}></div> }, &[("data-mixed", "nested")]);
}

thread_local! {
    static SAW_TREE_PROPS: Cell<bool> = const { Cell::new(false) };
    static RUNS: Cell<u32> = const { Cell::new(0) };
}

struct Inspect;

impl HostMixin for Inspect {
    type Args = ();
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        Inspect
    }
    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        if !host.children.is_empty() || host.attr("innerHTML").is_some() {
            SAW_TREE_PROPS.with(|s| s.set(true));
        }
    }
}

#[wasm_bindgen_test]
fn strips_children_and_inner_html_from_mixin_visible_props_on_both_sides() {
    SAW_TREE_PROPS.with(|s| s.set(false));
    expect_parity(|| rsx! { <div mix={[mixin::<Inspect>(())]}>"child"</div> }, &[]);
    expect_parity(|| rsx! { <div mix={[mixin::<Inspect>(())]} innerHTML={unsafe_html("child")}></div> }, &[]);
    assert!(!SAW_TREE_PROPS.with(Cell::get));
}

#[wasm_bindgen_test]
fn ignores_falsy_mix_entries_identically() {
    expect_parity(|| rsx! { <div mix={[None, None, Some(with_title("kept"))]}></div> }, &[("title", "kept")]);
}

/// Adds itself again on every render.
struct Recursive;

impl HostMixin for Recursive {
    type Args = ();
    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        Recursive
    }
    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        RUNS.with(|r| r.set(r.get() + 1));
        host.mix(mixin::<Recursive>(()));
    }
}

#[wasm_bindgen_test]
fn caps_runaway_descriptor_expansion_identically_on_both_sides() {
    let make = || rsx! { <div mix={[mixin::<Recursive>(())]}></div> };
    RUNS.with(|r| r.set(0));
    client_attributes(make());
    assert_eq!(RUNS.with(Cell::get), 1024);
    RUNS.with(|r| r.set(0));
    server_attributes(&make());
    assert_eq!(RUNS.with(Cell::get), 1024);
}
