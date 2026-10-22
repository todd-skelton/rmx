//! Ports of Remix's `vdom.insert-remove.test.tsx`. Run with
//! `pwsh ./test-browser.ps1 -Test browser_insert_remove`.
//!
//! JavaScript's `bigint` is a `u64`/`u128` here, and `null`/`undefined` are `None` and `()`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use rmx::{create_root, rsx, Component, Handle, IntoNode, Node};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn container() -> web_sys::HtmlElement {
    web_sys::window().unwrap().document().unwrap().create_element("div").unwrap().unchecked_into()
}

fn rendered(node: impl IntoNode) -> String {
    let el = container();
    create_root(&el).render(node);
    el.inner_html()
}

// inserts

#[wasm_bindgen_test]
fn renders_text() {
    assert_eq!(rendered("Hello, world!"), "Hello, world!");
}

#[wasm_bindgen_test]
fn renders_number() {
    assert_eq!(rendered(42), "42");
}

#[wasm_bindgen_test]
fn renders_0() {
    assert_eq!(rendered(0), "0");
}

#[wasm_bindgen_test]
fn renders_bigint() {
    assert_eq!(rendered(9007199254740991u64), "9007199254740991");
}

#[wasm_bindgen_test]
fn renders_true() {
    assert_eq!(rendered(true), "");
}

#[wasm_bindgen_test]
fn renders_false() {
    assert_eq!(rendered(false), "");
}

#[wasm_bindgen_test]
fn renders_null() {
    assert_eq!(rendered(None::<Node>), "");
}

#[wasm_bindgen_test]
fn renders_undefined() {
    assert_eq!(rendered(()), "");
}

// removals

#[wasm_bindgen_test]
fn removes_a_text_node() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div>"Hello, world!"</div> });
    assert_eq!(el.inner_html(), "<div>Hello, world!</div>");
    root.render(rsx! { <div/> });
    assert_eq!(el.inner_html(), "<div></div>");
}

#[wasm_bindgen_test]
fn removes_an_element() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div><span>"Hello, world!"</span></div> });
    assert_eq!(el.inner_html(), "<div><span>Hello, world!</span></div>");
    root.render(rsx! { <div/> });
    assert_eq!(el.inner_html(), "<div></div>");
}

#[wasm_bindgen_test]
fn removes_attributes() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <input id="hello" value="world"/> });
    let input: web_sys::HtmlInputElement = el.query_selector("input").unwrap().unwrap().unchecked_into();
    assert_eq!(input.value(), "world");
    assert_eq!(input.get_attribute("id").as_deref(), Some("hello"));
    root.render(rsx! { <input/> });
    root.flush();
    assert_eq!(input.value(), "");
    assert!(!input.has_attribute("id"));
    assert!(!input.has_attribute("value"));
}

#[wasm_bindgen_test]
fn removes_reflected_attributes_without_leaving_empty_values() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div id="hello" class="world">"content"</div> });
    let div = el.query_selector("div").unwrap().unwrap();
    assert_eq!(div.get_attribute("id").as_deref(), Some("hello"));
    assert_eq!(div.get_attribute("class").as_deref(), Some("world"));
    root.render(rsx! { <div>"content"</div> });
    root.flush();
    assert!(!div.has_attribute("id"));
    assert!(!div.has_attribute("class"));
}

#[wasm_bindgen_test]
fn removes_a_fragment() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div><><p>"Hello"</p><p>"world!"</p></></div> });
    assert_eq!(el.inner_html(), "<div><p>Hello</p><p>world!</p></div>");
    root.render(rsx! { <div/> });
    assert_eq!(el.inner_html(), "<div></div>");
}

struct App;

impl Component for App {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        App
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <div>"Hello, world!"</div> }
    }
}

#[wasm_bindgen_test]
fn removes_a_component() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div><App/></div> });
    assert_eq!(el.inner_html(), "<div><div>Hello, world!</div></div>");
    root.render(rsx! { <div></div> });
    assert_eq!(el.inner_html(), "<div></div>");
}
