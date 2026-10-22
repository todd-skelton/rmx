//! Ports of Remix's `ui/src/popover.test.tsx`, with its `PopoverHarness`, layout mock and
//! `showPopover` / `hidePopover` spies. Run with
//! `pwsh ./test-browser.ps1 -Crate rmx-ui -Test browser_popover`.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use rmx::test::{render, RenderResult};
use rmx::{on, rsx, Component, Handle, Node};
use rmx_ui::anchor::{AnchorOptions, Placement};
use rmx_ui::popover::{self, HideReason, HideRequest, SurfaceOptions};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

type Hides = Rc<RefCell<Vec<(HideReason, Option<String>)>>>;

/// Remix's `RenderPopoverOptions`. `on_hide` records each request's reason and the id of an
/// outside click's target.
#[derive(Clone, Default)]
struct HarnessProps {
    close_on_anchor_click: Option<bool>,
    close_on_hide: Option<bool>,
    hides: Hides,
    restore_focus_on_hide: Option<bool>,
    with_hide_focus: bool,
    with_show_focus: bool,
}

struct Harness {
    open: bool,
}

impl Component for Harness {
    type Props = HarnessProps;

    fn setup(_: &HarnessProps, _: &Handle<Self>) -> Self {
        Harness { open: false }
    }

    fn render(&self, props: &HarnessProps, h: &Handle<Self>) -> Node {
        let open_popover = h.on(|this, h| {
            this.open = true;
            h.update();
        });
        let close_popover = h.on(|this, h| {
            this.open = false;
            h.update();
        });
        let (hides, close_on_hide, handle) = (props.hides.clone(), props.close_on_hide, h.clone());
        let mut options = SurfaceOptions::new(self.open, move |request: &HideRequest| {
            let target = request.target.as_ref().and_then(|t| t.dyn_ref::<web_sys::Element>()).map(|e| e.id());
            hides.borrow_mut().push((request.reason, target));
            if close_on_hide == Some(false) {
                return;
            }
            handle.with(|this, h| {
                this.open = false;
                h.update();
            });
        });
        if let Some(close) = props.close_on_anchor_click {
            options = options.close_on_anchor_click(close);
        }
        if let Some(restore) = props.restore_focus_on_hide {
            options = options.restore_focus_on_hide(restore);
        }
        let hide_focus = props.with_hide_focus.then(popover::focus_on_hide);
        let show_focus = props.with_show_focus.then(|| rsx! { <button id="show-focus" mix={popover::focus_on_show()}>"Focus on show"</button> });
        rsx! {
            <popover::Context>
                <button id="anchor" mix={[Some(popover::anchor(AnchorOptions::new(Placement::BottomStart))), hide_focus, Some(on("click", open_popover))]}>"Anchor"</button>
                <div id="surface" mix={popover::surface(options)}>
                    {show_focus}
                    <button id="inside-close" mix={on("click", close_popover)}>"Close from inside"</button>
                    <button id="inside">"Inside"</button>
                </div>
                <button id="outside">"Outside"</button>
            </popover::Context>
        }
    }
}

fn render_popover(props: HarnessProps) -> RenderResult {
    let HarnessProps { close_on_anchor_click, close_on_hide, hides, restore_focus_on_hide, with_hide_focus, with_show_focus } = props;
    render(rsx! {
        <Harness close_on_anchor_click={close_on_anchor_click} close_on_hide={close_on_hide} hides={hides} restore_focus_on_hide={restore_focus_on_hide} with_hide_focus={with_hide_focus} with_show_focus={with_show_focus}/>
    })
}

fn js(body: &str) -> JsValue {
    js_sys::Function::new_no_args(body).call0(&JsValue::NULL).unwrap()
}

fn el(t: &RenderResult, id: &str) -> web_sys::HtmlElement {
    t.query(&format!("#{id}")).unwrap_or_else(|| panic!("#{id}"))
}

fn is_open(el: &web_sys::HtmlElement) -> bool {
    el.matches(":popover-open").unwrap()
}

/// Remix's `click`: a bubbling primary-button click event.
fn click(target: &web_sys::HtmlElement) {
    let init = web_sys::MouseEventInit::new();
    init.set_bubbles(true);
    init.set_button(0);
    target.dispatch_event(&web_sys::MouseEvent::new_with_mouse_event_init_dict("click", &init).unwrap()).unwrap();
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

async fn frame() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().request_animation_frame(&resolve).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

/// Remix's `settle`: flush, wait two frames and a microtask, flush.
async fn settle(t: &RenderResult) {
    t.act(|| {});
    frame().await;
    frame().await;
    JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await.unwrap();
    t.act(|| {});
}

fn is_active(el: &web_sys::HtmlElement) -> bool {
    web_sys::window().unwrap().document().unwrap().active_element().as_ref() == Some(el.unchecked_ref::<web_sys::Element>())
}

/// Count `showPopover` and `hidePopover` calls in `window.__popoverCalls`.
fn spy_popover_methods() {
    js("const proto = HTMLElement.prototype;
        const calls = { show: 0, hide: 0, show0: proto.showPopover, hide0: proto.hidePopover };
        window.__popoverCalls = calls;
        proto.showPopover = function (...args) { calls.show++; return calls.show0.apply(this, args) };
        proto.hidePopover = function (...args) { calls.hide++; return calls.hide0.apply(this, args) };");
}

/// The spied call counts, restoring the original methods.
fn popover_calls() -> (f64, f64) {
    let calls = js("const c = window.__popoverCalls; return [c.show, c.hide]");
    let calls = js_sys::Array::from(&calls);
    (calls.get(0).as_f64().unwrap(), calls.get(1).as_f64().unwrap())
}

fn restore_popover_methods() {
    js("const c = window.__popoverCalls; HTMLElement.prototype.showPopover = c.show0; HTMLElement.prototype.hidePopover = c.hide0; delete window.__popoverCalls;");
}

/// Remix's `mockLayout`: fixed offset sizes and bounding rect.
fn mock_layout(el: &web_sys::HtmlElement, top: f64, left: f64, width: f64, height: f64) {
    let mock = js_sys::Function::new_with_args(
        "el, top, left, width, height",
        "Object.defineProperty(el, 'offsetWidth', { configurable: true, get: () => width });
         Object.defineProperty(el, 'offsetHeight', { configurable: true, get: () => height });
         el.getBoundingClientRect = () => new DOMRect(left, top, width, height);",
    );
    let args = js_sys::Array::of5(el, &top.into(), &left.into(), &width.into(), &height.into());
    mock.apply(&JsValue::NULL, &args).unwrap();
}

fn style(el: &web_sys::HtmlElement, property: &str) -> String {
    el.style().get_property_value(property).unwrap()
}

fn root_style(property: &str) -> String {
    js(&format!("return document.documentElement.style.getPropertyValue('{property}')")).as_string().unwrap()
}

fn body_style(property: &str) -> String {
    js(&format!("return document.body.style.getPropertyValue('{property}')")).as_string().unwrap()
}

// popover.test.tsx: renders a manual popover surface and responds to controlled open state changes
#[wasm_bindgen_test]
async fn renders_a_manual_popover_surface_and_responds_to_controlled_open_state_changes() {
    spy_popover_methods();
    let t = render_popover(HarnessProps::default());
    let surface = el(&t, "surface");
    let before = popover_calls();
    click(&el(&t, "anchor"));
    settle(&t).await;
    let (opened, open_after_show) = (popover_calls(), is_open(&surface));
    click(&el(&t, "inside-close"));
    settle(&t).await;
    let (closed, open_after_hide) = (popover_calls(), is_open(&surface));
    restore_popover_methods();

    assert_eq!(surface.get_attribute("popover").as_deref(), Some("manual"));
    assert_eq!(before, (0.0, 0.0));
    assert_eq!(opened.0, 1.0);
    assert!(open_after_show);
    assert_eq!(closed.1, 1.0);
    assert!(!open_after_hide);
}

// popover.test.tsx: anchors on open and releases scroll locking on close
#[wasm_bindgen_test]
async fn anchors_on_open_and_releases_scroll_locking_on_close() {
    let t = render_popover(HarnessProps::default());
    let (anchor, surface) = (el(&t, "anchor"), el(&t, "surface"));
    mock_layout(&anchor, 40.0, 100.0, 80.0, 30.0);
    mock_layout(&surface, 0.0, 0.0, 160.0, 96.0);

    click(&anchor);
    settle(&t).await;
    let open = (style(&surface, "position"), style(&surface, "top"), style(&surface, "left"), root_style("overflow"), body_style("position"), body_style("overflow"));
    click(&el(&t, "inside-close"));
    settle(&t).await;

    let expected = ("fixed", "70px", "100px", "hidden", "", "");
    assert_eq!((open.0.as_str(), open.1.as_str(), open.2.as_str(), open.3.as_str(), open.4.as_str(), open.5.as_str()), expected);
    assert_eq!(root_style("overflow"), "");
}

// popover.test.tsx: moves focus to the registered show target when opening
#[wasm_bindgen_test]
async fn moves_focus_to_the_registered_show_target_when_opening() {
    let t = render_popover(HarnessProps { with_show_focus: true, ..Default::default() });
    let anchor = el(&t, "anchor");
    anchor.focus().unwrap();
    click(&anchor);
    settle(&t).await;
    assert!(is_active(&el(&t, "show-focus")));
}

// popover.test.tsx: calls onHide on Escape and focuses the registered hide target after closing
#[wasm_bindgen_test]
async fn calls_on_hide_on_escape_and_focuses_the_registered_hide_target_after_closing() {
    let hides = Hides::default();
    let t = render_popover(HarnessProps { hides: hides.clone(), with_hide_focus: true, with_show_focus: true, ..Default::default() });
    let (anchor, inside, surface) = (el(&t, "anchor"), el(&t, "inside"), el(&t, "surface"));
    click(&anchor);
    settle(&t).await;
    inside.focus().unwrap();

    let event = key(&inside, "Escape");
    settle(&t).await;

    assert!(event.default_prevented());
    assert_eq!(*hides.borrow(), [(HideReason::EscapeKey, None)]);
    assert!(!is_open(&surface));
    assert!(is_active(&anchor));
}

// popover.test.tsx: calls onHide on outside click and focuses the registered hide target
#[wasm_bindgen_test]
async fn calls_on_hide_on_outside_click_and_focuses_the_registered_hide_target() {
    let hides = Hides::default();
    let t = render_popover(HarnessProps { hides: hides.clone(), with_hide_focus: true, with_show_focus: true, ..Default::default() });
    let (anchor, surface) = (el(&t, "anchor"), el(&t, "surface"));
    click(&anchor);
    settle(&t).await;

    click(&el(&t, "outside"));
    settle(&t).await;

    assert_eq!(*hides.borrow(), [(HideReason::OutsideClick, Some("outside".to_string()))]);
    assert!(!is_open(&surface));
    assert!(is_active(&anchor));
}

// popover.test.tsx: does not close when clicking inside the surface
#[wasm_bindgen_test]
async fn does_not_close_when_clicking_inside_the_surface() {
    let hides = Hides::default();
    let t = render_popover(HarnessProps { hides: hides.clone(), ..Default::default() });
    click(&el(&t, "anchor"));
    settle(&t).await;
    click(&el(&t, "inside"));
    settle(&t).await;
    assert!(hides.borrow().is_empty());
    assert!(is_open(&el(&t, "surface")));
}

// popover.test.tsx: can ignore anchor clicks while open when closeOnAnchorClick is false
#[wasm_bindgen_test]
async fn can_ignore_anchor_clicks_while_open_when_close_on_anchor_click_is_false() {
    let hides = Hides::default();
    let t = render_popover(HarnessProps { close_on_anchor_click: Some(false), hides: hides.clone(), ..Default::default() });
    let anchor = el(&t, "anchor");
    click(&anchor);
    settle(&t).await;
    click(&anchor);
    settle(&t).await;
    assert!(hides.borrow().is_empty());
    assert!(is_open(&el(&t, "surface")));
}

// popover.test.tsx: stays open until the parent updates open to false after onHide runs
#[wasm_bindgen_test]
async fn stays_open_until_the_parent_updates_open_to_false_after_on_hide_runs() {
    let hides = Hides::default();
    let t = render_popover(HarnessProps { close_on_hide: Some(false), hides: hides.clone(), with_show_focus: true, ..Default::default() });
    let (inside, surface) = (el(&t, "inside"), el(&t, "surface"));
    click(&el(&t, "anchor"));
    settle(&t).await;
    inside.focus().unwrap();
    spy_popover_methods();

    key(&inside, "Escape");
    settle(&t).await;
    let (_, hidden) = popover_calls();
    restore_popover_methods();

    assert_eq!(hides.borrow().len(), 1);
    assert_eq!(hidden, 0.0);
    assert!(is_open(&surface));
}

// popover.test.tsx: focuses the registered hide target immediately when closing without a hide transition
#[wasm_bindgen_test]
async fn focuses_the_registered_hide_target_immediately_when_closing_without_a_hide_transition() {
    let t = render_popover(HarnessProps { with_hide_focus: true, with_show_focus: true, ..Default::default() });
    let anchor = el(&t, "anchor");
    click(&anchor);
    settle(&t).await;
    el(&t, "inside").focus().unwrap();
    click(&el(&t, "inside-close"));
    settle(&t).await;
    assert!(is_active(&anchor));
}

// popover.test.tsx: can skip restoring focus on hide
#[wasm_bindgen_test]
async fn can_skip_restoring_focus_on_hide() {
    let t = render_popover(HarnessProps { restore_focus_on_hide: Some(false), with_hide_focus: true, with_show_focus: true, ..Default::default() });
    let anchor = el(&t, "anchor");
    click(&anchor);
    settle(&t).await;
    el(&t, "inside").focus().unwrap();
    click(&el(&t, "inside-close"));
    settle(&t).await;
    assert!(!is_active(&anchor));
}
