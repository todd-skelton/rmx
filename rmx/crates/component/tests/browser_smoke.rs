//! Smoke test for the browser test harness.
#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn runs_in_a_browser() {
    let doc = web_sys::window().unwrap().document().unwrap();
    assert!(doc.body().is_some());
}
