//! Ports of Remix's `ui/src/menu.test.tsx`. Remix advances fake timers through the 60 ms
//! selection flash and waits out the 300 ms submenu delay in real time; here both are real
//! waits. Remix's `mock.method(window, 'setTimeout')` spies are a JS wrapper around
//! `window.setTimeout` (rmx's timers go through it), and `scrollIntoView` is stubbed like Remix's.
//! Run with `pwsh ./scripts/test-browser.ps1 -Crate rmx-ui -Test browser_menu`.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use rmx::test::{render, RenderResult};
use rmx::{node_ref, rsx, EventHandler, Mixin, Node};
use rmx_ui::menu::{self, ItemOptions, ItemType, MenuSelectItem, SubmenuTriggerOptions};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

type Selections = Rc<RefCell<Vec<MenuSelectItem>>>;

fn recorder(selections: &Selections) -> Mixin {
    let selections = selections.clone();
    menu::on_menu_select(EventHandler::new(move |event| {
        if let Some(item) = menu::menu_select(event) {
            selections.borrow_mut().push(item);
        }
    }))
}

/// Remix's `TestMenu`: a trigger button whose list re-dispatches selections from the button
/// (stopping the original), so ancestors of both see each selection once.
fn test_menu(label: &str, mix: Option<Mixin>, children: Vec<Node>) -> Node {
    let button: Rc<RefCell<Option<web_sys::Element>>> = Rc::default();
    let keep = button.clone();
    let button_ref = node_ref(move |node, signal| {
        let el = node.element().clone();
        *keep.borrow_mut() = Some(el.clone());
        let keep = keep.clone();
        signal.on_abort(move || {
            if keep.borrow().as_ref() == Some(&el) {
                *keep.borrow_mut() = None;
            }
        });
    });
    let redispatch = menu::on_menu_select(EventHandler::new(move |event| {
        let Some(button) = button.borrow().clone() else { return };
        event.stop_propagation();
        if let Some(item) = menu::menu_select(event) {
            menu::dispatch_menu_select(&button, &item);
        }
    }));
    rsx! {
        <menu::Context>
            <button type="button" mix={[Some(menu::trigger()), Some(button_ref), mix]}>{label.to_string()}</button>
            {test_menu_list(Some(redispatch), children)}
        </menu::Context>
    }
}

/// Remix's `TestMenuList`: the popover surface around the list.
fn test_menu_list(mix: Option<Mixin>, children: Vec<Node>) -> Node {
    rsx! {
        <div mix={menu::popover()}>
            <div mix={[Some(menu::list()), mix]}>{children}</div>
        </div>
    }
}

/// Remix's `TestMenuItem`.
fn test_item(options: ItemOptions, text: &str) -> Node {
    rsx! { <div mix={menu::item(options)}>{text.to_string()}</div> }
}

/// Remix's `TestSubmenu`: a nested context whose trigger is the label.
fn test_submenu(label: &str, children: Vec<Node>) -> Node {
    rsx! {
        <menu::Context>
            <div mix={menu::submenu_trigger(SubmenuTriggerOptions::default())}>{label.to_string()}</div>
            {test_menu_list(None, children)}
        </menu::Context>
    }
}

fn separator() -> Node {
    rsx! { <div role="separator"></div> }
}

fn zoom_submenu() -> Node {
    test_submenu("Zoom", vec![test_item(ItemOptions::new("zoomIn").value("zoom-in"), "Zoom in"), test_item(ItemOptions::new("zoomOut").value("zoom-out"), "Zoom out")])
}

fn render_app(node: Node) -> RenderResult {
    stub_scroll_into_view();
    render(node)
}

/// Remix's `renderMenu`: View with a checkbox item, a disabled item and the Zoom submenu,
/// inside an ancestor that also listens for selections.
fn render_menu_with(on_select: Option<Mixin>, on_ancestor_select: Option<Mixin>) -> RenderResult {
    let view = test_menu(
        "View",
        on_select,
        vec![test_item(ItemOptions::new("wordWrap").checkbox(false), "Word wrap"), test_item(ItemOptions::new("minimap").disabled(true), "Minimap"), zoom_submenu()],
    );
    render_app(rsx! { <div mix={on_ancestor_select}>{view}</div> })
}

fn render_menu() -> RenderResult {
    render_menu_with(None, None)
}

fn render_menu_with_separator() -> RenderResult {
    render_app(test_menu("View", None, vec![test_item(ItemOptions::new("newFile"), "New File"), separator(), test_item(ItemOptions::new("openFile"), "Open File")]))
}

fn render_menu_with_submenu_separator() -> RenderResult {
    render_app(test_menu("View", None, vec![separator(), zoom_submenu(), test_item(ItemOptions::new("wordWrap"), "Word wrap")]))
}

fn render_context_menu() -> RenderResult {
    let list = test_menu_list(None, vec![test_item(ItemOptions::new("rename"), "Rename"), test_item(ItemOptions::new("delete"), "Delete")]);
    render_app(rsx! {
        <menu::Context label="File actions">
            <div id="context-target" tabindex="0" mix={menu::context_trigger()}>"File.txt"</div>
            {list}
        </menu::Context>
    })
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

/// Remix's `t.mock.method(window, 'setTimeout')`: record each delay until [`timeout_delays`].
fn spy_set_timeout() {
    js("window.__timeouts = []; const original = window.setTimeout; window.__setTimeout = original; \
        window.setTimeout = function (f, delay, ...rest) { window.__timeouts.push(delay); return original.call(window, f, delay, ...rest) };");
}

/// The positive delays recorded since [`spy_set_timeout`], restoring `window.setTimeout`.
fn timeout_delays() -> Vec<f64> {
    let delays = js("const d = window.__timeouts; window.setTimeout = window.__setTimeout; delete window.__timeouts; delete window.__setTimeout; return d");
    js_sys::Array::from(&delays).iter().filter_map(|d| d.as_f64()).filter(|d| *d > 0.0).collect()
}

fn text(el: &web_sys::HtmlElement) -> String {
    el.text_content().unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn button(t: &RenderResult, label: &str) -> web_sys::HtmlElement {
    t.query_all("button").into_iter().find(|b| text(b) == label).unwrap_or_else(|| panic!("button {label}"))
}

fn item(t: &RenderResult, label: &str) -> web_sys::HtmlElement {
    t.query_all("[role^=\"menuitem\"]").into_iter().find(|i| text(i) == label).unwrap_or_else(|| panic!("item {label}"))
}

fn lists(t: &RenderResult) -> Vec<web_sys::HtmlElement> {
    t.query_all("[role=\"menu\"]")
}

fn surfaces(t: &RenderResult) -> Vec<web_sys::HtmlElement> {
    t.query_all("[popover]")
}

fn separator_el(t: &RenderResult) -> web_sys::HtmlElement {
    t.query("[role=\"separator\"]").expect("a separator")
}

fn context_target(t: &RenderResult) -> web_sys::HtmlElement {
    t.query("#context-target").expect("the context target")
}

fn is_open(el: &web_sys::HtmlElement) -> bool {
    el.matches(":popover-open").unwrap()
}

fn active() -> Option<web_sys::Element> {
    web_sys::window().unwrap().document().unwrap().active_element()
}

fn active_html() -> web_sys::HtmlElement {
    active().expect("an active element").unchecked_into()
}

fn is_active(el: &web_sys::HtmlElement) -> bool {
    active().as_ref() == Some(el.unchecked_ref::<web_sys::Element>())
}

fn attr(el: &web_sys::HtmlElement, name: &str) -> Option<String> {
    el.get_attribute(name)
}

fn click(target: &web_sys::HtmlElement) {
    let init = web_sys::MouseEventInit::new();
    init.set_bubbles(true);
    init.set_button(0);
    target.dispatch_event(&web_sys::MouseEvent::new_with_mouse_event_init_dict("click", &init).unwrap()).unwrap();
}

fn context_menu(target: &web_sys::HtmlElement, x: i32, y: i32) -> web_sys::MouseEvent {
    let init = web_sys::MouseEventInit::new();
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_client_x(x);
    init.set_client_y(y);
    let event = web_sys::MouseEvent::new_with_mouse_event_init_dict("contextmenu", &init).unwrap();
    target.dispatch_event(&event).unwrap();
    event
}

fn key_with(target: &web_sys::HtmlElement, key: &str, shift: bool) -> web_sys::KeyboardEvent {
    let init = web_sys::KeyboardEventInit::new();
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_key(key);
    init.set_shift_key(shift);
    let event = web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap();
    target.dispatch_event(&event).unwrap();
    event
}

fn key(target: &web_sys::HtmlElement, key: &str) -> web_sys::KeyboardEvent {
    key_with(target, key, false)
}

fn pointer_move(target: &web_sys::HtmlElement) {
    call("el", "el.dispatchEvent(new PointerEvent('pointermove', { bubbles: true }))", &[target]);
}

/// Remix's `pointerLeave`: a non-bubbling `pointerleave` with a forced `relatedTarget`.
fn pointer_leave(target: &web_sys::HtmlElement, related: Option<&web_sys::HtmlElement>, at: Option<(i32, i32)>) {
    let related = related.map_or(JsValue::NULL, |r| r.clone().into());
    let (x, y) = at.map_or((JsValue::UNDEFINED, JsValue::UNDEFINED), |(x, y)| (x.into(), y.into()));
    call(
        "el, related, x, y",
        "const init = x === undefined ? {} : { clientX: x, clientY: y }; const event = new PointerEvent('pointerleave', init); \
         Object.defineProperty(event, 'relatedTarget', { value: related }); el.dispatchEvent(event);",
        &[target, &related, &x, &y],
    );
}

/// Remix's `mockRect`: fixed offset sizes and bounding rect.
fn mock_rect(el: &web_sys::HtmlElement, left: f64, top: f64, width: f64, height: f64) {
    call(
        "el, left, top, width, height",
        "Object.defineProperty(el, 'offsetWidth', { configurable: true, get: () => width }); \
         Object.defineProperty(el, 'offsetHeight', { configurable: true, get: () => height }); \
         el.getBoundingClientRect = () => new DOMRect(left, top, width, height);",
        &[el, &left.into(), &top.into(), &width.into(), &height.into()],
    );
}

fn style(el: &web_sys::HtmlElement, property: &str) -> String {
    el.style().get_property_value(property).unwrap()
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

/// Remix's `finishSelectionFlash`: past the 60 ms flash, then settle.
async fn finish_selection_flash(t: &RenderResult) {
    sleep(80).await;
    settle(t).await;
}

/// Remix's `finishDismissal`: run and end the surface's opacity transition.
async fn finish_dismissal(surface: &web_sys::HtmlElement, t: &RenderResult) {
    microtask().await;
    call(
        "el",
        "el.dispatchEvent(new TransitionEvent('transitionrun', { propertyName: 'opacity' })); \
         el.dispatchEvent(new TransitionEvent('transitionend', { propertyName: 'opacity' }));",
        &[surface],
    );
    settle(t).await;
}

/// Open View from ArrowDown, move to Zoom, and open its submenu with ArrowRight.
async fn open_zoom_with_keyboard(t: &RenderResult) {
    key(&button(t, "View"), "ArrowDown");
    settle_frames(t).await;
    key(&active_html(), "ArrowDown");
    settle(t).await;
    key(&active_html(), "ArrowRight");
    settle_frames(t).await;
}

fn word_wrap_item(checked: bool) -> MenuSelectItem {
    MenuSelectItem { checked: Some(checked), label: "Word wrap".into(), name: "wordWrap".into(), item_type: ItemType::Checkbox, value: None, ..Default::default() }
}

/// The selection without its generated id (Remix's `toMatchObject`).
fn without_id(item: &MenuSelectItem) -> MenuSelectItem {
    MenuSelectItem { id: String::new(), ..item.clone() }
}

// menu.test.tsx: opens from click with focus on the list and toggles closed from the trigger
#[wasm_bindgen_test]
async fn opens_from_click_with_focus_on_the_list_and_toggles_closed_from_the_trigger() {
    let t = render_menu();
    let trigger = button(&t, "View");
    let (root_surface, root_list) = (surfaces(&t)[0].clone(), lists(&t)[0].clone());
    click(&trigger);
    settle_frames(&t).await;
    assert!(is_open(&root_surface));
    assert!(is_active(&root_list));

    click(&trigger);
    settle(&t).await;
    finish_dismissal(&root_surface, &t).await;
    assert!(!is_open(&root_surface));
    assert!(is_active(&trigger));
}

// menu.test.tsx: opens from contextmenu at the pointer coordinates
#[wasm_bindgen_test]
async fn opens_from_contextmenu_at_the_pointer_coordinates() {
    let t = render_context_menu();
    let trigger = context_target(&t);
    let (root_surface, root_list) = (surfaces(&t)[0].clone(), lists(&t)[0].clone());
    mock_rect(&root_surface, 0.0, 0.0, 160.0, 96.0);
    let event = context_menu(&trigger, 220, 140);
    settle_frames(&t).await;
    assert!(event.default_prevented());
    assert!(is_open(&root_surface));
    assert_eq!(style(&root_surface, "left"), "220px");
    assert_eq!(style(&root_surface, "top"), "140px");
    assert_eq!(attr(&root_surface, "data-anchor-placement").as_deref(), Some("bottom-start"));
    assert_eq!(attr(&trigger, "aria-expanded").as_deref(), Some("true"));
    assert!(is_active(&root_list));
}

// menu.test.tsx: repositions an open context menu when right-clicked again
#[wasm_bindgen_test]
async fn repositions_an_open_context_menu_when_right_clicked_again() {
    let t = render_context_menu();
    let trigger = context_target(&t);
    let root_surface = surfaces(&t)[0].clone();
    mock_rect(&root_surface, 0.0, 0.0, 160.0, 96.0);
    context_menu(&trigger, 220, 140);
    settle_frames(&t).await;
    assert_eq!(style(&root_surface, "left"), "220px");
    assert_eq!(style(&root_surface, "top"), "140px");

    context_menu(&trigger, 260, 180);
    settle_frames(&t).await;
    assert!(is_open(&root_surface));
    assert_eq!(style(&root_surface, "left"), "260px");
    assert_eq!(style(&root_surface, "top"), "180px");
}

// menu.test.tsx: opens a context menu from the keyboard at the trigger bounds
#[wasm_bindgen_test]
async fn opens_a_context_menu_from_the_keyboard_at_the_trigger_bounds() {
    let t = render_context_menu();
    let trigger = context_target(&t);
    let (root_surface, root_list) = (surfaces(&t)[0].clone(), lists(&t)[0].clone());
    mock_rect(&trigger, 80.0, 70.0, 200.0, 40.0);
    mock_rect(&root_surface, 0.0, 0.0, 160.0, 96.0);
    let event = key_with(&trigger, "F10", true);
    settle_frames(&t).await;
    assert!(event.default_prevented());
    assert!(is_open(&root_surface));
    assert_eq!(style(&root_surface, "left"), "80px");
    assert_eq!(style(&root_surface, "top"), "110px");
    assert!(is_active(&root_list));
}

// menu.test.tsx: opens from ArrowDown and skips disabled items during keyboard navigation
#[wasm_bindgen_test]
async fn opens_from_arrow_down_and_skips_disabled_items_during_keyboard_navigation() {
    let t = render_menu();
    let (word_wrap, zoom) = (item(&t, "Word wrap"), item(&t, "Zoom"));
    key(&button(&t, "View"), "ArrowDown");
    settle_frames(&t).await;
    assert!(is_active(&word_wrap));

    key(&word_wrap, "ArrowDown");
    settle(&t).await;
    assert!(is_active(&zoom));
    assert_eq!(attr(&zoom, "data-highlighted").as_deref(), Some("true"));
}

// menu.test.tsx: does not wrap keyboard navigation at the first or last enabled item
#[wasm_bindgen_test]
async fn does_not_wrap_keyboard_navigation_at_the_first_or_last_enabled_item() {
    let t = render_menu();
    let (word_wrap, zoom) = (item(&t, "Word wrap"), item(&t, "Zoom"));
    key(&button(&t, "View"), "ArrowDown");
    settle_frames(&t).await;
    assert!(is_active(&word_wrap));

    key(&word_wrap, "ArrowUp");
    settle(&t).await;
    assert!(is_active(&word_wrap));
    assert_eq!(attr(&word_wrap, "data-highlighted").as_deref(), Some("true"));

    key(&word_wrap, "ArrowDown");
    settle(&t).await;
    assert!(is_active(&zoom));

    key(&zoom, "ArrowDown");
    settle(&t).await;
    assert!(is_active(&zoom));
    assert_eq!(attr(&zoom, "data-highlighted").as_deref(), Some("true"));
}

// menu.test.tsx: opens submenus with ArrowRight and restores focus with ArrowLeft
#[wasm_bindgen_test]
async fn opens_submenus_with_arrow_right_and_restores_focus_with_arrow_left() {
    let t = render_menu();
    let (zoom, zoom_in, child_surface) = (item(&t, "Zoom"), item(&t, "Zoom in"), surfaces(&t)[1].clone());
    key(&button(&t, "View"), "ArrowDown");
    settle_frames(&t).await;
    key(&active_html(), "ArrowDown");
    settle(&t).await;
    assert!(is_active(&zoom));

    key(&zoom, "ArrowRight");
    settle_frames(&t).await;
    assert!(is_open(&child_surface));
    assert!(is_active(&zoom_in));

    key(&zoom_in, "ArrowLeft");
    settle(&t).await;
    assert_eq!(attr(&child_surface, "data-close-animation").as_deref(), Some("none"));
    assert!(!is_open(&child_surface));
    assert_eq!(attr(&zoom, "data-submenu-state"), None);
    assert_eq!(attr(&zoom, "data-highlighted").as_deref(), Some("true"));
    assert!(is_active(&zoom));
}

// menu.test.tsx: does not reopen a submenu after ArrowLeft restores focus to its trigger
#[wasm_bindgen_test]
async fn does_not_reopen_a_submenu_after_arrow_left_restores_focus_to_its_trigger() {
    let t = render_menu();
    let (zoom, zoom_in, child_surface) = (item(&t, "Zoom"), item(&t, "Zoom in"), surfaces(&t)[1].clone());
    key(&button(&t, "View"), "ArrowDown");
    settle_frames(&t).await;
    key(&active_html(), "ArrowDown");
    settle(&t).await;
    key(&zoom, "ArrowRight");
    settle_frames(&t).await;

    spy_set_timeout();
    key(&zoom_in, "ArrowLeft");
    settle(&t).await;
    assert_eq!(timeout_delays(), Vec::<f64>::new());
    assert!(!is_open(&child_surface));
    assert_eq!(attr(&zoom, "data-highlighted").as_deref(), Some("true"));
    assert_eq!(attr(&zoom, "aria-expanded").as_deref(), Some("false"));
    assert!(is_active(&zoom));
}

// menu.test.tsx: anchors submenus relative to their first item
#[wasm_bindgen_test]
async fn anchors_submenus_relative_to_their_first_item() {
    let t = render_menu();
    let (zoom, zoom_in, child_surface) = (item(&t, "Zoom"), item(&t, "Zoom in"), surfaces(&t)[1].clone());
    mock_rect(&zoom, 40.0, 40.0, 100.0, 32.0);
    mock_rect(&child_surface, 160.0, 100.0, 180.0, 120.0);
    mock_rect(&zoom_in, 160.0, 104.0, 128.0, 32.0);
    key(&button(&t, "View"), "ArrowDown");
    settle_frames(&t).await;
    key(&active_html(), "ArrowDown");
    settle(&t).await;
    key(&zoom, "ArrowRight");
    settle_frames(&t).await;
    assert_eq!(style(&child_surface, "left"), "140px");
    assert_eq!(style(&child_surface, "top"), "36px");
    assert_eq!(attr(&child_surface, "data-anchor-placement").as_deref(), Some("right-start"));
}

// menu.test.tsx: opens a submenu immediately when its trigger is clicked without moving focus
#[wasm_bindgen_test]
async fn opens_a_submenu_immediately_when_its_trigger_is_clicked_without_moving_focus() {
    let t = render_menu();
    let (zoom, child_surface) = (item(&t, "Zoom"), surfaces(&t)[1].clone());
    key(&button(&t, "View"), "ArrowDown");
    settle_frames(&t).await;
    key(&active_html(), "ArrowDown");
    settle(&t).await;
    assert!(is_active(&zoom));

    click(&zoom);
    settle(&t).await;
    assert!(is_open(&child_surface));
    assert_eq!(attr(&zoom, "aria-expanded").as_deref(), Some("true"));
    assert!(is_active(&zoom));
}

// menu.test.tsx: opens submenus after the focus delay without throwing
#[wasm_bindgen_test]
async fn opens_submenus_after_the_focus_delay_without_throwing() {
    let t = render_menu();
    let (zoom, child_surface) = (item(&t, "Zoom"), surfaces(&t)[1].clone());
    spy_set_timeout();
    key(&button(&t, "View"), "ArrowDown");
    settle_frames(&t).await;
    key(&active_html(), "ArrowDown");
    settle(&t).await;
    assert!(is_active(&zoom));

    let delay = *timeout_delays().first().expect("a submenu delay");
    sleep(delay as i32 + 10).await;
    settle_frames(&t).await;
    assert!(is_open(&child_surface));
    assert!(is_active(&zoom));
}

// menu.test.tsx: does not reschedule submenu hover-open after focus moves to the trigger
#[wasm_bindgen_test]
async fn does_not_reschedule_submenu_hover_open_after_focus_moves_to_the_trigger() {
    let t = render_menu();
    let zoom = item(&t, "Zoom");
    click(&button(&t, "View"));
    settle_frames(&t).await;

    spy_set_timeout();
    pointer_move(&zoom);
    settle(&t).await;
    assert_eq!(timeout_delays().len(), 1);
}

/// The two "reopens submenu hover-open" cases: hover Zoom, leave it (for `leave_to` or
/// left of it), then hover it again and wait out the delay.
async fn reopens_hover_open_after_leaving(t: &RenderResult, leave_to: Option<&web_sys::HtmlElement>) {
    let (zoom, child_surface) = (item(t, "Zoom"), surfaces(t)[1].clone());
    click(&button(t, "View"));
    settle_frames(t).await;

    spy_set_timeout();
    pointer_move(&zoom);
    settle(t).await;
    let delay = *timeout_delays().first().expect("a submenu delay");
    assert!(is_active(&zoom));

    match leave_to {
        Some(target) => pointer_leave(&zoom, Some(target), None),
        None => pointer_leave(&zoom, None, Some((39, 56))),
    }
    settle(t).await;
    assert_eq!(attr(&zoom, "data-highlighted"), None);
    assert!(is_active(&zoom));
    assert!(!is_open(&child_surface));

    pointer_move(&zoom);
    settle(t).await;
    sleep(delay as i32 + 10).await;
    settle_frames(t).await;
    assert_eq!(attr(&zoom, "data-highlighted").as_deref(), Some("true"));
    assert!(is_open(&child_surface));
    assert!(is_active(&zoom));
}

// menu.test.tsx: reopens submenu hover-open when the pointer re-enters a focused trigger from the left
#[wasm_bindgen_test]
async fn reopens_submenu_hover_open_when_the_pointer_re_enters_a_focused_trigger_from_the_left() {
    let t = render_menu();
    reopens_hover_open_after_leaving(&t, None).await;
}

// menu.test.tsx: reopens submenu hover-open when the pointer re-enters a focused trigger from a separator
#[wasm_bindgen_test]
async fn reopens_submenu_hover_open_when_the_pointer_re_enters_a_focused_trigger_from_a_separator() {
    let t = render_menu_with_submenu_separator();
    let separator = separator_el(&t);
    reopens_hover_open_after_leaving(&t, Some(&separator)).await;
}

// menu.test.tsx: clears an open submenu trigger when pointer leaves away from the child surface
#[wasm_bindgen_test]
async fn clears_an_open_submenu_trigger_when_pointer_leaves_away_from_the_child_surface() {
    let t = render_menu();
    let (zoom, child_surface) = (item(&t, "Zoom"), surfaces(&t)[1].clone());
    open_zoom_with_keyboard(&t).await;
    mock_rect(&zoom, 40.0, 40.0, 100.0, 32.0);
    mock_rect(&child_surface, 160.0, 40.0, 120.0, 120.0);
    assert_eq!(attr(&zoom, "data-highlighted").as_deref(), Some("true"));
    assert!(is_open(&child_surface));

    pointer_leave(&zoom, None, Some((39, 56)));
    settle(&t).await;
    assert_eq!(attr(&zoom, "data-highlighted"), None);
    assert!(!is_open(&child_surface));
}

// menu.test.tsx: keeps an open submenu trigger active when pointer leaves straight into the child surface
#[wasm_bindgen_test]
async fn keeps_an_open_submenu_trigger_active_when_pointer_leaves_straight_into_the_child_surface() {
    let t = render_menu();
    let (zoom, child_surface) = (item(&t, "Zoom"), surfaces(&t)[1].clone());
    open_zoom_with_keyboard(&t).await;
    mock_rect(&zoom, 40.0, 40.0, 100.0, 32.0);
    mock_rect(&child_surface, 140.0, 40.0, 120.0, 120.0);
    assert_eq!(attr(&zoom, "data-highlighted").as_deref(), Some("true"));
    assert!(is_open(&child_surface));

    pointer_leave(&zoom, Some(&child_surface), Some((140, 56)));
    settle(&t).await;
    assert_eq!(attr(&zoom, "data-highlighted").as_deref(), Some("true"));
    assert!(is_open(&child_surface));
}

// menu.test.tsx: keeps the parent submenu trigger highlighted when ArrowLeft closes a child under the pointer
#[wasm_bindgen_test]
async fn keeps_the_parent_submenu_trigger_highlighted_when_arrow_left_closes_a_child_under_the_pointer() {
    let t = render_menu();
    let (zoom, zoom_out, child_surface) = (item(&t, "Zoom"), item(&t, "Zoom out"), surfaces(&t)[1].clone());
    key(&button(&t, "View"), "ArrowDown");
    settle_frames(&t).await;
    key(&active_html(), "ArrowDown");
    settle(&t).await;
    key(&zoom, "ArrowRight");
    settle_frames(&t).await;
    mock_rect(&zoom, 40.0, 40.0, 100.0, 32.0);
    mock_rect(&child_surface, 140.0, 40.0, 120.0, 120.0);

    pointer_leave(&zoom, Some(&child_surface), Some((140, 56)));
    settle(&t).await;
    pointer_move(&zoom_out);
    settle(&t).await;
    assert!(is_active(&zoom_out));

    key(&zoom_out, "ArrowLeft");
    settle(&t).await;
    assert!(!is_open(&child_surface));
    assert_eq!(attr(&zoom, "data-highlighted").as_deref(), Some("true"));
    assert!(is_active(&zoom));
}

// menu.test.tsx: does not let non-active items clear an open submenu trigger on pointerleave
#[wasm_bindgen_test]
async fn does_not_let_non_active_items_clear_an_open_submenu_trigger_on_pointerleave() {
    let t = render_menu();
    let (word_wrap, zoom, child_surface) = (item(&t, "Word wrap"), item(&t, "Zoom"), surfaces(&t)[1].clone());
    open_zoom_with_keyboard(&t).await;
    assert_eq!(attr(&zoom, "data-highlighted").as_deref(), Some("true"));
    assert!(is_open(&child_surface));

    pointer_leave(&word_wrap, Some(&child_surface), None);
    settle(&t).await;
    assert_eq!(attr(&zoom, "data-highlighted").as_deref(), Some("true"));
    assert!(is_open(&child_surface));
}

// menu.test.tsx: unhighlights items when pointer leaves them for a separator
#[wasm_bindgen_test]
async fn unhighlights_items_when_pointer_leaves_them_for_a_separator() {
    let t = render_menu_with_separator();
    let (new_file, separator) = (item(&t, "New File"), separator_el(&t));
    click(&button(&t, "View"));
    settle_frames(&t).await;
    pointer_move(&new_file);
    settle(&t).await;
    assert_eq!(attr(&new_file, "data-highlighted").as_deref(), Some("true"));

    pointer_leave(&new_file, Some(&separator), None);
    settle(&t).await;
    assert_eq!(attr(&new_file, "data-highlighted"), None);
}

// menu.test.tsx: marks submenu triggers as dismissing while the full menu tree fades out
#[wasm_bindgen_test]
async fn marks_submenu_triggers_as_dismissing_while_the_full_menu_tree_fades_out() {
    let t = render_menu();
    let (trigger, zoom, zoom_in, root_surface) = (button(&t, "View"), item(&t, "Zoom"), item(&t, "Zoom in"), surfaces(&t)[0].clone());
    open_zoom_with_keyboard(&t).await;

    key(&zoom_in, "Escape");
    settle(&t).await;
    assert_eq!(attr(&zoom, "aria-expanded").as_deref(), Some("false"));
    assert_eq!(attr(&zoom, "data-submenu-state").as_deref(), Some("dismissing"));

    finish_dismissal(&root_surface, &t).await;
    assert_eq!(attr(&zoom, "data-submenu-state"), None);
    assert!(!is_open(&root_surface));
    assert!(is_active(&trigger));
}

// menu.test.tsx: dispatches one bubbled selection event from submenu items and closes the full tree
#[wasm_bindgen_test]
async fn dispatches_one_bubbled_selection_event_from_submenu_items_and_closes_the_full_tree() {
    let selections = Selections::default();
    let t = render_menu_with(Some(recorder(&selections)), None);
    let (trigger, zoom, zoom_in) = (button(&t, "View"), item(&t, "Zoom"), item(&t, "Zoom in"));
    let (root_surface, child_surface) = (surfaces(&t)[0].clone(), surfaces(&t)[1].clone());
    open_zoom_with_keyboard(&t).await;

    key(&zoom_in, "Enter");
    settle(&t).await;
    assert_eq!(attr(&zoom_in, "data-menu-flash").as_deref(), Some("true"));
    assert_eq!(attr(&zoom_in, "aria-checked"), None);
    assert_eq!(attr(&zoom_in, "data-menu-selected"), None);
    assert_eq!(attr(&zoom, "data-menu-flash"), None);
    assert_eq!(attr(&zoom, "aria-expanded").as_deref(), Some("true"));

    finish_selection_flash(&t).await;
    assert_eq!(attr(&zoom, "aria-expanded").as_deref(), Some("false"));
    assert_eq!(attr(&zoom, "data-submenu-state").as_deref(), Some("selecting"));
    assert_eq!(attr(&zoom_in, "data-menu-flash"), None);
    assert_eq!(attr(&zoom_in, "data-menu-selected"), None);
    assert_eq!(attr(&zoom_in, "data-highlighted").as_deref(), Some("true"));
    assert_eq!(attr(&root_surface, "data-close-animation"), None);
    assert_eq!(attr(&child_surface, "data-close-animation"), None);

    finish_dismissal(&root_surface, &t).await;
    assert_eq!(selections.borrow().len(), 1);
    let selection = selections.borrow()[0].clone();
    assert_eq!((selection.label.as_str(), selection.name.as_str(), selection.item_type, selection.value.as_deref()), ("Zoom in", "zoomIn", ItemType::Item, Some("zoom-in")));
    assert_eq!(attr(&zoom, "data-submenu-state"), None);
    assert!(!is_open(&root_surface));
    assert!(!is_open(&child_surface));
    assert!(is_active(&trigger));
}

// menu.test.tsx: re-dispatches one selection event from the button so shared ancestors only see it once
#[wasm_bindgen_test]
async fn re_dispatches_one_selection_event_from_the_button_so_shared_ancestors_only_see_it_once() {
    let (button_selections, ancestor_selections) = (Selections::default(), Selections::default());
    let t = render_menu_with(Some(recorder(&button_selections)), Some(recorder(&ancestor_selections)));
    let root_surface = surfaces(&t)[0].clone();
    click(&button(&t, "View"));
    settle_frames(&t).await;

    click(&item(&t, "Word wrap"));
    settle(&t).await;
    finish_selection_flash(&t).await;
    finish_dismissal(&root_surface, &t).await;
    assert_eq!(button_selections.borrow().iter().map(without_id).collect::<Vec<_>>(), [word_wrap_item(true)]);
    assert_eq!(ancestor_selections.borrow().iter().map(without_id).collect::<Vec<_>>(), [word_wrap_item(true)]);
}

// menu.test.tsx: does not swallow parent item clicks while a child submenu is open
#[wasm_bindgen_test]
async fn does_not_swallow_parent_item_clicks_while_a_child_submenu_is_open() {
    let selections = Selections::default();
    let t = render_menu_with(Some(recorder(&selections)), None);
    let (trigger, word_wrap, child_surface) = (button(&t, "View"), item(&t, "Word wrap"), surfaces(&t)[1].clone());
    open_zoom_with_keyboard(&t).await;

    click(&word_wrap);
    settle(&t).await;
    assert_eq!(attr(&child_surface, "data-close-animation").as_deref(), Some("none"));
    assert!(!is_open(&child_surface));
    assert_eq!(attr(&word_wrap, "data-menu-flash").as_deref(), Some("true"));
    assert_eq!(attr(&word_wrap, "aria-checked").as_deref(), Some("true"));

    let root_surface = surfaces(&t)[0].clone();
    finish_selection_flash(&t).await;
    assert_eq!(attr(&word_wrap, "data-menu-flash"), None);
    assert_eq!(attr(&word_wrap, "aria-checked").as_deref(), Some("false"));
    assert_eq!(attr(&word_wrap, "data-highlighted").as_deref(), Some("true"));
    assert_eq!(attr(&root_surface, "data-close-animation"), None);

    finish_dismissal(&root_surface, &t).await;
    assert_eq!(selections.borrow().iter().map(without_id).collect::<Vec<_>>(), [word_wrap_item(true)]);
    assert!(is_active(&trigger));
}
