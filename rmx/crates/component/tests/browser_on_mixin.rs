//! Ports of Remix's `on-mixin.test.tsx` and `vdom.events.test.tsx` (attach, update across
//! renders, clean up on removal): listeners live on their host element. Run with
//! `pwsh ./test-browser.ps1 -Test browser_on_mixin`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::cell::RefCell;
use std::rc::Rc;

use rmx::{create_root, on, on_capture, rsx, EventHandler, Root, Signal};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn container() -> web_sys::HtmlElement {
    document().create_element("div").unwrap().unchecked_into()
}

/// A container in the document, removed (and its root disposed) when dropped.
struct Connected {
    el: web_sys::HtmlElement,
    root: Root,
}

impl Connected {
    fn new() -> Connected {
        let el = container();
        document().body().unwrap().append_child(&el).unwrap();
        let root = create_root(&el);
        Connected { el, root }
    }
}

impl Drop for Connected {
    fn drop(&mut self) {
        self.root.dispose();
        self.el.remove();
    }
}

fn find(el: &web_sys::HtmlElement, selector: &str) -> web_sys::HtmlElement {
    el.query_selector(selector).unwrap().unwrap_or_else(|| panic!("no {selector}")).unchecked_into()
}

/// Wrap `target[method]` to record the first argument of each call. Dropping the spy puts
/// the original back.
struct Spy {
    calls: js_sys::Array,
    restore: js_sys::Function,
}

impl Spy {
    fn on(target: &JsValue, method: &str) -> Spy {
        let calls = js_sys::Array::new();
        let make = js_sys::Function::new_with_args(
            "o,m,log",
            "const orig = o[m]; o[m] = function (...a) { log.push(a[0]); return orig.apply(this, a) }; return () => { o[m] = orig }",
        );
        let restore = make.call3(&JsValue::NULL, target, &method.into(), &calls).unwrap().unchecked_into();
        Spy { calls, restore }
    }

    fn count(&self) -> u32 {
        self.calls.length()
    }

    fn called_with(&self, name: &str) -> bool {
        self.calls.iter().any(|v| v.as_string().as_deref() == Some(name))
    }
}

impl Drop for Spy {
    fn drop(&mut self) {
        self.restore.call0(&JsValue::NULL).unwrap();
    }
}

type Log = Rc<RefCell<Vec<&'static str>>>;

fn log() -> Log {
    Rc::new(RefCell::new(Vec::new()))
}

fn push(log: &Log, s: &'static str) -> EventHandler {
    let log = log.clone();
    EventHandler::new(move |_| log.borrow_mut().push(s))
}

fn fire(target: &web_sys::EventTarget, kind: &str, bubbles: bool) {
    let init = web_sys::EventInit::new();
    init.set_bubbles(bubbles);
    target.dispatch_event(&web_sys::Event::new_with_event_init_dict(kind, &init).unwrap()).unwrap();
}

#[wasm_bindgen_test]
fn updates_listeners_in_place_without_rebinding_when_capture_is_unchanged() {
    let calls = log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <button mix={on("click", push(&calls, "first"))}>"click"</button> });
    root.flush();
    let button = find(&el, "button");
    let add = Spy::on(&button, "addEventListener");
    let remove = Spy::on(&button, "removeEventListener");
    root.render(rsx! { <button mix={on("click", push(&calls, "second"))}>"click"</button> });
    root.flush();
    button.click();
    root.flush();
    assert_eq!(*calls.borrow(), ["second"]);
    assert_eq!(add.count(), 0);
    assert_eq!(remove.count(), 0);
}

#[wasm_bindgen_test]
fn rebinds_when_the_capture_option_changes() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <button mix={on("click", EventHandler::new(|_| {}))}>"click"</button> });
    root.flush();
    let button = find(&el, "button");
    let add = Spy::on(&button, "addEventListener");
    let remove = Spy::on(&button, "removeEventListener");
    root.render(rsx! { <button mix={on_capture("click", EventHandler::new(|_| {}))}>"click"</button> });
    root.flush();
    assert_eq!(add.count(), 1);
    assert_eq!(remove.count(), 1);
}

#[wasm_bindgen_test]
fn passes_an_abort_signal_to_the_handler() {
    let received: Rc<RefCell<Option<Signal>>> = Rc::new(RefCell::new(None));
    let r = received.clone();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <button mix={on("click", EventHandler::new(move |e| *r.borrow_mut() = Some(e.signal())))}>"click"</button> });
    root.flush();
    find(&el, "button").click();
    root.flush();
    let signal = received.borrow_mut().take().expect("handler ran");
    assert!(!signal.aborted());
}

#[wasm_bindgen_test]
fn supports_multiple_event_types_on_the_same_element() {
    let calls = log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <button mix={[on("click", push(&calls, "click")), on("focus", push(&calls, "focus"))]}>"click"</button> });
    root.flush();
    let button = find(&el, "button");
    fire(&button, "focus", true);
    button.click();
    root.flush();
    assert_eq!(*calls.borrow(), ["focus", "click"]);
}

#[wasm_bindgen_test]
fn keeps_bubbling_events_on_the_host_element_for_connected_roots() {
    let targets: Rc<RefCell<Vec<web_sys::Element>>> = Rc::new(RefCell::new(Vec::new()));
    let t = targets.clone();
    let c = Connected::new();
    let document_add = Spy::on(&document(), "addEventListener");
    c.root.render(rsx! { <input mix={on("input", EventHandler::new(move |e| t.borrow_mut().push(e.current_element().unwrap())))}/> });
    c.root.flush();
    let input = find(&c.el, "input");
    fire(&input, "input", true);
    c.root.flush();
    assert_eq!(targets.borrow().len(), 1);
    assert!(targets.borrow()[0].is_same_node(Some(&input)));
    assert!(!document_add.called_with("input"));
}

#[wasm_bindgen_test]
fn keeps_non_bubbling_events_on_the_host_element_for_connected_roots() {
    let calls = log();
    let c = Connected::new();
    c.root.render(rsx! { <button mix={on("mouseenter", push(&calls, "enter"))}>"hover"</button> });
    c.root.flush();
    fire(&find(&c.el, "button"), "mouseenter", false);
    c.root.flush();
    assert_eq!(*calls.borrow(), ["enter"]);
}

#[wasm_bindgen_test]
fn runs_host_listeners_before_ancestor_propagation_is_stopped() {
    let calls = log();
    let c = Connected::new();
    c.root.render(rsx! { <button mix={on("click", push(&calls, "button"))}>"click"</button> });
    c.root.flush();
    let l = calls.clone();
    let stop = Closure::<dyn Fn(web_sys::Event)>::new(move |e: web_sys::Event| {
        l.borrow_mut().push("container");
        e.stop_propagation();
    });
    c.el.add_event_listener_with_callback("click", stop.as_ref().unchecked_ref()).unwrap();
    find(&c.el, "button").click();
    c.root.flush();
    assert_eq!(*calls.borrow(), ["button", "container"]);
}

#[wasm_bindgen_test]
fn does_not_treat_stop_propagation_as_stop_immediate_propagation_on_the_same_host() {
    let calls = log();
    let c = Connected::new();
    let l = calls.clone();
    let first = EventHandler::new(move |e| {
        l.borrow_mut().push("first");
        e.stop_propagation();
    });
    c.root.render(rsx! { <button mix={[on("click", first), on("click", push(&calls, "second"))]}>"click"</button> });
    c.root.flush();
    find(&c.el, "button").click();
    c.root.flush();
    assert_eq!(*calls.borrow(), ["first", "second"]);
}

#[wasm_bindgen_test]
fn keeps_capture_listeners_on_the_host_element_for_connected_roots() {
    let calls = log();
    let c = Connected::new();
    let document_add = Spy::on(&document(), "addEventListener");
    c.root.render(rsx! { <button mix={on_capture("keydown", push(&calls, "keydown"))}>"press"</button> });
    c.root.flush();
    fire(&find(&c.el, "button"), "keydown", true);
    c.root.flush();
    assert_eq!(*calls.borrow(), ["keydown"]);
    assert!(!document_add.called_with("keydown"));
}

#[wasm_bindgen_test]
fn capture_listeners_run_before_target_listeners() {
    let calls = log();
    let c = Connected::new();
    c.root.render(rsx! {
        <div mix={[on_capture("click", push(&calls, "capture")), on("click", push(&calls, "bubble"))]}>
            <button mix={on("click", push(&calls, "target"))}>"click"</button>
        </div>
    });
    c.root.flush();
    find(&c.el, "button").click();
    assert_eq!(*calls.borrow(), ["capture", "target", "bubble"]);
}

#[wasm_bindgen_test]
fn removes_listeners_when_the_on_mixin_is_removed() {
    let calls = log();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <button mix={on("click", push(&calls, "click"))}>"click"</button> });
    root.flush();
    let button = find(&el, "button");
    button.click();
    root.flush();
    assert_eq!(calls.borrow().len(), 1);
    root.render(rsx! { <button>"click"</button> });
    root.flush();
    button.click();
    root.flush();
    assert_eq!(calls.borrow().len(), 1);
}

#[wasm_bindgen_test]
fn aborts_the_previous_handler_signal_on_reentry() {
    let signals: Rc<RefCell<Vec<Signal>>> = Rc::new(RefCell::new(Vec::new()));
    let s = signals.clone();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <button mix={on("click", EventHandler::new(move |e| s.borrow_mut().push(e.signal())))}>"click"</button> });
    root.flush();
    let button = find(&el, "button");
    button.click();
    button.click();
    root.flush();
    let signals = signals.borrow();
    assert_eq!(signals.len(), 2);
    assert!(signals[0].aborted());
    assert!(!signals[1].aborted());
}

// rmx: an element unmounted with its subtree keeps its DOM listener, so an event that
// still reaches it (a `toggle` queued by `hidePopover`) must not run the handler that
// took over its freed slot.
#[wasm_bindgen_test]
fn events_on_unmounted_elements_do_not_reach_handlers_that_reuse_their_slots() {
    let calls = log();
    let c = Connected::new();
    c.root.render(rsx! { <div><p mix={on("late", push(&calls, "old"))}>"old"</p></div> });
    c.root.flush();
    let old = find(&c.el, "p");
    c.root.render(rsx! { <section></section> });
    c.root.flush();
    c.root.render(rsx! { <div><span mix={on("other", push(&calls, "new"))}>"new"</span></div> });
    c.root.flush();
    fire(&old, "late", false);
    assert!(calls.borrow().is_empty(), "{:?}", calls.borrow());
    fire(&find(&c.el, "span"), "other", false);
    assert_eq!(*calls.borrow(), ["new"]);
}
