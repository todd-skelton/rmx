//! Prefetch triggers and eligibility, after Remix's `prefetch.test.ts`: `data-rmx-prefetch`
//! intent and viewport links and `rmx::prefetch`, with a `RunOptions::prefetch` that
//! records each request. The default strategy is in `browser_prefetch_assets.rs`. Run with
//! `pwsh ./scripts/test-browser.ps1 -Test browser_prefetch`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::cell::RefCell;
use std::rc::Rc;

use rmx::{create_root, prefetch, rsx, Frame, PrefetchOptions, RunOptions};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

thread_local! {
    static APP: RefCell<Option<rmx::App>> = const { RefCell::new(None) };
    static CALLS: RefCell<Vec<(String, Option<String>)>> = const { RefCell::new(Vec::new()) };
}

thread_local! {
    static BUSY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Async tests interleave in one page; prefetching is page-wide, so they take turns.
struct Serial;

async fn serial() -> Serial {
    while BUSY.with(|b| b.replace(true)) {
        sleep(10).await;
    }
    Serial
}

impl Drop for Serial {
    fn drop(&mut self) {
        BUSY.with(|b| b.set(false));
    }
}

async fn start() {
    if APP.with(|a| a.borrow().is_none()) {
        let record: rmx::PrefetchFrame = Rc::new(|src: &str, options: PrefetchOptions| CALLS.with(|c| c.borrow_mut().push((src.to_string(), options.target))));
        APP.with(|a| *a.borrow_mut() = Some(rmx::run(RunOptions { prefetch: Some(record), ..Default::default() })));
    }
    let app = APP.with(|a| a.borrow_mut().take()).expect("app");
    app.ready().await;
    APP.with(|a| *a.borrow_mut() = Some(app));
    take_calls();
    set_connection(None);
}

/// Wait up to a second for a prefetch, then take the calls.
async fn wait_calls() -> Vec<(String, Option<String>)> {
    for _ in 0..50 {
        if CALLS.with(|c| !c.borrow().is_empty()) {
            break;
        }
        sleep(20).await;
    }
    sleep(30).await;
    take_calls()
}

fn take_calls() -> Vec<(String, Option<String>)> {
    CALLS.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn absolute(path: &str) -> String {
    web_sys::Url::new_with_base(path, &document().base_uri().unwrap().unwrap()).unwrap().href()
}

/// `navigator.connection`, or none.
fn set_connection(connection: Option<(&str, bool)>) {
    let f = js_sys::Function::new_with_args(
        "type,saveData",
        "Object.defineProperty(navigator, 'connection', { configurable: true, get: () => type === undefined ? undefined : { effectiveType: type, saveData } })",
    );
    let (kind, save) = connection.map_or((JsValue::UNDEFINED, JsValue::FALSE), |(k, s)| (k.into(), s.into()));
    f.call2(&JsValue::NULL, &kind, &save).unwrap();
}

/// HTML in a container at the top of the page; removed on drop.
struct Fixture(web_sys::Element);

impl Fixture {
    fn new(html: &str) -> Fixture {
        let el = document().create_element("div").unwrap();
        el.set_inner_html(html);
        let body = document().body().unwrap();
        body.insert_before(&el, body.first_child().as_ref()).unwrap();
        Fixture(el)
    }

    fn get(&self, selector: &str) -> web_sys::Element {
        self.0.query_selector(selector).unwrap().expect(selector)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.0.remove();
    }
}

fn dispatch(target: &web_sys::Element, kind: &str, init: &str) {
    let f = js_sys::Function::new_with_args("target,kind", &format!("const Ctor = kind.startsWith('pointer') ? PointerEvent : FocusEvent; target.dispatchEvent(new Ctor(kind, {{ bubbles: true, composed: true, ...{init} }}))"));
    f.call2(&JsValue::NULL, target, &kind.into()).unwrap();
}

fn hover(el: &web_sys::Element) {
    dispatch(el, "pointerover", "{ pointerType: 'mouse', isPrimary: true }");
}

fn unhover(el: &web_sys::Element) {
    dispatch(el, "pointerout", "{ pointerType: 'mouse', isPrimary: true, relatedTarget: document.body }");
}

fn press(el: &web_sys::Element, init: &str) {
    dispatch(el, "pointerdown", init);
}

fn focus(el: &web_sys::Element) {
    dispatch(el, "focusin", "{}");
}

fn blur(el: &web_sys::Element) {
    dispatch(el, "focusout", "{ relatedTarget: document.body }");
}

fn top(path: &str) -> (String, Option<String>) {
    (absolute(path), None)
}

#[wasm_bindgen_test]
async fn prefetches_intent_links_after_mouse_hover_or_keyboard_focus_rests_on_them() {
    let _serial = serial().await;
    start().await;
    let f = Fixture::new(r#"<a id="a" href="/hover#section" data-rmx-prefetch="intent"><span>Hover</span></a><a id="b" href="/focus" data-rmx-prefetch="intent">Focus</a>"#);
    hover(&f.get("#a span"));
    sleep(40).await;
    assert_eq!(take_calls(), [], "not before the intent delay");
    sleep(120).await;
    assert_eq!(take_calls(), [top("/hover")], "the fragment is removed");
    focus(&f.get("#b"));
    sleep(160).await;
    assert_eq!(take_calls(), [top("/focus")]);
}

#[wasm_bindgen_test]
async fn cancels_the_prefetch_when_the_pointer_leaves_before_the_intent_delay() {
    let _serial = serial().await;
    start().await;
    let f = Fixture::new(r#"<a id="a" href="/leave" data-rmx-prefetch="intent">Leave</a>"#);
    hover(&f.get("#a"));
    sleep(30).await;
    unhover(&f.get("#a"));
    sleep(150).await;
    assert_eq!(take_calls(), []);
}

#[wasm_bindgen_test]
async fn prefetches_immediately_when_pressed_and_once_per_hover() {
    let _serial = serial().await;
    start().await;
    let f = Fixture::new(r#"<a id="a" href="/press" data-rmx-prefetch="intent"><b>x</b><i>y</i></a>"#);
    let a = f.get("#a");
    press(&a, "{ pointerType: 'mouse', isPrimary: true, button: 0 }");
    assert_eq!(take_calls(), [top("/press")], "a press does not wait for the delay");
    hover(&f.get("#a b"));
    hover(&f.get("#a i"));
    sleep(150).await;
    assert_eq!(take_calls(), [], "one prefetch per interaction");
    unhover(&a);
    hover(&a);
    sleep(150).await;
    assert_eq!(take_calls(), [top("/press")], "a later hover prefetches again");
    unhover(&a);
    press(&a, "{ pointerType: 'mouse', isPrimary: true, button: 1 }");
    press(&a, "{ pointerType: 'mouse', isPrimary: true, button: 0, ctrlKey: true }");
    assert_eq!(take_calls(), [], "presses that open another tab are skipped");
}

#[wasm_bindgen_test]
async fn keeps_focus_intent_when_the_pointer_leaves_the_same_link() {
    let _serial = serial().await;
    start().await;
    let f = Fixture::new(r#"<a id="a" href="/both" data-rmx-prefetch="intent">Both</a>"#);
    let a = f.get("#a");
    focus(&a);
    hover(&a);
    sleep(20).await;
    unhover(&a);
    sleep(150).await;
    assert_eq!(take_calls(), [top("/both")]);
    blur(&a);
}

#[wasm_bindgen_test]
async fn skips_automatic_prefetching_with_save_data_or_2g_but_not_imperative_prefetch() {
    let _serial = serial().await;
    start().await;
    let f = Fixture::new(r#"<a id="a" href="/slow" data-rmx-prefetch="intent">Slow</a>"#);
    let a = f.get("#a");
    for connection in [("4g", true), ("2g", false), ("slow-2g", false)] {
        set_connection(Some(connection));
        press(&a, "{ pointerType: 'mouse', isPrimary: true, button: 0 }");
        unhover(&a);
        assert_eq!(take_calls(), [], "{connection:?}");
    }
    set_connection(Some(("3g", false)));
    press(&a, "{ pointerType: 'mouse', isPrimary: true, button: 0 }");
    unhover(&a);
    assert_eq!(take_calls(), [top("/slow")]);
    set_connection(Some(("2g", true)));
    prefetch("/imperative", PrefetchOptions::default());
    assert_eq!(take_calls(), [top("/imperative")], "prefetch() is under the app's control");
    set_connection(None);
}

#[wasm_bindgen_test]
async fn skips_document_navigations_other_contexts_cross_origin_and_the_current_page() {
    let _serial = serial().await;
    start().await;
    let f = Fixture::new(
        r##"<a id="download" href="/d" download data-rmx-prefetch="intent">d</a>
           <a id="document" href="/d" data-rmx-document data-rmx-prefetch="intent">d</a>
           <a id="blank" href="/d" target="_blank" data-rmx-prefetch="intent">d</a>
           <a id="self" href="/self" target="_self" data-rmx-prefetch="intent">d</a>
           <a id="cross" href="https://example.com/x" data-rmx-prefetch="intent">d</a>
           <a id="missing" href="/d" data-rmx-target="missing" data-rmx-prefetch="intent">d</a>
           <a id="here" href="#top" data-rmx-prefetch="intent">d</a>"##,
    );
    for id in ["download", "document", "blank", "self", "cross", "missing", "here"] {
        press(&f.get(&format!("#{id}")), "{ pointerType: 'mouse', isPrimary: true, button: 0 }");
        unhover(&f.get(&format!("#{id}")));
    }
    assert_eq!(take_calls(), [top("/self")]);
    prefetch("https://example.com/x", PrefetchOptions::default());
    prefetch(&web_sys::window().unwrap().location().href().unwrap(), PrefetchOptions::default());
    assert_eq!(take_calls(), [], "cross-origin and current-page prefetches are skipped");
    prefetch(&web_sys::window().unwrap().location().href().unwrap(), PrefetchOptions { target: Some("side".into()) });
    let here = web_sys::Url::new(&web_sys::window().unwrap().location().href().unwrap()).unwrap();
    here.set_hash("");
    assert_eq!(take_calls(), [(here.href(), Some("side".into()))], "a named frame may load the current URL");
}

/// `fetch` answering every request with `html`; restored on drop.
struct FetchStub(JsValue);

impl FetchStub {
    fn html(html: &str) -> FetchStub {
        let f = js_sys::Function::new_with_args(
            "html",
            "const original = window.fetch; window.fetch = async () => new Response(html, { headers: { 'Content-Type': 'text/html' } }); return () => { window.fetch = original };",
        );
        FetchStub(f.call1(&JsValue::NULL, &html.into()).unwrap())
    }
}

impl Drop for FetchStub {
    fn drop(&mut self) {
        self.0.unchecked_ref::<js_sys::Function>().call0(&JsValue::NULL).unwrap();
    }
}

#[wasm_bindgen_test]
async fn prefetches_named_frame_links_with_their_target_and_source_override() {
    let _serial = serial().await;
    start().await;
    let _stub = FetchStub::html("<p>side</p>");
    let host = document().create_element("div").unwrap();
    document().body().unwrap().append_child(&host).unwrap();
    let root = create_root(&host);
    root.render(rsx! { <Frame name="side" src="/frame"/> });
    root.flush();
    sleep(50).await;
    let f = Fixture::new(
        r#"<a id="src" href="/page?x" data-rmx-target="side" data-rmx-src="/fragment#f" data-rmx-prefetch="intent">s</a>
           <a id="href" href="/page2" data-rmx-target="side" data-rmx-prefetch="intent">h</a>
           <a id="ignored" href="/page3" data-rmx-src="/ignored" data-rmx-prefetch="intent">i</a>
           <a id="bad" href="/page4" data-rmx-src="https://example.com/x" data-rmx-prefetch="intent">b</a>"#,
    );
    for id in ["src", "href", "ignored", "bad"] {
        press(&f.get(&format!("#{id}")), "{ pointerType: 'mouse', isPrimary: true, button: 0 }");
        unhover(&f.get(&format!("#{id}")));
    }
    assert_eq!(take_calls(), [(absolute("/fragment"), Some("side".into())), (absolute("/page2"), Some("side".into())), top("/page3")], "a source override only applies to named frames, and must be same-origin");
    root.dispose();
    host.remove();
}

#[wasm_bindgen_test]
async fn prefetches_viewport_links_once_when_they_enter_view_and_observes_changes() {
    let _serial = serial().await;
    start().await;
    web_sys::window().unwrap().scroll_to_with_x_and_y(0.0, 0.0);
    let f = Fixture::new(r#"<a id="seen" href="/seen" data-rmx-prefetch="viewport">Seen</a><div style="height: 5000px"></div><a id="later" href="/later" data-rmx-prefetch="viewport">Later</a>"#);
    assert_eq!(wait_calls().await, [top("/seen")], "only links in view");
    f.get("#later").scroll_into_view();
    assert_eq!(wait_calls().await, [top("/later")]);
    web_sys::window().unwrap().scroll_to_with_x_and_y(0.0, 0.0);
    sleep(100).await;
    f.get("#seen").scroll_into_view();
    sleep(100).await;
    assert_eq!(take_calls(), [], "reentering view does not repeat");
    f.get("#seen").set_attribute("href", "/seen#hash").unwrap();
    sleep(100).await;
    assert_eq!(take_calls(), [], "a fragment change is the same destination");
    f.get("#seen").set_attribute("href", "/changed").unwrap();
    assert_eq!(wait_calls().await, [top("/changed")], "a changed destination prefetches");

    let inserted = document().create_element("section").unwrap();
    inserted.set_inner_html(r#"<a href="/inserted" data-rmx-prefetch="viewport">New</a><a id="marked" href="/marked">Marked later</a>"#);
    f.0.insert_before(&inserted, f.0.first_child().as_ref()).unwrap();
    // Scroll anchoring may have kept the old content in view.
    web_sys::window().unwrap().scroll_to_with_x_and_y(0.0, 0.0);
    assert_eq!(wait_calls().await, [top("/inserted")], "inserted subtrees are scanned");
    f.get("#marked").set_attribute("data-rmx-prefetch", "viewport").unwrap();
    assert_eq!(wait_calls().await, [top("/marked")], "links marked after insertion are observed");
    web_sys::window().unwrap().scroll_to_with_x_and_y(0.0, 0.0);
}

#[wasm_bindgen_test]
async fn does_not_prefetch_ineligible_viewport_links_until_they_become_eligible() {
    let _serial = serial().await;
    start().await;
    let f = Fixture::new(r#"<a id="a" href="/eligible" download data-rmx-prefetch="viewport">a</a>"#);
    sleep(100).await;
    assert_eq!(take_calls(), []);
    f.get("#a").remove_attribute("download").unwrap();
    assert_eq!(wait_calls().await, [top("/eligible")]);
}
