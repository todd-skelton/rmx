//! Ports of Remix's `ui/src/toggle.test.tsx`. Run with
//! `pwsh ./test-browser.ps1 -Crate rmx-ui -Test browser_toggle`.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use rmx::test::render;
use rmx::{rsx, EventHandler};
use rmx_ui::toggle::{on_toggle_change, toggle, toggle_checked, ToggleOptions};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn log<T: 'static>() -> Rc<RefCell<Vec<T>>> {
    Rc::default()
}

// toggle.test.tsx: tracks switch state on a native checkbox input
#[wasm_bindgen_test]
fn tracks_switch_state_on_a_native_checkbox_input() {
    let (changes, events, input_ref) = (log::<bool>(), log::<Option<bool>>(), Rc::new(RefCell::new(None::<web_sys::Element>)));
    let (c, e, r) = (changes.clone(), events.clone(), input_ref.clone());
    let options = ToggleOptions::default()
        .default_checked(true)
        .name("notifications")
        .input_ref(move |node, _| *r.borrow_mut() = Some(node.element().clone()))
        .on_checked_change(move |checked| c.borrow_mut().push(checked));
    let t = render(rsx! {
        <div mix={on_toggle_change(EventHandler::new(move |event| e.borrow_mut().push(toggle_checked(event))))}>
            <input aria-label="Notifications" mix={toggle(options)}/>
        </div>
    });
    let input: web_sys::HtmlInputElement = t.query("input[type=\"checkbox\"]").expect("a checkbox").unchecked_into();

    assert_eq!(input_ref.borrow().as_ref(), Some(input.unchecked_ref::<web_sys::Element>()));
    assert_eq!(input.name(), "notifications");
    assert!(input.checked());
    assert_eq!(input.get_attribute("role").as_deref(), Some("switch"));
    assert_eq!(input.get_attribute("aria-checked"), None);
    assert_eq!(input.get_attribute("data-state").as_deref(), Some("checked"));

    t.act(|| input.click());

    assert!(!input.checked());
    assert_eq!(input.get_attribute("aria-checked"), None);
    assert_eq!(input.get_attribute("data-state").as_deref(), Some("unchecked"));
    assert_eq!(*changes.borrow(), [false]);
    assert_eq!(*events.borrow(), [Some(false)], "an rmx:toggle-change event carrying the new state");
}

// toggle.test.tsx: supports non-input hosts with switch role and space-key toggling
#[wasm_bindgen_test]
fn supports_non_input_hosts_with_switch_role_and_space_key_toggling() {
    let changes = log::<bool>();
    let c = changes.clone();
    let t = render(rsx! {
        <span aria-label="Standalone span" mix={toggle(ToggleOptions::default().on_checked_change(move |checked| c.borrow_mut().push(checked)))}></span>
    });
    let control = t.query("[role=\"switch\"]").expect("a switch");

    assert_eq!(control.tab_index(), 0);
    assert_eq!(control.get_attribute("aria-checked").as_deref(), Some("false"));
    assert_eq!(control.get_attribute("data-state").as_deref(), Some("unchecked"));

    let init = web_sys::KeyboardEventInit::new();
    init.set_bubbles(true);
    init.set_key(" ");
    let event = web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap();
    t.act(|| drop(control.dispatch_event(&event)));

    assert_eq!(control.get_attribute("aria-checked").as_deref(), Some("true"));
    assert_eq!(control.get_attribute("data-state").as_deref(), Some("checked"));
    assert_eq!(*changes.borrow(), [true]);
}

// toggle.test.tsx: does not toggle when disabled or read-only
#[wasm_bindgen_test]
fn does_not_toggle_when_disabled_or_read_only() {
    let (disabled_changes, read_only_changes) = (log::<bool>(), log::<bool>());
    let (d, r) = (disabled_changes.clone(), read_only_changes.clone());
    let t = render(rsx! {
        <>
            <input mix={toggle(ToggleOptions::default().default_checked(true).disabled(true).on_checked_change(move |checked| d.borrow_mut().push(checked)))}/>
            <input mix={toggle(ToggleOptions::default().default_checked(true).read_only(true).on_checked_change(move |checked| r.borrow_mut().push(checked)))}/>
        </>
    });
    let controls: Vec<web_sys::HtmlInputElement> = t.query_all("input[type=\"checkbox\"]").into_iter().map(|e| e.unchecked_into()).collect();
    assert_eq!(controls.len(), 2);

    t.act(|| {
        controls[0].click();
        controls[1].click();
    });

    for control in &controls {
        assert!(control.checked());
        assert_eq!(control.get_attribute("data-state").as_deref(), Some("checked"));
    }
    assert!(disabled_changes.borrow().is_empty());
    assert!(read_only_changes.borrow().is_empty());
}
