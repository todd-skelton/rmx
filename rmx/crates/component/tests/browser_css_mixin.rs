//! Ports of Remix's `css-mixin.test.tsx`, `attrs-mixin.test.tsx` and
//! `ref-mixin.test.tsx`. rmx's `css()` takes CSS text rather than a style object, so the
//! object cases become the equivalent declarations. Run with
//! `pwsh ./test-browser.ps1 -Test browser_css_mixin`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::cell::{Cell, RefCell};

use rmx::{attrs, create_root, css, mixin, node_ref, rsx, AttrValue, Component, Element, Handle, HostMixin, MixinHandle, Node};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn container() -> web_sys::HtmlElement {
    document().create_element("div").unwrap().unchecked_into()
}

fn find(el: &web_sys::HtmlElement, selector: &str) -> web_sys::HtmlElement {
    el.query_selector(selector).unwrap().unwrap_or_else(|| panic!("no {selector}")).unchecked_into()
}

fn classes(el: &web_sys::Element) -> Vec<String> {
    el.class_name().split_whitespace().map(String::from).collect()
}

fn generated(el: &web_sys::Element) -> Vec<String> {
    classes(el).into_iter().filter(|c| c.starts_with("rmxc-")).collect()
}

fn adopted_texts() -> Vec<String> {
    let list: js_sys::Array = js_sys::Reflect::get(&document(), &"adoptedStyleSheets".into()).unwrap().unchecked_into();
    list.iter()
        .map(|s| {
            let rules = s.unchecked_into::<web_sys::CssStyleSheet>().css_rules().unwrap();
            (0..rules.length()).map(|i| rules.item(i).unwrap().css_text()).collect::<Vec<_>>().join("\n")
        })
        .collect()
}

fn count_adopted_rules() -> u32 {
    let list: js_sys::Array = js_sys::Reflect::get(&document(), &"adoptedStyleSheets".into()).unwrap().unchecked_into();
    list.iter().map(|s| s.unchecked_into::<web_sys::CssStyleSheet>().css_rules().unwrap().length()).sum()
}

fn rule_text_present(text: &str) -> bool {
    adopted_texts().iter().any(|t| t.contains(text))
}

fn color(el: &web_sys::Element) -> String {
    web_sys::window().unwrap().get_computed_style(el).unwrap().unwrap().get_property_value("color").unwrap()
}

// css mixin

#[wasm_bindgen_test]
fn styles_select_elements() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <select mix={[css("background-color: white"), css("color: red")]}></select> });
    root.flush();
    assert!(!generated(&find(&el, "select")).is_empty());
}

#[wasm_bindgen_test]
fn concatenates_generated_classes_with_existing_class_name() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div class="base" mix={[css("color: red"), css("background-color: blue")]}></div> });
    root.flush();
    let div = find(&el, "div");
    assert!(classes(&div).contains(&"base".to_string()));
    let mut ours = generated(&div);
    assert_eq!(ours.len(), 2);
    ours.dedup();
    assert_eq!(ours.len(), 2);
}

#[wasm_bindgen_test]
fn coexists_with_an_existing_class_prop() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div class="from-class" mix={css("border-color: black; border-style: solid; border-width: 1px")}></div> });
    root.flush();
    let div = find(&el, "div");
    assert!(classes(&div).contains(&"from-class".to_string()));
    assert!(!generated(&div).is_empty());
}

#[wasm_bindgen_test]
fn supports_keyframes_and_nested_media_rules() {
    let el = container();
    document().body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    root.render(rsx! {
        <div mix={css(
            "animation-name: fade-in; animation-duration: 1s; \
             @keyframes fade-in { from { opacity: 0; transform: translateY(8px) } to { opacity: 1 } } \
             @media (min-width: 1px) { color: rgb(1, 2, 3); &:hover { color: rgb(4, 5, 6) } }",
        )}>"Animated"</div>
    });
    root.flush();
    assert!(!generated(&find(&el, "div")).is_empty());
    assert!(rule_text_present("@keyframes fade-in"), "{:?}", adopted_texts());
    assert!(rule_text_present("@media (min-width: 1px)"));
    root.dispose();
    el.remove();
}

#[wasm_bindgen_test]
fn handles_empty_keyframe_steps_and_at_rule_bodies() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! {
        <div mix={css("animation-name: empty-frames; animation-duration: 1s; @keyframes empty-frames { from {} to {} } @media (min-width: 1px) {}")}>
            "Empty blocks"
        </div>
    });
    root.flush();
    assert!(!generated(&find(&el, "div")).is_empty());
    assert!(rule_text_present("@keyframes empty-frames"));
}

#[wasm_bindgen_test]
fn releases_dynamic_client_only_rules_on_style_change_and_unmount() {
    let el = container();
    document().body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    let baseline = count_adopted_rules();

    root.render(rsx! { <div mix={css("width: 11px")}>"Dynamic"</div> });
    root.flush();
    let div = find(&el, "div");
    let first = div.class_name();
    assert_eq!(count_adopted_rules(), baseline + 1);

    root.render(rsx! { <div mix={css("width: 22px")}>"Dynamic"</div> });
    root.flush();
    assert_ne!(div.class_name(), first);
    assert_eq!(count_adopted_rules(), baseline + 1);
    assert!(!rule_text_present(&format!(".{first}")));

    root.render(());
    root.flush();
    assert_eq!(count_adopted_rules(), baseline);
    root.dispose();
    el.remove();
}

thread_local! {
    static EXIT_GATE: RefCell<Option<js_sys::Promise>> = const { RefCell::new(None) };
}

/// Keeps its host in the document on removal until `EXIT_GATE` resolves.
struct PersistOnRemove;

impl HostMixin for PersistOnRemove {
    type Args = ();
    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        h.on_before_remove(|_, _, event| {
            let gate = EXIT_GATE.with(|g| g.borrow().clone()).expect("exit gate");
            event.persist_node(move |_signal: web_sys::AbortSignal| async move {
                let _ = JsFuture::from(gate).await;
            });
        });
        PersistOnRemove
    }
    fn render(&mut self, _: &(), _: &mut Element, _: &MixinHandle<Self>) {}
}

async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms).unwrap();
    });
    JsFuture::from(promise).await.unwrap();
}

#[wasm_bindgen_test]
async fn keeps_a_dynamic_rule_alive_while_a_mixin_persists_the_node() {
    let mut exit = None;
    let gate = js_sys::Promise::new(&mut |resolve, _| exit = Some(resolve));
    EXIT_GATE.with(|g| *g.borrow_mut() = Some(gate));
    let el = container();
    document().body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    root.render(rsx! { <div id="exiting" mix={[css("color: rgb(77, 88, 99)"), mixin::<PersistOnRemove>(())]}>"Exiting"</div> });
    root.flush();
    let div = find(&el, "#exiting");
    assert_eq!(color(&div), "rgb(77, 88, 99)");

    root.render(());
    root.flush();
    assert!(el.query_selector("#exiting").unwrap().is_some_and(|d| d.is_same_node(Some(&div))));
    assert_eq!(color(&div), "rgb(77, 88, 99)");

    exit.expect("resolver").call0(&JsValue::NULL).unwrap();
    sleep(0).await;
    root.flush();
    assert!(el.query_selector("#exiting").unwrap().is_none());
    assert!(!rule_text_present("rgb(77, 88, 99)"));
    root.dispose();
    el.remove();
}

// attrs mixin

fn s(v: &'static str) -> AttrValue {
    AttrValue::Str(v.into())
}

#[wasm_bindgen_test]
fn attrs_applies_default_props_when_the_element_does_not_provide_them() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <button mix={attrs(vec![("type", s("button")), ("title", s("Save changes"))])}>"Save"</button> });
    root.flush();
    let button = find(&el, "button");
    assert_eq!(button.get_attribute("type").as_deref(), Some("button"));
    assert_eq!(button.get_attribute("title").as_deref(), Some("Save changes"));
}

#[wasm_bindgen_test]
fn attrs_preserves_explicit_props_from_the_element() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! {
        <button type="submit" title="Publish" mix={attrs(vec![("type", s("button")), ("title", s("Save changes"))])}>"Save"</button>
    });
    root.flush();
    let button = find(&el, "button");
    assert_eq!(button.get_attribute("type").as_deref(), Some("submit"));
    assert_eq!(button.get_attribute("title").as_deref(), Some("Publish"));
}

#[wasm_bindgen_test]
fn attrs_applies_class_and_style_defaults_without_merging_them() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={attrs(vec![("class", s("default-class")), ("style", s("color: red"))])}></div> });
    root.flush();
    let div = find(&el, "div");
    assert_eq!(div.class_name(), "default-class");
    assert_eq!(div.style().get_property_value("color").unwrap(), "red");
}

#[wasm_bindgen_test]
fn attrs_does_not_merge_class_and_style_when_explicit_props_are_present() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! {
        <div class="explicit-class" style="background-color: blue" mix={attrs(vec![("class", s("default-class")), ("style", s("color: red"))])}></div>
    });
    root.flush();
    let div = find(&el, "div");
    assert_eq!(div.class_name(), "explicit-class");
    assert_eq!(div.style().get_property_value("background-color").unwrap(), "blue");
    assert_eq!(div.style().get_property_value("color").unwrap(), "");
}

// ref mixin

thread_local! {
    static NODE: RefCell<Option<web_sys::Element>> = const { RefCell::new(None) };
    static SIGNAL: RefCell<Option<rmx::Signal>> = const { RefCell::new(None) };
    static EVENTS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

#[wasm_bindgen_test]
fn ref_passes_the_bound_node_and_signal_to_the_callback() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! {
        <button mix={node_ref(|node, signal| {
            NODE.with(|n| *n.borrow_mut() = Some(node.element().clone()));
            SIGNAL.with(|s| *s.borrow_mut() = Some(signal.clone()));
        })}>"click"</button>
    });
    root.flush();
    let button = find(&el, "button");
    assert!(NODE.with(|n| n.borrow().as_ref().is_some_and(|n| n.is_same_node(Some(&button)))));
    let signal = SIGNAL.with(|s| s.borrow_mut().take()).expect("signal");
    assert!(!signal.aborted());
}

#[wasm_bindgen_test]
fn ref_aborts_the_signal_when_the_host_node_is_removed() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={node_ref(|_, signal| SIGNAL.with(|s| *s.borrow_mut() = Some(signal.clone())))}></div> });
    root.flush();
    let signal = SIGNAL.with(|s| s.borrow_mut().take()).expect("signal");
    assert!(!signal.aborted());
    root.render(());
    root.flush();
    assert!(signal.aborted());
}

struct Toggle {
    ready: Cell<bool>,
}

impl Component for Toggle {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Toggle { ready: Cell::new(false) }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let handle = h.clone();
        let mark = h.node_ref(move |this, _| {
            if !this.ready.replace(true) {
                handle.update();
            }
        });
        let label = if self.ready.get() { "ready" } else { "loading" };
        rsx! { <button mix={mark}>{label}</button> }
    }
}

#[wasm_bindgen_test]
fn ref_allows_handle_update_during_the_insert_callback() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <Toggle/> });
    root.flush();
    assert_eq!(el.text_content().unwrap(), "ready");
}

struct Example;

impl Component for Example {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Example
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        h.queue_task(|_, _| EVENTS.with(|e| e.borrow_mut().push("task")));
        rsx! { <div mix={node_ref(|_, _| EVENTS.with(|e| e.borrow_mut().push("insert")))}></div> }
    }
}

#[wasm_bindgen_test]
fn ref_dispatches_insert_before_queued_component_tasks() {
    EVENTS.with(|e| e.borrow_mut().clear());
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <Example/> });
    root.flush();
    assert_eq!(EVENTS.with(|e| e.borrow().clone()), ["insert", "task"]);
}
