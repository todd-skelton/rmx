//! Ports of Remix's `ui/src/tabs.test.tsx` (the server rendering case is the
//! `renders_the_first_enabled_tab_active_on_the_server` unit test in `src/tabs.rs`). Run
//! with `pwsh ./test-browser.ps1 -Crate rmx-ui -Test browser_tabs`.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use rmx::test::{render, RenderResult};
use rmx::{rsx, Component, EventHandler, Handle, Node};
use rmx_ui::tabs::{self, on_tabs_change, tabs_change, OnActiveTabChange, TabOptions};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

type Log = Rc<RefCell<Vec<String>>>;

fn recorder(log: &Log) -> OnActiveTabChange {
    let log = log.clone();
    Rc::new(move |tab| log.borrow_mut().push(tab.to_string()))
}

/// Remix's `renderExampleTabs`: overview, activity and a disabled settings tab.
fn example(default_active_tab: Option<&str>, on_change: Option<OnActiveTabChange>) -> Node {
    let default_active_tab = default_active_tab.map(str::to_string);
    rsx! {
        <tabs::Context default_active_tab={default_active_tab} on_active_tab_change={on_change}>
            <div mix={tabs::root()}>
                <div mix={tabs::list()}>
                    <button mix={tabs::tab("overview")} type="button">"Overview"</button>
                    <button mix={tabs::tab("activity")} type="button">"Activity"</button>
                    <button mix={tabs::tab(TabOptions::new("settings").disabled(true))} type="button">"Settings"</button>
                </div>
                <div mix={tabs::panel("overview")}>"Project summary."</div>
                <div mix={tabs::panel("activity")}>"Recent changes."</div>
                <div mix={tabs::panel("settings")}>"Private controls."</div>
            </div>
        </tabs::Context>
    }
}

fn tab_buttons(t: &RenderResult) -> Vec<web_sys::HtmlButtonElement> {
    t.query_all("[role=\"tab\"]").into_iter().map(|e| e.unchecked_into()).collect()
}

fn selected(t: &RenderResult) -> Vec<String> {
    tab_buttons(t).iter().map(|b| b.get_attribute("aria-selected").unwrap_or_default()).collect()
}

fn hidden(t: &RenderResult) -> Vec<bool> {
    t.query_all("[role=\"tabpanel\"]").iter().map(|p| p.hidden()).collect()
}

fn key(t: &RenderResult, index: usize, key: &str) {
    let init = web_sys::KeyboardEventInit::new();
    init.set_bubbles(true);
    init.set_key(key);
    let event = web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap();
    let button = tab_buttons(t)[index].clone();
    t.act(|| drop(button.dispatch_event(&event)));
}

fn focused(t: &RenderResult, index: usize) -> bool {
    let active = web_sys::window().unwrap().document().unwrap().active_element();
    active.as_ref() == Some(tab_buttons(t)[index].unchecked_ref::<web_sys::Element>())
}

fn click(t: &RenderResult, index: usize) {
    let button = tab_buttons(t)[index].clone();
    t.act(|| button.click());
}

// tabs.test.tsx: supports uncontrolled mode with a default active tab
#[wasm_bindgen_test]
fn supports_uncontrolled_mode_with_a_default_active_tab() {
    let changes = Log::default();
    let t = render(example(Some("activity"), Some(recorder(&changes))));
    assert_eq!(selected(&t)[..2], ["false", "true"]);
    assert_eq!(tab_buttons(&t)[1].get_attribute("data-state").as_deref(), Some("active"));
    assert_eq!(hidden(&t)[..2], [true, false]);

    click(&t, 0);

    assert_eq!(*changes.borrow(), ["overview"]);
    assert_eq!(selected(&t)[..2], ["true", "false"]);
    assert_eq!(hidden(&t)[..2], [false, true]);
}

// tabs.test.tsx: uses the first enabled tab when no default active tab is provided
#[wasm_bindgen_test]
fn uses_the_first_enabled_tab_when_no_default_active_tab_is_provided() {
    let t = render(rsx! {
        <tabs::Context>
            <div mix={tabs::root()}>
                <div mix={tabs::list()}>
                    <button mix={tabs::tab(TabOptions::new("disabled").disabled(true))} type="button">"Disabled"</button>
                    <button mix={tabs::tab("first-enabled")} type="button">"First enabled"</button>
                </div>
                <div mix={tabs::panel("disabled")}>"Disabled content."</div>
                <div mix={tabs::panel("first-enabled")}>"Enabled content."</div>
            </div>
        </tabs::Context>
    });
    assert_eq!(selected(&t), ["false", "true"]);
    assert_eq!(hidden(&t), [true, false]);
}

#[derive(Clone, Default)]
struct ControlledProps {
    changes: Log,
}

/// Remix's controlled `App`: the active tab lives in the component and changes re-render it.
struct Controlled {
    active: Rc<RefCell<String>>,
}

impl Component for Controlled {
    type Props = ControlledProps;

    fn setup(_: &ControlledProps, _: &Handle<Self>) -> Self {
        Controlled { active: Rc::new(RefCell::new("overview".into())) }
    }

    fn render(&self, props: &ControlledProps, h: &Handle<Self>) -> Node {
        let (active, changes, h) = (self.active.clone(), props.changes.clone(), h.clone());
        let on_change: OnActiveTabChange = Rc::new(move |next| {
            changes.borrow_mut().push(next.to_string());
            *active.borrow_mut() = next.to_string();
            h.update();
        });
        let active_tab = self.active.borrow().clone();
        rsx! {
            <tabs::Context active_tab={active_tab} on_active_tab_change={on_change}>
                <div mix={tabs::root()}>
                    <div mix={tabs::list()}>
                        <button mix={tabs::tab("overview")} type="button">"Overview"</button>
                        <button mix={tabs::tab("activity")} type="button">"Activity"</button>
                    </div>
                    <div mix={tabs::panel("overview")}>"Project summary."</div>
                    <div mix={tabs::panel("activity")}>"Recent changes."</div>
                </div>
            </tabs::Context>
        }
    }
}

// tabs.test.tsx: supports controlled mode
#[wasm_bindgen_test]
fn supports_controlled_mode() {
    let changes = Log::default();
    let t = render(rsx! { <Controlled changes={changes.clone()}/> });
    click(&t, 1);
    assert_eq!(*changes.borrow(), ["activity"]);
    assert_eq!(selected(&t), ["false", "true"]);
}

// tabs.test.tsx: dispatches a bubbling Tabs change event from the root
#[wasm_bindgen_test]
fn dispatches_a_bubbling_tabs_change_event_from_the_root() {
    let captured = Rc::new(RefCell::new(None));
    let c = captured.clone();
    let t = render(rsx! {
        <div mix={on_tabs_change(EventHandler::new(move |event| *c.borrow_mut() = tabs_change(event)))}>
            {example(Some("overview"), None)}
        </div>
    });
    click(&t, 1);
    assert_eq!(*captured.borrow(), Some(("activity".to_string(), Some("overview".to_string()))));
}

// tabs.test.tsx: uses arrow keys to activate tabs
#[wasm_bindgen_test]
fn uses_arrow_keys_to_activate_tabs() {
    let t = render(example(Some("overview"), None));
    tab_buttons(&t)[0].focus().unwrap();
    key(&t, 0, "ArrowRight");
    assert!(focused(&t, 1));
    assert_eq!(selected(&t)[..2], ["false", "true"]);
    assert_eq!(hidden(&t)[..2], [true, false]);

    key(&t, 1, "ArrowRight");
    assert!(focused(&t, 0), "skips the disabled tab and wraps");
    assert_eq!(selected(&t)[..2], ["true", "false"]);
}

// tabs.test.tsx: uses Home and End to activate the first and last enabled tabs
#[wasm_bindgen_test]
fn uses_home_and_end_to_activate_the_first_and_last_enabled_tabs() {
    let t = render(example(Some("overview"), None));
    tab_buttons(&t)[0].focus().unwrap();
    key(&t, 0, "End");
    assert!(focused(&t, 1));
    assert_eq!(selected(&t)[..2], ["false", "true"]);

    key(&t, 1, "Home");
    assert!(focused(&t, 0));
    assert_eq!(selected(&t)[..2], ["true", "false"]);
}

// tabs.test.tsx: supports Enter and Space activation
#[wasm_bindgen_test]
fn supports_enter_and_space_activation() {
    let t = render(example(Some("overview"), None));
    tab_buttons(&t)[1].focus().unwrap();
    key(&t, 1, "Enter");
    assert_eq!(selected(&t)[1], "true");

    tab_buttons(&t)[0].focus().unwrap();
    key(&t, 0, " ");
    assert_eq!(selected(&t)[0], "true");
}

// tabs.test.tsx: does not activate disabled tabs
#[wasm_bindgen_test]
fn does_not_activate_disabled_tabs() {
    let changes = Log::default();
    let t = render(example(Some("overview"), Some(recorder(&changes))));
    click(&t, 2);
    assert!(changes.borrow().is_empty());
    assert_eq!(selected(&t)[0], "true");
    assert!(tab_buttons(&t)[2].disabled());
}
