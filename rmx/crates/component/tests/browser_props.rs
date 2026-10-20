//! Ports of Remix's `vdom.props.test.tsx`: host prop safety, enumerated attributes,
//! `innerHTML`/`srcdoc` branding and the `css` mixin. Run with
//! `pwsh ./test-browser.ps1 -Test browser_props`.
//!
//! Remix builds the invalid cases with `createElement`; the equivalent here is
//! `Node::element` or `Node::from_json`, since `rsx!` rejects them at compile time.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::borrow::Cow;

use rmx::{create_root, css, rsx, unsafe_html, AttrValue, Node};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

const BLOCKED: &str = "javascript:throw new Error('rmx has blocked a javascript: URL as a security precaution.')";

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn container() -> web_sys::HtmlElement {
    document().create_element("div").unwrap().unchecked_into()
}

fn find(el: &web_sys::HtmlElement, selector: &str) -> web_sys::Element {
    el.query_selector(selector).unwrap().unwrap_or_else(|| panic!("no {selector}"))
}

fn attr(el: &web_sys::HtmlElement, selector: &str, name: &str) -> Option<String> {
    find(el, selector).get_attribute(name)
}

fn str(v: &'static str) -> AttrValue {
    AttrValue::Str(Cow::Borrowed(v))
}

fn color(el: &web_sys::Element) -> String {
    web_sys::window().unwrap().get_computed_style(el).unwrap().unwrap().get_property_value("color").unwrap()
}

fn class_name(el: &web_sys::Element) -> String {
    el.class_name()
}

// special props

#[wasm_bindgen_test]
fn ignores_invalid_host_prop_names() {
    let el = container();
    let root = create_root(&el);
    root.render(Node::element(
        "div",
        None,
        vec![("data-value", str("ok")), ("aria-label", str("Example")), ("invalid name", str("ignored"))],
        vec![],
        vec![],
    ));
    let names: Vec<String> = find(&el, "div").get_attribute_names().iter().map(|n| n.as_string().unwrap()).collect();
    assert_eq!(names, ["data-value", "aria-label"]);
}

#[wasm_bindgen_test]
fn does_not_reflect_inline_event_handler_properties() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div>"content"</div> });
    let div = find(&el, "div");
    let global = js_sys::global();
    js_sys::Reflect::set(&global, &"__rmxInlineCalls".into(), &0.into()).unwrap();
    root.render(Node::element("div", None, vec![("onclick", str("__rmxInlineCalls++"))], vec![], vec![rmx::IntoNode::into_node("content")]));
    assert!(find(&el, "div").is_same_node(Some(&div)));
    assert!(!div.has_attribute("onclick"));
    div.unchecked_ref::<web_sys::HtmlElement>().click();
    assert_eq!(js_sys::Reflect::get(&global, &"__rmxInlineCalls".into()).unwrap(), 0);
}

#[wasm_bindgen_test]
fn does_not_set_inline_event_attributes_on_svg_elements() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <svg onclick="alert(1)" data-value="ok"></svg> });
    let svg = find(&el, "svg");
    assert!(!svg.has_attribute("onclick"));
    assert_eq!(svg.get_attribute("data-value").as_deref(), Some("ok"));
}

#[wasm_bindgen_test]
fn does_not_mutate_an_element_prototype_through_a_host_prop() {
    let el = container();
    let root = create_root(&el);
    root.render(Node::element("div", None, vec![("__proto__", str("changed")), ("id", str("target"))], vec![], vec![]));
    let div = find(&el, "#target");
    assert!(!div.has_attribute("__proto__"));
    assert!(!js_sys::Reflect::has(&div, &"changed".into()).unwrap());
    let div_class = js_sys::Reflect::get(&js_sys::global(), &"HTMLDivElement".into()).unwrap();
    let div_proto = js_sys::Reflect::get(&div_class, &"prototype".into()).unwrap();
    assert_eq!(JsValue::from(js_sys::Object::get_prototype_of(&div)), div_proto);
}

#[wasm_bindgen_test]
fn reflects_custom_element_properties() {
    js_sys::Function::new_no_args(
        "if (!customElements.get('x-rmx-host-props-test')) customElements.define('x-rmx-host-props-test', class extends HTMLElement { settings = undefined })",
    )
    .call0(&JsValue::NULL)
    .unwrap();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <x-rmx-host-props-test settings="compact" title="t"></x-rmx-host-props-test> });
    let custom = find(&el, "x-rmx-host-props-test");
    assert_eq!(js_sys::Reflect::get(&custom, &"settings".into()).unwrap().as_string().as_deref(), Some("compact"));
    // A property the element defines is set as a property, not reflected to an attribute.
    assert!(!custom.has_attribute("settings"));
    assert_eq!(custom.get_attribute("title").as_deref(), Some("t"));

    root.render(rsx! { <x-rmx-host-props-test settings="full"></x-rmx-host-props-test> });
    assert_eq!(js_sys::Reflect::get(&custom, &"settings".into()).unwrap().as_string().as_deref(), Some("full"));
}

#[wasm_bindgen_test]
fn blocks_javascript_urls_in_executable_url_attributes() {
    let el = container();
    let root = create_root(&el);
    let sneaky = "\u{0} \tJ\na\rv\ta\ns\rc\tr\ni\tp\tt:alert(1)";
    root.render(rsx! {
        <>
            <a id="link" href="javascript:alert(1)"></a>
            <img id="image" alt="" src={sneaky}/>
            <form id="form" action="javascript:alert(1)"></form>
            <button id="button" formaction="javascript:alert(1)"></button>
            <svg><use id="use" xlink:href="javascript:alert(1)"></use></svg>
            <object id="object" data="javascript:alert(1)"></object>
        </>
    });
    assert_eq!(attr(&el, "#link", "href").as_deref(), Some(BLOCKED));
    assert_eq!(attr(&el, "#image", "src").as_deref(), Some(BLOCKED));
    assert_eq!(attr(&el, "#form", "action").as_deref(), Some(BLOCKED));
    assert_eq!(attr(&el, "#button", "formaction").as_deref(), Some(BLOCKED));
    assert_eq!(attr(&el, "#use", "xlink:href").as_deref(), Some(BLOCKED));
    assert_eq!(attr(&el, "#object", "data").as_deref(), Some(BLOCKED));
}

#[wasm_bindgen_test]
fn preserves_allowed_urls_and_non_executable_url_attributes() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! {
        <>
            <a id="relative" href="/docs"></a>
            <a id="mailto" href="mailto:test@example.com"></a>
            <a id="blob" href="blob:https://example.com/id"></a>
            <img id="data-url" alt="" src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP"/>
            <img id="srcset" alt="" srcset="javascript:alert(1) 1x"/>
            <video id="poster" poster="javascript:alert(1)"></video>
            <div id="ordinary-data" data="javascript:alert(1)"></div>
        </>
    });
    assert_eq!(attr(&el, "#relative", "href").as_deref(), Some("/docs"));
    assert_eq!(attr(&el, "#mailto", "href").as_deref(), Some("mailto:test@example.com"));
    assert_eq!(attr(&el, "#blob", "href").as_deref(), Some("blob:https://example.com/id"));
    assert_eq!(attr(&el, "#data-url", "src").as_deref(), Some("data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP"));
    assert_eq!(attr(&el, "#srcset", "srcset").as_deref(), Some("javascript:alert(1) 1x"));
    assert_eq!(attr(&el, "#poster", "poster").as_deref(), Some("javascript:alert(1)"));
    assert_eq!(attr(&el, "#ordinary-data", "data").as_deref(), Some("javascript:alert(1)"));
}

// enumerated attributes

#[wasm_bindgen_test]
fn sets_booleanish_string_attributes_as_explicit_false_values() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <img alt="" contenteditable={false} draggable={false} spellcheck={false}/> });
    assert_eq!(attr(&el, "img", "contenteditable").as_deref(), Some("false"));
    assert_eq!(attr(&el, "img", "draggable").as_deref(), Some("false"));
    assert_eq!(attr(&el, "img", "spellcheck").as_deref(), Some("false"));
}

#[wasm_bindgen_test]
fn sets_translate_with_yes_no_attribute_values() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div translate="no"></div> });
    assert_eq!(attr(&el, "div", "translate").as_deref(), Some("no"));
}

#[wasm_bindgen_test]
fn preserves_string_false_values_for_booleanish_string_attributes() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <img alt="" contenteditable="false" draggable="false" spellcheck="false"/> });
    assert_eq!(attr(&el, "img", "contenteditable").as_deref(), Some("false"));
    assert_eq!(attr(&el, "img", "draggable").as_deref(), Some("false"));
    assert_eq!(attr(&el, "img", "spellcheck").as_deref(), Some("false"));
}

// innerHTML prop

#[wasm_bindgen_test]
fn sets_inner_html_on_element() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div innerHTML={unsafe_html("<span>Hello</span>")}></div> });
    assert_eq!(el.inner_html(), "<div><span>Hello</span></div>");
}

#[wasm_bindgen_test]
#[should_panic(expected = "Invalid innerHTML prop: expected a value created with unsafe_html()")]
fn rejects_unbranded_inner_html_values() {
    Node::from_json(&serde_json::json!({ "$rmx": true, "type": "div", "props": { "innerHTML": "<img src=invalid onerror=alert(1)>" } }));
}

#[wasm_bindgen_test]
fn sets_branded_iframe_src_doc() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <iframe srcDoc={unsafe_html("<p>HTML</p>")}></iframe> });
    assert_eq!(attr(&el, "iframe", "srcdoc").as_deref(), Some("<p>HTML</p>"));
}

#[wasm_bindgen_test]
fn sets_branded_iframe_srcdoc() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <iframe srcdoc={unsafe_html("<p>HTML</p>")}></iframe> });
    assert_eq!(attr(&el, "iframe", "srcdoc").as_deref(), Some("<p>HTML</p>"));
}

#[wasm_bindgen_test]
#[should_panic(expected = "Invalid srcDoc prop: expected a value created with unsafe_html()")]
fn rejects_unbranded_iframe_src_doc_values_before_dom_mutation() {
    Node::element("iframe", None, vec![("srcDoc", str("<img src=invalid onerror=alert(1)>"))], vec![], vec![]);
}

#[wasm_bindgen_test]
#[should_panic(expected = "Invalid srcDoc prop: expected a value created with unsafe_html()")]
fn rejects_json_shaped_iframe_src_doc_values_before_dom_mutation() {
    Node::from_json(&serde_json::json!({ "$rmx": true, "type": "iframe", "props": { "srcDoc": { "value": "<p>HTML</p>" } } }));
}

#[wasm_bindgen_test]
#[should_panic(expected = "Invalid srcdoc prop: expected a value created with unsafe_html()")]
fn rejects_unbranded_iframe_srcdoc_values_before_dom_mutation() {
    Node::element("iframe", None, vec![("srcdoc", str("<img src=invalid onerror=alert(1)>"))], vec![], vec![]);
}

#[wasm_bindgen_test]
#[should_panic(expected = "Invalid srcdoc prop: expected a value created with unsafe_html()")]
fn rejects_json_shaped_iframe_srcdoc_values_before_dom_mutation() {
    Node::from_json(&serde_json::json!({ "$rmx": true, "type": "iframe", "props": { "srcdoc": { "value": "<p>HTML</p>" } } }));
}

#[wasm_bindgen_test]
#[should_panic(expected = "Invalid outerHTML prop: outerHTML is not supported on host elements")]
fn rejects_outer_html_before_replacing_a_host_node() {
    Node::element("div", None, vec![("outerHTML", str("<p>replacement</p>"))], vec![], vec![]);
}

#[wasm_bindgen_test]
fn ignores_children_when_inner_html_is_set() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div innerHTML={unsafe_html("<span>From innerHTML</span>")}><p>"Ignored child"</p></div> });
    assert_eq!(el.inner_html(), "<div><span>From innerHTML</span></div>");
}

#[wasm_bindgen_test]
fn updates_inner_html_on_re_render() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div innerHTML={unsafe_html("<span>First</span>")}></div> });
    assert_eq!(el.inner_html(), "<div><span>First</span></div>");
    let div = find(&el, "div");
    root.render(rsx! { <div innerHTML={unsafe_html("<span>Second</span>")}></div> });
    assert_eq!(el.inner_html(), "<div><span>Second</span></div>");
    assert!(find(&el, "div").is_same_node(Some(&div)));
}

#[wasm_bindgen_test]
fn clears_inner_html_when_removed() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div innerHTML={unsafe_html("<span>Hello</span>")}></div> });
    assert_eq!(el.inner_html(), "<div><span>Hello</span></div>");
    root.render(rsx! { <div></div> });
    assert_eq!(el.inner_html(), "<div></div>");
}

#[wasm_bindgen_test]
fn switches_from_inner_html_to_children() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div innerHTML={unsafe_html("<span>From innerHTML</span>")}></div> });
    assert_eq!(el.inner_html(), "<div><span>From innerHTML</span></div>");
    root.render(rsx! { <div><p>"From children"</p></div> });
    assert_eq!(el.inner_html(), "<div><p>From children</p></div>");
}

#[wasm_bindgen_test]
fn switches_from_children_to_inner_html() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div><p>"From children"</p></div> });
    assert_eq!(el.inner_html(), "<div><p>From children</p></div>");
    root.render(rsx! { <div innerHTML={unsafe_html("<span>From innerHTML</span>")}></div> });
    assert_eq!(el.inner_html(), "<div><span>From innerHTML</span></div>");
}

#[wasm_bindgen_test]
fn switches_from_text_children_to_inner_html_without_errors() {
    let el = container();
    let root = create_root(&el);
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let s = seen.clone();
    root.on_error(move |e| s.borrow_mut().push(e.message.clone()));
    root.render(rsx! { <div>"From text child"</div> });
    assert_eq!(el.inner_html(), "<div>From text child</div>");
    root.render(rsx! { <div innerHTML={unsafe_html("<span>From innerHTML</span>")}></div> });
    assert_eq!(el.inner_html(), "<div><span>From innerHTML</span></div>");
    assert!(seen.borrow().is_empty());
}

// css mixin

#[wasm_bindgen_test]
fn adds_class_based_styles() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={css("color: rgb(255, 0, 0)")}>"Hello"</div> });
    let div = find(&el, "div");
    assert!(class_name(&div).contains("rmxc-"));
    document().body().unwrap().append_child(&el).unwrap();
    assert_eq!(color(&div), "rgb(255, 0, 0)");
    el.remove();
}

#[wasm_bindgen_test]
fn composes_with_class_without_overriding_it() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={css("color: rgb(255, 0, 0)")} class="custom-class">"Hello"</div> });
    let class = class_name(&find(&el, "div"));
    assert!(class.split(' ').any(|c| c == "custom-class"), "{class}");
    assert!(class.contains("rmxc-"), "{class}");
}

#[wasm_bindgen_test]
fn keeps_css_mixin_class_with_another_class() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={css("color: rgb(0, 255, 0)")} class="another-class">"Hello"</div> });
    assert!(class_name(&find(&el, "div")).contains("rmxc-"));
}

#[wasm_bindgen_test]
fn class_updates_independently_of_css_mixin_output() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div mix={css("color: rgb(255, 0, 0)")} class="first">"Hello"</div> });
    let div = find(&el, "div");
    let class = class_name(&div);
    assert!(class.split(' ').any(|c| c == "first"));
    let generated = class.split(' ').find(|c| c.starts_with("rmxc-")).unwrap().to_string();
    root.render(rsx! { <div mix={css("color: rgb(255, 0, 0)")} class="second">"Hello"</div> });
    let class = class_name(&div);
    assert!(class.split(' ').any(|c| c == "second"), "{class}");
    assert!(!class.split(' ').any(|c| c == "first"), "{class}");
    assert!(class.split(' ').any(|c| c == generated), "{class}");
}

#[wasm_bindgen_test]
fn removes_nested_selector_rules_when_they_go_away() {
    let el = container();
    document().body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    root.render(rsx! {
        <div mix={css("& span { color: rgb(0, 0, 255) } & span.special { color: rgb(255, 0, 0) }")}>
            <span class="special">"Test"</span>
        </div>
    });
    let child = find(&el, "span");
    assert_eq!(color(&child), "rgb(255, 0, 0)");
    root.render(rsx! {
        <div mix={css("& span { color: rgb(0, 0, 255) }")}>
            <span class="special">"Test"</span>
        </div>
    });
    assert_eq!(color(&child), "rgb(0, 0, 255)");
    el.remove();
}
