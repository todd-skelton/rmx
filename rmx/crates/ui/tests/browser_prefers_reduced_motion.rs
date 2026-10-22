//! Ports of Remix's `ui/src/animation/prefers-reduced-motion.test.ts`, with `window.matchMedia`
//! replaced the same way. Run with
//! `pwsh ./test-browser.ps1 -Crate rmx-ui -Test browser_prefers_reduced_motion`.
#![cfg(target_arch = "wasm32")]

use rmx_ui::animation::prefers_reduced_motion;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn js(body: &str) -> JsValue {
    js_sys::Function::new_no_args(body).call0(&JsValue::NULL).unwrap()
}

/// Replaces `matchMedia` with a mock that answers `window.__reduced` and records queries.
fn mock_match_media(reduced: bool) {
    js(&format!(
        "if (!window.__originalMatchMedia) window.__originalMatchMedia = Object.getOwnPropertyDescriptor(window, 'matchMedia') || null;
         window.__reduced = {reduced}; window.__queries = [];
         Object.defineProperty(window, 'matchMedia', {{ configurable: true, writable: true, value: (q) => {{ window.__queries.push(q); return {{ matches: window.__reduced }}; }} }});"
    ));
}

fn restore() {
    js("let d = window.__originalMatchMedia; if (d) Object.defineProperty(window, 'matchMedia', d); else delete window.matchMedia; window.__originalMatchMedia = undefined;");
}

// prefers-reduced-motion.test.ts: returns true when reduced motion is requested
#[wasm_bindgen_test]
fn returns_true_when_reduced_motion_is_requested() {
    mock_match_media(true);
    assert!(prefers_reduced_motion());
    assert_eq!(js("return window.__queries.join('|')").as_string().unwrap(), "(prefers-reduced-motion: reduce)");
    restore();
}

// prefers-reduced-motion.test.ts: reads the current preference on each call
#[wasm_bindgen_test]
fn reads_the_current_preference_on_each_call() {
    mock_match_media(false);
    assert!(!prefers_reduced_motion());
    js("window.__reduced = true");
    assert!(prefers_reduced_motion());
    js("window.__reduced = false");
    assert!(!prefers_reduced_motion());
    restore();
}

// prefers-reduced-motion.test.ts: returns false when matchMedia is unavailable
#[wasm_bindgen_test]
fn returns_false_when_match_media_is_unavailable() {
    mock_match_media(true);
    js("Object.defineProperty(window, 'matchMedia', { configurable: true, value: undefined })");
    assert!(!prefers_reduced_motion());
    restore();
}
