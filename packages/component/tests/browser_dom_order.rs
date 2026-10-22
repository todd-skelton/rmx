//! Ports of Remix's `vdom.dom-order.test.tsx`. Run with
//! `pwsh ./scripts/test-browser.ps1 -Test browser_dom_order`.
//!
//! A component that renders nothing keeps an empty text node as its position in rmx (Remix
//! keeps none), so sibling checks use `nextElementSibling`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::cell::{Cell, RefCell};

use rmx::{create_root, rsx, Component, Handle, Node};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn container() -> web_sys::HtmlElement {
    web_sys::window().unwrap().document().unwrap().create_element("div").unwrap().unchecked_into()
}

fn find(el: &web_sys::HtmlElement, selector: &str) -> web_sys::Element {
    el.query_selector(selector).unwrap().unwrap_or_else(|| panic!("no {selector}"))
}

fn next(e: &web_sys::Element) -> Option<web_sys::Element> {
    e.next_element_sibling()
}

fn prev(e: &web_sys::Element) -> Option<web_sys::Element> {
    e.previous_element_sibling()
}

fn is(a: Option<web_sys::Element>, b: &web_sys::Element) -> bool {
    a.is_some_and(|a| a.is_same_node(Some(b)))
}

thread_local! {
    static FLAG: Cell<bool> = const { Cell::new(false) };
    static FLAG_B: Cell<bool> = const { Cell::new(false) };
    static NUM: Cell<u32> = const { Cell::new(0) };
    static ITEMS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    static UPDATES: RefCell<Vec<Box<dyn Fn()>>> = const { RefCell::new(Vec::new()) };
}

fn flag() -> bool {
    FLAG.with(Cell::get)
}

fn set_flag(v: bool) {
    FLAG.with(|f| f.set(v));
}

fn capture<C: Component>(h: &Handle<C>) {
    let h = h.clone();
    UPDATES.with(|u| u.borrow_mut().push(Box::new(move || h.update())));
}

/// Call every captured `handle.update` (or just the `i`th).
fn update(i: Option<usize>) {
    UPDATES.with(|u| match i {
        Some(i) => (u.borrow()[i])(),
        None => u.borrow().iter().for_each(|f| f()),
    });
}

fn reset() {
    set_flag(false);
    FLAG_B.with(|f| f.set(false));
    NUM.with(|n| n.set(0));
    ITEMS.with(|i| i.borrow_mut().clear());
    UPDATES.with(|u| u.borrow_mut().clear());
}

/// A component with no props and a render function, like Remix's `function X() { return () => ... }`.
macro_rules! component {
    ($name:ident, |$h:ident| $render:expr) => {
        component!($name, |$h| {}, $render);
    };
    ($name:ident, |$h:ident| $setup:block, $render:expr) => {
        struct $name;
        impl Component for $name {
            type Props = ();
            #[allow(unused_variables)]
            fn setup(_: &(), $h: &Handle<Self>) -> Self {
                $setup;
                $name
            }
            #[allow(unused_variables)]
            fn render(&self, _: &(), $h: &Handle<Self>) -> Node {
                $render
            }
        }
    };
}

component!(SwitchTag, |h| { capture(h) }, if flag() { rsx! { <span>"B"</span> } } else { rsx! { <div>"A"</div> } });

#[wasm_bindgen_test]
fn maintains_dom_order_when_component_switches_element_types_via_self_update() {
    reset();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <main><SwitchTag/><p>"C"</p></main> });
    assert_eq!(el.inner_html(), "<main><div>A</div><p>C</p></main>");
    set_flag(true);
    update(None);
    root.flush();
    assert_eq!(el.inner_html(), "<main><span>B</span><p>C</p></main>");
    set_flag(false);
    update(None);
    root.flush();
    assert_eq!(el.inner_html(), "<main><div>A</div><p>C</p></main>");
}

component!(SpanB, |h| rsx! { <span>"B"</span> });
component!(SwitchToComponent, |h| { capture(h) }, if flag() { rsx! { <SpanB/> } } else { rsx! { <div>"A"</div> } });

#[wasm_bindgen_test]
fn maintains_dom_order_when_component_switches_from_component_to_element_via_self_update() {
    reset();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <main><SwitchToComponent/><p>"C"</p></main> });
    assert_eq!(el.inner_html(), "<main><div>A</div><p>C</p></main>");
    set_flag(true);
    update(None);
    root.flush();
    assert_eq!(el.inner_html(), "<main><span>B</span><p>C</p></main>");
    set_flag(false);
    update(None);
    root.flush();
    assert_eq!(el.inner_html(), "<main><div>A</div><p>C</p></main>");
}

component!(LoadingDiv, |h| rsx! { <div>"Loading..."</div> });
component!(PageA, |h| rsx! { <div>"Page A"</div> });
component!(PageB, |h| { capture(h) }, if flag() { rsx! { <div>"Loaded!"</div> } } else { rsx! { <LoadingDiv/> } });
component!(
    PageApp,
    |h| if FLAG_B.with(Cell::get) { rsx! { <main><nav>"Nav"</nav><PageB/></main> } } else { rsx! { <main><nav>"Nav"</nav><PageA/></main> } }
);

#[wasm_bindgen_test]
fn updates_correctly_when_replaced_component_self_updates_from_component_to_element() {
    reset();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <PageApp/> });
    assert_eq!(el.inner_html(), "<main><nav>Nav</nav><div>Page A</div></main>");
    FLAG_B.with(|f| f.set(true));
    root.render(rsx! { <PageApp/> });
    assert_eq!(el.inner_html(), "<main><nav>Nav</nav><div>Loading...</div></main>");
    set_flag(true);
    update(None);
    root.flush();
    assert_eq!(el.inner_html(), "<main><nav>Nav</nav><div>Loaded!</div></main>");
}

component!(LoadingSpan, |h| rsx! { <span>"Loading..."</span> });
component!(ToLoading, |h| { capture(h) }, if flag() { rsx! { <LoadingSpan/> } } else { rsx! { <div>"Content"</div> } });

#[wasm_bindgen_test]
fn updates_correctly_when_component_switches_from_element_to_component_via_self_update() {
    reset();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <main><ToLoading/><p>"Footer"</p></main> });
    assert_eq!(el.inner_html(), "<main><div>Content</div><p>Footer</p></main>");
    set_flag(true);
    update(None);
    root.flush();
    assert_eq!(el.inner_html(), "<main><span>Loading...</span><p>Footer</p></main>");
    set_flag(false);
    update(None);
    root.flush();
    assert_eq!(el.inner_html(), "<main><div>Content</div><p>Footer</p></main>");
}

component!(Inner, |h| rsx! { <span>"Inner"</span> });
component!(Middle, |h| rsx! { <Inner/> });
component!(Outer, |h| { capture(h) }, if !flag() { rsx! { <Middle/> } } else { rsx! { <div>"Direct"</div> } });

#[wasm_bindgen_test]
fn updates_correctly_with_deeply_nested_component_type_changes() {
    reset();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <main><Outer/><footer>"Footer"</footer></main> });
    assert_eq!(el.inner_html(), "<main><span>Inner</span><footer>Footer</footer></main>");
    set_flag(true);
    update(None);
    root.flush();
    assert_eq!(el.inner_html(), "<main><div>Direct</div><footer>Footer</footer></main>");
    set_flag(false);
    update(None);
    root.flush();
    assert_eq!(el.inner_html(), "<main><span>Inner</span><footer>Footer</footer></main>");
}

component!(LoadingA, |h| rsx! { <span>"Loading A..."</span> });
component!(LoadingB, |h| rsx! { <span>"Loading B..."</span> });
component!(CompA, |h| { capture(h) }, if flag() { rsx! { <div>"A Done"</div> } } else { rsx! { <LoadingA/> } });
component!(CompB, |h| { capture(h) }, if FLAG_B.with(Cell::get) { rsx! { <div>"B Done"</div> } } else { rsx! { <LoadingB/> } });

#[wasm_bindgen_test]
fn updates_correctly_when_multiple_components_are_replaced_and_self_update() {
    reset();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <main><CompA/><CompB/></main> });
    assert_eq!(el.inner_html(), "<main><span>Loading A...</span><span>Loading B...</span></main>");
    set_flag(true);
    update(Some(0));
    root.flush();
    assert_eq!(el.inner_html(), "<main><div>A Done</div><span>Loading B...</span></main>");
    FLAG_B.with(|f| f.set(true));
    update(Some(1));
    root.flush();
    assert_eq!(el.inner_html(), "<main><div>A Done</div><div>B Done</div></main>");
}

component!(CountPage, |h| { capture(h) }, rsx! { <div>"Count: " {NUM.with(Cell::get)}</div> });
component!(
    CountApp,
    |h| if FLAG_B.with(Cell::get) {
        rsx! { <main><nav>"Nav"</nav><CountPage/><footer>"Footer"</footer></main> }
    } else {
        rsx! { <main><nav>"Nav"</nav><PageA/><footer>"Footer"</footer></main> }
    }
);

#[wasm_bindgen_test]
fn maintains_dom_order_when_replaced_component_self_updates_with_same_element_type() {
    reset();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <CountApp/> });
    assert_eq!(el.inner_html(), "<main><nav>Nav</nav><div>Page A</div><footer>Footer</footer></main>");
    FLAG_B.with(|f| f.set(true));
    root.render(rsx! { <CountApp/> });
    assert_eq!(el.inner_html(), "<main><nav>Nav</nav><div>Count: 0</div><footer>Footer</footer></main>");
    for n in 1..=2 {
        NUM.with(|c| c.set(n));
        update(None);
        root.flush();
        assert_eq!(el.inner_html(), format!("<main><nav>Nav</nav><div>Count: {n}</div><footer>Footer</footer></main>"));
    }
}

component!(SpanList, |h| { capture(h) }, {
    let items: Vec<Node> = ITEMS.with(|i| i.borrow().iter().map(|i| rsx! { <span>{*i}</span> }).collect());
    rsx! { <>{items}</> }
});

#[wasm_bindgen_test]
fn maintains_dom_order_when_fragment_component_adds_children_via_self_update_with_siblings() {
    reset();
    ITEMS.with(|i| *i.borrow_mut() = vec![0]);
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <main><SpanList/><footer>"Footer"</footer></main> });
    assert_eq!(el.inner_html(), "<main><span>0</span><footer>Footer</footer></main>");
    ITEMS.with(|i| *i.borrow_mut() = vec![0, 1]);
    update(None);
    root.flush();
    assert_eq!(el.inner_html(), "<main><span>0</span><span>1</span><footer>Footer</footer></main>");
    ITEMS.with(|i| *i.borrow_mut() = vec![0, 1, 2]);
    update(None);
    root.flush();
    assert_eq!(el.inner_html(), "<main><span>0</span><span>1</span><span>2</span><footer>Footer</footer></main>");
}

component!(Dynamic, |h| { capture(h) }, if flag() { rsx! { <span id="middle">"Middle"</span> } } else { Node::Empty });
component!(DynamicApp, |h| rsx! { <main><><Dynamic/><span id="tail">"Tail"</span></><footer id="footer">"Footer"</footer></main> });

#[wasm_bindgen_test]
fn maintains_dom_order_when_fragment_component_has_multiple_renders_before_changing() {
    reset();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <DynamicApp/> });
    let tail = find(&el, "#tail");
    let footer = find(&el, "#footer");
    assert_eq!(el.inner_html(), r#"<main><span id="tail">Tail</span><footer id="footer">Footer</footer></main>"#);
    root.render(rsx! { <DynamicApp/> });
    root.flush();
    assert!(find(&el, "#tail").is_same_node(Some(&tail)));
    assert!(find(&el, "#footer").is_same_node(Some(&footer)));
    set_flag(true);
    update(None);
    root.flush();
    assert_eq!(
        el.inner_html(),
        r#"<main><span id="middle">Middle</span><span id="tail">Tail</span><footer id="footer">Footer</footer></main>"#
    );
    assert!(find(&el, "#tail").is_same_node(Some(&tail)));
    assert!(find(&el, "#footer").is_same_node(Some(&footer)));
}

#[derive(Clone, Default)]
struct MiddleProps {
    id: &'static str,
}

struct MaybeMiddle;

impl Component for MaybeMiddle {
    type Props = MiddleProps;
    fn setup(_: &MiddleProps, h: &Handle<Self>) -> Self {
        capture(h);
        MaybeMiddle
    }
    fn render(&self, p: &MiddleProps, _: &Handle<Self>) -> Node {
        if flag() {
            rsx! { <span id={p.id}>"Middle"</span> }
        } else {
            Node::Empty
        }
    }
}

component!(NestedApp, |h| rsx! {
    <>
        <span id="first">"First"</span>
        <MaybeMiddle id="middle-1"/>
        <span id="second">"Second"</span>
        <>
            <span id="third">"Third"</span>
            <><><MaybeMiddle id="middle-2"/></></>
            <span id="fourth">"Fourth"</span>
        </>
        <div id="div">
            <>
                <span id="fifth">"Fifth"</span>
                <MaybeMiddle id="middle-3"/>
                <span id="sixth">"Sixth"</span>
            </>
        </div>
        <span id="seventh">"Seventh"</span>
    </>
});

#[wasm_bindgen_test]
fn maintains_dom_order_with_non_render_content_in_nested_fragments_after_self_updates() {
    reset();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <NestedApp/> });
    let [first, second, third, fourth, div, fifth, sixth, seventh] =
        ["#first", "#second", "#third", "#fourth", "#div", "#fifth", "#sixth", "#seventh"].map(|s| find(&el, s));
    assert!(is(next(&first), &second));
    assert!(is(next(&second), &third));
    assert!(is(next(&third), &fourth));
    assert!(is(next(&fourth), &div));
    assert!(fifth.parent_node().is_some_and(|p| p.is_same_node(Some(&div))));
    assert!(is(next(&fifth), &sixth));
    assert!(is(next(&div), &seventh));

    set_flag(true);
    update(None);
    root.flush();
    let [m1, m2, m3] = ["#middle-1", "#middle-2", "#middle-3"].map(|s| find(&el, s));
    assert!(is(prev(&m1), &first) && is(next(&m1), &second));
    assert!(is(prev(&m2), &third) && is(next(&m2), &fourth));
    assert!(m3.parent_node().is_some_and(|p| p.is_same_node(Some(&div))));
    assert!(is(prev(&m3), &fifth) && is(next(&m3), &sixth));
}
