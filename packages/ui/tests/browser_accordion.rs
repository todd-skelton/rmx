//! Ports of Remix's `ui/src/accordion.test.tsx`. Run with
//! `pwsh ./scripts/test-browser.ps1 -Crate rsmix-ui -Test browser_accordion`.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use rsmix::test::{render, RenderResult};
use rsmix::{rsx, Component, EventHandler, Handle, Node};
use rsmix_ui::accordion::{self, accordion_change, on_accordion_change, AccordionChange, ContextProps, OnValueChange, Value};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

type Log = Rc<RefCell<Vec<Vec<String>>>>;

fn recorder(log: &Log) -> OnValueChange {
    let log = log.clone();
    Rc::new(move |value| log.borrow_mut().push(value.to_vec()))
}

/// Remix's `TestAccordionItem` with its trigger (in an `<h2>`) and content.
fn item(value: &str, disabled: bool, label: &str, content: &str) -> Node {
    let (value, label, content) = (value.to_string(), label.to_string(), content.to_string());
    rsx! {
        <accordion::ItemContext value={value} disabled={disabled}>
            <div mix={accordion::item()}>
                <h2><button mix={accordion::trigger()} type="button">{label}</button></h2>
                <div mix={accordion::content()}>{content}</div>
            </div>
        </accordion::ItemContext>
    }
}

fn account() -> Node {
    item("account", false, "Account", "Manage your account preferences.")
}

fn billing() -> Node {
    item("billing", false, "Billing", "Review billing details.")
}

/// Remix's `TestAccordion`: the provider and its root element around `items`.
fn accordion_with(props: ContextProps, items: Vec<Node>) -> Node {
    let ContextProps { multiple, value, default_value, collapsible, disabled, on_value_change, .. } = props;
    rsx! {
        <accordion::Context multiple={multiple} value={value} default_value={default_value} collapsible={collapsible} disabled={disabled} on_value_change={on_value_change}>
            <div mix={accordion::root()}>{items}</div>
        </accordion::Context>
    }
}

/// Remix's `renderExampleAccordion`: account, a disabled security item, and billing.
fn example(props: ContextProps) -> Node {
    accordion_with(props, vec![account(), item("security", true, "Security", "Configure security defaults."), billing()])
}

fn buttons(t: &RenderResult) -> Vec<web_sys::HtmlButtonElement> {
    t.query_all("button").into_iter().map(|e| e.unchecked_into()).collect()
}

fn expanded(t: &RenderResult) -> Vec<String> {
    buttons(t).iter().map(|b| b.get_attribute("aria-expanded").unwrap_or_default()).collect()
}

fn activate(t: &RenderResult, index: usize) {
    let button = buttons(t)[index].clone();
    t.act(|| button.click());
}

fn key(button: &web_sys::HtmlButtonElement, key: &str) {
    let init = web_sys::KeyboardEventInit::new();
    init.set_bubbles(true);
    init.set_key(key);
    button.dispatch_event(&web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap()).unwrap();
}

fn is_focused(button: &web_sys::HtmlButtonElement) -> bool {
    web_sys::window().unwrap().document().unwrap().active_element().as_ref() == Some(button.unchecked_ref::<web_sys::Element>())
}

fn items(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// accordion.test.tsx: supports single uncontrolled mode
#[wasm_bindgen_test]
fn supports_single_uncontrolled_mode() {
    let t = render(example(ContextProps { default_value: Value::Items(items(&["account"])), ..Default::default() }));
    assert_eq!((expanded(&t)[0].as_str(), expanded(&t)[2].as_str()), ("true", "false"));
    activate(&t, 2);
    assert_eq!((expanded(&t)[0].as_str(), expanded(&t)[2].as_str()), ("false", "true"));
}

// accordion.test.tsx: lets a single item collapse by default
#[wasm_bindgen_test]
fn lets_a_single_item_collapse_by_default() {
    let t = render(accordion_with(ContextProps { default_value: Value::Items(items(&["account"])), ..Default::default() }, vec![account()]));
    assert_eq!(buttons(&t)[0].get_attribute("aria-disabled"), None);
    activate(&t, 0);
    assert_eq!(expanded(&t), ["false"]);
}

#[derive(Clone, Default)]
struct ControlledProps {
    multiple: bool,
    changes: Log,
}

/// Remix's controlled `App`s: the open items live in the component.
struct Controlled {
    value: Rc<RefCell<Vec<String>>>,
}

impl Component for Controlled {
    type Props = ControlledProps;

    fn setup(_: &ControlledProps, _: &Handle<Self>) -> Self {
        Controlled { value: Rc::new(RefCell::new(items(&["account"]))) }
    }

    fn render(&self, props: &ControlledProps, h: &Handle<Self>) -> Node {
        let (value, changes, h) = (self.value.clone(), props.changes.clone(), h.clone());
        let on_value_change: OnValueChange = Rc::new(move |next| {
            changes.borrow_mut().push(next.to_vec());
            *value.borrow_mut() = next.to_vec();
            h.update();
        });
        let value = Value::Items(self.value.borrow().clone());
        accordion_with(ContextProps { multiple: props.multiple, value, on_value_change: Some(on_value_change), ..Default::default() }, vec![account(), billing()])
    }
}

// accordion.test.tsx: supports single controlled mode
#[wasm_bindgen_test]
fn supports_single_controlled_mode() {
    let changes = Log::default();
    let t = render(rsx! { <Controlled changes={changes.clone()}/> });
    activate(&t, 1);
    assert_eq!(*changes.borrow(), [items(&["billing"])]);
    assert_eq!(expanded(&t), ["false", "true"]);
}

// accordion.test.tsx: keeps a single non-collapsible item open
#[wasm_bindgen_test]
fn keeps_a_single_non_collapsible_item_open() {
    let changes = Log::default();
    let props = ContextProps { collapsible: Some(false), default_value: Value::Items(items(&["account"])), on_value_change: Some(recorder(&changes)), ..Default::default() };
    let t = render(accordion_with(props, vec![account()]));
    assert_eq!(buttons(&t)[0].get_attribute("aria-disabled").as_deref(), Some("true"));
    activate(&t, 0);
    assert_eq!(expanded(&t), ["true"]);
    assert!(changes.borrow().is_empty());
}

// accordion.test.tsx: supports multiple uncontrolled mode
#[wasm_bindgen_test]
fn supports_multiple_uncontrolled_mode() {
    let props = ContextProps { multiple: true, default_value: Value::Items(items(&["account"])), ..Default::default() };
    let t = render(accordion_with(props, vec![account(), billing()]));
    activate(&t, 1);
    assert_eq!(expanded(&t), ["true", "true"]);
    activate(&t, 0);
    assert_eq!(expanded(&t), ["false", "true"]);
}

// accordion.test.tsx: supports multiple controlled mode
#[wasm_bindgen_test]
fn supports_multiple_controlled_mode() {
    let changes = Log::default();
    let t = render(rsx! { <Controlled multiple={true} changes={changes.clone()}/> });
    activate(&t, 1);
    assert_eq!(*changes.borrow(), [items(&["account", "billing"])]);
    assert_eq!(expanded(&t), ["true", "true"]);
}

// accordion.test.tsx: supports disabled root and disabled items
#[wasm_bindgen_test]
fn supports_disabled_root_and_disabled_items() {
    let t = render(example(ContextProps { disabled: true, ..Default::default() }));
    assert_eq!(buttons(&t).iter().map(|b| b.disabled()).collect::<Vec<_>>(), [true, true, true]);
}

// accordion.test.tsx: links triggers and panels with aria attributes
#[wasm_bindgen_test]
fn links_triggers_and_panels_with_aria_attributes() {
    let t = render(example(ContextProps::default()));
    let button = buttons(&t)[0].clone();
    let panel = t.query("[id$=\"-panel\"]").expect("a panel");
    assert_eq!(button.get_attribute("aria-controls"), Some(panel.id()));
    assert_eq!(panel.get_attribute("aria-labelledby"), Some(button.id()));
}

// accordion.test.tsx: supports arrow, home, and end keyboard navigation
#[wasm_bindgen_test]
fn supports_arrow_home_and_end_keyboard_navigation() {
    let t = render(example(ContextProps::default()));
    let b = buttons(&t);
    b[0].focus().unwrap();
    key(&b[0], "ArrowDown");
    assert!(is_focused(&b[2]), "skips the disabled item");
    key(&b[2], "ArrowUp");
    assert!(is_focused(&b[0]));
    key(&b[0], "End");
    assert!(is_focused(&b[2]));
    key(&b[2], "Home");
    assert!(is_focused(&b[0]));
}

// accordion.test.tsx: dispatches a bubbling Accordion change event from the root
#[wasm_bindgen_test]
fn dispatches_a_bubbling_accordion_change_event_from_the_root() {
    let captured = Rc::new(RefCell::new(None));
    let c = captured.clone();
    let t = render(rsx! {
        <div mix={on_accordion_change(EventHandler::new(move |event| *c.borrow_mut() = accordion_change(event)))}>
            {accordion_with(ContextProps::default(), vec![account(), billing()])}
        </div>
    });
    activate(&t, 1);
    assert_eq!(*captured.borrow(), Some(AccordionChange { multiple: false, item_value: "billing".into(), value: items(&["billing"]) }));
}
