//! Ports of Remix's `vdom.svg.test.tsx` plus attribute safety checks. Run with
//! `pwsh ./scripts/test-browser.ps1 -Test browser_svg`.
//!
//! JSX camelCase props (`strokeLinecap`, `xlinkHref`) are written as the real attribute
//! names in rsx (`stroke-linecap`, `xlink:href`), so there is no name translation.
#![cfg(target_arch = "wasm32")]

extern crate rsmix_component as rsmix;

use std::cell::Cell;

use rsmix::{create_root, on, rsx, Children, Component, EventHandler, Handle, Node};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

const SVG: &str = "http://www.w3.org/2000/svg";
const XLINK: &str = "http://www.w3.org/1999/xlink";
const XML: &str = "http://www.w3.org/XML/1998/namespace";

fn container() -> web_sys::HtmlElement {
    web_sys::window().unwrap().document().unwrap().create_element("div").unwrap().unchecked_into()
}

fn find(el: &web_sys::HtmlElement, selector: &str) -> web_sys::Element {
    el.query_selector(selector).unwrap().unwrap_or_else(|| panic!("no {selector}"))
}

fn ns(e: &web_sys::Element) -> String {
    e.namespace_uri().unwrap_or_default()
}

#[wasm_bindgen_test]
fn renders_svg_root_and_children_with_svg_namespace_and_attributes() {
    let el = container();
    create_root(&el).render(rsx! {
        <svg viewBox="0 0 24 24" fill="none" class="icon">
            <path stroke-linecap="round" stroke-linejoin="round" d="m4.5 12.75 6 6 9-13.5"/>
        </svg>
    });
    let svg = find(&el, "svg");
    let path = find(&el, "path");
    assert_eq!(ns(&svg), SVG);
    assert_eq!(ns(&path), SVG);
    assert_eq!(svg.get_attribute("viewBox").as_deref(), Some("0 0 24 24"));
    assert_eq!(svg.get_attribute("class").as_deref(), Some("icon"));
    assert_eq!(path.get_attribute("stroke-linecap").as_deref(), Some("round"));
}

#[wasm_bindgen_test]
fn updates_and_removes_namespaced_svg_attributes() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <svg><use id="u" xlink:href="#one"/><text id="t" xml:lang="en">"Hi"</text></svg> });
    let u = find(&el, "#u");
    let t = find(&el, "#t");
    assert_eq!(u.get_attribute_ns(Some(XLINK), "href").as_deref(), Some("#one"));
    assert_eq!(t.get_attribute_ns(Some(XML), "lang").as_deref(), Some("en"));
    root.render(rsx! { <svg><use id="u" xlink:href="#two"/><text id="t" xml:lang="fr">"Hi"</text></svg> });
    assert_eq!(u.get_attribute_ns(Some(XLINK), "href").as_deref(), Some("#two"));
    assert_eq!(t.get_attribute_ns(Some(XML), "lang").as_deref(), Some("fr"));
    root.render(rsx! { <svg><use id="u"/><text id="t">"Hi"</text></svg> });
    assert_eq!(u.get_attribute_ns(Some(XLINK), "href"), None);
    assert_eq!(u.get_attribute("xlink:href"), None);
    assert_eq!(t.get_attribute_ns(Some(XML), "lang"), None);
}

#[wasm_bindgen_test]
fn renders_html_subtree_inside_foreign_object_with_html_namespace() {
    let el = container();
    create_root(&el).render(rsx! { <svg><foreignObject><div id="x">"Hello"</div></foreignObject></svg> });
    assert_eq!(ns(&find(&el, "foreignObject")), SVG);
    let div = find(&el, "#x");
    assert!(div.is_instance_of::<web_sys::HtmlDivElement>());
    assert_eq!(ns(&div), "http://www.w3.org/1999/xhtml");
}

#[wasm_bindgen_test]
fn updates_and_removes_svg_attributes() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <svg><path id="p" stroke-linecap="round"/></svg> });
    let path = find(&el, "#p");
    root.render(rsx! { <svg><path id="p" stroke-linecap="square"/></svg> });
    assert!(find(&el, "#p").is_same_node(Some(&path)));
    assert_eq!(path.get_attribute("stroke-linecap").as_deref(), Some("square"));
    root.render(rsx! { <svg><path id="p"/></svg> });
    assert!(!path.has_attribute("stroke-linecap"));
}

#[wasm_bindgen_test]
fn uses_canonical_case_for_critical_svg_attributes_and_elements() {
    let el = container();
    create_root(&el).render(rsx! {
        <svg>
            <defs>
                <filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="100" height="100">
                    <feGaussianBlur id="blur" stdDeviation="2.5"/>
                </filter>
                <linearGradient id="g" gradientUnits="userSpaceOnUse"/>
                <mask id="m" maskUnits="userSpaceOnUse"/>
                <clipPath id="c" clipPathUnits="objectBoundingBox"/>
            </defs>
        </svg>
    });
    let filter = find(&el, "#f");
    assert_eq!(ns(&filter), SVG);
    assert_eq!(filter.get_attribute("filterUnits").as_deref(), Some("userSpaceOnUse"));
    let blur = find(&el, "#blur");
    assert_eq!(ns(&blur), SVG);
    assert_eq!(blur.local_name(), "feGaussianBlur");
    assert_eq!(blur.get_attribute("stdDeviation").as_deref(), Some("2.5"));
    assert_eq!(ns(&find(&el, "#g")), SVG);
    assert_eq!(ns(&find(&el, "#m")), SVG);
    assert_eq!(find(&el, "#c").get_attribute("clipPathUnits").as_deref(), Some("objectBoundingBox"));
}

#[wasm_bindgen_test]
fn sets_booleanish_string_attributes_as_explicit_values() {
    let el = container();
    create_root(&el).render(rsx! {
        <svg>
            <animate id="a" autoReverse={false}/>
            <rect id="r" externalResourcesRequired={false} focusable={false}/>
            <feColorMatrix id="m" preserveAlpha={false}/>
        </svg>
    });
    assert_eq!(find(&el, "#a").get_attribute("autoReverse").as_deref(), Some("false"));
    assert_eq!(find(&el, "#r").get_attribute("externalResourcesRequired").as_deref(), Some("false"));
    assert_eq!(find(&el, "#r").get_attribute("focusable").as_deref(), Some("false"));
    assert_eq!(find(&el, "#m").get_attribute("preserveAlpha").as_deref(), Some("false"));
    let el = container();
    create_root(&el).render(rsx! { <div draggable={true} spellcheck={false}>"x"</div> });
    assert_eq!(find(&el, "div").get_attribute("draggable").as_deref(), Some("true"));
    assert_eq!(find(&el, "div").get_attribute("spellcheck").as_deref(), Some("false"));
}

thread_local! {
    static CLICKED: Cell<bool> = const { Cell::new(false) };
}

#[wasm_bindgen_test]
fn attaches_events_on_svg_elements() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <svg><circle id="c" mix={on("click", EventHandler::new(|_| CLICKED.with(|c| c.set(true))))}/></svg> });
    root.flush();
    let init = web_sys::MouseEventInit::new();
    init.set_bubbles(true);
    let event = web_sys::MouseEvent::new_with_mouse_event_init_dict("click", &init).unwrap();
    find(&el, "#c").dispatch_event(&event).unwrap();
    assert!(CLICKED.with(Cell::get));
}

#[derive(Clone, Default)]
struct GroupProps {
    href: &'static str,
    children: Children,
}

struct SvgGroup;

impl Component for SvgGroup {
    type Props = GroupProps;
    fn setup(_: &GroupProps, _: &Handle<Self>) -> Self {
        SvgGroup
    }
    fn render(&self, p: &GroupProps, _: &Handle<Self>) -> Node {
        rsx! { <g href={p.href}>{&p.children}</g> }
    }
}

struct Anchor;

impl Component for Anchor {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Anchor
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <a href="/x"><title>"t"</title></a> }
    }
}

#[wasm_bindgen_test]
fn propagates_svg_namespace_through_components() {
    let el = container();
    create_root(&el).render(rsx! {
        <svg width="100" height="100">
            <SvgGroup href="/test"><path id="p"/><Anchor/></SvgGroup>
        </svg>
    });
    for selector in ["svg", "g", "#p", "a", "title"] {
        assert_eq!(ns(&find(&el, selector)), SVG, "{selector}");
    }
}

#[wasm_bindgen_test]
fn svg_namespace_survives_updates() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <svg><g/></svg> });
    root.render(rsx! { <svg><g><a href="/y"/></g><foreignObject><p id="html">"x"</p></foreignObject></svg> });
    assert_eq!(ns(&find(&el, "a")), SVG);
    assert_eq!(ns(&find(&el, "#html")), "http://www.w3.org/1999/xhtml");
}

// attribute safety (Remix's sanitizeUrlAttribute and isAllowedHostPropName)

#[wasm_bindgen_test]
fn blocks_javascript_urls_and_inline_handler_attributes() {
    let el = container();
    let root = create_root(&el);
    let href = String::from(" java\tscript:alert(1)");
    root.render(rsx! { <a href={href} onclick="alert(1)">"x"</a> });
    let a = find(&el, "a");
    assert!(a.get_attribute("href").unwrap().contains("blocked a javascript: URL"));
    assert!(!a.has_attribute("onclick"));
    root.render(rsx! { <a href="/safe">"x"</a> });
    assert_eq!(a.get_attribute("href").as_deref(), Some("/safe"));
}
