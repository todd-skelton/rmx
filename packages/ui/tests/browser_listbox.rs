//! Ports of Remix's `ui/src/listbox.test.tsx`. The flash case waits the real 60 ms where
//! Remix advances fake timers. Run with
//! `pwsh ./scripts/test-browser.ps1 -Crate rsmix-ui -Test browser_listbox`.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use rsmix::test::{render, RenderResult};
use rsmix::{rsx, Component, Handle, Node};
use rsmix_ui::listbox::{self, ListboxOption, ListboxState, OnChange, OnRef, OnSelectSettled, RegisteredOption};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

/// `(value, (id, label, value) of the option)` per call.
type Calls = Rc<RefCell<Vec<(Option<String>, Option<(String, String, String)>)>>>;

fn describe(option: Option<&RegisteredOption>) -> Option<(String, String, String)> {
    option.map(|o| (o.id.clone(), o.option.label.clone(), o.option.value.clone()))
}

fn recorder(calls: &Calls) -> OnChange {
    let calls = calls.clone();
    Rc::new(move |value, option| calls.borrow_mut().push((value.map(str::to_string), describe(option))))
}

fn values(calls: &Calls) -> Vec<Option<String>> {
    calls.borrow().iter().map(|c| c.0.clone()).collect()
}

fn frameworks() -> Vec<ListboxOption> {
    vec![
        ListboxOption::new("remix", "Remix"),
        ListboxOption::new("react-router", "React Router").disabled(true),
        ListboxOption::new("react", "React"),
        ListboxOption::new("preact", "Preact"),
    ]
}

/// Remix's `RenderListboxOptions`.
#[derive(Clone, Default)]
struct Options {
    active_value: Option<String>,
    flash_selection: bool,
    on_highlight: Option<OnChange>,
    on_select: Option<OnChange>,
    on_select_settled: Option<OnSelectSettled>,
    options: Option<Vec<ListboxOption>>,
    on_ref: Option<OnRef>,
    tab_index: Option<i32>,
    value: Option<String>,
}

/// Remix's `renderListbox`.
fn listbox_node(o: Options) -> Node {
    let options = o.options.unwrap_or_else(frameworks);
    let items: Vec<Node> = options
        .into_iter()
        .map(|option| {
            let (key, label) = (option.value.clone(), option.label.clone());
            rsx! { <div key={key} mix={listbox::option(option)}>{label}</div> }
        })
        .collect();
    let tab_index = o.tab_index.map(|t| t.to_string());
    rsx! {
        <listbox::Context active_value={o.active_value} flash_selection={o.flash_selection} on_highlight={o.on_highlight} on_select={o.on_select} on_select_settled={o.on_select_settled} on_ref={o.on_ref} selection_flash_attribute={Some("data-listbox-flash".to_string())} value={o.value}>
            <div aria-label="Frameworks" tabindex={tab_index} mix={listbox::list()}>{items}</div>
        </listbox::Context>
    }
}

#[derive(Clone, Default)]
struct ControlledProps {
    initial_active_value: Option<String>,
    initial_value: Option<String>,
    options: Options,
}

/// Remix's `renderControlledListbox` `App`: the value and highlight live in the component.
struct Controlled {
    value: Option<String>,
    active_value: Option<String>,
}

impl Component for Controlled {
    type Props = ControlledProps;

    fn setup(props: &ControlledProps, _: &Handle<Self>) -> Self {
        Controlled { value: props.initial_value.clone(), active_value: props.initial_active_value.clone() }
    }

    fn render(&self, props: &ControlledProps, h: &Handle<Self>) -> Node {
        let (outer_highlight, outer_select) = (props.options.on_highlight.clone(), props.options.on_select.clone());
        let handle = h.clone();
        let on_highlight: OnChange = Rc::new(move |value, option| {
            handle.with(|this, h| {
                this.active_value = value.map(str::to_string);
                h.update();
            });
            if let Some(f) = &outer_highlight {
                f(value, option);
            }
        });
        let handle = h.clone();
        let on_select: OnChange = Rc::new(move |value, option| {
            handle.with(|this, h| {
                this.value = value.map(str::to_string);
                h.update();
            });
            if let Some(f) = &outer_select {
                f(value, option);
            }
        });
        listbox_node(Options {
            active_value: self.active_value.clone(),
            value: self.value.clone(),
            on_highlight: Some(on_highlight),
            on_select: Some(on_select),
            ..props.options.clone()
        })
    }
}

fn controlled(initial_active_value: Option<&str>, initial_value: Option<&str>, options: Options) -> RenderResult {
    let props = ControlledProps { initial_active_value: initial_active_value.map(str::to_string), initial_value: initial_value.map(str::to_string), options };
    let ControlledProps { initial_active_value, initial_value, options } = props;
    render(rsx! { <Controlled initial_active_value={initial_active_value} initial_value={initial_value} options={options}/> })
}

fn list(t: &RenderResult) -> web_sys::HtmlElement {
    t.query("[role=\"listbox\"]").expect("a listbox")
}

fn option_by_text(t: &RenderResult, text: &str) -> web_sys::HtmlElement {
    t.query_all("[role=\"option\"]").into_iter().find(|o| o.text_content().unwrap_or_default().trim() == text).unwrap_or_else(|| panic!("option {text}"))
}

fn highlighted(el: &web_sys::HtmlElement) -> Option<String> {
    el.get_attribute("data-highlighted")
}

fn selected(el: &web_sys::HtmlElement) -> Option<String> {
    el.get_attribute("aria-selected")
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

fn click(target: &web_sys::HtmlElement) {
    let init = web_sys::MouseEventInit::new();
    init.set_bubbles(true);
    init.set_button(0);
    target.dispatch_event(&web_sys::MouseEvent::new_with_mouse_event_init_dict("click", &init).unwrap()).unwrap();
}

fn mouse(target: &web_sys::HtmlElement, kind: &str) {
    let init = web_sys::MouseEventInit::new();
    init.set_bubbles(true);
    target.dispatch_event(&web_sys::MouseEvent::new_with_mouse_event_init_dict(kind, &init).unwrap()).unwrap();
}

/// Remix's `settle`: a microtask and a flush, twice.
async fn settle(t: &RenderResult) {
    for _ in 0..2 {
        JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await.unwrap();
        t.act(|| {});
    }
}

async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

/// Remix's `stubScrollIntoView`: an own `scrollIntoView` that records its options as
/// `"block,inline"`.
fn stub_scroll_into_view(el: &web_sys::HtmlElement) -> js_sys::Array {
    let calls = js_sys::Array::new();
    let stub = js_sys::Function::new_with_args("el, calls", "Object.defineProperty(el, 'scrollIntoView', { configurable: true, value: (o) => calls.push(o.block + ',' + o.inline) })");
    stub.call2(&JsValue::NULL, el, &calls).unwrap();
    calls
}

fn refs() -> (Rc<RefCell<Vec<Rc<ListboxState>>>>, OnRef) {
    let refs: Rc<RefCell<Vec<Rc<ListboxState>>>> = Rc::default();
    let r = refs.clone();
    (refs, Rc::new(move |state| r.borrow_mut().push(state)))
}

fn some(label_value: (&str, &str), el: &web_sys::HtmlElement) -> Option<(String, String, String)> {
    Some((el.id(), label_value.0.to_string(), label_value.1.to_string()))
}

// listbox.test.tsx: renders listbox roles, generated option ids, and the list tabIndex contract
#[wasm_bindgen_test]
async fn renders_listbox_roles_generated_option_ids_and_the_list_tab_index_contract() {
    let first = render(listbox_node(Options::default()));
    let second = render(listbox_node(Options { tab_index: Some(0), ..Default::default() }));
    settle(&first).await;
    settle(&second).await;
    let (first_list, remix, react_router) = (list(&first), option_by_text(&first, "Remix"), option_by_text(&first, "React Router"));

    assert_eq!(first_list.get_attribute("role").as_deref(), Some("listbox"));
    assert_eq!(first_list.tab_index(), -1);
    assert_eq!(list(&second).tab_index(), 0);
    assert_eq!(remix.get_attribute("role").as_deref(), Some("option"));
    assert_ne!(remix.id(), "");
    assert_eq!(selected(&remix).as_deref(), Some("false"));
    assert_eq!(highlighted(&remix).as_deref(), Some("false"));
    assert_eq!(react_router.get_attribute("aria-disabled").as_deref(), Some("true"));
}

// listbox.test.tsx: calls onHighlight without changing DOM until the parent rerenders
#[wasm_bindgen_test]
async fn calls_on_highlight_without_changing_dom_until_the_parent_rerenders() {
    let highlights = Calls::default();
    let t = render(listbox_node(Options { on_highlight: Some(recorder(&highlights)), ..Default::default() }));
    let remix = option_by_text(&t, "Remix");
    key(&list(&t), "ArrowDown");
    settle(&t).await;
    assert_eq!(*highlights.borrow(), [(Some("remix".to_string()), some(("Remix", "remix"), &remix))]);
    assert_eq!(highlighted(&remix).as_deref(), Some("false"));
}

// listbox.test.tsx: calls onSelect without changing selection state until the parent rerenders
#[wasm_bindgen_test]
async fn calls_on_select_without_changing_selection_state_until_the_parent_rerenders() {
    let selections = Calls::default();
    let t = render(listbox_node(Options { active_value: Some("react".into()), on_select: Some(recorder(&selections)), ..Default::default() }));
    let react = option_by_text(&t, "React");
    key(&list(&t), "Enter");
    settle(&t).await;
    assert_eq!(*selections.borrow(), [(Some("react".to_string()), some(("React", "react"), &react))]);
    assert_eq!(selected(&react).as_deref(), Some("false"));
}

fn ref_option(option: Option<RegisteredOption>) -> Option<(String, String, String)> {
    describe(option.as_ref())
}

// listbox.test.tsx: calls ref once with a live object exposing active and selected options
#[wasm_bindgen_test]
async fn calls_ref_once_with_a_live_object_exposing_active_and_selected_options() {
    let (refs, on_ref) = refs();
    let t = controlled(Some("react"), Some("remix"), Options { on_ref: Some(on_ref), ..Default::default() });
    settle(&t).await;
    let (remix, react, preact) = (option_by_text(&t, "Remix"), option_by_text(&t, "React"), option_by_text(&t, "Preact"));
    assert_eq!(refs.borrow().len(), 1);
    assert_eq!(ref_option(refs.borrow()[0].active()), some(("React", "react"), &react));
    assert_eq!(ref_option(refs.borrow()[0].selected()), some(("Remix", "remix"), &remix));

    key(&list(&t), "End");
    settle(&t).await;
    key(&list(&t), "Enter");
    settle(&t).await;

    assert_eq!(refs.borrow().len(), 1);
    assert_eq!(ref_option(refs.borrow()[0].active()), some(("Preact", "preact"), &preact));
    assert_eq!(ref_option(refs.borrow()[0].selected()), some(("Preact", "preact"), &preact));
}

// listbox.test.tsx: returns undefined ref values when the current values do not match an option
#[wasm_bindgen_test]
async fn returns_no_ref_values_when_the_current_values_do_not_match_an_option() {
    let (refs, on_ref) = refs();
    let t = render(listbox_node(Options { active_value: Some("missing".into()), on_ref: Some(on_ref), ..Default::default() }));
    settle(&t).await;
    assert_eq!(refs.borrow().len(), 1);
    assert!(refs.borrow()[0].active().is_none());
    assert!(refs.borrow()[0].selected().is_none());
}

// listbox.test.tsx: ArrowDown and ArrowUp update the controlled highlight and skip disabled options
#[wasm_bindgen_test]
async fn arrow_down_and_arrow_up_update_the_controlled_highlight_and_skip_disabled_options() {
    let highlights = Calls::default();
    let t = controlled(None, None, Options { on_highlight: Some(recorder(&highlights)), ..Default::default() });
    for k in ["ArrowDown", "ArrowDown", "ArrowDown", "ArrowUp"] {
        key(&list(&t), k);
        settle(&t).await;
    }
    assert_eq!(values(&highlights), [Some("remix".into()), Some("react".into()), Some("preact".into()), Some("react".into())]);
    assert_eq!(highlighted(&option_by_text(&t, "React Router")).as_deref(), Some("false"));
    assert_eq!(highlighted(&option_by_text(&t, "React")).as_deref(), Some("true"));
}

// listbox.test.tsx: ArrowUp with no active option and Home and End target enabled boundary options
#[wasm_bindgen_test]
async fn arrow_up_with_no_active_option_and_home_and_end_target_enabled_boundary_options() {
    let highlights = Calls::default();
    let t = controlled(None, None, Options { on_highlight: Some(recorder(&highlights)), ..Default::default() });
    let (remix, preact) = (option_by_text(&t, "Remix"), option_by_text(&t, "Preact"));
    key(&list(&t), "ArrowUp");
    settle(&t).await;
    assert_eq!(highlighted(&preact).as_deref(), Some("true"));
    key(&list(&t), "Home");
    settle(&t).await;
    assert_eq!(highlighted(&remix).as_deref(), Some("true"));
    key(&list(&t), "End");
    settle(&t).await;
    assert_eq!(highlighted(&preact).as_deref(), Some("true"));
    assert_eq!(values(&highlights), [Some("preact".into()), Some("remix".into()), Some("preact".into())]);
}

// listbox.test.tsx: prevents default ArrowDown and ArrowUp handling
#[wasm_bindgen_test]
async fn prevents_default_arrow_down_and_arrow_up_handling() {
    let t = controlled(Some("react"), None, Options::default());
    let down = key(&list(&t), "ArrowDown");
    settle(&t).await;
    let up = key(&list(&t), "ArrowUp");
    settle(&t).await;
    assert!(down.default_prevented());
    assert!(up.default_prevented());
}

// listbox.test.tsx: scrolls the active option into view on focus and keyboard navigation
#[wasm_bindgen_test]
async fn scrolls_the_active_option_into_view_on_focus_and_keyboard_navigation() {
    let t = controlled(Some("react"), None, Options::default());
    let (react, preact) = (option_by_text(&t, "React"), option_by_text(&t, "Preact"));
    let (scroll_react, scroll_preact) = (stub_scroll_into_view(&react), stub_scroll_into_view(&preact));
    list(&t).focus().unwrap();
    settle(&t).await;
    assert!(scroll_react.iter().any(|c| c.as_string().as_deref() == Some("nearest,nearest")), "{:?}", scroll_react.to_vec());

    key(&list(&t), "ArrowDown");
    settle(&t).await;
    assert_eq!(highlighted(&preact).as_deref(), Some("true"));
    assert!(scroll_preact.iter().any(|c| c.as_string().as_deref() == Some("nearest,nearest")), "{:?}", scroll_preact.to_vec());
}

// listbox.test.tsx: Enter and Space select the active option and pass option metadata
#[wasm_bindgen_test]
async fn enter_and_space_select_the_active_option_and_pass_option_metadata() {
    let selections = Calls::default();
    let t = controlled(None, None, Options { on_select: Some(recorder(&selections)), ..Default::default() });
    let (react, preact) = (option_by_text(&t, "React"), option_by_text(&t, "Preact"));
    for k in ["ArrowDown", "ArrowDown", "Enter"] {
        key(&list(&t), k);
        settle(&t).await;
    }
    assert_eq!(*selections.borrow(), [(Some("react".to_string()), some(("React", "react"), &react))]);
    assert_eq!(selected(&react).as_deref(), Some("true"));

    for k in ["ArrowDown", " "] {
        key(&list(&t), k);
        settle(&t).await;
    }
    assert_eq!(selections.borrow().len(), 2);
    assert_eq!(selections.borrow()[1], (Some("preact".to_string()), some(("Preact", "preact"), &preact)));
    assert_eq!(selected(&preact).as_deref(), Some("true"));
    assert_eq!(selected(&react).as_deref(), Some("false"));
}

// listbox.test.tsx: Tab prevents default and highlights the first enabled option
#[wasm_bindgen_test]
async fn tab_prevents_default_and_highlights_the_first_enabled_option() {
    let highlights = Calls::default();
    let t = controlled(Some("preact"), None, Options { on_highlight: Some(recorder(&highlights)), ..Default::default() });
    let event = key(&list(&t), "Tab");
    settle(&t).await;
    assert!(event.default_prevented());
    assert_eq!(values(&highlights), [Some("remix".to_string())]);
    assert_eq!(highlighted(&option_by_text(&t, "Remix")).as_deref(), Some("true"));
}

// listbox.test.tsx: typeahead on the list highlights the next matching enabled option without selecting it
#[wasm_bindgen_test]
async fn typeahead_on_the_list_highlights_the_next_matching_enabled_option_without_selecting_it() {
    let highlights = Calls::default();
    let t = controlled(Some("remix"), Some("remix"), Options { on_highlight: Some(recorder(&highlights)), ..Default::default() });
    let (remix, react) = (option_by_text(&t, "Remix"), option_by_text(&t, "React"));
    key(&list(&t), "r");
    settle(&t).await;
    assert_eq!(*highlights.borrow(), [(Some("react".to_string()), some(("React", "react"), &react))]);
    assert_eq!(highlighted(&react).as_deref(), Some("true"));
    assert_eq!(selected(&remix).as_deref(), Some("true"));
    assert_eq!(selected(&react).as_deref(), Some("false"));
}

// listbox.test.tsx: typeahead on the list matches option textValue values without selecting
#[wasm_bindgen_test]
async fn typeahead_on_the_list_matches_option_text_value_values_without_selecting() {
    let highlights = Calls::default();
    let options = vec![ListboxOption::new("production", "Production"), ListboxOption::new("staging", "Staging").text_value(&["beta"]), ListboxOption::new("local", "Local")];
    let t = controlled(None, None, Options { on_highlight: Some(recorder(&highlights)), options: Some(options), ..Default::default() });
    let (production, staging) = (option_by_text(&t, "Production"), option_by_text(&t, "Staging"));
    key(&list(&t), "b");
    settle(&t).await;
    assert_eq!(values(&highlights), [Some("staging".to_string())]);
    assert_eq!(highlighted(&staging).as_deref(), Some("true"));
    assert_eq!(selected(&production).as_deref(), Some("false"));
    assert_eq!(selected(&staging).as_deref(), Some("false"));
}

// listbox.test.tsx: exposes matchSearchText on the ref and skips disabled options
#[wasm_bindgen_test]
async fn exposes_match_search_text_on_the_ref_and_skips_disabled_options() {
    let (refs, on_ref) = refs();
    let t = render(listbox_node(Options { on_ref: Some(on_ref), ..Default::default() }));
    settle(&t).await;
    let (remix, react) = (option_by_text(&t, "Remix"), option_by_text(&t, "React"));
    let state = refs.borrow()[0].clone();
    assert_eq!(refs.borrow().len(), 1);
    assert_eq!(ref_option(state.match_search_text("r", None)), some(("Remix", "remix"), &remix));
    assert_eq!(ref_option(state.match_search_text("r", Some("remix"))), some(("React", "react"), &react));
    assert!(state.match_search_text("zz", None).is_none());
}

// listbox.test.tsx: mousemove and mouseleave update the controlled highlight for enabled options
#[wasm_bindgen_test]
async fn mousemove_and_mouseleave_update_the_controlled_highlight_for_enabled_options() {
    let highlights = Calls::default();
    let t = controlled(None, None, Options { on_highlight: Some(recorder(&highlights)), ..Default::default() });
    let react = option_by_text(&t, "React");
    mouse(&react, "mousemove");
    settle(&t).await;
    assert_eq!(*highlights.borrow(), [(Some("react".to_string()), some(("React", "react"), &react))]);
    assert_eq!(highlighted(&react).as_deref(), Some("true"));

    mouse(&react, "mouseleave");
    settle(&t).await;
    assert_eq!(highlights.borrow().len(), 2);
    assert_eq!(highlights.borrow()[1], (None, None));
    assert_eq!(highlighted(&react).as_deref(), Some("false"));
}

// listbox.test.tsx: ignores mousemove and click interactions on disabled options
#[wasm_bindgen_test]
async fn ignores_mousemove_and_click_interactions_on_disabled_options() {
    let (highlights, selections) = (Calls::default(), Calls::default());
    let t = controlled(None, None, Options { on_highlight: Some(recorder(&highlights)), on_select: Some(recorder(&selections)), ..Default::default() });
    let react_router = option_by_text(&t, "React Router");
    mouse(&react_router, "mousemove");
    settle(&t).await;
    click(&react_router);
    settle(&t).await;
    assert!(highlights.borrow().is_empty());
    assert!(selections.borrow().is_empty());
    assert_eq!(highlighted(&react_router).as_deref(), Some("false"));
    assert_eq!(selected(&react_router).as_deref(), Some("false"));
}

// listbox.test.tsx: selects with click-only activation
#[wasm_bindgen_test]
async fn selects_with_click_only_activation() {
    let selections = Calls::default();
    let t = controlled(None, None, Options { on_select: Some(recorder(&selections)), ..Default::default() });
    let react = option_by_text(&t, "React");
    click(&react);
    settle(&t).await;
    assert_eq!(*selections.borrow(), [(Some("react".to_string()), some(("React", "react"), &react))]);
    assert_eq!(selected(&react).as_deref(), Some("true"));
}

// listbox.test.tsx: tracks hover with mousemove and selects the clicked option after moving between options
#[wasm_bindgen_test]
async fn tracks_hover_with_mousemove_and_selects_the_clicked_option_after_moving_between_options() {
    let (highlights, selections) = (Calls::default(), Calls::default());
    let t = controlled(None, None, Options { on_highlight: Some(recorder(&highlights)), on_select: Some(recorder(&selections)), ..Default::default() });
    let (react, preact) = (option_by_text(&t, "React"), option_by_text(&t, "Preact"));
    mouse(&react, "mousemove");
    settle(&t).await;
    mouse(&preact, "mousemove");
    settle(&t).await;
    click(&preact);
    settle(&t).await;
    assert_eq!(values(&highlights), [Some("react".to_string()), Some("preact".to_string())]);
    assert_eq!(*selections.borrow(), [(Some("preact".to_string()), some(("Preact", "preact"), &preact))]);
    assert_eq!(selected(&preact).as_deref(), Some("true"));
}

// listbox.test.tsx: delays onSelect during flashSelection and ignores interactions until the flash completes
#[wasm_bindgen_test]
async fn delays_on_select_during_flash_selection_and_ignores_interactions_until_the_flash_completes() {
    let (highlights, selections, settled) = (Calls::default(), Calls::default(), Calls::default());
    let record_settled = settled.clone();
    let on_select_settled: OnSelectSettled = Rc::new(move |value, option| {
        record_settled.borrow_mut().push((value, describe(option.as_ref())));
        Box::pin(async {})
    });
    let options = Options { flash_selection: true, on_highlight: Some(recorder(&highlights)), on_select: Some(recorder(&selections)), on_select_settled: Some(on_select_settled), ..Default::default() };
    let t = controlled(None, None, options);
    let (react, preact) = (option_by_text(&t, "React"), option_by_text(&t, "Preact"));
    for k in ["ArrowDown", "ArrowDown", "Enter"] {
        key(&list(&t), k);
        settle(&t).await;
    }
    assert_eq!(values(&selections), [Some("react".to_string())]);
    assert!(settled.borrow().is_empty());
    assert_eq!(react.get_attribute("data-listbox-flash").as_deref(), Some("true"));

    key(&list(&t), "End");
    mouse(&preact, "mousemove");
    settle(&t).await;
    assert_eq!(values(&highlights), [Some("remix".to_string()), Some("react".to_string())]);
    assert_eq!(highlighted(&preact).as_deref(), Some("false"));
    assert_eq!(selected(&preact).as_deref(), Some("false"));

    sleep(80).await;
    settle(&t).await;
    assert_eq!(*settled.borrow(), [(Some("react".to_string()), some(("React", "react"), &react))]);
    assert_eq!(react.get_attribute("data-listbox-flash"), None);
    assert_eq!(selected(&react).as_deref(), Some("true"));
}

// rsmix: the ref's `select` ignores disabled options and a second selection while the first
// is still settling (Remix's suite reaches neither path).
#[wasm_bindgen_test]
async fn ref_select_ignores_disabled_options_and_reentry_while_settling() {
    let (refs, on_ref) = refs();
    let selections = Calls::default();
    let t = controlled(None, None, Options { flash_selection: true, on_ref: Some(on_ref), on_select: Some(recorder(&selections)), ..Default::default() });
    settle(&t).await;
    let state = refs.borrow()[0].clone();
    state.select(Some("react-router")).await;
    assert!(selections.borrow().is_empty());

    let first = state.clone();
    wasm_bindgen_futures::spawn_local(async move { first.select(Some("react")).await });
    settle(&t).await;
    state.select(Some("preact")).await;
    assert_eq!(values(&selections), [Some("react".to_string())]);
    sleep(80).await;
    settle(&t).await;
    assert_eq!(selected(&option_by_text(&t, "React")).as_deref(), Some("true"));
}
