//! Ports of Remix's `vdom.controlled-props.test.tsx`. Run with
//! `pwsh ./scripts/test-browser.ps1 -Test browser_controlled`.
#![cfg(target_arch = "wasm32")]

extern crate rsmix_component as rsmix;

use rsmix::{create_root, on, rsx, Component, Handle, Node};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn container() -> web_sys::HtmlElement {
    document().create_element("div").unwrap().unchecked_into()
}

fn input(el: &web_sys::HtmlElement, selector: &str) -> web_sys::HtmlInputElement {
    el.query_selector(selector).unwrap().unwrap().unchecked_into()
}

fn fire(target: &web_sys::EventTarget, kind: &str) {
    let init = web_sys::EventInit::new();
    init.set_bubbles(true);
    target.dispatch_event(&web_sys::Event::new_with_event_init_dict(kind, &init).unwrap()).unwrap();
}

async fn microtasks() {
    for _ in 0..2 {
        wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&wasm_bindgen::JsValue::UNDEFINED)).await.unwrap();
    }
}

#[wasm_bindgen_test]
async fn restores_controlled_value_on_native_input_when_no_update_happens() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <input value="hello"/> });
    root.flush();
    let field = input(&el, "input");
    field.set_value("hello123");
    fire(&field, "input");
    microtasks().await;
    assert_eq!(field.value(), "hello");
}

#[wasm_bindgen_test]
async fn restores_controlled_checked_on_native_change_when_no_update_happens() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <input type="checkbox" checked={true}/> });
    root.flush();
    let field = input(&el, "input");
    field.set_checked(false);
    fire(&field, "change");
    microtasks().await;
    assert!(field.checked());
}

/// Remix's App that rejects input containing digits.
struct Validated {
    value: String,
    renders: u32,
}

impl Component for Validated {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Validated { value: "hello".into(), renders: 0 }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let handler = h.on_event(|this, h, event| {
            let next = event.target_value();
            if next.chars().any(|c| c.is_ascii_digit()) {
                return;
            }
            this.value = next;
            this.renders += 1;
            h.update();
        });
        rsx! {
            <>
                <label for="tracked">"Tracked"</label>
                <input id="tracked" value={self.value.clone()} mix={on("input", handler)}/>
                <output>{format!("{}:{}", self.renders, self.value)}</output>
            </>
        }
    }
}

#[wasm_bindgen_test]
fn allows_controlled_value_changes_when_an_input_event_calls_update() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <Validated/> });
    root.flush();
    let field = input(&el, "input");
    field.set_value("helloa");
    fire(&field, "input");
    root.flush();
    assert_eq!(field.value(), "helloa");
    assert_eq!(el.query_selector("output").unwrap().unwrap().text_content().unwrap(), "1:helloa");
}

#[wasm_bindgen_test]
async fn preserves_controlled_updates_from_typing_while_rejecting_invalid_input() {
    let el = container();
    document().body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    root.render(rsx! { <Validated/> });
    root.flush();
    let field = input(&el, "input");
    let output = || el.query_selector("output").unwrap().unwrap().text_content().unwrap();
    let typed = |c: char| {
        field.focus().unwrap();
        field.set_value(&format!("{}{c}", field.value()));
        fire(&field, "input");
    };
    typed('a');
    root.flush();
    assert_eq!((field.value(), output()), ("helloa".to_string(), "1:helloa".to_string()));
    typed('1');
    microtasks().await;
    assert_eq!((field.value(), output()), ("helloa".to_string(), "1:helloa".to_string()));
    typed('b');
    root.flush();
    assert_eq!((field.value(), output()), ("helloab".to_string(), "2:helloab".to_string()));
    root.dispose();
    el.remove();
}

struct Committing {
    value: String,
}

impl Component for Committing {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Committing { value: "hello".into() }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let handler = h.on_event(|this, h, event| {
            this.value = event.target_value();
            h.update();
        });
        rsx! { <input value={self.value.clone()} mix={on("input", handler)}/> }
    }
}

#[wasm_bindgen_test]
async fn does_not_clobber_controlled_value_when_input_event_commits_a_new_value() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <Committing/> });
    root.flush();
    let field = input(&el, "input");
    field.set_value("helloa");
    fire(&field, "input");
    microtasks().await;
    assert_eq!(field.value(), "helloa");
}

#[wasm_bindgen_test]
async fn does_not_control_value_or_checked_when_the_prop_is_absent() {
    let el = container();
    let root = create_root(&el);
    let (value, checked): (Option<String>, Option<bool>) = (None, None);
    root.render(rsx! { <><input id="text" value={value}/><input id="check" type="checkbox" checked={checked}/></> });
    root.flush();
    let text = input(&el, "#text");
    text.set_value("user typed");
    fire(&text, "input");
    let check = input(&el, "#check");
    check.set_checked(true);
    fire(&check, "change");
    microtasks().await;
    assert_eq!(text.value(), "user typed");
    assert!(check.checked());
}

#[wasm_bindgen_test]
async fn restores_controlled_value_on_native_change_for_select_when_no_update_happens() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <select value="b"><option value="a">"A"</option><option value="b">"B"</option></select> });
    root.flush();
    let select: web_sys::HtmlSelectElement = el.query_selector("select").unwrap().unwrap().unchecked_into();
    assert_eq!(select.value(), "b");
    select.set_value("a");
    fire(&select, "change");
    microtasks().await;
    assert_eq!(select.value(), "b");
}

struct Picker {
    value: String,
}

impl Component for Picker {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Picker { value: "alpha".into() }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let handler = h.on_event(|this, h, event| {
            this.value = event.target_value();
            h.update();
        });
        rsx! {
            <>
                <select value={self.value.clone()} mix={on("change", handler)}>
                    <option value="alpha">"Alpha"</option>
                    <option value="beta">"Beta"</option>
                    <option value="gamma">"Gamma"</option>
                </select>
                <output>{self.value.clone()}</output>
            </>
        }
    }
}

#[wasm_bindgen_test]
async fn applies_next_select_value_from_change_handlers_after_prior_input_event() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <Picker/> });
    root.flush();
    let select: web_sys::HtmlSelectElement = el.query_selector("select").unwrap().unwrap().unchecked_into();
    select.set_value("beta");
    fire(&select, "input");
    microtasks().await;
    fire(&select, "change");
    root.flush();
    assert_eq!(select.value(), "beta");
    assert_eq!(el.query_selector("output").unwrap().unwrap().text_content().unwrap(), "beta");
}

#[wasm_bindgen_test]
async fn detaches_controlled_listeners_on_dispose() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <input value="hello"/> });
    root.flush();
    let field = input(&el, "input");
    root.dispose();
    root.flush();
    field.set_value("post-dispose");
    fire(&field, "input");
    fire(&field, "change");
    microtasks().await;
    assert_eq!(field.value(), "post-dispose");
}
