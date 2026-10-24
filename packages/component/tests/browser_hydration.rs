//! Ports of Remix's `hydration.*.test.tsx` suites: server HTML from `render_to_string`,
//! then `create_root` adopts it. Run with `pwsh ./scripts/test-browser.ps1 -Test browser_hydration`.
//!
//! JSX's camelCase props are written as HTML attributes here (`class`, `for`, `tabindex`,
//! `readonly`), and style objects as strings.
#![cfg(target_arch = "wasm32")]

extern crate rsmix_component as rsmix;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rsmix::{create_root, css, link, node_ref, on, render_to_string, rsx, unsafe_html, Children, Component, EventHandler, Handle, Node, Root};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

/// A container in the document holding the server HTML for `node`.
fn server(node: Node) -> web_sys::HtmlElement {
    let el: web_sys::HtmlElement = document().create_element("div").unwrap().unchecked_into();
    document().body().unwrap().append_child(&el).unwrap();
    el.set_inner_html(&render_to_string(&node));
    el
}

/// Hydrate `el` with `node`.
fn hydrate(el: &web_sys::HtmlElement, node: Node) -> Root {
    let root = create_root(el);
    root.render(node);
    root.flush();
    root
}

fn find(el: &web_sys::HtmlElement, selector: &str) -> web_sys::HtmlElement {
    el.query_selector(selector).unwrap().unwrap_or_else(|| panic!("no {selector}")).unchecked_into()
}

fn all(el: &web_sys::HtmlElement, selector: &str) -> Vec<web_sys::Element> {
    let list = el.query_selector_all(selector).unwrap();
    (0..list.length()).map(|i| list.get(i).unwrap().unchecked_into()).collect()
}

fn same(a: &web_sys::Element, b: &web_sys::Element) -> bool {
    a.is_same_node(Some(b))
}

fn prop(el: &JsValue, name: &str) -> JsValue {
    js_sys::Reflect::get(el, &name.into()).unwrap()
}

fn color(el: &web_sys::Element) -> String {
    web_sys::window().unwrap().get_computed_style(el).unwrap().unwrap().get_property_value("color").unwrap()
}

/// Replace `console.error` while `f` runs; returns how often it was called.
fn quiet_errors(f: impl FnOnce()) -> usize {
    let console = js_sys::Reflect::get(&js_sys::global(), &"console".into()).unwrap();
    let original = js_sys::Reflect::get(&console, &"error".into()).unwrap();
    let count = Rc::new(Cell::new(0));
    let c = count.clone();
    let spy = Closure::<dyn Fn()>::new(move || c.set(c.get() + 1));
    js_sys::Reflect::set(&console, &"error".into(), spy.as_ref()).unwrap();
    f();
    js_sys::Reflect::set(&console, &"error".into(), &original).unwrap();
    count.get()
}

/// Check that hydrating `make()` adopts the server element at `selector`.
fn adopts(make: impl Fn() -> Node, selector: &str) -> (web_sys::HtmlElement, web_sys::HtmlElement) {
    let el = server(make());
    let existing = find(&el, selector);
    hydrate(&el, make());
    assert!(same(&find(&el, selector), &existing), "{selector} was not adopted");
    (el, existing)
}

// hydration.attributes: special case props to HTML attributes

#[wasm_bindgen_test]
fn hydrates_class() {
    let (_, div) = adopts(|| rsx! { <div class="my-class">"Hello"</div> }, "div");
    assert_eq!(div.get_attribute("class").as_deref(), Some("my-class"));
}

#[wasm_bindgen_test]
fn hydrates_for() {
    let (_, label) = adopts(|| rsx! { <div><label for="my-input">"Label"</label><input id="my-input"/></div> }, "label");
    assert_eq!(label.get_attribute("for").as_deref(), Some("my-input"));
}

#[wasm_bindgen_test]
fn hydrates_tabindex() {
    let (_, button) = adopts(|| rsx! { <button tabindex={0}>"Click"</button> }, "button");
    assert_eq!(button.get_attribute("tabindex").as_deref(), Some("0"));
}

#[wasm_bindgen_test]
fn hydrates_role_and_tabindex_added_by_link_mixins() {
    let (_, li) = adopts(|| rsx! { <li mix={link("/docs")}>"Docs"</li> }, "li");
    assert_eq!(li.get_attribute("role").as_deref(), Some("link"));
    assert_eq!(li.get_attribute("tabindex").as_deref(), Some("0"));
}

#[wasm_bindgen_test]
fn hydrates_accept_charset() {
    let (_, form) = adopts(|| rsx! { <form accept-charset="UTF-8"></form> }, "form");
    assert_eq!(form.get_attribute("accept-charset").as_deref(), Some("UTF-8"));
}

#[wasm_bindgen_test]
fn hydrates_http_equiv_in_place() {
    let (_, meta) = adopts(|| rsx! { <meta http-equiv="refresh" content="5"/> }, "meta");
    assert!(document().head().unwrap().query_selector(r#"meta[http-equiv="refresh"]"#).unwrap().is_none());
    assert_eq!(meta.get_attribute("http-equiv").as_deref(), Some("refresh"));
}

#[wasm_bindgen_test]
fn hydrates_aria_attributes_unchanged() {
    let (_, button) = adopts(|| rsx! { <button aria-label="Close" aria-expanded="false">"X"</button> }, "button");
    assert_eq!(button.get_attribute("aria-label").as_deref(), Some("Close"));
    assert_eq!(button.get_attribute("aria-expanded").as_deref(), Some("false"));
}

#[wasm_bindgen_test]
fn hydrates_data_attributes_unchanged() {
    let (_, div) = adopts(|| rsx! { <div data-testid="my-div" data-value="42"></div> }, "div");
    assert_eq!(div.get_attribute("data-testid").as_deref(), Some("my-div"));
    assert_eq!(div.get_attribute("data-value").as_deref(), Some("42"));
}

#[wasm_bindgen_test]
fn hydrates_data_rmx_preserve_dom_as_a_boolean_attribute() {
    assert_eq!(render_to_string(&rsx! { <div data-rmx-preserve-dom></div> }), "<div data-rmx-preserve-dom></div>");
    let (_, div) = adopts(|| rsx! { <div data-rmx-preserve-dom></div> }, "div");
    assert!(div.has_attribute("data-rmx-preserve-dom"));
}

#[wasm_bindgen_test]
fn hydrates_svg_xlink_href() {
    let (_, use_el) = adopts(|| rsx! { <svg><use xlink:href="#icon-star"></use></svg> }, "use");
    assert_eq!(use_el.get_attribute_ns(Some("http://www.w3.org/1999/xlink"), "href").as_deref(), Some("#icon-star"));
}

#[wasm_bindgen_test]
fn hydrates_svg_view_box_with_preserved_case() {
    let (_, svg) = adopts(|| rsx! { <svg viewBox="0 0 24 24"></svg> }, "svg");
    assert_eq!(svg.get_attribute("viewBox").as_deref(), Some("0 0 24 24"));
}

#[wasm_bindgen_test]
fn hydrates_svg_preserve_aspect_ratio_with_preserved_case() {
    let (_, svg) = adopts(|| rsx! { <svg preserveAspectRatio="xMidYMid meet"></svg> }, "svg");
    assert_eq!(svg.get_attribute("preserveAspectRatio").as_deref(), Some("xMidYMid meet"));
}

#[wasm_bindgen_test]
fn hydrates_svg_filter_units_with_canonical_case_and_semantics() {
    let (_, filter) = adopts(|| rsx! { <svg><defs><filter id="f" filterUnits="userSpaceOnUse"></filter></defs></svg> }, "#f");
    assert_eq!(filter.get_attribute("filterUnits").as_deref(), Some("userSpaceOnUse"));
    assert_eq!(filter.get_attribute("filter-units"), None);
    let base = prop(&prop(&filter, "filterUnits"), "baseVal");
    assert_eq!(base.as_f64(), Some(1.0));
}

// hydration.boolean-attrs

#[wasm_bindgen_test]
fn hydrates_disabled_attribute() {
    let (_, button) = adopts(|| rsx! { <button disabled>"Click"</button> }, "button");
    assert_eq!(prop(&button, "disabled"), JsValue::TRUE);
}

#[wasm_bindgen_test]
fn hydrates_readonly_attribute() {
    let (_, input) = adopts(|| rsx! { <input readonly/> }, "input");
    assert_eq!(prop(&input, "readOnly"), JsValue::TRUE);
}

#[wasm_bindgen_test]
fn hydrates_required_attribute() {
    let (_, input) = adopts(|| rsx! { <input required/> }, "input");
    assert_eq!(prop(&input, "required"), JsValue::TRUE);
}

#[wasm_bindgen_test]
fn hydrates_multiple_attribute_on_select() {
    let (_, select) = adopts(|| rsx! { <select multiple><option>"A"</option><option>"B"</option></select> }, "select");
    assert_eq!(prop(&select, "multiple"), JsValue::TRUE);
}

#[wasm_bindgen_test]
fn hydrates_hidden_attribute() {
    let (_, div) = adopts(|| rsx! { <div hidden>"Hidden content"</div> }, "div");
    assert!(div.hidden());
}

#[wasm_bindgen_test]
fn hydrates_boolean_attribute_with_true_value() {
    let (_, button) = adopts(|| rsx! { <button disabled={true}>"Click"</button> }, "button");
    assert_eq!(prop(&button, "disabled"), JsValue::TRUE);
}

#[wasm_bindgen_test]
fn hydrates_boolean_attribute_with_false_value() {
    let (_, button) = adopts(|| rsx! { <button disabled={false}>"Click"</button> }, "button");
    assert_eq!(prop(&button, "disabled"), JsValue::FALSE);
}

// hydration.components

struct NullComponent;

impl Component for NullComponent {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        NullComponent
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        Node::Empty
    }
}

#[wasm_bindgen_test]
fn hydrates_component_that_returns_null() {
    let (_, span) = adopts(|| rsx! { <div><NullComponent/><span>"After"</span></div> }, "span");
    assert_eq!(span.text_content().as_deref(), Some("After"));
}

struct FragmentComponent;

impl Component for FragmentComponent {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        FragmentComponent
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <><span>"First"</span><span>"Second"</span></> }
    }
}

#[wasm_bindgen_test]
fn hydrates_component_that_returns_fragment() {
    let make = || rsx! { <div><FragmentComponent/></div> };
    let el = server(make());
    let spans = all(&el, "span");
    assert_eq!(spans.len(), 2);
    hydrate(&el, make());
    let hydrated = all(&el, "span");
    assert!(same(&hydrated[0], &spans[0]) && same(&hydrated[1], &spans[1]));
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct OuterProps {
    children: Children,
}

struct Outer;

impl Component for Outer {
    type Props = OuterProps;
    rsmix::client_entry!("Outer");
    fn setup(_: &OuterProps, _: &Handle<Self>) -> Self {
        Outer
    }
    fn render(&self, props: &OuterProps, _: &Handle<Self>) -> Node {
        rsx! { <div class="outer">{&props.children}</div> }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct NoProps {}

struct Inner;

impl Component for Inner {
    type Props = NoProps;
    rsmix::client_entry!("Inner");
    fn setup(_: &NoProps, _: &Handle<Self>) -> Self {
        Inner
    }
    fn render(&self, _: &NoProps, _: &Handle<Self>) -> Node {
        rsx! { <span class="inner">"Inner content"</span> }
    }
}

#[wasm_bindgen_test]
fn hydrates_nested_hydration_boundaries() {
    let make = || rsx! { <Outer><Inner/></Outer> };
    let html = render_to_string(&make());
    assert!(html.contains("<!--rmx:h:"), "{html}");
    assert!(html.contains("<!--/rmx:h-->"), "{html}");
    let el = server(make());
    let (outer, inner) = (find(&el, ".outer"), find(&el, ".inner"));
    hydrate(&el, make());
    assert!(same(&find(&el, ".outer"), &outer));
    assert!(same(&find(&el, ".inner"), &inner));
}

#[derive(Clone, Default)]
struct CounterProps {
    initial_count: i32,
}

struct Counter {
    count: i32,
}

impl Component for Counter {
    type Props = CounterProps;
    fn setup(props: &CounterProps, _: &Handle<Self>) -> Self {
        Counter { count: props.initial_count }
    }
    fn render(&self, _: &CounterProps, h: &Handle<Self>) -> Node {
        let click = h.on(|this, h| {
            this.count += 1;
            h.update();
        });
        rsx! { <button mix={on("click", click)}>"Count: "{self.count}</button> }
    }
}

#[wasm_bindgen_test]
fn hydrates_component_with_state_preservation() {
    let el = server(rsx! { <Counter initial_count={5}/> });
    let button = find(&el, "button");
    assert_eq!(button.text_content().as_deref(), Some("Count: 5"));
    let root = hydrate(&el, rsx! { <Counter initial_count={5}/> });
    assert!(same(&find(&el, "button"), &button));
    button.click();
    root.flush();
    assert_eq!(button.text_content().as_deref(), Some("Count: 6"));
}

thread_local! {
    static SECOND: RefCell<Node> = const { RefCell::new(Node::Empty) };
    static WITH_NULL: RefCell<Option<Handle<WithNull>>> = const { RefCell::new(None) };
}

struct WithNull;

impl Component for WithNull {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        WITH_NULL.with(|w| *w.borrow_mut() = Some(h.clone()));
        WithNull
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let second = SECOND.with(|s| s.borrow().clone());
        rsx! { <div><span>"First"</span>{second}<span>"Second"</span></div> }
    }
}

#[wasm_bindgen_test]
fn hydrates_null_elements_without_breaking_sibling_order_when_they_become_renderable() {
    SECOND.with(|s| *s.borrow_mut() = Node::Empty);
    let el = server(rsx! { <WithNull/> });
    let spans = all(&el, "span");
    assert_eq!(spans.len(), 2);
    let root = hydrate(&el, rsx! { <WithNull/> });
    let hydrated = all(&el, "span");
    assert_eq!(hydrated.len(), 2);
    assert!(same(&hydrated[0], &spans[0]) && same(&hydrated[1], &spans[1]));

    SECOND.with(|s| *s.borrow_mut() = rsx! { <span>"after First"</span> });
    WITH_NULL.with(|w| w.borrow().as_ref().unwrap().update());
    root.flush();
    let updated = all(&el, "span");
    assert_eq!(updated.len(), 3);
    assert!(same(&updated[0], &spans[0]));
    assert_eq!(updated[1].text_content().as_deref(), Some("after First"));
    assert!(same(&updated[2], &spans[1]));
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
        h.set_context(ProviderValue("from context"));
        Provider
    }
    fn render(&self, props: &ProviderProps, _: &Handle<Self>) -> Node {
        rsx! { <div class="provider">{&props.children}</div> }
    }
}

struct Consumer(&'static str);

impl Component for Consumer {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        Consumer(h.context::<ProviderValue>().map_or("no context", |v| v.0))
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <span class="consumer">{self.0}</span> }
    }
}

#[wasm_bindgen_test]
fn hydrates_context_across_component_boundaries() {
    let make = || rsx! { <Provider><Consumer/></Provider> };
    let el = server(make());
    let (provider, consumer) = (find(&el, ".provider"), find(&el, ".consumer"));
    assert_eq!(consumer.text_content().as_deref(), Some("from context"));
    hydrate(&el, make());
    assert!(same(&find(&el, ".provider"), &provider));
    assert!(same(&find(&el, ".consumer"), &consumer));
    assert_eq!(consumer.text_content().as_deref(), Some("from context"));
}

#[wasm_bindgen_test]
fn hydrates_svg_elements_with_case_sensitive_tags() {
    let make = || {
        rsx! {
            <svg>
                <defs>
                    <linearGradient id="grad1">
                        <stop offset="0%" stop-color="red"></stop>
                        <stop offset="100%" stop-color="blue"></stop>
                    </linearGradient>
                </defs>
                <rect fill="url(#grad1)" width="100" height="100"></rect>
            </svg>
        }
    };
    let el = server(make());
    let (svg, gradient, rect) = (find(&el, "svg"), find(&el, "linearGradient"), find(&el, "rect"));
    hydrate(&el, make());
    assert!(same(&find(&el, "svg"), &svg));
    assert!(same(&find(&el, "linearGradient"), &gradient));
    assert!(same(&find(&el, "rect"), &rect));
}

#[wasm_bindgen_test]
fn hydrates_inner_html_prop() {
    let (_, div) = adopts(|| rsx! { <div innerHTML={unsafe_html("<span>Raw HTML</span>")}></div> }, "div");
    assert_eq!(div.inner_html(), "<span>Raw HTML</span>");
}

#[wasm_bindgen_test]
fn hydrates_style_string() {
    let (_, div) = adopts(|| rsx! { <div style="color: red; background-color: blue; padding: 10px">"Styled"</div> }, "div");
    let style = prop(&div, "style");
    assert_eq!(prop(&style, "color").as_string().as_deref(), Some("red"));
    assert_eq!(prop(&style, "backgroundColor").as_string().as_deref(), Some("blue"));
}

thread_local! {
    static CONNECTED: RefCell<Option<web_sys::Element>> = const { RefCell::new(None) };
}

struct WithConnect;

impl Component for WithConnect {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        WithConnect
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let connect = node_ref(|node, _| CONNECTED.with(|c| *c.borrow_mut() = Some(node.element().clone())));
        rsx! { <div mix={connect}>"Connected"</div> }
    }
}

#[wasm_bindgen_test]
fn calls_ref_callback_after_hydration() {
    let (_, div) = adopts(|| rsx! { <WithConnect/> }, "div");
    assert!(CONNECTED.with(|c| c.borrow().as_ref().is_some_and(|n| same(n, &div))));
}

#[wasm_bindgen_test]
fn attaches_event_handlers_during_hydration() {
    let clicked = Rc::new(Cell::new(false));
    let make = || {
        let c = clicked.clone();
        rsx! { <button mix={on("click", EventHandler::new(move |_| c.set(true)))}>"Click me"</button> }
    };
    let (_, button) = adopts(make, "button");
    button.click();
    assert!(clicked.get());
}

#[wasm_bindgen_test]
fn hydrates_keyed_elements() {
    let make = || {
        let items: Vec<Node> = [("a", "Item A"), ("b", "Item B"), ("c", "Item C")]
            .into_iter()
            .map(|(id, text)| rsx! { <li key={id}>{text}</li> })
            .collect();
        rsx! { <ul>{items}</ul> }
    };
    let el = server(make());
    let items = all(&el, "li");
    assert_eq!(items.len(), 3);
    hydrate(&el, make());
    let hydrated = all(&el, "li");
    for i in 0..3 {
        assert!(same(&hydrated[i], &items[i]));
    }
}

#[wasm_bindgen_test]
fn hydrates_deeply_nested_elements() {
    let make = || {
        rsx! {
            <div class="level-1"><div class="level-2"><div class="level-3"><div class="level-4"><span>"Deep content"</span></div></div></div></div>
        }
    };
    let el = server(make());
    let selectors = [".level-1", ".level-2", ".level-3", ".level-4", "span"];
    let before: Vec<_> = selectors.iter().map(|s| find(&el, s)).collect();
    hydrate(&el, make());
    for (s, node) in selectors.iter().zip(&before) {
        assert!(same(&find(&el, s), node), "{s}");
    }
}

#[wasm_bindgen_test]
fn hydrates_head_like_elements_in_place() {
    let el: web_sys::HtmlElement = document().create_element("div").unwrap().unchecked_into();
    document().body().unwrap().append_child(&el).unwrap();
    el.set_inner_html(concat!(
        "<title>Hydrated title</title>",
        r#"<meta name="description" content="Hydrated description" />"#,
        r#"<script type="application/ld+json">{"@type":"Thing","name":"Hydrated"}</script>"#,
        r#"<div id="content">Body content</div>"#,
    ));
    let selectors = ["title", r#"meta[name="description"]"#, r#"script[type="application/ld+json"]"#, "#content"];
    let before: Vec<_> = selectors.iter().map(|s| find(&el, s)).collect();
    hydrate(
        &el,
        rsx! {
            <>
                <title>"Hydrated title"</title>
                <meta name="description" content="Hydrated description"/>
                <script type="application/ld+json">{r#"{"@type":"Thing","name":"Hydrated"}"#}</script>
                <div id="content">"Body content"</div>
            </>
        },
    );
    for (s, node) in selectors.iter().zip(&before) {
        assert!(same(&find(&el, s), node), "{s}");
    }
    let head = document().head().unwrap();
    assert!(head.query_selector(r#"meta[name="description"]"#).unwrap().is_none());
    assert!(head.query_selector(r#"script[type="application/ld+json"]"#).unwrap().is_none());
}

// hydration.css

#[wasm_bindgen_test]
fn hydrates_element_with_css_mixin_and_adopts_server_style() {
    let make = || rsx! { <div mix={css("color: red")}>"Hello"</div> };
    let el = server(make());
    let div = find(&el, "div");
    let class = div.class_name();
    assert!(class.contains("rmxc-"));
    hydrate(&el, make());
    assert!(same(&find(&el, "div"), &div));
    assert_eq!(div.class_name(), class);
    assert_eq!(color(&div), "rgb(255, 0, 0)");
}

#[wasm_bindgen_test]
fn updates_css_mixin_after_hydration() {
    let el = server(rsx! { <div mix={css("color: red")}>"Hello"</div> });
    let div = find(&el, "div");
    let root = hydrate(&el, rsx! { <div mix={css("color: red")}>"Hello"</div> });
    assert_eq!(color(&div), "rgb(255, 0, 0)");
    root.render(rsx! { <div mix={css("color: blue")}>"Hello"</div> });
    root.flush();
    assert_eq!(color(&div), "rgb(0, 0, 255)");
    assert!(div.class_name().contains("rmxc-"));
}

#[wasm_bindgen_test]
fn hydrates_css_mixin_combined_with_class() {
    let make = || rsx! { <div class="my-class" mix={css("color: green")}>"Hello"</div> };
    let el = server(make());
    let div = find(&el, "div");
    assert!(div.class_name().contains("my-class") && div.class_name().contains("rmxc-"));
    hydrate(&el, make());
    assert!(same(&find(&el, "div"), &div));
    assert!(div.class_name().contains("my-class") && div.class_name().contains("rmxc-"));
    assert_eq!(color(&div), "rgb(0, 128, 0)");
}

#[wasm_bindgen_test]
fn multiple_elements_with_the_same_css_mixin_share_a_style_during_hydration() {
    let make = || rsx! { <div><span mix={css("color: purple")}>"First"</span><span mix={css("color: purple")}>"Second"</span></div> };
    let el = server(make());
    let spans = all(&el, "span");
    let class = spans[0].class_name();
    assert_eq!(spans[1].class_name(), class);
    assert!(class.contains("rmxc-"));
    hydrate(&el, make());
    let hydrated = all(&el, "span");
    for (h, s) in hydrated.iter().zip(&spans) {
        assert!(same(h, s));
        assert_eq!(h.class_name(), class);
        assert_eq!(color(h), "rgb(128, 0, 128)");
    }
}

#[wasm_bindgen_test]
fn handles_element_unmount_with_css_mixin_after_hydration() {
    let make = || rsx! { <div><span mix={css("color: orange")}>"Will unmount"</span></div> };
    let el = server(make());
    let root = hydrate(&el, make());
    assert!(el.query_selector("span").unwrap().is_some());
    root.render(rsx! { <div></div> });
    root.flush();
    assert!(el.query_selector("span").unwrap().is_none());
}

#[wasm_bindgen_test]
fn keeps_an_earlier_island_styled_when_a_second_island_is_created_before_the_first_renders() {
    let one = || rsx! { <div mix={css("color: rgb(120, 30, 40)")}>"One"</div> };
    let two = || rsx! { <div mix={css("color: rgb(50, 60, 70)")}>"Two"</div> };
    let (el1, el2) = (server(one()), server(two()));
    let (root1, root2) = (create_root(&el1), create_root(&el2));
    let (div1, div2) = (find(&el1, "div"), find(&el2, "div"));
    assert_eq!(color(&div1), "rgb(120, 30, 40)");
    assert_eq!(color(&div2), "rgb(50, 60, 70)");
    root1.render(one());
    root1.flush();
    root2.render(two());
    root2.flush();
    assert_eq!(color(&div1), "rgb(120, 30, 40)");
    assert_eq!(color(&div2), "rgb(50, 60, 70)");
    root1.dispose();
    root2.dispose();
}

#[wasm_bindgen_test]
fn adds_css_mixin_during_hydration_when_server_had_none() {
    let el = server(rsx! { <div>"Hello"</div> });
    let div = find(&el, "div");
    assert_eq!(div.class_name(), "");
    hydrate(&el, rsx! { <div mix={css("color: cyan")}>"Hello"</div> });
    assert!(same(&find(&el, "div"), &div));
    assert!(div.class_name().contains("rmxc-"));
    assert_eq!(color(&div), "rgb(0, 255, 255)");
}

// hydration.extra-nodes: browser extension injection

fn inject(parent: &web_sys::Element, id: &str, before: Option<&web_sys::Node>) -> web_sys::Element {
    let aside = document().create_element("aside").unwrap();
    aside.set_id(id);
    aside.set_text_content(Some("extension content"));
    parent.insert_before(&aside, before).unwrap();
    aside
}

#[wasm_bindgen_test]
fn ignores_extra_nodes_at_the_end_of_an_element() {
    let make = || rsx! { <div><span>"Our content"</span></div> };
    let el = server(make());
    let (div, span) = (find(&el, "div"), find(&el, "span"));
    let injected = inject(&div, "ext-injected", None);
    hydrate(&el, make());
    assert!(same(&find(&el, "span"), &span));
    assert!(same(&find(&div, "#ext-injected"), &injected));
}

#[wasm_bindgen_test]
fn skips_an_injected_node_at_the_start_and_adopts_our_content() {
    let make = || rsx! { <div><span>"Our content"</span></div> };
    let el = server(make());
    let (div, span) = (find(&el, "div"), find(&el, "span"));
    let injected = inject(&div, "ext-start", Some(&span));
    let errors = quiet_errors(|| {
        hydrate(&el, make());
    });
    assert!(same(&find(&el, "span"), &span));
    assert!(same(&find(&div, "#ext-start"), &injected));
    assert_eq!(errors, 0);
}

#[wasm_bindgen_test]
fn handles_injected_nodes_at_both_start_and_end() {
    let make = || rsx! { <div><span>"Our content"</span></div> };
    let el = server(make());
    let (div, span) = (find(&el, "div"), find(&el, "span"));
    let start = inject(&div, "ext-start", Some(&span));
    let end = inject(&div, "ext-end", None);
    hydrate(&el, make());
    assert!(same(&find(&el, "span"), &span));
    assert!(same(&find(&div, "#ext-start"), &start));
    assert!(same(&find(&div, "#ext-end"), &end));
}

#[wasm_bindgen_test]
fn extra_nodes_survive_subsequent_updates() {
    let el = server(rsx! { <div><span>"Content 1"</span></div> });
    let div = find(&el, "div");
    let injected = inject(&div, "extension", None);
    let root = hydrate(&el, rsx! { <div><span>"Content 1"</span></div> });
    assert!(same(&find(&div, "#extension"), &injected));
    root.render(rsx! { <div><span>"Content 2"</span></div> });
    root.flush();
    assert!(same(&find(&div, "#extension"), &injected));
    assert_eq!(find(&div, "span").text_content().as_deref(), Some("Content 2"));
}

// hydration.forms

fn input_value(el: &web_sys::HtmlElement) -> String {
    prop(el, "value").as_string().unwrap()
}

#[wasm_bindgen_test]
fn hydrates_input_with_value_attribute() {
    let (_, input) = adopts(|| rsx! { <input type="text" value="server value" readonly/> }, "input");
    assert_eq!(input_value(&input), "server value");
}

#[wasm_bindgen_test]
fn hydrates_input_with_default_value() {
    let (_, input) = adopts(|| rsx! { <input type="text" defaultValue="default"/> }, "input");
    assert_eq!(input_value(&input), "default");
}

#[wasm_bindgen_test]
fn hydrates_checkbox_with_checked_attribute() {
    let (_, input) = adopts(|| rsx! { <input type="checkbox" checked readonly/> }, "input");
    assert_eq!(prop(&input, "checked"), JsValue::TRUE);
}

#[wasm_bindgen_test]
fn hydrates_checkbox_with_default_checked() {
    let (_, input) = adopts(|| rsx! { <input type="checkbox" defaultChecked/> }, "input");
    assert_eq!(prop(&input, "checked"), JsValue::TRUE);
}

#[wasm_bindgen_test]
fn hydrates_radio_with_checked_attribute() {
    let make = || rsx! { <div><input type="radio" name="choice" value="a" checked readonly/><input type="radio" name="choice" value="b" readonly/></div> };
    let el = server(make());
    let inputs = all(&el, "input");
    assert_eq!(prop(&inputs[0], "checked"), JsValue::TRUE);
    assert_eq!(prop(&inputs[1], "checked"), JsValue::FALSE);
    hydrate(&el, make());
    let hydrated = all(&el, "input");
    assert!(same(&hydrated[0], &inputs[0]) && same(&hydrated[1], &inputs[1]));
    assert_eq!(prop(&hydrated[0], "checked"), JsValue::TRUE);
    assert_eq!(prop(&hydrated[1], "checked"), JsValue::FALSE);
}

#[wasm_bindgen_test]
fn hydrates_textarea_with_value() {
    let (_, textarea) = adopts(|| rsx! { <textarea value="textarea content" readonly></textarea> }, "textarea");
    assert_eq!(input_value(&textarea), "textarea content");
}

thread_local! {
    static TEXTAREA: RefCell<Option<Handle<Textarea>>> = const { RefCell::new(None) };
    static TEXTAREA_VALUE: RefCell<Option<&'static str>> = const { RefCell::new(None) };
}

/// `defaultValue` when `TEXTAREA_VALUE` is `None`, a controlled `value` otherwise.
struct Textarea;

impl Component for Textarea {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        TEXTAREA.with(|t| *t.borrow_mut() = Some(h.clone()));
        Textarea
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        match TEXTAREA_VALUE.with(|v| *v.borrow()) {
            Some(value) => rsx! { <textarea value={value} readonly></textarea> },
            None => rsx! { <textarea defaultValue="default text"></textarea> },
        }
    }
}

fn update_textarea(root: &Root) {
    TEXTAREA.with(|t| t.borrow().as_ref().unwrap().update());
    root.flush();
}

#[wasm_bindgen_test]
fn preserves_an_untouched_textarea_default_value_across_updates() {
    TEXTAREA_VALUE.with(|v| *v.borrow_mut() = None);
    let el = server(rsx! { <Textarea/> });
    let textarea = find(&el, "textarea");
    let root = hydrate(&el, rsx! { <Textarea/> });
    assert_eq!(input_value(&textarea), "default text");
    update_textarea(&root);
    assert_eq!(input_value(&textarea), "default text");
    update_textarea(&root);
    assert_eq!(input_value(&textarea), "default text");
}

#[wasm_bindgen_test]
fn preserves_a_user_edited_textarea_default_value_across_updates() {
    TEXTAREA_VALUE.with(|v| *v.borrow_mut() = None);
    let el = server(rsx! { <Textarea/> });
    let textarea = find(&el, "textarea");
    let root = hydrate(&el, rsx! { <Textarea/> });
    js_sys::Reflect::set(&textarea, &"value".into(), &"user text".into()).unwrap();
    update_textarea(&root);
    assert_eq!(input_value(&textarea), "user text");
    update_textarea(&root);
    assert_eq!(input_value(&textarea), "user text");
}

#[wasm_bindgen_test]
fn updates_a_controlled_textarea_value() {
    TEXTAREA_VALUE.with(|v| *v.borrow_mut() = Some("first value"));
    let el = server(rsx! { <Textarea/> });
    let textarea = find(&el, "textarea");
    let root = hydrate(&el, rsx! { <Textarea/> });
    assert_eq!(input_value(&textarea), "first value");
    update_textarea(&root);
    assert_eq!(input_value(&textarea), "first value");
    TEXTAREA_VALUE.with(|v| *v.borrow_mut() = Some("second value"));
    update_textarea(&root);
    assert_eq!(input_value(&textarea), "second value");
}

#[wasm_bindgen_test]
fn hydrates_select_with_selected_option() {
    let (_, select) = adopts(
        || rsx! { <select value="b"><option value="a">"A"</option><option value="b">"B"</option><option value="c">"C"</option></select> },
        "select",
    );
    assert_eq!(input_value(&select), "b");
}

// hydration.mismatch

#[wasm_bindgen_test]
fn adopts_element_and_patches_mismatched_attributes() {
    let el = server(rsx! { <div class="server-class" data-value="server"></div> });
    let div = find(&el, "div");
    hydrate(&el, rsx! { <div class="client-class" data-value="client"></div> });
    assert!(same(&find(&el, "div"), &div));
    assert_eq!(div.get_attribute("class").as_deref(), Some("client-class"));
    assert_eq!(div.get_attribute("data-value").as_deref(), Some("client"));
}

#[wasm_bindgen_test]
fn adds_missing_attributes_during_hydration() {
    let el = server(rsx! { <div class="existing"></div> });
    let div = find(&el, "div");
    hydrate(&el, rsx! { <div class="existing" data-new="added" title="hello"></div> });
    assert!(same(&find(&el, "div"), &div));
    assert_eq!(div.get_attribute("data-new").as_deref(), Some("added"));
    assert_eq!(div.get_attribute("title").as_deref(), Some("hello"));
}

#[wasm_bindgen_test]
fn leaves_extra_attributes_alone_during_hydration() {
    let el = server(rsx! { <div class="keep" data-extra="yes" title="extra"></div> });
    let div = find(&el, "div");
    hydrate(&el, rsx! { <div class="keep"></div> });
    assert!(same(&find(&el, "div"), &div));
    assert_eq!(div.get_attribute("class").as_deref(), Some("keep"));
    assert!(div.has_attribute("data-extra") && div.has_attribute("title"));
}

#[wasm_bindgen_test]
fn preserves_dom_node_identity_when_only_attributes_differ() {
    let el = server(rsx! { <div id="test" class="old" data-value="old"><span>"Child"</span></div> });
    let (div, span) = (find(&el, "#test"), find(&el, "span"));
    hydrate(&el, rsx! { <div id="test" class="new" data-value="new"><span>"Child"</span></div> });
    assert!(same(&find(&el, "#test"), &div));
    assert!(same(&find(&el, "span"), &span));
}

#[wasm_bindgen_test]
fn advances_the_cursor_once_on_a_type_mismatch_to_find_our_element() {
    let make = || rsx! { <div><span>"Our content"</span></div> };
    let el = server(make());
    let (div, span) = (find(&el, "div"), find(&el, "span"));
    let injected = document().create_element("div").unwrap();
    injected.set_class_name("injected");
    div.insert_before(&injected, Some(&span)).unwrap();
    hydrate(&el, make());
    assert!(same(&find(&el, "span"), &span));
}

#[wasm_bindgen_test]
fn recreates_the_element_when_the_retry_also_fails() {
    let make = || rsx! { <div><span>"Original"</span></div> };
    let el = server(make());
    let (div, span) = (find(&el, "div"), find(&el, "span"));
    div.set_inner_html("<div>Wrong</div><p>Also wrong</p>");
    let errors = quiet_errors(|| {
        hydrate(&el, make());
    });
    let new_span = find(&el, "span");
    assert!(!same(&new_span, &span));
    assert_eq!(new_span.text_content().as_deref(), Some("Original"));
    assert_eq!(errors, 1, "one hydration mismatch is logged");
}

#[wasm_bindgen_test]
fn leaves_skipped_nodes_in_place() {
    let make = || rsx! { <div><span>"Our content"</span></div> };
    let el = server(make());
    let (div, span) = (find(&el, "div"), find(&el, "span"));
    let skipped = inject(&div, "skipped", Some(&span));
    hydrate(&el, make());
    assert!(same(&find(&div, "#skipped"), &skipped));
}

// hydration.text

#[wasm_bindgen_test]
fn adopts_a_single_server_text_node_when_the_client_has_several_text_children() {
    let el = server(rsx! { <span>"Hello world"</span> });
    let span = find(&el, "span");
    hydrate(&el, rsx! { <span>{"Hello "}{"world"}</span> });
    assert!(same(&find(&el, "span"), &span));
    assert_eq!(span.text_content().as_deref(), Some("Hello world"));
}

#[wasm_bindgen_test]
fn subsequent_update_patches_consolidated_text_content() {
    let el = server(rsx! { <span>"Hello world"</span> });
    let span = find(&el, "span");
    let root = hydrate(&el, rsx! { <span>{"Hello "}{"world"}</span> });
    assert_eq!(span.text_content().as_deref(), Some("Hello world"));
    root.render(rsx! { <span>{"Hello "}{"Ryan"}</span> });
    root.flush();
    assert_eq!(span.text_content().as_deref(), Some("Hello Ryan"));
}

#[wasm_bindgen_test]
fn handles_empty_children_as_empty_text() {
    let makers: [fn() -> Node; 2] = [|| rsx! { <div>{()}</div> }, || rsx! { <div>{None::<&str>}</div> }];
    for make in makers {
        let el = server(make());
        let div = find(&el, "div");
        hydrate(&el, make());
        assert!(same(&find(&el, "div"), &div));
        assert_eq!(div.text_content().as_deref(), Some(""));
    }
}

#[wasm_bindgen_test]
fn consolidates_parser_split_server_text_nodes() {
    let line_count = 10000;
    let large: String = (0..line_count).map(|i| format!("row-{i}")).collect::<Vec<_>>().join("\n");
    assert!(large.len() > 65536);
    let el = server(rsx! { <pre>{large.clone()}</pre> });
    let pre = find(&el, "pre");
    let first = document().create_text_node(&large[..65536]);
    let second = document().create_text_node(&large[65536..]);
    pre.replace_children_with_node_2(&first, &second);
    assert_eq!(pre.child_nodes().length(), 2);
    hydrate(&el, rsx! { <pre>{large.clone()}</pre> });
    assert!(same(&find(&el, "pre"), &pre));
    assert!(pre.first_child().unwrap().is_same_node(Some(&first)));
    assert_eq!(pre.child_nodes().length(), 1);
    assert_eq!(pre.text_content().unwrap(), large);
}

#[wasm_bindgen_test]
fn hydrates_adjacent_text_children_across_parser_split_nodes() {
    let (a, b) = ("a".repeat(70000), "b".repeat(70000));
    let combined = format!("{a}{b}");
    let el = server(rsx! { <pre>{a.clone()}{b.clone()}<span>"after"</span></pre> });
    let (pre, span) = (find(&el, "pre"), find(&el, "span"));
    let split = 65536;
    let t1 = document().create_text_node(&combined[..split]);
    let t2 = document().create_text_node(&combined[split..split * 2]);
    let t3 = document().create_text_node(&combined[split * 2..]);
    pre.replace_children_with_node(&js_sys::Array::of4(&t1, &t2, &t3, &span));
    let root = hydrate(&el, rsx! { <pre>{a.clone()}{b.clone()}<span>"after"</span></pre> });
    assert!(same(&find(&el, "pre"), &pre));
    assert!(pre.first_child().unwrap().is_same_node(Some(&t1)));
    assert_eq!(pre.child_nodes().length(), 3);
    assert!(pre.last_child().unwrap().is_same_node(Some(&span)));
    assert_eq!(pre.text_content().unwrap(), format!("{combined}after"));

    root.render(rsx! { <pre>{"updated first"}{"updated second"}<span>"after"</span></pre> });
    root.flush();
    assert_eq!(pre.child_nodes().length(), 3);
    assert!(pre.last_child().unwrap().is_same_node(Some(&span)));
    assert_eq!(pre.text_content().unwrap(), "updated firstupdated secondafter");
}

// hydration.void-elements

#[wasm_bindgen_test]
fn hydrates_input_element() {
    let (_, input) = adopts(|| rsx! { <input type="text" placeholder="Enter text"/> }, "input");
    assert_eq!(input.get_attribute("placeholder").as_deref(), Some("Enter text"));
}

#[wasm_bindgen_test]
fn hydrates_br_element() {
    adopts(|| rsx! { <div>"Line 1"<br/>"Line 2"</div> }, "br");
}

#[wasm_bindgen_test]
fn hydrates_img_element() {
    adopts(|| rsx! { <img src="/test.png" alt="Test image"/> }, "img");
}

#[wasm_bindgen_test]
fn hydrates_hr_element() {
    adopts(|| rsx! { <div><p>"Above"</p><hr/><p>"Below"</p></div> }, "hr");
}

#[wasm_bindgen_test]
fn hydrates_meta_element() {
    adopts(|| rsx! { <meta name="viewport" content="width=device-width"/> }, "meta");
}

#[wasm_bindgen_test]
fn hydrates_link_element() {
    adopts(|| rsx! { <link rel="stylesheet" href="/styles.css"/> }, "link");
}
