//! Ports of Remix's `vdom.elements-fragments` and `vdom.components` tests. Run with
//! `pwsh ./scripts/test-browser.ps1 -Test browser_elements`.
//!
//! Style objects are a JSX convenience; rsmix takes `style` as a string, so the style
//! tests check string updates instead.
#![cfg(target_arch = "wasm32")]

extern crate rsmix_component as rsmix;

use std::cell::{Cell, RefCell};

use rsmix::{create_root, rsx, Component, Handle, Node};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn container() -> web_sys::HtmlElement {
    document().create_element("div").unwrap().unchecked_into()
}

fn find(el: &web_sys::Element, selector: &str) -> Option<web_sys::Element> {
    el.query_selector(selector).unwrap()
}

thread_local! {
    static PHASE: Cell<u32> = const { Cell::new(0) };
    static UPDATE: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
}

fn capture<C: Component>(h: &Handle<C>) {
    let h = h.clone();
    UPDATE.with(|u| *u.borrow_mut() = Some(Box::new(move || h.update())));
}

fn bump() {
    PHASE.with(|p| p.set(p.get() + 1));
    UPDATE.with(|u| (u.borrow().as_ref().unwrap())());
}

// elements

#[wasm_bindgen_test]
fn renders_basic_and_nested_elements() {
    let el = container();
    create_root(&el).render(rsx! { <div>"Hello, world!"</div> });
    assert_eq!(el.inner_html(), "<div>Hello, world!</div>");
    let el = container();
    create_root(&el).render(rsx! { <div>"Hello, " <span>"world!"</span></div> });
    assert_eq!(el.inner_html(), "<div>Hello, <span>world!</span></div>");
}

#[wasm_bindgen_test]
fn renders_attributes_and_sets_value_as_a_property() {
    let el = container();
    create_root(&el).render(rsx! { <input id="hello" value="world"/> });
    let input: web_sys::HtmlInputElement = find(&el, "input").unwrap().unchecked_into();
    assert_eq!(input.value(), "world");
    assert_eq!(el.inner_html(), r#"<input id="hello">"#);
}

#[wasm_bindgen_test]
fn renders_0_as_a_child() {
    let el = container();
    create_root(&el).render(rsx! { <div>{0}</div> });
    assert_eq!(el.inner_html(), "<div>0</div>");
}

#[wasm_bindgen_test]
fn updates_style_strings() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div style="color: red; margin-top: 12px">"X"</div> });
    let div: web_sys::HtmlElement = find(&el, "div").unwrap().unchecked_into();
    assert_eq!(div.style().get_property_value("color").unwrap(), "red");
    assert_eq!(div.style().get_property_value("margin-top").unwrap(), "12px");
    root.render(rsx! { <div style="color: red; padding: 5px">"X"</div> });
    assert_eq!(div.style().get_property_value("margin-top").unwrap(), "");
    assert_eq!(div.style().get_property_value("padding").unwrap(), "5px");
}

#[wasm_bindgen_test]
fn hydration_replaces_a_different_style() {
    let el = container();
    el.set_inner_html(r#"<div style="color: red">X</div>"#);
    create_root(&el).render(rsx! { <div style="color: blue">"X"</div> });
    let div: web_sys::HtmlElement = find(&el, "div").unwrap().unchecked_into();
    assert_eq!(div.style().get_property_value("color").unwrap(), "blue");
}

// fragments

#[wasm_bindgen_test]
fn inserts_fragments_and_nested_fragments() {
    let el = container();
    create_root(&el).render(rsx! { <><p>"Hello"</p><p>"world!"</p></> });
    assert_eq!(el.inner_html(), "<p>Hello</p><p>world!</p>");
    let el = container();
    create_root(&el).render(rsx! { <div><><p>"Hello"</p><p>"world!"</p></><p>"Goodbye"</p></div> });
    assert_eq!(el.inner_html(), "<div><p>Hello</p><p>world!</p><p>Goodbye</p></div>");
}

#[wasm_bindgen_test]
fn inserts_new_nodes_in_a_parent() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div><p>"Hello"</p></div> });
    let p = find(&el, "p").unwrap();
    root.render(rsx! { <div><p>"Hello"</p><p>"Goodbye"</p></div> });
    assert_eq!(el.inner_html(), "<div><p>Hello</p><p>Goodbye</p></div>");
    assert!(find(&el, "p").unwrap().is_same_node(Some(&p)));
}

// components

struct QueuedInRender {
    value: &'static str,
    queued: Cell<bool>,
}

impl Component for QueuedInRender {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        QueuedInRender { value: "initial", queued: Cell::new(false) }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        if !self.queued.replace(true) {
            h.queue_task(|this, h| {
                this.value = "updated";
                h.update();
            });
        }
        rsx! { <p>{self.value}</p> }
    }
}

#[wasm_bindgen_test]
fn updates_from_a_task_queued_during_the_initial_render() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <QueuedInRender/> });
    root.flush();
    assert_eq!(el.inner_html(), "<p>updated</p>");
}

struct Count {
    count: u32,
}

impl Component for Count {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        capture(h);
        Count { count: 1 }
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <div>{self.count + PHASE.with(Cell::get)}</div> }
    }
}

#[wasm_bindgen_test]
fn updates_a_component_in_place() {
    PHASE.with(|p| p.set(0));
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <Count/> });
    assert_eq!(el.inner_html(), "<div>1</div>");
    let div = find(&el, "div").unwrap();
    for n in 2..=3 {
        bump();
        root.flush();
        assert_eq!(el.inner_html(), format!("<div>{n}</div>"));
        assert!(find(&el, "div").unwrap().is_same_node(Some(&div)));
    }
}

struct Spans;

impl Component for Spans {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        capture(h);
        Spans
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let spans: Vec<Node> = (0..=PHASE.with(Cell::get)).map(|i| rsx! { <span>{i}</span> }).collect();
        rsx! { <>{spans}</> }
    }
}

#[wasm_bindgen_test]
fn updates_a_component_with_a_fragment() {
    PHASE.with(|p| p.set(0));
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <Spans/> });
    assert_eq!(el.inner_html(), "<span>0</span>");
    let span = find(&el, "span").unwrap();
    bump();
    root.flush();
    assert_eq!(el.inner_html(), "<span>0</span><span>1</span>");
    assert!(find(&el, "span").unwrap().is_same_node(Some(&span)));
    bump();
    root.flush();
    assert_eq!(el.inner_html(), "<span>0</span><span>1</span><span>2</span>");
}

struct HeadLike;

impl Component for HeadLike {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        capture(h);
        HeadLike
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        match PHASE.with(Cell::get) {
            0 => rsx! {
                <>
                    <title>"Page A"</title>
                    <meta name="description" content="A"/>
                    <script type="application/ld+json">{r#"{"name":"A"}"#}</script>
                    <script type="text/javascript">"window.__regular = 'A'"</script>
                    <div>"Phase A"</div>
                </>
            },
            1 => rsx! {
                <>
                    <title>"Page B"</title>
                    <meta name="description" content="B"/>
                    <script type="application/ld+json">{r#"{"name":"B"}"#}</script>
                    <div>"Phase B"</div>
                </>
            },
            _ => rsx! { <div>"Phase C"</div> },
        }
    }
}

#[wasm_bindgen_test]
fn renders_head_like_elements_in_place_on_client_updates() {
    PHASE.with(|p| p.set(0));
    let el = container();
    document().body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    root.render(rsx! { <HeadLike/> });
    root.flush();
    let head = document().head().unwrap();
    assert_eq!(find(&el, "title").unwrap().text_content().unwrap(), "Page A");
    assert_eq!(find(&el, "meta[name=description]").unwrap().get_attribute("content").unwrap(), "A");
    assert_eq!(find(&el, r#"script[type="application/ld+json"]"#).unwrap().text_content().unwrap(), r#"{"name":"A"}"#);
    assert!(find(&el, r#"script[type="text/javascript"]"#).is_some());
    assert!(find(&head, "title").is_none());

    bump();
    root.flush();
    assert_eq!(find(&el, "title").unwrap().text_content().unwrap(), "Page B");
    assert_eq!(el.query_selector_all("meta[name=description]").unwrap().length(), 1);
    assert_eq!(find(&el, r#"script[type="application/ld+json"]"#).unwrap().text_content().unwrap(), r#"{"name":"B"}"#);
    assert!(find(&el, r#"script[type="text/javascript"]"#).is_none());
    assert_eq!(find(&el, "div").unwrap().text_content().unwrap(), "Phase B");
    assert!(find(&head, "title").is_none());

    bump();
    root.flush();
    assert!(find(&el, "title").is_none());
    assert_eq!(el.inner_html(), "<div>Phase C</div>");
    root.dispose();
    el.remove();
}

#[wasm_bindgen_test]
fn renders_an_explicit_head_into_document_head_and_dispose_cleans_it_up() {
    let el = container();
    document().body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    root.render(rsx! {
        <>
            <head>
                <title>"Dispose title"</title>
                <meta name="dispose-description" content="dispose"/>
                <script type="application/ld+json">{r#"{"dispose":true}"#}</script>
            </head>
            <div>"Content"</div>
        </>
    });
    root.flush();
    let head = document().head().unwrap();
    assert_eq!(find(&head, "title").unwrap().text_content().unwrap(), "Dispose title");
    assert!(find(&head, "meta[name=dispose-description]").is_some());
    assert_eq!(find(&head, r#"script[type="application/ld+json"]"#).unwrap().text_content().unwrap(), r#"{"dispose":true}"#);
    assert_eq!(el.inner_html(), "<div>Content</div>");
    root.dispose();
    assert!(find(&head, "title").is_none());
    assert!(find(&head, "meta[name=dispose-description]").is_none());
    assert!(find(&head, r#"script[type="application/ld+json"]"#).is_none());
    assert_eq!(el.inner_html(), "");
    el.remove();
}

struct HeadSwitch;

impl Component for HeadSwitch {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        capture(h);
        HeadSwitch
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let title = format!("Title {}", PHASE.with(Cell::get));
        if PHASE.with(Cell::get) < 2 {
            rsx! { <><head><title>{title}</title></head><p>"body"</p></> }
        } else {
            rsx! { <p>"body"</p> }
        }
    }
}

#[wasm_bindgen_test]
fn updates_and_removes_head_children_without_touching_other_head_content() {
    PHASE.with(|p| p.set(0));
    let head = document().head().unwrap();
    let other = document().create_element("meta").unwrap();
    other.set_attribute("name", "keep-me").unwrap();
    head.append_child(&other).unwrap();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <HeadSwitch/> });
    assert_eq!(find(&head, "title").unwrap().text_content().unwrap(), "Title 0");
    bump();
    root.flush();
    assert_eq!(head.query_selector_all("title").unwrap().length(), 1);
    assert_eq!(find(&head, "title").unwrap().text_content().unwrap(), "Title 1");
    bump();
    root.flush();
    assert!(find(&head, "title").is_none());
    assert!(find(&head, "meta[name=keep-me]").is_some());
    assert_eq!(el.inner_html(), "<p>body</p>");
    other.remove();
}
