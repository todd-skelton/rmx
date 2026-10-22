//! Ports of Remix's `ui/src/lib/wait-for-css-transition.test.ts`. `requestAnimationFrame` is
//! mocked the same way, so frames run only when a test flushes them. Run with
//! `pwsh ./scripts/test-browser.ps1 -Crate rmx-ui -Test browser_wait_for_css_transition`.
#![cfg(target_arch = "wasm32")]

use std::cell::Cell;
use std::rc::Rc;

use rmx_ui::wait_for_css_transition;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn js(body: &str) -> JsValue {
    js_sys::Function::new_no_args(body).call0(&JsValue::NULL).unwrap()
}

fn setup() -> web_sys::HtmlElement {
    js("
        window.__frames = [];
        window.requestAnimationFrame = (callback) => { window.__frames.push(callback); return window.__frames.length; };
        window.cancelAnimationFrame = () => {};
        for (let node of document.querySelectorAll('[data-wait-test]')) node.remove();
    ");
    // Remix clears the body; here that would also remove the test runner's output element.
    js("let node = document.createElement('div'); node.dataset.waitTest = ''; document.body.append(node); return node").unchecked_into()
}

fn flush_animation_frame() {
    js("let callbacks = window.__frames; window.__frames = []; for (let callback of callbacks) callback(performance.now());");
}

/// Lets the spawned wait and every pending microtask run.
async fn tick() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0).unwrap();
    });
    JsFuture::from(promise).await.unwrap();
}

fn dispatch(target: &web_sys::Element, kind: &str) {
    js_sys::Function::new_with_args("target, kind", "target.dispatchEvent(new TransitionEvent(kind, { propertyName: 'opacity', bubbles: true }))")
        .call2(&JsValue::NULL, target, &kind.into())
        .unwrap();
}

fn controller() -> (JsValue, web_sys::AbortSignal) {
    let controller = js("return new AbortController()");
    let signal = js_sys::Reflect::get(&controller, &"signal".into()).unwrap().unchecked_into();
    (controller, signal)
}

fn abort(controller: &JsValue) {
    js_sys::Function::new_with_args("c", "c.abort()").call1(&JsValue::NULL, controller).unwrap();
}

/// Starts waiting on `node` (setting `data-state`, as the Remix action does) and returns a
/// flag that turns true when the wait resolves.
fn start(node: &web_sys::HtmlElement, signal: web_sys::AbortSignal) -> Rc<Cell<bool>> {
    let resolved = Rc::new(Cell::new(false));
    let (flag, node) = (resolved.clone(), node.clone());
    node.set_attribute("data-state", "open").unwrap();
    spawn_local(async move {
        wait_for_css_transition(node, signal).await;
        flag.set(true);
    });
    resolved
}

// wait-for-css-transition.test.ts: resolves after the detection window when no transition runs
#[wasm_bindgen_test]
async fn resolves_after_the_detection_window_when_no_transition_runs() {
    let node = setup();
    let resolved = start(&node, controller().1);
    tick().await;
    assert!(!resolved.get());

    flush_animation_frame();
    tick().await;
    assert!(!resolved.get());

    flush_animation_frame();
    tick().await;
    assert!(resolved.get());
}

// wait-for-css-transition.test.ts: waits for transitionend after transitionrun is observed
#[wasm_bindgen_test]
async fn waits_for_transitionend_after_transitionrun_is_observed() {
    let node = setup();
    let resolved = start(&node, controller().1);
    tick().await;
    flush_animation_frame();
    dispatch(&node, "transitionrun");
    flush_animation_frame();
    tick().await;
    assert!(!resolved.get());

    dispatch(&node, "transitionend");
    tick().await;
    assert!(resolved.get());
}

// wait-for-css-transition.test.ts: ignores delayed transitions that start after the detection window
#[wasm_bindgen_test]
async fn ignores_delayed_transitions_that_start_after_the_detection_window() {
    let node = setup();
    let resolved = start(&node, controller().1);
    tick().await;
    flush_animation_frame();
    flush_animation_frame();
    tick().await;
    assert!(resolved.get());

    dispatch(&node, "transitionrun");
    tick().await;
}

// wait-for-css-transition.test.ts: resolves when the abort signal fires before transitionend
#[wasm_bindgen_test]
async fn resolves_when_the_abort_signal_fires_before_transitionend() {
    let node = setup();
    let (controller, signal) = controller();
    let resolved = start(&node, signal);
    tick().await;
    flush_animation_frame();
    tick().await;
    assert!(!resolved.get());

    abort(&controller);
    tick().await;
    assert!(resolved.get());
}

#[wasm_bindgen_test]
async fn ignores_transitions_of_descendants() {
    let node = setup();
    let child = web_sys::window().unwrap().document().unwrap().create_element("span").unwrap();
    node.append_child(&child).unwrap();
    let resolved = start(&node, controller().1);
    tick().await;
    dispatch(&child, "transitionrun");
    dispatch(&node, "transitionrun");
    flush_animation_frame();
    flush_animation_frame();
    dispatch(&child, "transitionend");
    tick().await;
    assert!(!resolved.get(), "a descendant's transitionend does not end the wait");

    dispatch(&node, "transitioncancel");
    tick().await;
    assert!(resolved.get());
}
