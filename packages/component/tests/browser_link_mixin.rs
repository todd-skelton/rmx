//! Ports of Remix's `link-mixin.test.tsx`. `Reset` options are written as
//! `after-transition` / `manual` (Remix writes `String(true)` / `String(false)` for the
//! boolean aliases; the runtime reads both the same way). Run with
//! `pwsh ./scripts/test-browser.ps1 -Test browser_link_mixin`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use rmx::{create_root, link, link_with, rsx, History, NavigateOptions, Node, Reset, Root};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

/// A rendered tree in a container attached to the body; removed when dropped.
struct Rendered {
    el: web_sys::HtmlElement,
    root: Root,
}

impl Drop for Rendered {
    fn drop(&mut self) {
        self.root.dispose();
        self.el.remove();
    }
}

fn render(node: Node) -> Rendered {
    let el: web_sys::HtmlElement = document().create_element("div").unwrap().unchecked_into();
    document().body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    root.render(node);
    root.flush();
    Rendered { el, root }
}

impl Rendered {
    fn find(&self, selector: &str) -> web_sys::Element {
        self.el.query_selector(selector).unwrap().unwrap_or_else(|| panic!("no {selector}"))
    }
}

/// Replace `target[method]` with a function that records its arguments and returns
/// `result`. Dropping the stub puts the original back.
struct Stub {
    calls: js_sys::Array,
    restore: js_sys::Function,
}

impl Stub {
    fn new(target: &JsValue, method: &str, result: &JsValue) -> Stub {
        let calls = js_sys::Array::new();
        let make = js_sys::Function::new_with_args(
            "o,m,log,ret",
            "const orig = o[m]; o[m] = function (...a) { log.push(a); return ret }; return () => { o[m] = orig }",
        );
        let restore = make.apply(&JsValue::NULL, &js_sys::Array::of4(target, &method.into(), &calls, result)).unwrap().unchecked_into();
        Stub { calls, restore }
    }

    fn navigation() -> Stub {
        let navigation = js_sys::Reflect::get(&web_sys::window().unwrap(), &"navigation".into()).unwrap();
        let result = js_sys::Object::new();
        js_sys::Reflect::set(&result, &"finished".into(), &js_sys::Promise::resolve(&JsValue::UNDEFINED)).unwrap();
        js_sys::Reflect::set(&result, &"committed".into(), &js_sys::Promise::resolve(&JsValue::UNDEFINED)).unwrap();
        Stub::new(&navigation, "navigate", &result)
    }

    fn open() -> Stub {
        Stub::new(&web_sys::window().unwrap(), "open", &JsValue::NULL)
    }

    fn count(&self) -> u32 {
        self.calls.length()
    }

    fn args(&self, i: u32) -> js_sys::Array {
        self.calls.get(i).unchecked_into()
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.restore.call0(&JsValue::NULL).unwrap();
    }
}

fn get(v: &JsValue, key: &str) -> JsValue {
    js_sys::Reflect::get(v, &key.into()).unwrap()
}

/// `navigation.navigate(href, { state, history })` as Remix's tests expect it.
fn assert_navigated(stub: &Stub, call: u32, href: &str, target: Option<&str>, scroll: &str, focus: &str, history: Option<&str>) {
    let args = stub.args(call);
    assert_eq!(args.get(0).as_string().as_deref(), Some(href));
    let options = args.get(1);
    let state = get(&options, "state");
    assert_eq!(get(&state, "target").as_string().as_deref(), target);
    assert_eq!(get(&state, "src").as_string().as_deref(), Some(href));
    assert_eq!(get(&state, "resetScroll").as_string().as_deref(), Some(scroll));
    assert_eq!(get(&state, "resetFocus").as_string().as_deref(), Some(focus));
    assert_eq!(get(&state, "$rmx"), JsValue::TRUE);
    assert_eq!(get(&options, "history").as_string().as_deref(), history);
}

/// Dispatch `new {ctor}(type, init)` (bubbling and cancelable) at `el`.
fn fire(el: &web_sys::Element, ctor: &str, kind: &str, init: &str) {
    let f = js_sys::Function::new_with_args(
        "el,C,t,init",
        "el.dispatchEvent(new globalThis[C](t, Object.assign({ bubbles: true, cancelable: true }, JSON.parse(init))))",
    );
    f.apply(&JsValue::NULL, &js_sys::Array::of4(el, &ctor.into(), &kind.into(), &init.into())).unwrap();
}

fn click(el: &web_sys::Element) {
    fire(el, "MouseEvent", "click", "{}");
}

fn options() -> NavigateOptions {
    NavigateOptions::default()
}

#[wasm_bindgen_test]
fn adds_href_and_runtime_attributes_to_anchors() {
    let r = render(rsx! {
        <a mix={link_with("/login", NavigateOptions {
            src: Some("/partials/login".into()),
            target: Some("auth".into()),
            history: Some(History::Replace),
            reset_scroll: Some(false.into()),
            reset_focus: Some(false.into()),
        })}>"Login"</a>
    });
    let a = r.find("a");
    assert_eq!(a.get_attribute("href").as_deref(), Some("/login"));
    assert_eq!(a.get_attribute("data-rmx-target").as_deref(), Some("auth"));
    assert_eq!(a.get_attribute("data-rmx-src").as_deref(), Some("/partials/login"));
    assert_eq!(a.get_attribute("data-rmx-history").as_deref(), Some("replace"));
    assert_eq!(a.get_attribute("data-rmx-reset-scroll").as_deref(), Some("manual"));
    assert_eq!(a.get_attribute("data-rmx-reset-focus").as_deref(), Some("manual"));
    assert_eq!(a.get_attribute("role"), None);
}

#[wasm_bindgen_test]
fn adds_manual_scroll_and_focus_attributes_to_anchors() {
    let r = render(rsx! {
        <a mix={link_with("/login", NavigateOptions { reset_scroll: Some(Reset::Manual), reset_focus: Some(Reset::Manual), ..options() })}>"Login"</a>
    });
    let a = r.find("a");
    assert_eq!(a.get_attribute("data-rmx-reset-scroll").as_deref(), Some("manual"));
    assert_eq!(a.get_attribute("data-rmx-reset-focus").as_deref(), Some("manual"));
}

#[wasm_bindgen_test]
fn lets_after_transition_options_override_manual_anchor_attributes() {
    let r = render(rsx! {
        <a data-rmx-reset-scroll="manual" data-rmx-reset-focus="manual"
            mix={link_with("/login", NavigateOptions { reset_scroll: Some(Reset::AfterTransition), reset_focus: Some(Reset::AfterTransition), ..options() })}>
            "Login"
        </a>
    });
    let a = r.find("a");
    assert_eq!(a.get_attribute("data-rmx-reset-scroll").as_deref(), Some("after-transition"));
    assert_eq!(a.get_attribute("data-rmx-reset-focus").as_deref(), Some("after-transition"));
}

#[wasm_bindgen_test]
fn lets_true_options_override_manual_anchor_attributes() {
    let r = render(rsx! {
        <a data-rmx-reset-scroll="manual" data-rmx-reset-focus="manual"
            mix={link_with("/login", NavigateOptions { reset_scroll: Some(true.into()), reset_focus: Some(true.into()), ..options() })}>
            "Login"
        </a>
    });
    let a = r.find("a");
    assert_eq!(a.get_attribute("data-rmx-reset-scroll").as_deref(), Some("after-transition"));
    assert_eq!(a.get_attribute("data-rmx-reset-focus").as_deref(), Some("after-transition"));
}

#[wasm_bindgen_test]
fn passes_manual_scroll_and_focus_options_through_for_non_anchor_navigation() {
    let nav = Stub::navigation();
    let r = render(rsx! {
        <button mix={link_with("/login", NavigateOptions { reset_scroll: Some(Reset::Manual), reset_focus: Some(Reset::Manual), ..options() })}>"Login"</button>
    });
    click(&r.find("button"));
    assert_eq!(nav.count(), 1);
    assert_navigated(&nav, 0, "/login", None, "manual", "manual", None);
}

#[wasm_bindgen_test]
fn adds_push_history_to_anchors() {
    let r = render(rsx! { <a mix={link_with("/login", NavigateOptions { history: Some(History::Push), ..options() })}>"Login"</a> });
    assert_eq!(r.find("a").get_attribute("data-rmx-history").as_deref(), Some("push"));
}

#[wasm_bindgen_test]
fn adds_link_semantics_to_buttons() {
    let r = render(rsx! { <button mix={link("/login")}>"Login"</button> });
    let button = r.find("button");
    assert_eq!(button.get_attribute("role").as_deref(), Some("link"));
    assert_eq!(button.get_attribute("type").as_deref(), Some("button"));
    assert_eq!(button.get_attribute("tabindex"), None);
}

#[wasm_bindgen_test]
fn adds_link_semantics_and_tabindex_to_generic_elements() {
    let r = render(rsx! { <li mix={link("/login")}>"Login"</li> });
    let item = r.find("li");
    assert_eq!(item.get_attribute("role").as_deref(), Some("link"));
    assert_eq!(item.get_attribute("tabindex").as_deref(), Some("0"));
}

#[wasm_bindgen_test]
fn does_not_add_tabindex_to_editable_elements() {
    let r = render(rsx! { <div contenteditable="true" mix={link("/login")}>"Login"</div> });
    assert_eq!(r.find("div").get_attribute("tabindex"), None);
}

#[wasm_bindgen_test]
fn omits_runtime_attributes_on_anchors_when_options_are_not_provided() {
    let r = render(rsx! { <a mix={link("/docs")}>"Docs"</a> });
    let a = r.find("a");
    assert_eq!(a.get_attribute("href").as_deref(), Some("/docs"));
    for name in ["data-rmx-target", "data-rmx-src", "data-rmx-history", "data-rmx-reset-focus"] {
        assert_eq!(a.get_attribute(name), None, "{name}");
    }
}

#[wasm_bindgen_test]
fn navigates_on_plain_click_for_non_anchor_hosts() {
    let nav = Stub::navigation();
    let r = render(rsx! { <button mix={link_with("/login", NavigateOptions { target: Some("auth".into()), ..options() })}>"Login"</button> });
    click(&r.find("button"));
    assert_eq!(nav.count(), 1);
    assert_navigated(&nav, 0, "/login", Some("auth"), "after-transition", "after-transition", None);
}

#[wasm_bindgen_test]
fn passes_history_options_through_for_non_anchor_navigation() {
    let nav = Stub::navigation();
    let r = render(rsx! { <button mix={link_with("/login", NavigateOptions { history: Some(History::Replace), ..options() })}>"Login"</button> });
    click(&r.find("button"));
    assert_navigated(&nav, 0, "/login", None, "after-transition", "after-transition", Some("replace"));
}

#[wasm_bindgen_test]
fn passes_reset_scroll_false_through_for_non_anchor_navigation() {
    let nav = Stub::navigation();
    let r = render(rsx! { <button mix={link_with("/login", NavigateOptions { reset_scroll: Some(false.into()), ..options() })}>"Login"</button> });
    click(&r.find("button"));
    assert_navigated(&nav, 0, "/login", None, "manual", "after-transition", None);
}

#[wasm_bindgen_test]
fn passes_reset_focus_false_through_for_non_anchor_navigation() {
    let nav = Stub::navigation();
    let r = render(rsx! { <button mix={link_with("/login", NavigateOptions { reset_focus: Some(false.into()), ..options() })}>"Login"</button> });
    click(&r.find("button"));
    assert_navigated(&nav, 0, "/login", None, "after-transition", "manual", None);
}

#[wasm_bindgen_test]
fn activates_link_buttons_on_enter_and_suppresses_keyboard_clicks_from_enter_and_space() {
    let nav = Stub::navigation();
    let r = render(rsx! { <button mix={link("/login")}>"Login"</button> });
    let button = r.find("button");

    fire(&button, "KeyboardEvent", "keydown", r#"{"key":"Enter"}"#);
    assert_eq!(nav.count(), 1);

    fire(&button, "MouseEvent", "click", r#"{"detail":0}"#);
    assert_eq!(nav.count(), 1);

    fire(&button, "KeyboardEvent", "keydown", r#"{"key":" "}"#);
    fire(&button, "KeyboardEvent", "keyup", r#"{"key":" "}"#);
    fire(&button, "MouseEvent", "click", r#"{"detail":0}"#);
    assert_eq!(nav.count(), 1);
}

#[wasm_bindgen_test]
fn activates_generic_link_elements_on_enter() {
    let nav = Stub::navigation();
    let r = render(rsx! { <li mix={link("/login")}>"Login"</li> });
    fire(&r.find("li"), "KeyboardEvent", "keydown", r#"{"key":"Enter"}"#);
    assert_eq!(nav.count(), 1);
    assert_navigated(&nav, 0, "/login", None, "after-transition", "after-transition", None);
}

#[wasm_bindgen_test]
fn opens_a_new_tab_for_meta_click_ctrl_click_and_middle_click_on_non_anchor_hosts() {
    let nav = Stub::navigation();
    let open = Stub::open();
    let r = render(rsx! { <button mix={link("/login")}>"Login"</button> });
    let button = r.find("button");
    fire(&button, "MouseEvent", "click", r#"{"metaKey":true}"#);
    fire(&button, "MouseEvent", "click", r#"{"ctrlKey":true}"#);
    fire(&button, "MouseEvent", "auxclick", r#"{"button":1}"#);

    assert_eq!(open.count(), 3);
    for i in 0..3 {
        let args = open.args(i);
        assert_eq!(args.get(0).as_string().as_deref(), Some("/login"));
        assert_eq!(args.get(1).as_string().as_deref(), Some("_blank"));
    }
    assert_eq!(nav.count(), 0);
}

#[wasm_bindgen_test]
fn does_not_navigate_disabled_or_aria_disabled_link_hosts() {
    let nav = Stub::navigation();
    let r = render(rsx! {
        <div>
            <button disabled={true} mix={link("/login")}>"Login"</button>
            <li aria-disabled="true" mix={link("/help")}>"Help"</li>
        </div>
    });
    let button = r.find("button");
    let item = r.find("li");
    click(&button);
    click(&item);
    fire(&item, "KeyboardEvent", "keydown", r#"{"key":"Enter"}"#);

    assert_eq!(button.get_attribute("aria-disabled").as_deref(), Some("true"));
    assert_eq!(item.get_attribute("aria-disabled").as_deref(), Some("true"));
    assert_eq!(nav.count(), 0);
}
