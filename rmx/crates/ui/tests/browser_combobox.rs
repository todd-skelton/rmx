//! Ports of Remix's `ui/src/combobox.test.tsx`. Remix's fake timers stop the clock between
//! the 60 ms flash, the close transition and the 50 ms input commit; here the tests wait in
//! real time: they poll until the surface closes (the transition wait then gives them two
//! frames to observe the closing state), and wait past the commit delay. `scrollIntoView`
//! is stubbed like Remix's. Run with `pwsh ./test-browser.ps1 -Crate rmx-ui -Test browser_combobox`.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use rmx::test::{render, RenderResult};
use rmx::{rsx, EventHandler, Node};
use rmx_ui::combobox::{self, combobox_change, on_combobox_change, ComboboxState, OnRef};
use rmx_ui::listbox::ListboxOption;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

/// Remix's `TestCombobox` props.
#[derive(Default)]
struct Props {
    default_value: Option<&'static str>,
    input_id: Option<&'static str>,
    name: Option<&'static str>,
    placeholder: Option<&'static str>,
    on_ref: Option<OnRef>,
}

/// Remix's `TestCombobox`: the input, the surface and list, and a hidden input when named.
fn test_combobox(props: Props, options: Vec<Node>) -> Node {
    let Props { default_value, input_id, name, placeholder, on_ref } = props;
    let default_value = default_value.map(str::to_string);
    let hidden = name.map(|_| rsx! { <input mix={combobox::hidden_input()}/> });
    rsx! {
        <combobox::Context default_value={default_value.clone()} name={name.map(str::to_string)} on_ref={on_ref}>
            <div>
                <input defaultValue={default_value} id={input_id} mix={combobox::input()} placeholder={placeholder}/>
                <div mix={combobox::popover()}>
                    <div mix={combobox::list()}>{options}</div>
                </div>
                {hidden}
            </div>
        </combobox::Context>
    }
}

/// Remix's `TestComboboxOption`: the option's text is `text`, or its label.
fn option(option: ListboxOption, text: Option<&str>) -> Node {
    let text = text.map_or_else(|| option.label.clone(), str::to_string);
    rsx! { <div mix={combobox::option(option)}>{text}</div> }
}

fn frameworks() -> Vec<Node> {
    vec![
        option(ListboxOption::new("remix", "Remix framework"), Some("Remix")),
        option(ListboxOption::new("react-router", "React Router framework").disabled(true), Some("React Router")),
        option(ListboxOption::new("react", "React framework"), Some("React")),
    ]
}

/// Remix's `renderCombobox`.
fn framework_combobox(default_value: Option<&'static str>, on_ref: Option<OnRef>) -> Node {
    let props = Props { default_value, input_id: Some("framework"), name: Some("framework"), placeholder: Some("Search a framework"), on_ref };
    test_combobox(props, frameworks())
}

fn environments(dev_null: bool) -> Vec<Node> {
    let mut options = vec![option(ListboxOption::new("production", "Production"), None), option(ListboxOption::new("staging", "Staging").text_value(&["staging", "beta"]), None)];
    if dev_null {
        options.push(option(ListboxOption::new("dev-null", "Dev Null").disabled(true).text_value(&["dev"]), None));
    }
    options.push(option(ListboxOption::new("local", "Local").text_value(&["local", "dev", "workbench"]), None));
    options
}

fn environment_combobox(dev_null: bool) -> Node {
    let props = Props { input_id: Some("environment"), name: Some("environment"), placeholder: Some("Search an environment"), ..Default::default() };
    test_combobox(props, environments(dev_null))
}

fn render_app(node: Node) -> RenderResult {
    stub_scroll_into_view();
    render(node)
}

type Changes = Rc<RefCell<Vec<Option<String>>>>;

/// Remix's `renderObservedCombobox`: record each change's value from an ancestor.
fn render_observed(changes: &Changes, node: Node) -> RenderResult {
    let record = changes.clone();
    let observe = on_combobox_change(EventHandler::new(move |event| record.borrow_mut().push(combobox_change(event).value)));
    render_app(rsx! { <div mix={observe}>{node}</div> })
}

fn js(body: &str) -> JsValue {
    js_sys::Function::new_no_args(body).call0(&JsValue::NULL).unwrap()
}

fn call(args: &str, body: &str, values: &[&JsValue]) -> JsValue {
    let f = js_sys::Function::new_with_args(args, body);
    f.apply(&JsValue::NULL, &values.iter().copied().collect::<js_sys::Array>()).unwrap()
}

fn stub_scroll_into_view() {
    js("if (!window.__scrollIntoView) { window.__scrollIntoView = HTMLElement.prototype.scrollIntoView; HTMLElement.prototype.scrollIntoView = function () {} }");
}

/// Remix's `stubScrollIntoView(node)`: record each call's `block,inline`.
fn stub_node_scroll_into_view(el: &web_sys::HtmlElement) -> js_sys::Array {
    let calls = js_sys::Array::new();
    call("el, calls", "Object.defineProperty(el, 'scrollIntoView', { configurable: true, value: (o) => calls.push(o.block + ',' + o.inline) })", &[el, &calls]);
    calls
}

fn input(t: &RenderResult) -> web_sys::HtmlInputElement {
    t.query("input[type=\"text\"]").expect("the input").unchecked_into()
}

fn hidden_input(t: &RenderResult) -> web_sys::HtmlInputElement {
    t.query("input[type=\"hidden\"]").expect("the hidden input").unchecked_into()
}

fn surface(t: &RenderResult) -> web_sys::HtmlElement {
    t.query("[popover]").expect("the surface")
}

fn list(t: &RenderResult) -> web_sys::HtmlElement {
    t.query("[role=\"listbox\"]").expect("the list")
}

fn option_by_text(t: &RenderResult, text: &str) -> web_sys::HtmlElement {
    t.query_all("[role=\"option\"]").into_iter().find(|o| o.text_content().unwrap_or_default().trim() == text).unwrap_or_else(|| panic!("option {text}"))
}

fn is_open(el: &web_sys::HtmlElement) -> bool {
    el.matches(":popover-open").unwrap()
}

fn is_active(el: &web_sys::HtmlElement) -> bool {
    web_sys::window().unwrap().document().unwrap().active_element().as_ref() == Some(el.unchecked_ref::<web_sys::Element>())
}

fn attr(el: &web_sys::Element, name: &str) -> Option<String> {
    el.get_attribute(name)
}

fn display(el: &web_sys::HtmlElement) -> String {
    call("el", "return getComputedStyle(el).display", &[el]).as_string().unwrap()
}

/// Remix's `hidden` and computed `display` checks together.
fn assert_hidden(el: &web_sys::HtmlElement, hidden: bool) {
    assert_eq!(el.hidden(), hidden, "hidden of {:?}", el.text_content());
    assert_eq!(display(el) == "none", hidden, "display of {:?}", el.text_content());
}

/// Remix's `expectInputSelection`: the whole value is selected.
fn assert_input_selected(input: &web_sys::HtmlInputElement) {
    let len = input.value().encode_utf16().count() as u32;
    assert_eq!((input.selection_start().unwrap(), input.selection_end().unwrap()), (Some(0), Some(len)));
}

/// Remix's `expectInputSelectionCleared`: a caret at the end.
fn assert_input_selection_cleared(input: &web_sys::HtmlInputElement) {
    let len = input.value().encode_utf16().count() as u32;
    assert_eq!((input.selection_start().unwrap(), input.selection_end().unwrap()), (Some(len), Some(len)));
}

fn key(target: &web_sys::HtmlElement, key: &str) -> web_sys::KeyboardEvent {
    let init = web_sys::KeyboardEventInit::new();
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_key(key);
    let event = web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap();
    target.dispatch_event(&event).unwrap();
    event
}

fn pointer(target: &web_sys::HtmlElement, kind: &str) {
    let init = web_sys::MouseEventInit::new();
    init.set_bubbles(true);
    init.set_button(0);
    target.dispatch_event(&web_sys::MouseEvent::new_with_mouse_event_init_dict(kind, &init).unwrap()).unwrap();
}

fn fire_input(input: &web_sys::HtmlInputElement) {
    let init = web_sys::EventInit::new();
    init.set_bubbles(true);
    input.dispatch_event(&web_sys::Event::new_with_event_init_dict("input", &init).unwrap()).unwrap();
}

/// Remix's `changeInputValue`.
fn change_input_value(input: &web_sys::HtmlInputElement, value: &str) {
    input.set_value(value);
    fire_input(input);
}

/// Remix's `typeText`: a keydown per character, then the character and an `input`
/// event unless the keydown was prevented.
async fn type_text(input: &web_sys::HtmlInputElement, t: &RenderResult, text: &str) {
    for character in text.chars() {
        let event = key(input, &character.to_string());
        if !event.default_prevented() {
            input.set_value(&format!("{}{character}", input.value()));
            fire_input(input);
        }
        settle(t).await;
    }
}

fn blur(target: &web_sys::HtmlElement) {
    call("el", "el.dispatchEvent(new FocusEvent('blur'))", &[target]);
}

async fn microtask() {
    JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await.unwrap();
}

/// Remix's `settle`: a microtask and a flush, twice.
async fn settle(t: &RenderResult) {
    for _ in 0..2 {
        microtask().await;
        t.act(|| {});
    }
}

async fn frame() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().request_animation_frame(&resolve).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

/// Remix's `settleFrames`: two frames, then [`settle`].
async fn settle_frames(t: &RenderResult) {
    frame().await;
    frame().await;
    settle(t).await;
}

async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

/// Remix's `finishSelectionFlash`: past the 60 ms flash, until the surface starts closing.
async fn finish_selection_flash(t: &RenderResult) {
    let surface = surface(t);
    for _ in 0..200 {
        if !is_open(&surface) {
            settle(t).await;
            return;
        }
        sleep(2).await;
        t.act(|| {});
    }
    panic!("the surface never closed after the flash");
}

/// Remix's `finishCloseTransition`: run and end the surface's opacity transition.
async fn finish_close_transition(surface: &web_sys::HtmlElement) {
    microtask().await;
    call(
        "el",
        "el.dispatchEvent(new TransitionEvent('transitionrun', { propertyName: 'opacity' })); \
         el.dispatchEvent(new TransitionEvent('transitionend', { propertyName: 'opacity' }));",
        &[surface],
    );
    microtask().await;
}

/// Remix's `finishInputCommit`: past the 50 ms input commit delay.
async fn finish_input_commit(t: &RenderResult) {
    sleep(70).await;
    settle(t).await;
}

// combobox.test.tsx: applies the raw defaultValue to the input text while keeping the hidden input committed
#[wasm_bindgen_test]
async fn applies_the_raw_default_value_to_the_input_text_while_keeping_the_hidden_input_committed() {
    let t = render_app(framework_combobox(Some("react"), None));
    settle(&t).await;
    assert_eq!(input(&t).value(), "react");
    assert_eq!(hidden_input(&t).value(), "react");
}

// combobox.test.tsx: calls the public ref only on first render
#[wasm_bindgen_test]
async fn calls_the_public_ref_only_on_first_render() {
    let handles: Rc<RefCell<Vec<Rc<ComboboxState>>>> = Rc::default();
    let record = handles.clone();
    let on_ref: OnRef = Rc::new(move |state| record.borrow_mut().push(state));
    let t = render_app(rsx! {
        <combobox::Context name={Some("framework".to_string())} on_ref={Some(on_ref)}>
            <input id="framework" mix={combobox::input()} placeholder="Search a framework"/>
            <div mix={combobox::popover()}>
                <div mix={combobox::list()}>{frameworks()}</div>
            </div>
        </combobox::Context>
    });
    let input = input(&t);
    settle(&t).await;
    assert_eq!(handles.borrow().len(), 1);

    input.focus().unwrap();
    key(&input, "ArrowDown");
    settle_frames(&t).await;
    assert_eq!(handles.borrow().len(), 1);
    assert!(handles.borrow()[0].is_open());
}

// combobox.test.tsx: clears the committed selection as soon as the user types
#[wasm_bindgen_test]
async fn clears_the_committed_selection_as_soon_as_the_user_types() {
    let changes = Changes::default();
    let t = render_observed(&changes, framework_combobox(Some("react"), None));
    let input = input(&t);
    input.focus().unwrap();
    change_input_value(&input, "rea");
    settle(&t).await;
    assert_eq!(input.value(), "rea");
    assert_eq!(hidden_input(&t).value(), "");
    assert!(is_open(&surface(&t)));
    assert_eq!(attr(&input, "aria-activedescendant"), None);
    assert_eq!(*changes.borrow(), [None]);
}

// combobox.test.tsx: typing matching text opens the popover, filters visible options, and keeps focus on the input
#[wasm_bindgen_test]
async fn typing_matching_text_opens_the_popover_filters_visible_options_and_keeps_focus_on_the_input() {
    let t = render_app(framework_combobox(None, None));
    let (input, surface, list) = (input(&t), surface(&t), list(&t));
    let (remix, react_router, react) = (option_by_text(&t, "Remix"), option_by_text(&t, "React Router"), option_by_text(&t, "React"));
    input.focus().unwrap();
    change_input_value(&input, "rea");
    settle(&t).await;
    assert!(is_open(&surface));
    assert!(is_active(&input));
    assert_eq!(attr(&input, "aria-activedescendant"), None);
    assert_eq!(attr(&surface, "data-show-reason").as_deref(), Some("hint"));
    assert_eq!(attr(&input, "data-surface-visible").as_deref(), Some("true"));
    assert_hidden(&remix, true);
    assert_hidden(&react_router, false);
    assert_hidden(&react, false);
    assert_eq!(Some(list.id()), attr(&input, "aria-controls"));
}

// combobox.test.tsx: allows real typing without resetting the input value between characters
#[wasm_bindgen_test]
async fn allows_real_typing_without_resetting_the_input_value_between_characters() {
    let t = render_app(framework_combobox(None, None));
    let input = input(&t);
    input.focus().unwrap();
    type_text(&input, &t, "rea").await;
    assert_eq!(input.value(), "rea");
    assert!(is_open(&surface(&t)));
    assert!(is_active(&input));
    assert_eq!(attr(&input, "aria-activedescendant"), None);
}

// combobox.test.tsx: allows typing spaces into the input while the popup is open
#[wasm_bindgen_test]
async fn allows_typing_spaces_into_the_input_while_the_popup_is_open() {
    let t = render_app(framework_combobox(None, None));
    let input = input(&t);
    input.focus().unwrap();
    type_text(&input, &t, "react f").await;
    assert_eq!(input.value(), "react f");
    assert!(is_open(&surface(&t)));
    assert!(is_active(&input));
    assert_eq!(attr(&input, "aria-activedescendant"), None);
}

// combobox.test.tsx: hides the popover when the input is empty or there are no matches
#[wasm_bindgen_test]
async fn hides_the_popover_when_the_input_is_empty_or_there_are_no_matches() {
    let t = render_app(framework_combobox(None, None));
    let (input, surface) = (input(&t), surface(&t));
    input.focus().unwrap();
    change_input_value(&input, "re");
    settle(&t).await;
    assert!(is_open(&surface));

    change_input_value(&input, "");
    settle(&t).await;
    assert!(!is_open(&surface));

    change_input_value(&input, "zzz");
    settle(&t).await;
    assert!(!is_open(&surface));
}

// combobox.test.tsx: typing switches an open nav popover into hint mode
#[wasm_bindgen_test]
async fn typing_switches_an_open_nav_popover_into_hint_mode() {
    let t = render_app(framework_combobox(None, None));
    let (input, surface) = (input(&t), surface(&t));
    input.focus().unwrap();
    key(&input, "ArrowDown");
    settle_frames(&t).await;
    assert_eq!(attr(&surface, "data-show-reason").as_deref(), Some("nav"));

    change_input_value(&input, "re");
    settle(&t).await;
    assert!(is_open(&surface));
    assert_eq!(attr(&surface, "data-show-reason").as_deref(), Some("hint"));
}

// combobox.test.tsx: typing no matches switches an open nav popover into hint mode before closing
#[wasm_bindgen_test]
async fn typing_no_matches_switches_an_open_nav_popover_into_hint_mode_before_closing() {
    let t = render_app(framework_combobox(None, None));
    let (input, surface) = (input(&t), surface(&t));
    input.focus().unwrap();
    key(&input, "ArrowDown");
    settle_frames(&t).await;
    assert_eq!(attr(&surface, "data-show-reason").as_deref(), Some("nav"));

    change_input_value(&input, "zzz");
    settle(&t).await;
    assert!(!is_open(&surface));
    assert_eq!(attr(&surface, "data-show-reason").as_deref(), Some("hint"));
}

// combobox.test.tsx: ArrowDown on an exact-match closed input opens an unfiltered list while keeping focus on the input
#[wasm_bindgen_test]
async fn arrow_down_on_an_exact_match_closed_input_opens_an_unfiltered_list_while_keeping_focus_on_the_input() {
    let t = render_app(framework_combobox(None, None));
    let (input, surface) = (input(&t), surface(&t));
    let (remix, react_router, react) = (option_by_text(&t, "Remix"), option_by_text(&t, "React Router"), option_by_text(&t, "React"));
    input.focus().unwrap();
    change_input_value(&input, "react framework");
    settle(&t).await;
    key(&input, "Escape");
    settle(&t).await;
    assert_eq!(attr(&input, "aria-activedescendant"), None);

    key(&input, "ArrowDown");
    settle_frames(&t).await;
    assert!(is_open(&surface));
    assert!(is_active(&input));
    assert!(!remix.hidden());
    assert!(!react_router.hidden());
    assert!(!react.hidden());
    assert_eq!(attr(&surface, "data-show-reason").as_deref(), Some("nav"));
    assert_eq!(attr(&input, "aria-activedescendant"), Some(react.id()));
}

// combobox.test.tsx: clicking the focused input opens the list without an active item when nothing is selected
#[wasm_bindgen_test]
async fn clicking_the_focused_input_opens_the_list_without_an_active_item_when_nothing_is_selected() {
    let t = render_app(framework_combobox(None, None));
    let (input, surface) = (input(&t), surface(&t));
    input.focus().unwrap();
    pointer(&input, "click");
    settle_frames(&t).await;
    assert!(is_open(&surface));
    assert!(is_active(&input));
    assert_eq!(attr(&surface, "data-show-reason").as_deref(), Some("nav"));
    assert_eq!(attr(&input, "aria-activedescendant"), None);
}

// combobox.test.tsx: clicking the focused input activates the selected item when one exists
#[wasm_bindgen_test]
async fn clicking_the_focused_input_activates_the_selected_item_when_one_exists() {
    let t = render_app(framework_combobox(Some("react"), None));
    let (input, surface, react) = (input(&t), surface(&t), option_by_text(&t, "React"));
    input.focus().unwrap();
    pointer(&input, "click");
    settle_frames(&t).await;
    assert!(is_open(&surface));
    assert!(is_active(&input));
    assert_eq!(attr(&input, "aria-activedescendant"), Some(react.id()));
}

// combobox.test.tsx: ArrowUp and ArrowDown navigate the active descendant while the input stays focused
#[wasm_bindgen_test]
async fn arrow_up_and_arrow_down_navigate_the_active_descendant_while_the_input_stays_focused() {
    let t = render_app(framework_combobox(None, None));
    let (input, remix, react) = (input(&t), option_by_text(&t, "Remix"), option_by_text(&t, "React"));
    input.focus().unwrap();
    key(&input, "ArrowDown");
    settle_frames(&t).await;
    assert!(is_active(&input));
    assert_eq!(attr(&input, "aria-activedescendant"), Some(remix.id()));
    assert_eq!(attr(&input, "data-surface-visible").as_deref(), Some("true"));

    key(&input, "ArrowDown");
    settle(&t).await;
    assert!(is_active(&input));
    assert_eq!(attr(&input, "aria-activedescendant"), Some(react.id()));

    key(&input, "ArrowUp");
    settle(&t).await;
    assert_eq!(attr(&input, "aria-activedescendant"), Some(remix.id()));
}

// combobox.test.tsx: scrolls the active descendant into view during keyboard navigation
#[wasm_bindgen_test]
async fn scrolls_the_active_descendant_into_view_during_keyboard_navigation() {
    let t = render_app(framework_combobox(None, None));
    let (input, remix, react) = (input(&t), option_by_text(&t, "Remix"), option_by_text(&t, "React"));
    let (scroll_remix, scroll_react) = (stub_node_scroll_into_view(&remix), stub_node_scroll_into_view(&react));
    input.focus().unwrap();
    key(&input, "ArrowDown");
    settle_frames(&t).await;
    assert_eq!(attr(&input, "aria-activedescendant"), Some(remix.id()));
    assert!(scroll_remix.iter().any(|c| c.as_string().as_deref() == Some("nearest,nearest")), "{:?}", scroll_remix.to_vec());

    key(&input, "ArrowDown");
    settle(&t).await;
    assert_eq!(attr(&input, "aria-activedescendant"), Some(react.id()));
    assert!(scroll_react.iter().any(|c| c.as_string().as_deref() == Some("nearest,nearest")), "{:?}", scroll_react.to_vec());
}

// combobox.test.tsx: Enter does not open from the closed input and Space does not select from the open input
#[wasm_bindgen_test]
async fn enter_does_not_open_from_the_closed_input_and_space_does_not_select_from_the_open_input() {
    let t = render_app(framework_combobox(None, None));
    let (input, surface) = (input(&t), surface(&t));
    input.focus().unwrap();
    key(&input, "Enter");
    settle(&t).await;
    assert!(!is_open(&surface));

    key(&input, "ArrowDown");
    settle_frames(&t).await;
    assert!(is_open(&surface));

    key(&input, " ");
    settle(&t).await;
    assert!(is_open(&surface));
    assert_eq!(hidden_input(&t).value(), "");
}

// combobox.test.tsx: Enter selects the active option, flashes it, then closes the popover and emits onComboboxChange
#[wasm_bindgen_test]
async fn enter_selects_the_active_option_flashes_it_then_closes_the_popover_and_emits_on_combobox_change() {
    let changes = Changes::default();
    let t = render_observed(&changes, framework_combobox(None, None));
    let (input, hidden, surface) = (input(&t), hidden_input(&t), surface(&t));
    let (remix, react_router, react) = (option_by_text(&t, "Remix"), option_by_text(&t, "React Router"), option_by_text(&t, "React"));
    input.focus().unwrap();
    change_input_value(&input, "rea");
    settle(&t).await;
    key(&input, "ArrowDown");
    settle(&t).await;
    assert_eq!(attr(&input, "aria-activedescendant"), Some(react.id()));

    key(&input, "Enter");
    settle(&t).await;
    assert_eq!(attr(&react, "data-combobox-flash").as_deref(), Some("true"));
    assert!(is_open(&surface));
    assert!(changes.borrow().is_empty());
    assert_eq!(input.value(), "rea");
    assert_eq!(hidden.value(), "react");
    assert_eq!(attr(&input, "data-surface-visible").as_deref(), Some("true"));
    assert_hidden(&remix, true);
    assert_hidden(&react_router, false);

    finish_selection_flash(&t).await;
    assert!(!is_open(&surface));
    assert_eq!(input.value(), "rea");
    assert_eq!(attr(&input, "data-surface-visible").as_deref(), Some("true"));
    assert_eq!(attr(&input, "aria-activedescendant"), Some(react.id()));

    finish_close_transition(&surface).await;
    settle(&t).await;
    assert!(!is_open(&surface));
    assert!(is_active(&input));
    assert_eq!(input.value(), "rea");
    assert_eq!(hidden.value(), "react");
    assert_eq!(attr(&input, "data-surface-visible"), None);
    assert_eq!(attr(&input, "aria-activedescendant"), None);

    finish_input_commit(&t).await;
    assert!(!is_open(&surface));
    assert!(is_active(&input));
    assert_eq!(input.value(), "React framework");
    assert_eq!(hidden.value(), "react");
    assert_input_selected(&input);
    assert_eq!(attr(&react, "data-combobox-flash"), None);
    assert_eq!(*changes.borrow(), [Some("react".to_string())]);
}

// combobox.test.tsx: selecting from an untouched input delays the visible input commit until after close
#[wasm_bindgen_test]
async fn selecting_from_an_untouched_input_delays_the_visible_input_commit_until_after_close() {
    let changes = Changes::default();
    let t = render_observed(&changes, framework_combobox(None, None));
    let (input, hidden, surface, remix) = (input(&t), hidden_input(&t), surface(&t), option_by_text(&t, "Remix"));
    input.focus().unwrap();
    key(&input, "ArrowDown");
    settle_frames(&t).await;
    assert_eq!(attr(&input, "aria-activedescendant"), Some(remix.id()));
    assert_eq!(input.value(), "");

    key(&input, "Enter");
    settle(&t).await;
    assert_eq!(attr(&remix, "data-combobox-flash").as_deref(), Some("true"));
    assert!(is_open(&surface));
    assert_eq!(input.value(), "");
    assert_eq!(hidden.value(), "remix");
    assert!(changes.borrow().is_empty());

    finish_selection_flash(&t).await;
    assert!(!is_open(&surface));
    assert_eq!(input.value(), "");

    finish_close_transition(&surface).await;
    settle(&t).await;
    assert!(!is_open(&surface));
    assert_eq!(input.value(), "");

    finish_input_commit(&t).await;
    assert!(!is_open(&surface));
    assert!(is_active(&input));
    assert_eq!(input.value(), "Remix framework");
    assert_eq!(hidden.value(), "remix");
    assert_input_selected(&input);
    assert_eq!(attr(&remix, "data-combobox-flash"), None);
    assert_eq!(*changes.borrow(), [Some("remix".to_string())]);
}

// combobox.test.tsx: Enter does nothing when typing has opened the popover but no option is active
#[wasm_bindgen_test]
async fn enter_does_nothing_when_typing_has_opened_the_popover_but_no_option_is_active() {
    let changes = Changes::default();
    let t = render_observed(&changes, framework_combobox(None, None));
    let (input, surface) = (input(&t), surface(&t));
    input.focus().unwrap();
    change_input_value(&input, "rea");
    settle(&t).await;
    assert!(is_open(&surface));
    assert_eq!(attr(&input, "aria-activedescendant"), None);

    key(&input, "Enter");
    settle(&t).await;
    assert!(is_open(&surface));
    assert_eq!(input.value(), "rea");
    assert_eq!(hidden_input(&t).value(), "");
    assert!(changes.borrow().is_empty());
}

/// Type the full React label, move to it and select it with Enter, through the close.
async fn select_react_by_label(t: &RenderResult, input: &web_sys::HtmlInputElement, surface: &web_sys::HtmlElement) {
    input.focus().unwrap();
    change_input_value(input, "React framework");
    settle(t).await;
    key(input, "ArrowDown");
    settle(t).await;
    key(input, "Enter");
    settle(t).await;
    finish_selection_flash(t).await;
    finish_close_transition(surface).await;
    settle(t).await;
}

// combobox.test.tsx: selects the input text after menu selection even when the label is already visible
#[wasm_bindgen_test]
async fn selects_the_input_text_after_menu_selection_even_when_the_label_is_already_visible() {
    let t = render_app(framework_combobox(None, None));
    let (input, surface) = (input(&t), surface(&t));
    select_react_by_label(&t, &input, &surface).await;
    assert!(!is_open(&surface));
    assert!(is_active(&input));
    assert_eq!(input.value(), "React framework");
    assert_input_selected(&input);
}

// combobox.test.tsx: clears the input selection when arrow navigation starts
#[wasm_bindgen_test]
async fn clears_the_input_selection_when_arrow_navigation_starts() {
    let t = render_app(framework_combobox(None, None));
    let (input, surface, react) = (input(&t), surface(&t), option_by_text(&t, "React"));
    select_react_by_label(&t, &input, &surface).await;
    assert_input_selected(&input);

    key(&input, "ArrowDown");
    settle(&t).await;
    assert!(is_active(&input));
    assert_eq!(attr(&input, "aria-activedescendant"), Some(react.id()));
    assert_input_selection_cleared(&input);
}

// combobox.test.tsx: Escape with a non-matching value clears the input and selection
#[wasm_bindgen_test]
async fn escape_with_a_non_matching_value_clears_the_input_and_selection() {
    let changes = Changes::default();
    let t = render_observed(&changes, framework_combobox(Some("react"), None));
    let input = input(&t);
    input.focus().unwrap();
    change_input_value(&input, "zzz");
    settle(&t).await;
    key(&input, "Escape");
    settle(&t).await;
    assert_eq!(input.value(), "");
    assert_eq!(hidden_input(&t).value(), "");
    assert_eq!(*changes.borrow(), [None]);
}

// combobox.test.tsx: blur commits an exact input match without rewriting the visible input text
#[wasm_bindgen_test]
async fn blur_commits_an_exact_input_match_without_rewriting_the_visible_input_text() {
    let changes = Changes::default();
    let t = render_observed(&changes, environment_combobox(false));
    let (input, surface) = (input(&t), surface(&t));
    input.focus().unwrap();
    change_input_value(&input, "beta");
    settle(&t).await;
    blur(&input);
    settle(&t).await;
    assert!(!is_open(&surface));
    assert_eq!(input.value(), "beta");
    assert_eq!(hidden_input(&t).value(), "staging");
    assert_eq!(*changes.borrow(), [Some("staging".to_string())]);
}

// combobox.test.tsx: blur with a non-matching value clears the input and selection
#[wasm_bindgen_test]
async fn blur_with_a_non_matching_value_clears_the_input_and_selection() {
    let changes = Changes::default();
    let t = render_observed(&changes, framework_combobox(Some("remix"), None));
    let input = input(&t);
    input.focus().unwrap();
    change_input_value(&input, "zzz");
    settle(&t).await;
    blur(&input);
    settle(&t).await;
    assert_eq!(input.value(), "");
    assert_eq!(hidden_input(&t).value(), "");
    assert_eq!(*changes.borrow(), [None]);
}

// combobox.test.tsx: skips disabled options and supports searchValue string and array filtering
#[wasm_bindgen_test]
async fn skips_disabled_options_and_supports_search_value_string_and_array_filtering() {
    let t = render_app(environment_combobox(true));
    let input = input(&t);
    let (production, staging, dev_null, local) = (option_by_text(&t, "Production"), option_by_text(&t, "Staging"), option_by_text(&t, "Dev Null"), option_by_text(&t, "Local"));
    input.focus().unwrap();
    change_input_value(&input, "bet");
    settle(&t).await;
    assert_hidden(&staging, false);
    assert_hidden(&local, true);
    assert_eq!(attr(&input, "aria-activedescendant"), None);

    change_input_value(&input, "dev");
    settle(&t).await;
    assert_hidden(&production, true);
    assert_hidden(&staging, true);
    assert_hidden(&dev_null, false);
    assert_hidden(&local, false);
    assert_eq!(attr(&input, "aria-activedescendant"), None);
}

// combobox.test.tsx: pointer selection keeps focus on the input
#[wasm_bindgen_test]
async fn pointer_selection_keeps_focus_on_the_input() {
    let t = render_app(framework_combobox(None, None));
    let (input, hidden, surface) = (input(&t), hidden_input(&t), surface(&t));
    input.focus().unwrap();
    change_input_value(&input, "rea");
    settle(&t).await;

    let react = option_by_text(&t, "React");
    pointer(&react, "pointerdown");
    pointer(&react, "pointerup");
    pointer(&react, "click");
    settle(&t).await;
    assert_eq!(attr(&react, "data-combobox-flash").as_deref(), Some("true"));
    assert!(is_open(&surface));
    assert!(is_active(&input));
    assert_eq!(input.value(), "rea");
    assert_eq!(hidden.value(), "react");

    finish_selection_flash(&t).await;
    assert!(!is_open(&surface));
    assert_eq!(input.value(), "rea");

    finish_close_transition(&surface).await;
    settle(&t).await;
    assert!(!is_open(&surface));
    assert!(is_active(&input));

    finish_input_commit(&t).await;
    assert!(!is_open(&surface));
    assert!(is_active(&input));
    assert_eq!(input.value(), "React framework");
    assert_eq!(hidden.value(), "react");
    assert_input_selected(&input);
    assert_eq!(attr(&react, "data-combobox-flash"), None);
}
