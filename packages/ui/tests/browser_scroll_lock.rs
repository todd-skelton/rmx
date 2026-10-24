//! Ports of Remix's `ui/src/lib/scroll-lock.test.tsx`, with the same window mocks: the page
//! is scrolled to (24, 120), `innerWidth` is 1000 and the root's `clientWidth` is 980, so
//! there is a 20px scrollbar. Run with `pwsh ./scripts/test-browser.ps1 -Crate rsmix-ui`.
#![cfg(target_arch = "wasm32")]

use rsmix_ui::popover::scroll_lock::lock;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

const MOCKS: &str = "
    window.__scroll = { x: 24, y: 120, calls: [] };
    document.body.removeAttribute('style');
    document.documentElement.removeAttribute('style');
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1000, writable: true });
    Object.defineProperty(window, 'scrollX', { configurable: true, get: () => window.__scroll.x });
    Object.defineProperty(window, 'scrollY', { configurable: true, get: () => window.__scroll.y });
    Object.defineProperty(document.documentElement, 'clientWidth', { configurable: true, value: 980 });
    window.scrollTo = (x, y) => { window.__scroll.calls.push([x, y]); window.__scroll.x = x; window.__scroll.y = y; };
";

fn js(body: &str) -> JsValue {
    js_sys::Function::new_no_args(body).call0(&JsValue::NULL).unwrap()
}

fn setup() {
    js(MOCKS);
}

fn root_style(property: &str) -> String {
    js(&format!("return document.documentElement.style.getPropertyValue('{property}')")).as_string().unwrap()
}

fn body_style(property: &str) -> String {
    js(&format!("return document.body.style.getPropertyValue('{property}')")).as_string().unwrap()
}

/// `scrollTo` calls so far, as `"x,y"` strings.
fn scroll_calls() -> Vec<String> {
    let calls = js("return window.__scroll.calls.map(c => c.join(','))");
    js_sys::Array::from(&calls).iter().map(|c| c.as_string().unwrap()).collect()
}

const BODY: [(&str, &str); 7] = [
    ("overflow", "clip"),
    ("position", "relative"),
    ("top", "1px"),
    ("left", "2px"),
    ("right", "3px"),
    ("width", "80%"),
    ("padding-right", "4px"),
];

fn assert_body_unchanged() {
    for (property, value) in BODY {
        assert_eq!(body_style(property), value, "body {property}");
    }
}

// scroll-lock.test.tsx: locks document scrolling and restores inline styles on unlock
#[wasm_bindgen_test]
fn locks_document_scrolling_and_restores_inline_styles_on_unlock() {
    setup();
    js("document.documentElement.style.overflow = 'auto'; document.documentElement.style.scrollbarGutter = 'stable both-edges';");
    for (property, value) in BODY {
        js(&format!("document.body.style.setProperty('{property}', '{value}')"));
    }

    let unlock = lock();
    assert_eq!(root_style("overflow"), "hidden");
    assert_eq!(root_style("scrollbar-gutter"), "stable both-edges");
    assert_body_unchanged();

    js("window.__scroll.x = 0; window.__scroll.y = 0;");
    drop(unlock);
    assert_eq!(root_style("overflow"), "auto");
    assert_eq!(root_style("scrollbar-gutter"), "stable both-edges");
    assert_body_unchanged();
    assert_eq!(scroll_calls(), ["24,120"]);
}

// scroll-lock.test.tsx: reserves the scrollbar gutter while locked
#[wasm_bindgen_test]
fn reserves_the_scrollbar_gutter_while_locked() {
    setup();
    let unlock = lock();
    assert_eq!(root_style("overflow"), "hidden");
    assert_eq!(root_style("scrollbar-gutter"), "stable");

    drop(unlock);
    assert_eq!(root_style("overflow"), "");
    assert_eq!(root_style("scrollbar-gutter"), "");
}

// scroll-lock.test.tsx: keeps the document locked until the last unlock runs (a second call of
// the same unlock is impossible here: a ScrollLock is released by dropping it, once)
#[wasm_bindgen_test]
fn keeps_the_document_locked_until_the_last_unlock_runs() {
    setup();
    let first = lock();
    let second = lock();

    drop(first);
    assert_eq!(root_style("overflow"), "hidden");
    assert!(scroll_calls().is_empty());

    drop(second);
    assert_eq!(root_style("overflow"), "");
    assert_eq!(scroll_calls(), ["24,120"]);
}

#[wasm_bindgen_test]
fn does_not_reserve_a_gutter_without_a_scrollbar() {
    setup();
    js("Object.defineProperty(document.documentElement, 'clientWidth', { configurable: true, value: 1000 });");
    let unlock = lock();
    assert_eq!(root_style("overflow"), "hidden");
    assert_eq!(root_style("scrollbar-gutter"), "");
    drop(unlock);
}
