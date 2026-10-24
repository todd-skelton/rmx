//! Ports of the default resolver cases of Remix's `frame.test.tsx` (`run`): the request a
//! frame load sends (headers, method, body encoding, source) and which responses render.
//! Remix runs most of them against the top frame; here they use a named client-created
//! `<Frame>`, since a top frame reload would replace the test runner's page. `fetch` is
//! replaced by a stub that records each request. Run with
//! `pwsh ./scripts/test-browser.ps1 -Test browser_frame_requests`.
#![cfg(target_arch = "wasm32")]

extern crate rsmix_component as rsmix;

use std::cell::RefCell;
use std::rc::Rc;

use rsmix::{create_root, rsx, Frame, FrameHandle, Frames, ReloadOptions, Root, RunOptions};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

thread_local! {
    static APP: RefCell<Option<rsmix::App>> = const { RefCell::new(None) };
}

/// Start the runtime once for the page.
async fn start() {
    if APP.with(|a| a.borrow().is_none()) {
        APP.with(|a| *a.borrow_mut() = Some(rsmix::run(RunOptions::default())));
    }
    let app = APP.with(|a| a.borrow_mut().take()).expect("app");
    app.ready().await;
    APP.with(|a| *a.borrow_mut() = Some(app));
}

fn get(target: &JsValue, key: &str) -> JsValue {
    js_sys::Reflect::get(target, &key.into()).unwrap_or(JsValue::UNDEFINED)
}

async fn tick() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

/// `window.fetch` replaced by a stub that records `new Request(input, init)` with the raw
/// arguments and answers every call with the same response. Dropping it restores fetch.
struct FetchStub {
    stub: JsValue,
}

impl FetchStub {
    fn new(body: &str, status: u16, status_text: &str, content_type: Option<&str>) -> FetchStub {
        FetchStub::with_redirect(body, status, status_text, content_type, None)
    }

    /// Responses that report `redirected` with `url` when `redirected_to` is set.
    fn with_redirect(body: &str, status: u16, status_text: &str, content_type: Option<&str>, redirected_to: Option<&str>) -> FetchStub {
        let make = js_sys::Function::new_with_args(
            "body,status,statusText,contentType,redirectedTo",
            "const original = window.fetch; const calls = [];
             window.fetch = async (input, init) => {
               calls.push({ input, init, request: new Request(typeof input === 'string' ? new URL(input, document.baseURI) : input, init) });
               const headers = contentType ? { 'Content-Type': contentType } : {};
               const response = new Response(body, { status, statusText, headers });
               if (redirectedTo) Object.defineProperties(response, { redirected: { value: true }, url: { value: redirectedTo } });
               return response;
             };
             return { calls, restore() { window.fetch = original } };",
        );
        let opt = |v: Option<&str>| v.map_or(JsValue::UNDEFINED, JsValue::from_str);
        let args = js_sys::Array::of5(&body.into(), &status.into(), &status_text.into(), &opt(content_type), &opt(redirected_to));
        FetchStub { stub: make.apply(&JsValue::NULL, &args).unwrap() }
    }

    fn html(body: &str) -> FetchStub {
        FetchStub::new(body, 200, "OK", None)
    }

    fn calls(&self) -> js_sys::Array {
        get(&self.stub, "calls").unchecked_into()
    }

    fn count(&self) -> u32 {
        self.calls().length()
    }

    fn clear(&self) {
        self.calls().set_length(0);
    }

    fn input(&self, i: u32) -> String {
        get(&self.calls().get(i), "input").as_string().unwrap()
    }

    fn init(&self, i: u32) -> JsValue {
        get(&self.calls().get(i), "init")
    }

    fn request(&self, i: u32) -> web_sys::Request {
        get(&self.calls().get(i), "request").unchecked_into()
    }
}

impl Drop for FetchStub {
    fn drop(&mut self) {
        let restore: js_sys::Function = get(&self.stub, "restore").unchecked_into();
        restore.call0(&self.stub).unwrap();
    }
}

async fn text(request: &web_sys::Request) -> String {
    JsFuture::from(request.text().unwrap()).await.unwrap().as_string().unwrap()
}

fn header(request: &web_sys::Request, name: &str) -> Option<String> {
    request.headers().get(name).unwrap()
}

/// A client-created frame in a container in the page, loaded from the stub.
struct Mounted {
    el: web_sys::HtmlElement,
    root: Root,
}

impl Drop for Mounted {
    fn drop(&mut self) {
        self.root.dispose();
        self.el.remove();
    }
}

impl Mounted {
    fn text_of(&self, selector: &str) -> Option<String> {
        self.el.query_selector(selector).unwrap().and_then(|e| e.text_content())
    }
}

/// Mount `<Frame name=.. src="/frame"/>` and wait for its first load to finish.
async fn mount(stub: &FetchStub, name: Option<&str>) -> Mounted {
    start().await;
    let document = web_sys::window().unwrap().document().unwrap();
    let el: web_sys::HtmlElement = document.create_element("div").unwrap().unchecked_into();
    document.body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    let node = match name {
        Some(name) => rsx! { <Frame name={name.to_string()} src="/frame"/> },
        None => rsx! { <Frame src="/frame"/> },
    };
    root.render(node);
    root.flush();
    let mounted = Mounted { el, root };
    for _ in 0..200 {
        if stub.count() > 0 && mounted.el.query_selector("*").unwrap().is_some() {
            break;
        }
        tick().await;
    }
    assert_eq!(stub.count(), 1, "the frame loads once when mounted");
    mounted
}

fn frame(name: &str) -> FrameHandle {
    Frames.get(name).expect("a mounted frame")
}

fn fields(list: &[(&str, &str)]) -> Option<Vec<(String, String)>> {
    Some(list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
}

fn absolute(path: &str) -> String {
    let base = web_sys::window().unwrap().document().unwrap().base_uri().unwrap().unwrap();
    web_sys::Url::new_with_base(path, &base).unwrap().href()
}

/// Collect the app's error reports while the returned guard lives.
fn errors() -> (Rc<RefCell<Vec<String>>>, ErrorGuard) {
    let seen: Rc<RefCell<Vec<String>>> = Rc::default();
    let s = seen.clone();
    let id = APP.with(|a| a.borrow().as_ref().unwrap().on_error(move |e| s.borrow_mut().push(e.message.clone())));
    (seen, ErrorGuard(Some(id)))
}

struct ErrorGuard(Option<rsmix::ListenerId>);

impl Drop for ErrorGuard {
    fn drop(&mut self) {
        if let Some(id) = self.0.take() {
            APP.with(|a| a.borrow().as_ref().unwrap().remove_error_listener(id));
        }
    }
}

const CONTENT: &str = r#"<p id="frame-content">Frame content</p>"#;

#[wasm_bindgen_test]
async fn sends_frame_headers_and_the_target_when_reloading_a_named_frame() {
    let stub = FetchStub::html(CONTENT);
    let m = mount(&stub, Some("details")).await;
    stub.clear();

    let _ = frame("details").reload_async(ReloadOptions::default()).await;

    assert_eq!(stub.count(), 1);
    assert_eq!(stub.input(0), "/frame");
    assert_eq!(get(&stub.init(0), "mode").as_string().as_deref(), Some("same-origin"));
    let request = stub.request(0);
    assert_eq!(header(&request, "Accept").as_deref(), Some("text/html"));
    assert_eq!(header(&request, "X-Remix-Frame").as_deref(), Some("true"));
    assert_eq!(header(&request, "X-Remix-Target").as_deref(), Some("details"));
    assert_eq!(header(&request, "X-Remix-Top-Frame-Src"), None);
    assert_eq!(m.text_of("#frame-content").as_deref(), Some("Frame content"));
}

#[wasm_bindgen_test]
async fn sends_frame_headers_without_a_target_when_loading_an_unnamed_frame() {
    let stub = FetchStub::html(CONTENT);
    let m = mount(&stub, None).await;

    assert_eq!(stub.input(0), "/frame");
    assert_eq!(get(&stub.init(0), "mode").as_string().as_deref(), Some("same-origin"));
    let request = stub.request(0);
    assert_eq!(header(&request, "Accept").as_deref(), Some("text/html"));
    assert_eq!(header(&request, "X-Remix-Frame").as_deref(), Some("true"));
    assert_eq!(header(&request, "X-Remix-Target"), None);
    assert_eq!(m.text_of("#frame-content").as_deref(), Some("Frame content"));
}

#[wasm_bindgen_test]
async fn retains_the_requested_source_without_retaining_the_previous_method_or_body() {
    let stub = FetchStub::html("<main>Saved</main>");
    let _m = mount(&stub, Some("body-retain")).await;
    stub.clear();

    let f = frame("body-retain");
    let _ = f.reload_async(ReloadOptions { src: Some("/save".into()), method: Some("post".into()), body: fields(&[("name", "Ada")]), ..Default::default() }).await;
    let _ = f.reload_async(ReloadOptions::default()).await;

    assert_eq!(stub.count(), 2);
    assert_eq!(stub.request(0).method(), "POST");
    let second = stub.request(1);
    assert_eq!(second.method(), "GET");
    assert_eq!(second.url(), absolute("/save"));
    assert!(second.body().is_none());
}

#[wasm_bindgen_test]
async fn reloads_with_multipart_form_data_and_lets_fetch_generate_its_boundary() {
    let stub = FetchStub::html("<main>Saved</main>");
    let _m = mount(&stub, Some("multipart")).await;
    stub.clear();

    let _ = frame("multipart")
        .reload_async(ReloadOptions { method: Some("post".into()), enc_type: Some("multipart/form-data".into()), body: fields(&[("name", "Ada")]), ..Default::default() })
        .await;

    assert_eq!(stub.count(), 1);
    let request = stub.request(0);
    assert!(header(&request, "Content-Type").unwrap().starts_with("multipart/form-data; boundary="));
    let form: web_sys::FormData = JsFuture::from(request.form_data().unwrap()).await.unwrap().unchecked_into();
    assert_eq!(form.get("name").as_string().as_deref(), Some("Ada"));
}

#[wasm_bindgen_test]
async fn reloads_with_text_plain_form_data_and_normalizes_line_breaks() {
    let stub = FetchStub::html("<main>Saved</main>");
    let _m = mount(&stub, Some("plain")).await;
    stub.clear();

    let _ = frame("plain")
        .reload_async(ReloadOptions { method: Some("post".into()), enc_type: Some("text/plain".into()), body: fields(&[("name", "Ada\nLovelace")]), ..Default::default() })
        .await;

    assert_eq!(stub.count(), 1);
    let request = stub.request(0);
    assert_eq!(header(&request, "Content-Type").as_deref(), Some("text/plain"));
    assert_eq!(text(&request).await, "name=Ada\r\nLovelace\r\n");
}

#[wasm_bindgen_test]
async fn defaults_reloads_with_fields_to_get_and_replaces_the_source_query_without_a_body() {
    let stub = FetchStub::html("<main>Saved</main>");
    let m = mount(&stub, Some("get-fields")).await;
    stub.clear();

    let f = frame("get-fields");
    let _ = f.reload_async(ReloadOptions {
        src: Some("/search?old=1#results".into()),
        enc_type: Some("multipart/form-data".into()),
        body: fields(&[("name", "Ada Lovelace"), ("name", "Grace")]),
        ..Default::default()
    })
    .await;

    assert_eq!(stub.count(), 1);
    let request = stub.request(0);
    let expected = absolute("/search?name=Ada+Lovelace&name=Grace#results");
    assert_eq!(request.url(), expected);
    assert_eq!(request.method(), "GET");
    assert!(request.body().is_none());
    assert_eq!(f.src(), expected);
    assert_eq!(m.text_of("main").as_deref(), Some("Saved"));
}

#[wasm_bindgen_test]
async fn defaults_post_fields_to_url_encoding_with_normalized_line_breaks() {
    let stub = FetchStub::html("<main>Saved</main>");
    let _m = mount(&stub, Some("urlencoded")).await;
    stub.clear();

    let _ = frame("urlencoded").reload_async(ReloadOptions { method: Some("post".into()), body: fields(&[("name\nline", "Ada\nLovelace")]), ..Default::default() }).await;

    assert_eq!(stub.count(), 1);
    let request = stub.request(0);
    assert_eq!(header(&request, "Content-Type").as_deref(), Some("application/x-www-form-urlencoded"));
    assert_eq!(text(&request).await, "name%0D%0Aline=Ada%0D%0ALovelace");
}

#[wasm_bindgen_test]
async fn defaults_an_invalid_form_method_to_get_and_encodes_its_fields_in_the_query() {
    let stub = FetchStub::html("<main>Saved</main>");
    let _m = mount(&stub, Some("invalid-method")).await;
    stub.clear();

    let _ = frame("invalid-method")
        .reload_async(ReloadOptions { src: Some("/search?old=1".into()), method: Some("PATCH".into()), body: fields(&[("name", "Ada")]), ..Default::default() })
        .await;

    assert_eq!(stub.count(), 1);
    let request = stub.request(0);
    assert_eq!(request.method(), "GET");
    assert_eq!(request.url(), absolute("/search?name=Ada"));
    assert!(request.body().is_none());
    assert_eq!(header(&request, "Content-Type"), None);
}

#[wasm_bindgen_test]
async fn accepts_case_insensitive_form_methods_and_encodings() {
    let stub = FetchStub::html("<main>Saved</main>");
    let _m = mount(&stub, Some("case")).await;
    stub.clear();

    let _ = frame("case")
        .reload_async(ReloadOptions { method: Some("PoSt".into()), enc_type: Some("TEXT/PLAIN".into()), body: fields(&[("name", "Ada")]), ..Default::default() })
        .await;

    assert_eq!(stub.count(), 1);
    let request = stub.request(0);
    assert_eq!(request.method(), "POST");
    assert_eq!(header(&request, "Content-Type").as_deref(), Some("text/plain"));
    assert_eq!(text(&request).await, "name=Ada\r\n");
}

#[wasm_bindgen_test]
async fn falls_back_to_url_encoding_for_an_invalid_encoding() {
    let stub = FetchStub::html("<main>Saved</main>");
    let _m = mount(&stub, Some("invalid-enc")).await;
    stub.clear();

    let _ = frame("invalid-enc")
        .reload_async(ReloadOptions { method: Some("post".into()), enc_type: Some("invalid".into()), body: fields(&[("name", "Ada")]), ..Default::default() })
        .await;

    assert_eq!(stub.count(), 1);
    let request = stub.request(0);
    assert_eq!(header(&request, "Content-Type").as_deref(), Some("application/x-www-form-urlencoded"));
    assert_eq!(text(&request).await, "name=Ada");
}

#[wasm_bindgen_test]
async fn submits_fields_through_one_default_fetch_and_renders_the_returned_html_without_navigation() {
    let stub = FetchStub::html(r#"<main id="saved">Saved</main>"#);
    let m = mount(&stub, Some("submit")).await;
    stub.clear();
    let window = web_sys::window().unwrap();
    let initial_url = window.location().href().unwrap();
    let initial_entries = window.history().unwrap().length().unwrap();

    let _ = frame("submit")
        .reload_async(ReloadOptions {
            src: Some("/account/save".into()),
            method: Some("post".into()),
            enc_type: Some("application/x-www-form-urlencoded".into()),
            body: fields(&[("name", "Ada Lovelace")]),
        })
        .await;

    assert_eq!(stub.count(), 1);
    assert_eq!(stub.input(0), "/account/save");
    let body = get(&stub.init(0), "body");
    assert!(body.is_instance_of::<web_sys::UrlSearchParams>());
    assert_eq!(String::from(body.unchecked_into::<web_sys::UrlSearchParams>().to_string()), "name=Ada+Lovelace");
    assert_eq!(stub.request(0).method(), "POST");
    assert_eq!(m.text_of("#saved").as_deref(), Some("Saved"));
    assert_eq!(window.location().href().unwrap(), initial_url);
    assert_eq!(window.history().unwrap().length().unwrap(), initial_entries);
}

#[wasm_bindgen_test]
async fn uses_same_origin_requests_for_explicitly_cross_origin_frame_sources() {
    let stub = FetchStub::html(r#"<main id="external">External frame</main>"#);
    let _m = mount(&stub, Some("cross-origin")).await;
    stub.clear();

    let _ = frame("cross-origin").reload_async(ReloadOptions { src: Some("https://frames.example/partial".into()), ..Default::default() }).await;

    assert_eq!(stub.input(0), "https://frames.example/partial");
    assert_eq!(get(&stub.init(0), "mode").as_string().as_deref(), Some("same-origin"));
}

#[wasm_bindgen_test]
async fn renders_3xx_html_responses_from_the_default_resolver() {
    let stub = FetchStub::html(CONTENT);
    let m = mount(&stub, Some("three")).await;
    drop(stub);
    let stub = FetchStub::new(r#"<main id="choices">Multiple choices</main>"#, 300, "Multiple Choices", Some("text/html"));

    let _ = frame("three").reload_async(ReloadOptions::default()).await;

    assert_eq!(stub.count(), 1);
    assert_eq!(m.text_of("#choices").as_deref(), Some("Multiple choices"));
}

#[wasm_bindgen_test]
async fn rejects_non_html_3xx_responses_from_the_default_resolver() {
    let stub = FetchStub::html(r#"<main id="initial">Initial</main>"#);
    let m = mount(&stub, Some("three-json")).await;
    drop(stub);
    let stub = FetchStub::new(r#"{"next":"/account"}"#, 300, "Multiple Choices", Some("application/json"));
    let (seen, _guard) = errors();

    let _ = frame("three-json").reload_async(ReloadOptions::default()).await;

    assert_eq!(stub.count(), 1);
    assert_eq!(*seen.borrow(), ["Failed to resolve frame: 300 Multiple Choices"]);
    assert_eq!(m.text_of("#initial").as_deref(), Some("Initial"));
}

#[wasm_bindgen_test]
async fn renders_4xx_html_responses_from_the_default_resolver() {
    let stub = FetchStub::html(CONTENT);
    let m = mount(&stub, Some("four")).await;
    drop(stub);
    let stub = FetchStub::new(r#"<p role="alert">Name is required</p>"#, 422, "Unprocessable Content", Some("Text/HTML; charset=utf-8"));

    let _ = frame("four").reload_async(ReloadOptions { method: Some("post".into()), body: fields(&[]), ..Default::default() }).await;

    assert_eq!(stub.count(), 1);
    assert_eq!(m.text_of("[role=alert]").as_deref(), Some("Name is required"));
}

#[wasm_bindgen_test]
async fn rejects_non_html_4xx_responses_from_the_default_resolver() {
    let stub = FetchStub::html(r#"<main id="initial">Initial</main>"#);
    let m = mount(&stub, Some("four-text")).await;
    drop(stub);
    let stub = FetchStub::new("Not Found", 404, "Not Found", Some("text/plain"));
    let (seen, _guard) = errors();

    let _ = frame("four-text").reload_async(ReloadOptions::default()).await;

    assert_eq!(stub.count(), 1);
    assert_eq!(*seen.borrow(), ["Failed to resolve frame: 404 Not Found"]);
    assert_eq!(m.text_of("#initial").as_deref(), Some("Initial"));
}

#[wasm_bindgen_test]
async fn rejects_5xx_html_responses_from_the_default_resolver() {
    let stub = FetchStub::html(r#"<main id="initial">Initial</main>"#);
    let m = mount(&stub, Some("five")).await;
    drop(stub);
    let stub = FetchStub::new(r#"<main id="error">Internal Server Error</main>"#, 500, "Internal Server Error", Some("text/html"));
    let (seen, _guard) = errors();

    let _ = frame("five").reload_async(ReloadOptions::default()).await;

    assert_eq!(stub.count(), 1);
    assert_eq!(*seen.borrow(), ["Failed to resolve frame: 500 Internal Server Error"]);
    assert_eq!(m.text_of("#initial").as_deref(), Some("Initial"));
    assert_eq!(m.text_of("#error"), None);
}

#[wasm_bindgen_test]
async fn renders_a_redirected_reload_response_while_retaining_its_requested_source() {
    let stub = FetchStub::html(CONTENT);
    let m = mount(&stub, Some("redirected")).await;
    drop(stub);
    let _stub = FetchStub::with_redirect("<p>Saved</p>", 200, "OK", None, Some("https://example.com/account"));
    let initial_url = web_sys::window().unwrap().location().href().unwrap();

    let f = frame("redirected");
    let _ = f.reload_async(ReloadOptions { src: Some("https://example.com/save".into()), method: Some("post".into()), body: fields(&[]), ..Default::default() }).await;

    assert_eq!(m.text_of("p").as_deref(), Some("Saved"));
    assert_eq!(f.src(), "https://example.com/save");
    assert_eq!(web_sys::window().unwrap().location().href().unwrap(), initial_url);
}
