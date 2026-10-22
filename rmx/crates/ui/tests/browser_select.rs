//! Ports of Remix's `ui/src/select.test.tsx` (its two `it.todo` cases have no body). Remix
//! advances fake timers through the 60 ms flash and the label delay; here the tests wait
//! for them in real time, and the custom delay case uses 400 ms so the default 75 ms delay
//! and the custom one are told apart with wide margins (Remix checks 99 and 100 ms).
//! `scrollIntoView` is stubbed like Remix's. Run with
//! `pwsh ./test-browser.ps1 -Crate rmx-ui -Test browser_select`.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use rmx::test::{render, RenderResult};
use rmx::{on, rsx, Component, EventHandler, Handle, Mixin, Node};
use rmx_ui::anchor::{AnchorOptions, Placement};
use rmx_ui::listbox::ListboxOption;
use rmx_ui::popover::{self, SurfaceOptions};
use rmx_ui::select::{self, on_select_change, select_change, SelectChange, SelectState};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

/// Remix's `SelectLabel`: the displayed label from the select context.
struct SelectLabel;

impl Component for SelectLabel {
    type Props = ();

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        SelectLabel
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let label = h.context::<Rc<SelectState>>().map(|s| s.displayed_label()).unwrap_or_default();
        rsx! { <>{label}</> }
    }
}

/// Remix's `TestSelect` props.
#[derive(Default)]
struct Select {
    default_label: &'static str,
    default_value: Option<&'static str>,
    id: Option<&'static str>,
    label_swap_delay_ms: Option<u32>,
    mix: Option<Mixin>,
    name: Option<&'static str>,
}

impl Select {
    fn new(default_label: &'static str) -> Select {
        Select { default_label, ..Default::default() }
    }

    fn render(self, options: Vec<Node>) -> Node {
        let Select { default_label, default_value, id, label_swap_delay_ms, mix, name } = self;
        let (default_value, name_prop) = (default_value.map(str::to_string), name.map(str::to_string));
        let hidden = name.map(|_| rsx! { <input mix={select::hidden_input()}/> });
        rsx! {
            <select::Context default_label={default_label} default_value={default_value} label_swap_delay_ms={label_swap_delay_ms} name={name_prop}>
                <button id={id} type="button" mix={[Some(select::trigger()), mix]}><SelectLabel/></button>
                <popover::Context>
                    <div mix={select::popover()}>
                        <div mix={select::list()}>{options}</div>
                    </div>
                </popover::Context>
                {hidden}
            </select::Context>
        }
    }
}

/// Remix's `TestOption`: the option's text is `text`, or its label.
fn option(option: ListboxOption, text: Option<&str>) -> Node {
    let text = text.map_or_else(|| option.label.clone(), str::to_string);
    rsx! { <div mix={select::option(option)}>{text}</div> }
}

/// Remix's `renderSelect` options: Remix, a disabled React Router, React.
fn frameworks() -> Vec<Node> {
    vec![
        option(ListboxOption::new("remix", "Remix framework"), Some("Remix")),
        option(ListboxOption::new("react-router", "React Router framework").disabled(true), Some("React Router")),
        option(ListboxOption::new("react", "React framework"), Some("React")),
    ]
}

fn render_select(default_value: Option<&'static str>) -> RenderResult {
    stub_scroll_into_view();
    render(Select { default_value, ..Select::new("Select a framework") }.render(frameworks()))
}

fn js(body: &str) -> JsValue {
    js_sys::Function::new_no_args(body).call0(&JsValue::NULL).unwrap()
}

fn stub_scroll_into_view() {
    js("if (!window.__scrollIntoView) { window.__scrollIntoView = HTMLElement.prototype.scrollIntoView; HTMLElement.prototype.scrollIntoView = function () {} }");
}

fn trigger(t: &RenderResult) -> web_sys::HtmlElement {
    t.query("button").expect("a trigger")
}

fn surface(t: &RenderResult) -> web_sys::HtmlElement {
    t.query("[popover]").expect("a surface")
}

fn list(t: &RenderResult) -> web_sys::HtmlElement {
    t.query("[role=\"listbox\"]").expect("a listbox")
}

fn option_by_text(t: &RenderResult, text: &str) -> web_sys::HtmlElement {
    t.query_all("[role=\"option\"]").into_iter().find(|o| o.text_content().unwrap_or_default().trim() == text).unwrap_or_else(|| panic!("option {text}"))
}

fn text(el: &web_sys::HtmlElement) -> String {
    el.text_content().unwrap_or_default()
}

fn is_open(el: &web_sys::HtmlElement) -> bool {
    el.matches(":popover-open").unwrap()
}

fn is_active(el: &web_sys::HtmlElement) -> bool {
    web_sys::window().unwrap().document().unwrap().active_element().as_ref() == Some(el.unchecked_ref::<web_sys::Element>())
}

fn click(target: &web_sys::HtmlElement) {
    let init = web_sys::MouseEventInit::new();
    init.set_bubbles(true);
    target.dispatch_event(&web_sys::MouseEvent::new_with_mouse_event_init_dict("click", &init).unwrap()).unwrap();
}

fn key(target: &web_sys::HtmlElement, key: &str) {
    let init = web_sys::KeyboardEventInit::new();
    init.set_bubbles(true);
    init.set_key(key);
    target.dispatch_event(&web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap()).unwrap();
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

/// Remix's `openSelect`: click the trigger (or press `key` on it), then settle frames.
async fn open_select(t: &RenderResult, key_to_open: Option<&str>) {
    match key_to_open {
        Some(k) => key(&trigger(t), k),
        None => click(&trigger(t)),
    }
    settle_frames(t).await;
}

/// Wait (up to 2 s) until the trigger shows `label`.
async fn wait_for_label(t: &RenderResult, label: &str) {
    for _ in 0..200 {
        if text(&trigger(t)).contains(label) {
            return;
        }
        sleep(10).await;
        t.act(|| {});
    }
    panic!("the trigger never showed {label:?}: {:?}", text(&trigger(t)));
}

type Changes = Rc<RefCell<Vec<(SelectChange, String)>>>;

/// Record `rmx:select-change` events with their target's id.
fn record_changes(changes: &Changes) -> Mixin {
    let changes = changes.clone();
    on_select_change(EventHandler::new(move |event| {
        let target = js_sys::Reflect::get(event.raw(), &"target".into()).ok().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
        changes.borrow_mut().push((select_change(event).unwrap_or_default(), target.map(|t| t.id()).unwrap_or_default()));
    }))
}

// select.test.tsx: applies defaultValue to the selected option while keeping defaultLabel on
// the trigger and wiring aria-describedby
#[wasm_bindgen_test]
async fn applies_default_value_to_the_selected_option_while_keeping_default_label_on_the_trigger() {
    let t = render_select(Some("react"));
    settle_frames(&t).await;
    let react = option_by_text(&t, "React");
    assert!(text(&trigger(&t)).contains("Select a framework"));
    assert_eq!(trigger(&t).get_attribute("aria-describedby"), Some(react.id()));
    assert_eq!(react.get_attribute("aria-selected").as_deref(), Some("true"));
}

/// Remix's `Indirection`: options rendered by a nested component.
struct Indirection;

impl Component for Indirection {
    type Props = ();

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Indirection
    }

    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! {
            <>
                <div mix={select::option(ListboxOption::new("bug", "Bug"))}>"Bug"</div>
                <div mix={select::option(ListboxOption::new("feature", "Feature"))}>"Feature"</div>
            </>
        }
    }
}

// select.test.tsx: supports nested option components while keeping defaultLabel before
// selection settles
#[wasm_bindgen_test]
async fn supports_nested_option_components_while_keeping_default_label_before_selection_settles() {
    stub_scroll_into_view();
    let t = render(rsx! {
        <select::Context default_label="Select a type">
            <button type="button" mix={select::trigger()}><SelectLabel/></button>
            <popover::Context>
                <div mix={select::popover()}>
                    <div mix={select::list()}><Indirection/></div>
                </div>
            </popover::Context>
        </select::Context>
    });
    assert!(text(&trigger(&t)).contains("Select a type"));
    open_select(&t, None).await;

    let feature = option_by_text(&t, "Feature");
    click(&feature);
    settle(&t).await;
    assert_eq!(feature.get_attribute("data-select-flash").as_deref(), Some("true"));
    assert!(text(&trigger(&t)).contains("Select a type"));

    wait_for_label(&t, "Feature").await;
}

// select.test.tsx: commits the option label to the trigger after the flash, close transition,
// and label delay
#[wasm_bindgen_test]
async fn commits_the_option_label_after_the_flash_close_transition_and_label_delay() {
    let t = render_select(None);
    let surface = surface(&t);
    assert!(text(&trigger(&t)).contains("Select a framework"));
    assert!(!is_open(&surface));
    open_select(&t, None).await;

    let react = option_by_text(&t, "React");
    click(&react);
    settle(&t).await;
    assert_eq!(react.get_attribute("data-select-flash").as_deref(), Some("true"));
    assert!(text(&trigger(&t)).contains("Select a framework"));
    assert!(is_open(&surface));

    wait_for_label(&t, "React framework").await;
    assert!(!text(&trigger(&t)).contains("React Router framework"));
    assert_eq!(trigger(&t).get_attribute("aria-describedby"), Some(react.id()));
    assert!(!is_open(&surface));
}

// select.test.tsx: supports a custom label swap delay on the primitive context
#[wasm_bindgen_test]
async fn supports_a_custom_label_swap_delay_on_the_primitive_context() {
    stub_scroll_into_view();
    let options = vec![option(ListboxOption::new("remix", "Remix framework"), Some("Remix")), option(ListboxOption::new("react", "React framework"), Some("React"))];
    let t = render(Select { label_swap_delay_ms: Some(400), ..Select::new("Select a framework") }.render(options));
    let surface = surface(&t);
    open_select(&t, None).await;
    click(&option_by_text(&t, "React"));
    settle(&t).await;
    for _ in 0..100 {
        if !is_open(&surface) {
            break;
        }
        sleep(5).await;
        t.act(|| {});
    }
    assert!(!is_open(&surface), "closed after the flash");
    // The default 75 ms delay (plus the two-frame transition wait) would be over by now.
    sleep(200).await;
    t.act(|| {});
    assert!(text(&trigger(&t)).contains("Select a framework"), "{}", text(&trigger(&t)));
    wait_for_label(&t, "React framework").await;
}

// select.test.tsx: opens on click, syncs min-width, moves focus into the list, and restores
// focus on escape
#[wasm_bindgen_test]
async fn opens_on_click_syncs_min_width_moves_focus_into_the_list_and_restores_focus_on_escape() {
    let t = render_select(None);
    let (trigger, surface) = (trigger(&t), surface(&t));
    js_sys::Reflect::define_property(&trigger, &"offsetWidth".into(), &js("return { configurable: true, value: 212 }").unchecked_into()).unwrap();
    assert_eq!(surface.style().get_property_value("min-width").unwrap(), "");
    assert_eq!(trigger.get_attribute("aria-expanded").as_deref(), Some("false"));

    open_select(&t, None).await;
    let list = list(&t);
    assert_eq!(trigger.get_attribute("aria-haspopup").as_deref(), Some("listbox"));
    assert_eq!(trigger.get_attribute("aria-controls"), Some(list.id()));
    assert_eq!(trigger.get_attribute("aria-expanded").as_deref(), Some("true"));
    assert_eq!(surface.style().get_property_value("min-width").unwrap(), "212px");
    assert!(is_open(&surface));
    assert!(is_active(&list));

    key(&list, "Escape");
    settle_frames(&t).await;
    assert_eq!(trigger.get_attribute("aria-expanded").as_deref(), Some("false"));
    assert!(!is_open(&surface));
    assert!(is_active(&trigger));
}

/// Remix's `OuterPopoverWithSelect`.
struct OuterPopoverWithSelect {
    open: bool,
}

impl Component for OuterPopoverWithSelect {
    type Props = ();

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        OuterPopoverWithSelect { open: false }
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let open = h.on(|this, h| {
            this.open = true;
            h.update();
        });
        let handle = h.clone();
        let on_hide = move |_: &popover::HideRequest| {
            handle.with(|this, h| {
                this.open = false;
                h.update();
            });
        };
        let grouping = Select { default_value: Some("none"), id: Some("grouping"), mix: Some(popover::focus_on_show()), ..Select::new("No grouping") }.render(vec![
            option(ListboxOption::new("none", "No grouping"), None),
            option(ListboxOption::new("status", "Status"), None),
            option(ListboxOption::new("priority", "Priority"), None),
        ]);
        rsx! {
            <popover::Context>
                <button id="outer-trigger" mix={[popover::anchor(AnchorOptions::new(Placement::BottomStart)), popover::focus_on_hide(), on("click", open)]}>"Open filters"</button>
                <div id="outer-surface" mix={popover::surface(SurfaceOptions::new(self.open, on_hide))}>{grouping}</div>
            </popover::Context>
        }
    }
}

// select.test.tsx: lets an outer popover focus the select trigger with popover.focusOnShow()
#[wasm_bindgen_test]
async fn lets_an_outer_popover_focus_the_select_trigger_with_focus_on_show() {
    stub_scroll_into_view();
    let t = render(rsx! { <OuterPopoverWithSelect/> });
    let outer = t.query("#outer-trigger").unwrap();
    outer.focus().unwrap();
    click(&outer);
    settle_frames(&t).await;
    assert!(is_active(&t.query("#grouping").unwrap()));
}

// select.test.tsx: typeahead on the closed trigger selects a matching option immediately
#[wasm_bindgen_test]
async fn typeahead_on_the_closed_trigger_selects_a_matching_option_immediately() {
    stub_scroll_into_view();
    let changes = Changes::default();
    let t = render(rsx! { <div mix={record_changes(&changes)}>{Select { id: Some("framework"), ..Select::new("Select a framework") }.render(frameworks())}</div> });
    let (trigger, surface, remix) = (trigger(&t), surface(&t), option_by_text(&t, "Remix"));
    trigger.focus().unwrap();
    key(&trigger, "r");
    settle(&t).await;
    for _ in 0..20 {
        if !changes.borrow().is_empty() {
            break;
        }
        frame().await;
    }

    assert!(!is_open(&surface));
    assert!(text(&trigger).contains("Remix framework"));
    assert_eq!(trigger.get_attribute("aria-describedby"), Some(remix.id()));
    assert_eq!(remix.get_attribute("aria-selected").as_deref(), Some("true"));
    let changes = changes.borrow();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].0.value.as_deref(), Some("remix"));
    assert_eq!(changes[0].1, "framework", "dispatched from the trigger");
}

// select.test.tsx: typeahead on the closed trigger matches option textValue values
#[wasm_bindgen_test]
async fn typeahead_on_the_closed_trigger_matches_option_text_value_values() {
    stub_scroll_into_view();
    let t = render(Select::new("Select an environment").render(vec![
        option(ListboxOption::new("production", "Production environment"), Some("Production")),
        option(ListboxOption::new("staging", "Staging environment").text_value(&["beta"]), Some("Staging")),
        option(ListboxOption::new("local", "Local environment").text_value(&["dev"]), Some("Local")),
    ]));
    let (trigger, staging) = (trigger(&t), option_by_text(&t, "Staging"));
    trigger.focus().unwrap();
    key(&trigger, "b");
    settle(&t).await;
    assert!(text(&trigger).contains("Staging environment"));
    assert_eq!(trigger.get_attribute("aria-describedby"), Some(staging.id()));
    assert_eq!(staging.get_attribute("aria-selected").as_deref(), Some("true"));
}

// select.test.tsx: ArrowDown and ArrowUp reopen from the selected value
#[wasm_bindgen_test]
async fn arrow_down_and_arrow_up_reopen_from_the_selected_value() {
    stub_scroll_into_view();
    let t = render(Select { default_value: Some("staging"), ..Select::new("Select an environment") }.render(vec![
        option(ListboxOption::new("local", "Local"), None),
        option(ListboxOption::new("staging", "Staging"), None),
        option(ListboxOption::new("production", "Production"), None),
    ]));
    settle(&t).await;
    open_select(&t, Some("ArrowDown")).await;
    assert!(is_active(&list(&t)));
    assert_eq!(option_by_text(&t, "Staging").get_attribute("data-highlighted").as_deref(), Some("true"));

    key(&list(&t), "Escape");
    settle_frames(&t).await;
    assert!(is_active(&trigger(&t)));

    open_select(&t, Some("ArrowUp")).await;
    assert!(is_active(&list(&t)));
    assert_eq!(option_by_text(&t, "Staging").get_attribute("data-highlighted").as_deref(), Some("true"));
}

// select.test.tsx: participates in formdata with hidden input
#[wasm_bindgen_test]
async fn participates_in_formdata_with_hidden_input() {
    stub_scroll_into_view();
    let t = render(rsx! { <form>{Select { name: Some("framework"), ..Select::new("Select a framework") }.render(frameworks())}</form> });
    let form: web_sys::HtmlFormElement = t.query("form").unwrap().unchecked_into();
    let hidden: web_sys::HtmlInputElement = t.query("input[type=\"hidden\"]").expect("a hidden input").unchecked_into();
    assert_eq!(hidden.name(), "framework");
    assert_eq!(hidden.value(), "");

    open_select(&t, None).await;
    click(&option_by_text(&t, "React"));
    settle(&t).await;
    wait_for_label(&t, "React framework").await;

    let data = web_sys::FormData::new_with_form(&form).unwrap();
    assert_eq!(hidden.value(), "react");
    assert_eq!(data.get("framework").as_string().as_deref(), Some("react"));
}

// select.test.tsx: dispatches change event
#[wasm_bindgen_test]
async fn dispatches_change_event() {
    stub_scroll_into_view();
    let changes = Changes::default();
    let t = render(rsx! { <div mix={record_changes(&changes)}>{Select { id: Some("framework"), ..Select::new("Select a framework") }.render(frameworks())}</div> });
    open_select(&t, None).await;
    let react = option_by_text(&t, "React");
    click(&react);
    settle(&t).await;
    wait_for_label(&t, "React framework").await;
    for _ in 0..20 {
        if !changes.borrow().is_empty() {
            break;
        }
        frame().await;
    }

    let changes = changes.borrow();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].0, SelectChange { label: Some("React framework".into()), option_id: Some(react.id()), value: Some("react".into()) });
    assert_eq!(changes[0].1, "framework", "dispatched from the trigger");
}
