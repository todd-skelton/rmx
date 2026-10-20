//! Ports of Remix's `diff-dom.test.tsx` cases that do not need a client entry or frame
//! owner (basic diffing, comments, keys and live browser state). The HMR cases do not
//! apply: rmx has no HMR. Run with `pwsh ./test-browser.ps1 -Test browser_diff_dom`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::cell::Cell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn container(html: &str) -> web_sys::HtmlElement {
    let el: web_sys::HtmlElement = document().create_element("div").unwrap().unchecked_into();
    el.set_inner_html(html);
    el
}

/// A container attached to the body, removed when dropped.
struct Attached(web_sys::HtmlElement);

impl Attached {
    fn new(html: &str) -> Attached {
        let el = container(html);
        document().body().unwrap().append_child(&el).unwrap();
        Attached(el)
    }
}

impl Drop for Attached {
    fn drop(&mut self) {
        self.0.remove();
    }
}

fn diff(el: &web_sys::HtmlElement, next: &str) {
    rmx::test::diff_html(el, next);
}

fn q(el: &web_sys::Element, selector: &str) -> web_sys::Element {
    el.query_selector(selector).unwrap().unwrap_or_else(|| panic!("no {selector}"))
}

fn same(a: Option<web_sys::Node>, b: &web_sys::Node) -> bool {
    a.is_some_and(|a| a.is_same_node(Some(b)))
}

fn prop(el: &JsValue, name: &str) -> JsValue {
    js_sys::Reflect::get(el, &name.into()).unwrap()
}

// basic diffing

#[wasm_bindgen_test]
fn diffs_text_nodes() {
    let el = container("Hello, world!");
    let text = el.first_child().unwrap();
    diff(&el, "Goodbye, world!");
    assert_eq!(el.inner_html(), "Goodbye, world!");
    assert!(same(el.first_child(), &text));
}

#[wasm_bindgen_test]
fn diffs_element_nodes() {
    let el = container("<div>Hello, world!</div>");
    let div = el.first_child().unwrap();
    diff(&el, "<div>Goodbye, world!</div>");
    assert_eq!(el.inner_html(), "<div>Goodbye, world!</div>");
    assert!(same(el.first_child(), &div));
}

#[wasm_bindgen_test]
fn diffs_element_nodes_with_attributes() {
    let el = container(r#"<div id="hello">Hello, world!</div>"#);
    let div = el.first_child().unwrap();
    diff(&el, r#"<div id="goodbye">Goodbye, world!</div>"#);
    assert_eq!(el.inner_html(), r#"<div id="goodbye">Goodbye, world!</div>"#);
    assert!(same(el.first_child(), &div));
}

#[wasm_bindgen_test]
fn diffs_children() {
    let el = container("<div><span>Hello, world!</span></div>");
    let div = el.first_child().unwrap();
    let span = q(&el, "span");
    diff(&el, "<div><span>Goodbye, world!</span></div>");
    assert_eq!(el.inner_html(), "<div><span>Goodbye, world!</span></div>");
    assert!(same(el.first_child(), &div));
    assert!(q(&el, "span").is_same_node(Some(&span)));
}

#[wasm_bindgen_test]
fn replaces_children_elements() {
    let el = container("<div><span>Hello, world!</span></div>");
    let div = el.first_child().unwrap();
    diff(&el, "<div><p>Goodbye, world!</p></div>");
    assert_eq!(el.inner_html(), "<div><p>Goodbye, world!</p></div>");
    assert!(same(el.first_child(), &div));
}

#[wasm_bindgen_test]
fn retains_focus_when_a_following_sibling_changes_element_type() {
    let c = Attached::new("<form><input></form><ul><li>Result</li></ul>");
    let input: web_sys::HtmlElement = q(&c.0, "input").unchecked_into();
    input.focus().unwrap();
    diff(&c.0, "<form><input></form><p>No results</p>");
    assert_eq!(c.0.inner_html(), "<form><input></form><p>No results</p>");
    assert!(q(&c.0, "input").is_same_node(Some(&input)));
    assert!(document().active_element().is_some_and(|a| a.is_same_node(Some(&input))));
}

// comments

#[wasm_bindgen_test]
fn retains_focus_in_a_hydration_range_when_a_following_sibling_is_removed() {
    let c = Attached::new("<!-- rmx:h:old --><input><!-- /rmx:h --><button>Filter</button><a>Clear</a>");
    let input: web_sys::HtmlElement = q(&c.0, "input").unchecked_into();
    input.focus().unwrap();
    diff(&c.0, "<!-- rmx:h:old --><input><!-- /rmx:h --><button>Filter</button>");
    assert!(q(&c.0, "input").is_same_node(Some(&input)));
    assert!(document().active_element().is_some_and(|a| a.is_same_node(Some(&input))));
}

#[wasm_bindgen_test]
fn retains_comments() {
    let el = container("<!-- start --><div>hello</div><!-- end -->");
    let comment = el.first_child().unwrap();
    diff(&el, "<!-- start --><div>goodbye</div><!-- end -->");
    assert_eq!(el.inner_html(), "<!-- start --><div>goodbye</div><!-- end -->");
    assert!(same(el.first_child(), &comment));
}

#[wasm_bindgen_test]
fn diffs_comment_data() {
    let el = container("<!-- a --><div>hello</div><!-- z -->");
    let (first, last) = (el.first_child().unwrap(), el.last_child().unwrap());
    diff(&el, "<!-- b --><div>hello</div><!-- y -->");
    assert_eq!(el.inner_html(), "<!-- b --><div>hello</div><!-- y -->");
    assert!(same(el.first_child(), &first));
    assert!(same(el.last_child(), &last));
}

// keyed diffs

#[wasm_bindgen_test]
fn retains_keyed_elements_via_data_rmx_key() {
    let el = container(r#"<ul><li data-rmx-key="a">A</li><li data-rmx-key="b">B</li><li data-rmx-key="c">C</li></ul>"#);
    let (a, b, c) = (q(&el, "[data-rmx-key=a]"), q(&el, "[data-rmx-key=b]"), q(&el, "[data-rmx-key=c]"));
    diff(&el, r#"<ul><li data-rmx-key="b">B</li><li data-rmx-key="a">A</li><li data-rmx-key="c">C</li></ul>"#);
    let item = |n: u32| q(&el, &format!("li:nth-child({n})"));
    assert!(item(1).is_same_node(Some(&b)));
    assert!(item(2).is_same_node(Some(&a)));
    assert!(item(3).is_same_node(Some(&c)));
    assert_eq!(q(&el, "ul").inner_html(), r#"<li data-rmx-key="b">B</li><li data-rmx-key="a">A</li><li data-rmx-key="c">C</li>"#);
}

// live browser state

#[wasm_bindgen_test]
fn preserves_current_details_open_state_when_incoming_html_removes_open() {
    let el = container("<details open><summary>Toggle</summary><p>Body</p></details>");
    let details = q(&el, "details");
    diff(&el, "<details><summary>Toggle</summary><p>Body</p></details>");
    assert_eq!(prop(&details, "open"), JsValue::TRUE);
    assert!(details.has_attribute("open"));
}

#[wasm_bindgen_test]
fn preserves_current_dialog_open_state_when_incoming_html_removes_open() {
    let el = container("<dialog open>Hello</dialog>");
    let dialog = q(&el, "dialog");
    diff(&el, "<dialog>Hello</dialog>");
    assert_eq!(prop(&dialog, "open"), JsValue::TRUE);
    assert!(dialog.has_attribute("open"));
}

#[wasm_bindgen_test]
fn preserves_current_input_checked_state_when_incoming_html_removes_checked() {
    let el = container(r#"<input type="checkbox" checked>"#);
    let input = q(&el, "input");
    diff(&el, r#"<input type="checkbox">"#);
    assert_eq!(prop(&input, "checked"), JsValue::TRUE);
    assert!(input.has_attribute("checked"));
}

#[wasm_bindgen_test]
fn preserves_current_input_value_when_incoming_html_changes_value() {
    let el = container(r#"<input value="server">"#);
    let input: web_sys::HtmlInputElement = q(&el, "input").unchecked_into();
    input.set_value("user");
    diff(&el, r#"<input value="server-next">"#);
    assert_eq!(input.value(), "user");
    assert_eq!(input.get_attribute("value").as_deref(), Some("server"));
}

#[wasm_bindgen_test]
fn preserves_current_textarea_value_when_incoming_html_changes_its_text() {
    let el = container("<textarea>server</textarea>");
    let textarea = q(&el, "textarea");
    js_sys::Reflect::set(&textarea, &"value".into(), &"user".into()).unwrap();
    diff(&el, "<textarea>server-next</textarea>");
    assert_eq!(prop(&textarea, "value").as_string().as_deref(), Some("user"));
    assert_eq!(textarea.text_content().as_deref(), Some("server"));
}

#[wasm_bindgen_test]
fn removes_client_added_element_children_when_incoming_html_has_none() {
    let el = container("<div></div>");
    let div = q(&el, "div");
    let button = document().create_element("button").unwrap();
    button.set_text_content(Some("Search"));
    div.append_child(&button).unwrap();
    diff(&el, "<div></div>");
    assert!(el.first_element_child().is_some_and(|f| f.is_same_node(Some(&div))));
    assert!(div.first_element_child().is_none());
    assert!(!button.is_connected() && button.parent_node().is_none());
}

#[wasm_bindgen_test]
fn preserves_data_rmx_preserve_dom_element_attributes_and_children() {
    let el = container(r#"<div data-rmx-preserve-dom data-state="client"><button>Client</button></div>"#);
    let div = q(&el, "div");
    let button = q(&div, "button");
    diff(&el, r#"<div data-rmx-preserve-dom data-state="server"><span>Server</span></div>"#);
    assert!(el.first_element_child().is_some_and(|f| f.is_same_node(Some(&div))));
    assert_eq!(div.get_attribute("data-state").as_deref(), Some("client"));
    assert!(div.first_element_child().is_some_and(|f| f.is_same_node(Some(&button))));
    assert_eq!(div.inner_html(), "<button>Client</button>");
}

#[wasm_bindgen_test]
fn preserves_named_attributes_while_reconciling_other_attributes_and_children() {
    let el = container(r#"<div class="layout" data-theme="light" title="Old" data-page="old"><p>Old</p></div>"#);
    let div = q(&el, "div");
    div.set_class_name("layout dark");
    div.set_attribute("data-theme", "dark").unwrap();
    diff(&el, "<div data-rmx-preserve-attrs=\" class\tdata-theme\nclass \" class=\"new-layout\" title=\"New\" lang=\"en\"><p>New</p></div>");
    assert!(el.first_element_child().is_some_and(|f| f.is_same_node(Some(&div))));
    assert_eq!(div.class_name(), "layout dark");
    assert_eq!(div.get_attribute("data-theme").as_deref(), Some("dark"));
    assert_eq!(div.get_attribute("title").as_deref(), Some("New"));
    assert_eq!(div.get_attribute("lang").as_deref(), Some("en"));
    assert!(!div.has_attribute("data-page"));
    assert_eq!(div.inner_html(), "<p>New</p>");
}

#[wasm_bindgen_test]
fn preserves_the_absence_of_attributes_removed_by_client_code() {
    let el = container(r#"<div class="dark" data-theme="dark"></div>"#);
    let div = q(&el, "div");
    div.remove_attribute("class").unwrap();
    div.remove_attribute("data-theme").unwrap();
    diff(&el, r#"<div data-rmx-preserve-attrs="class data-theme" class="dark" data-theme="dark"></div>"#);
    assert!(!div.has_attribute("class"));
    assert!(!div.has_attribute("data-theme"));
}

#[wasm_bindgen_test]
fn uses_the_incoming_preservation_list_when_ownership_changes() {
    let el = container(r#"<div data-rmx-preserve-attrs="class data-theme" class="dark" data-theme="dark"></div>"#);
    let div = q(&el, "div");
    diff(&el, r#"<div data-rmx-preserve-attrs="data-theme" class="light" data-theme="light"></div>"#);
    assert_eq!(div.class_name(), "light");
    assert_eq!(div.get_attribute("data-theme").as_deref(), Some("dark"));
    assert_eq!(div.get_attribute("data-rmx-preserve-attrs").as_deref(), Some("data-theme"));
}

#[wasm_bindgen_test]
fn returns_attributes_to_normal_reconciliation_when_the_preservation_list_is_empty() {
    let el = container(r#"<div data-rmx-preserve-attrs="class data-theme" class="dark" data-theme="dark"></div>"#);
    let div = q(&el, "div");
    diff(&el, r#"<div data-rmx-preserve-attrs="" class="light"></div>"#);
    assert_eq!(div.class_name(), "light");
    assert!(!div.has_attribute("data-theme"));
    assert_eq!(div.get_attribute("data-rmx-preserve-attrs").as_deref(), Some(""));
}

#[wasm_bindgen_test]
fn returns_attributes_to_normal_reconciliation_when_the_preservation_list_is_omitted() {
    let el = container(r#"<div data-rmx-preserve-attrs="class data-theme" class="dark" data-theme="dark"></div>"#);
    let div = q(&el, "div");
    diff(&el, r#"<div data-theme="light"></div>"#);
    assert!(!div.has_attribute("class"));
    assert_eq!(div.get_attribute("data-theme").as_deref(), Some("light"));
    assert!(!div.has_attribute("data-rmx-preserve-attrs"));
}

#[wasm_bindgen_test]
fn preserves_named_svg_attributes_without_changing_their_case() {
    let el = container(r#"<svg viewBox="0 0 10 10"><circle r="2"></circle></svg>"#);
    let svg = q(&el, "svg");
    svg.set_attribute("viewBox", "0 0 20 20").unwrap();
    diff(&el, r#"<svg data-rmx-preserve-attrs="viewBox" viewBox="0 0 10 10"><circle r="4"></circle></svg>"#);
    assert_eq!(svg.get_attribute("viewBox").as_deref(), Some("0 0 20 20"));
    assert_eq!(q(&svg, "circle").get_attribute("r").as_deref(), Some("4"));
}

#[wasm_bindgen_test]
fn does_not_preserve_descendants_with_an_attribute_preservation_list() {
    let el = container(r#"<div class="dark"><span class="old">Old</span></div>"#);
    let div = q(&el, "div");
    diff(&el, r#"<div data-rmx-preserve-attrs="class" class="light"><span class="new">New</span></div>"#);
    assert_eq!(div.class_name(), "dark");
    assert_eq!(div.inner_html(), r#"<span class="new">New</span>"#);
}

#[wasm_bindgen_test]
fn can_replace_and_remove_elements_with_preserved_attributes() {
    let el = container(r#"<div data-rmx-preserve-attrs="class" class="dark">Old</div>"#);
    let div = q(&el, "div");
    diff(&el, r#"<section data-rmx-preserve-attrs="class" class="light">New</section>"#);
    assert!(!el.contains(Some(&div)));
    assert_eq!(el.first_element_child().unwrap().get_attribute("class").as_deref(), Some("light"));
    assert_eq!(el.text_content().as_deref(), Some("New"));
    diff(&el, "");
    assert_eq!(el.child_nodes().length(), 0);
}

#[wasm_bindgen_test]
fn preserves_the_whole_dom_when_both_preservation_attributes_are_present() {
    let el = container(r#"<div class="dark" data-theme="dark"><p>Client</p></div>"#);
    let div = q(&el, "div");
    diff(&el, r#"<div data-rmx-preserve-dom data-rmx-preserve-attrs="class" class="light" data-theme="light"><p>Server</p></div>"#);
    assert_eq!(div.class_name(), "dark");
    assert_eq!(div.get_attribute("data-theme").as_deref(), Some("dark"));
    assert_eq!(div.inner_html(), "<p>Client</p>");
}

/// Define a custom element whose connected/disconnected callbacks bump the counters, and
/// which (when `fill`) replaces its children with a button the first time it connects.
fn define_element(tag: &str, fill: bool, connects: Rc<Cell<u32>>, disconnects: Rc<Cell<u32>>) {
    let on_connect = Closure::<dyn Fn()>::new(move || connects.set(connects.get() + 1)).into_js_value();
    let on_disconnect = Closure::<dyn Fn()>::new(move || disconnects.set(disconnects.get() + 1)).into_js_value();
    let define = js_sys::Function::new_with_args(
        "tag,fill,onc,ond",
        "if (customElements.get(tag)) return; customElements.define(tag, class extends HTMLElement { \
           connectedCallback() { onc(); if (!fill || this._init) return; this._init = true; this.innerHTML = ''; \
             const b = document.createElement('button'); b.type = 'button'; b.className = 'pf-trigger-btn'; b.textContent = 'Search'; this.appendChild(b) } \
           disconnectedCallback() { ond(); this._init = false } })",
    );
    define
        .apply(&JsValue::NULL, &js_sys::Array::of4(&tag.into(), &JsValue::from_bool(fill), &on_connect, &on_disconnect))
        .unwrap();
}

#[wasm_bindgen_test]
fn preserves_data_rmx_preserve_dom_custom_element_children_added_during_initialization() {
    let tag = "mock-pagefind-modal-trigger-lifecycle";
    define_element(tag, true, Rc::default(), Rc::default());
    let c = Attached::new("");
    let trigger = document().create_element(tag).unwrap();
    c.0.append_child(&trigger).unwrap();
    let button = q(&trigger, "button");
    diff(&c.0, &format!("<{tag} data-rmx-preserve-dom></{tag}>"));
    assert!(c.0.first_element_child().is_some_and(|f| f.is_same_node(Some(&trigger))));
    assert!(trigger.has_attribute("data-rmx-preserve-dom"));
    assert!(q(&trigger, "button").is_same_node(Some(&button)));
    assert!(button.is_connected());
}

#[wasm_bindgen_test]
fn can_pair_preserve_dom_elements_with_data_rmx_key_before_index_fallback_moves_them() {
    let tag = "mock-pagefind-modal-lifecycle";
    let (connects, disconnects) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    define_element(tag, false, connects.clone(), disconnects.clone());
    let c = Attached::new(&format!(
        r#"<section><span>Old</span><{tag} data-rmx-key="modal" data-rmx-preserve-dom><dialog>Client</dialog></{tag}></section>"#
    ));
    let modal = q(&c.0, tag);
    let dialog = q(&modal, "dialog");
    connects.set(0);
    disconnects.set(0);
    diff(&c.0, &format!(r#"<section><span>New</span><p>Inserted</p><{tag} data-rmx-key="modal" data-rmx-preserve-dom></{tag}></section>"#));
    assert!(q(&c.0, tag).is_same_node(Some(&modal)));
    assert!(q(&modal, "dialog").is_same_node(Some(&dialog)));
    assert_eq!(modal.inner_html(), "<dialog>Client</dialog>");
    assert_eq!((connects.get(), disconnects.get()), (0, 0));
}

#[wasm_bindgen_test]
fn does_not_reconnect_keyed_preserve_dom_elements_during_reordering() {
    let tag = "mock-pagefind-modal-stationary";
    let (connects, disconnects) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    define_element(tag, false, connects.clone(), disconnects.clone());
    let c = Attached::new(&format!(
        r#"<section><{tag} data-rmx-key="modal" data-rmx-preserve-dom><dialog>Client</dialog></{tag}><p>Old</p></section>"#
    ));
    let modal = q(&c.0, tag);
    let dialog = q(&modal, "dialog");
    connects.set(0);
    disconnects.set(0);
    diff(&c.0, &format!(r#"<section><p>New</p><{tag} data-rmx-key="modal" data-rmx-preserve-dom></{tag}></section>"#));
    assert!(q(&c.0, tag).is_same_node(Some(&modal)));
    assert!(q(&modal, "dialog").is_same_node(Some(&dialog)));
    assert_eq!((connects.get(), disconnects.get()), (0, 0));
}

#[wasm_bindgen_test]
fn preserves_current_select_value_when_incoming_html_changes_selected_options() {
    let el = container(r#"<select><option value="a">A</option><option value="b">B</option></select>"#);
    let select: web_sys::HtmlSelectElement = q(&el, "select").unchecked_into();
    let (first, second) = (q(&el, "option[value=a]"), q(&el, "option[value=b]"));
    select.set_value("b");
    diff(&el, r#"<select><option value="a" selected>A</option><option value="b">B</option></select>"#);
    assert_eq!(select.value(), "b");
    assert_eq!(prop(&first, "selected"), JsValue::FALSE);
    assert_eq!(prop(&second, "selected"), JsValue::TRUE);
    assert!(!first.has_attribute("selected"));
}

#[wasm_bindgen_test]
fn preserves_current_popover_visibility_when_incoming_html_removes_popover() {
    let c = Attached::new(r#"<div popover="auto">Hello</div>"#);
    let popover = q(&c.0, "div");
    js_sys::Reflect::get(&popover, &"showPopover".into())
        .unwrap()
        .unchecked_into::<js_sys::Function>()
        .call0(&popover)
        .unwrap();
    diff(&c.0, "<div>Hello</div>");
    assert!(popover.matches(":popover-open").unwrap());
    assert_eq!(popover.get_attribute("popover").as_deref(), Some("auto"));
}
