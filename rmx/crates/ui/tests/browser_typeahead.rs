//! Ports of Remix's `ui/src/lib/typeahead.test.tsx` `hiddenTypeahead` cases (the matching
//! helpers are unit tests in `src/typeahead.rs`). The timeout case waits the real 750 ms
//! where Remix advances fake timers. Run with
//! `pwsh ./test-browser.ps1 -Crate rmx-ui -Test browser_typeahead`.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use rmx::rsx;
use rmx::test::{render, RenderResult};
use rmx_ui::typeahead::{hidden_typeahead, HIDDEN_TYPEAHEAD_TIMEOUT_MS};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

type Calls = Rc<RefCell<Vec<String>>>;

fn render_typeahead() -> (RenderResult, Calls) {
    let calls: Calls = Rc::default();
    let record = calls.clone();
    let t = render(rsx! {
        <>
            <div id="scope" tabindex="-1" mix={hidden_typeahead(move |text| record.borrow_mut().push(text.to_string()))}>
                <button id="inside-a" type="button">"Inside A"</button>
                <button id="inside-b" type="button">"Inside B"</button>
            </div>
            <button id="outside" type="button">"Outside"</button>
        </>
    });
    (t, calls)
}

/// Dispatch a `keydown` for `key` on the scope, with the modifier `modifier` (`ctrlKey`,
/// ...) set when given, then flush.
fn key(t: &RenderResult, key: &str, modifier: Option<&str>) {
    let init = web_sys::KeyboardEventInit::new();
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_key(key);
    match modifier {
        Some("ctrlKey") => init.set_ctrl_key(true),
        Some("altKey") => init.set_alt_key(true),
        Some("metaKey") => init.set_meta_key(true),
        _ => {}
    }
    let event = web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap();
    t.act(|| drop(t.query("#scope").unwrap().dispatch_event(&event)));
}

/// Dispatch a `focusout` on the scope whose `relatedTarget` is `#related`.
fn focus_out(t: &RenderResult, related: &str) {
    let init = web_sys::FocusEventInit::new();
    init.set_bubbles(true);
    init.set_related_target(t.query(&format!("#{related}")).as_ref().map(|e| e.unchecked_ref()));
    let event = web_sys::FocusEvent::new_with_focus_event_init_dict("focusout", &init).unwrap();
    t.act(|| drop(t.query("#scope").unwrap().dispatch_event(&event)));
}

fn calls(calls: &Calls) -> Vec<String> {
    calls.borrow().clone()
}

async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

// typeahead.test.tsx: builds a lowercase query from printable key presses and ignores
// modified and non-printable keys
#[wasm_bindgen_test]
fn builds_a_lowercase_query_and_ignores_modified_and_non_printable_keys() {
    let (t, seen) = render_typeahead();
    key(&t, "R", None);
    key(&t, "e", None);
    key(&t, "ArrowDown", None);
    key(&t, "x", Some("ctrlKey"));
    key(&t, "y", Some("altKey"));
    key(&t, "z", Some("metaKey"));
    assert_eq!(calls(&seen), ["r", "re"]);
}

// typeahead.test.tsx: trims the current query with Backspace and lets Escape clear it without
// emitting
#[wasm_bindgen_test]
fn trims_the_query_with_backspace_and_lets_escape_clear_it_without_emitting() {
    let (t, seen) = render_typeahead();
    for k in ["R", "e", "Backspace", "Backspace", "Escape", "a"] {
        key(&t, k, None);
    }
    assert_eq!(calls(&seen), ["r", "re", "r", "", "a"]);
}

// typeahead.test.tsx: clears the query after the timeout so the next key starts a new search
#[wasm_bindgen_test]
async fn clears_the_query_after_the_timeout_so_the_next_key_starts_a_new_search() {
    let (t, seen) = render_typeahead();
    key(&t, "r", None);
    sleep(HIDDEN_TYPEAHEAD_TIMEOUT_MS + 1).await;
    key(&t, "e", None);
    assert_eq!(calls(&seen), ["r", "e"]);
}

// typeahead.test.tsx: keeps the query when focus stays inside the subtree and clears it when
// focus moves outside
#[wasm_bindgen_test]
fn keeps_the_query_when_focus_stays_inside_and_clears_it_when_focus_moves_outside() {
    let (t, seen) = render_typeahead();
    key(&t, "r", None);
    focus_out(&t, "inside-b");
    key(&t, "e", None);
    assert_eq!(calls(&seen), ["r", "re"]);
    focus_out(&t, "outside");
    key(&t, "a", None);
    assert_eq!(calls(&seen), ["r", "re", "a"]);
}
