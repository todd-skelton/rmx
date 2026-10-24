//! Ports of Remix's `vdom.replacements.test.tsx`. Run with
//! `pwsh ./scripts/test-browser.ps1 -Test browser_replacements`.
#![cfg(target_arch = "wasm32")]

extern crate rsmix_component as rsmix;

use std::cell::Cell;

use rsmix::{create_root, rsx, Component, Handle, IntoNode, Node};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn container() -> web_sys::HtmlElement {
    web_sys::window().unwrap().document().unwrap().create_element("div").unwrap().unchecked_into()
}

fn find(el: &web_sys::HtmlElement, selector: &str) -> web_sys::Element {
    el.query_selector(selector).unwrap().unwrap_or_else(|| panic!("no {selector}"))
}

/// Render `a` then `b` into one root, checking the HTML after each.
fn replace(a: Node, html_a: &str, b: Node, html_b: &str) -> web_sys::HtmlElement {
    let el = container();
    let root = create_root(&el);
    root.render(a);
    assert_eq!(el.inner_html(), html_a);
    root.render(b);
    assert_eq!(el.inner_html(), html_b);
    el
}

struct Hello;

impl Component for Hello {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Hello
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <div>"Hello, world!"</div> }
    }
}

struct Goodbye;

impl Component for Goodbye {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Goodbye
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <div>"Goodbye, world!"</div> }
    }
}

// type <--> type updates

#[wasm_bindgen_test]
fn updates_a_text_node() {
    replace("Hello, world!".into_node(), "Hello, world!", "Hello, world! 2".into_node(), "Hello, world! 2");
}

#[wasm_bindgen_test]
fn updates_an_element_in_place() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div>"Hello, world!"</div> });
    let div = find(&el, "div");
    root.render(rsx! { <div>"Hello, world! 2"</div> });
    assert_eq!(el.inner_html(), "<div>Hello, world! 2</div>");
    assert!(find(&el, "div").is_same_node(Some(&div)));
}

#[wasm_bindgen_test]
fn updates_an_element_with_attributes() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <input id="hello" value="world"/> });
    let input: web_sys::HtmlInputElement = find(&el, "input").unchecked_into();
    assert_eq!(input.value(), "world");
    root.render(rsx! { <input id="hello" value="world 2"/> });
    assert!(find(&el, "input").is_same_node(Some(&input)));
    assert_eq!(input.get_attribute("id").as_deref(), Some("hello"));
    assert_eq!(input.value(), "world 2");
}

#[wasm_bindgen_test]
fn updates_a_fragment_in_place() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <><p>"Hello"</p><p>"world!"</p></> });
    let first = find(&el, "p");
    root.render(rsx! { <><p>"Goodbye"</p><p>"Universe"</p></> });
    assert_eq!(el.inner_html(), "<p>Goodbye</p><p>Universe</p>");
    assert!(find(&el, "p").is_same_node(Some(&first)));
}

thread_local! {
    static SETUPS: Cell<u32> = const { Cell::new(0) };
}

#[derive(Clone, Default)]
struct TitleProps {
    title: &'static str,
    description: Option<&'static str>,
}

struct Titled {
    state: u32,
}

impl Component for Titled {
    type Props = TitleProps;
    fn setup(_: &TitleProps, _: &Handle<Self>) -> Self {
        Titled { state: SETUPS.with(|s| {
            s.set(s.get() + 1);
            s.get()
        }) }
    }
    fn render(&self, p: &TitleProps, _: &Handle<Self>) -> Node {
        let description = p.description.map(|d| format!(" {d}")).unwrap_or_default();
        rsx! { <div>{p.title} " " {self.state} {description}</div> }
    }
}

#[wasm_bindgen_test]
fn updates_a_component_keeping_its_state() {
    SETUPS.with(|s| s.set(0));
    replace(rsx! { <Titled title="Hello"/> }, "<div>Hello 1</div>", rsx! { <Titled title="Goodbye"/> }, "<div>Goodbye 1</div>");
}

#[wasm_bindgen_test]
fn render_sees_the_latest_props_with_optional_fields_cleared() {
    SETUPS.with(|s| s.set(0));
    replace(
        rsx! { <Titled title="Hello" description={Some("World")}/> },
        "<div>Hello 1 World</div>",
        rsx! { <Titled title="Goodbye"/> },
        "<div>Goodbye 1</div>",
    );
}

// simple replacement

#[wasm_bindgen_test]
fn replaces_between_text_elements_components_and_fragments() {
    let hello_div = || rsx! { <div>"Hello, world!"</div> };
    let bye_div = || rsx! { <div>"Goodbye, world!"</div> };
    let h = "<div>Hello, world!</div>";
    let g = "<div>Goodbye, world!</div>";
    replace(hello_div(), h, "Goodbye, element!".into_node(), "Goodbye, element!");
    replace("Hello, world!".into_node(), "Hello, world!", bye_div(), g);
    replace(hello_div(), h, rsx! { <Goodbye/> }, g);
    replace(rsx! { <Hello/> }, h, bye_div(), g);
    replace(hello_div(), h, rsx! { <nav>"Goodbye, world!"</nav> }, "<nav>Goodbye, world!</nav>");
    replace(rsx! { <Hello/> }, h, rsx! { <Goodbye/> }, g);
    replace(rsx! { <Hello/> }, h, rsx! { <><p>"Goodbye"</p><p>"world!"</p></> }, "<p>Goodbye</p><p>world!</p>");
    replace(rsx! { <><div>"Hello, world!"</div></> }, h, rsx! { <Goodbye/> }, g);
    replace(rsx! { <><div>"Hello, world!"</div></> }, h, bye_div(), g);
    replace(rsx! { <><div>"Hello, world!"</div></> }, h, "Goodbye, world!".into_node(), "Goodbye, world!");
    replace("Hello, world!".into_node(), "Hello, world!", rsx! { <Goodbye/> }, g);
}

// complex replacements

#[wasm_bindgen_test]
fn preserves_siblings() {
    let el = replace(
        rsx! { <div><div>"div"</div><span>"span"</span><p>"p"</p></div> },
        "<div><div>div</div><span>span</span><p>p</p></div>",
        rsx! { <div><div>"div"</div><nav>"nav"</nav><p>"p"</p></div> },
        "<div><div>div</div><nav>nav</nav><p>p</p></div>",
    );
    assert_eq!(el.query_selector_all("p").unwrap().length(), 1);
}

#[wasm_bindgen_test]
fn replaces_null_children_keeping_siblings() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div><div>"div"</div>{None::<Node>}<p>"p"</p></div> });
    assert_eq!(el.inner_html(), "<div><div>div</div><p>p</p></div>");
    let div = find(&el, "div > div");
    let p = find(&el, "p");
    root.render(rsx! { <div><div>"div"</div><span>"span"</span><p>"p"</p></div> });
    assert_eq!(el.inner_html(), "<div><div>div</div><span>span</span><p>p</p></div>");
    assert!(find(&el, "div > div").is_same_node(Some(&div)));
    assert!(find(&el, "p").is_same_node(Some(&p)));
}

struct Frag;

impl Component for Frag {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Frag
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <><span>"A"</span><span>"B"</span></> }
    }
}

#[wasm_bindgen_test]
fn replaces_fragment_components_keeping_siblings() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div><Frag/><main>"main"</main></div> });
    assert_eq!(el.inner_html(), "<div><span>A</span><span>B</span><main>main</main></div>");
    let main = find(&el, "main");
    root.render(rsx! { <div><div>"one"</div><main>"main"</main></div> });
    assert_eq!(el.inner_html(), "<div><div>one</div><main>main</main></div>");
    assert!(find(&el, "main").is_same_node(Some(&main)));
}

#[wasm_bindgen_test]
fn replaces_components_within_elements() {
    replace(
        rsx! { <div><Hello/></div> },
        "<div><div>Hello, world!</div></div>",
        rsx! { <div><Goodbye/></div> },
        "<div><div>Goodbye, world!</div></div>",
    );
}
