//! Ports of Remix's custom `resolveFrame` cases from `frame.test.ts` and
//! `frame.test.tsx`: what a resolver receives for reloads and submissions, redirected
//! responses, and node content. Run with
//! `pwsh ./test-browser.ps1 -Test browser_frame_resolver`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::cell::RefCell;
use std::rc::Rc;

use rmx::{create_root, rsx, Frame, FrameContent, FrameHandle, FrameRequest, FrameResponse, Frames, ReloadOptions, Root, RunOptions};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

thread_local! {
    static APP: RefCell<Option<rmx::App>> = const { RefCell::new(None) };
    static REQUESTS: RefCell<Vec<FrameRequest>> = const { RefCell::new(Vec::new()) };
    /// What the resolver answers: HTML plus an optional redirected URL, or a node.
    static NEXT: RefCell<Answer> = RefCell::new(Answer::Html("<p id=\"content\">Content</p>".into(), None));
}

#[derive(Clone)]
enum Answer {
    Html(String, Option<String>),
    Node,
}

fn answer(next: Answer) {
    NEXT.with(|n| *n.borrow_mut() = next);
}

fn resolve(request: FrameRequest) -> rmx::ResolveFuture {
    let by_src = request.url.ends_with("/error-node");
    REQUESTS.with(|r| r.borrow_mut().push(request));
    let next = NEXT.with(|n| n.borrow().clone());
    Box::pin(async move {
        if by_src {
            return Ok(FrameResponse {
                content: FrameContent::Node(rsx! { <section id="frame-error"><h2>"Frame Error"</h2><p>"Retry the page."</p></section> }),
                url: None,
            });
        }
        Ok(match next {
            Answer::Html(html, url) => FrameResponse { content: FrameContent::Html(html), url },
            Answer::Node => FrameResponse { content: FrameContent::Node(rsx! { <p id="node">"From a node"</p> }), url: None },
        })
    })
}

async fn start() {
    if APP.with(|a| a.borrow().is_none()) {
        let app = rmx::run(RunOptions { resolve_frame: Some(Rc::new(resolve)), ..Default::default() });
        APP.with(|a| *a.borrow_mut() = Some(app));
    }
    let app = APP.with(|a| a.borrow_mut().take()).expect("app");
    app.ready().await;
    APP.with(|a| *a.borrow_mut() = Some(app));
}

fn requests() -> Vec<FrameRequest> {
    REQUESTS.with(|r| r.borrow().clone())
}

fn clear() {
    REQUESTS.with(|r| r.borrow_mut().clear());
}

async fn tick() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

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

/// Mount `<Frame name=.. src="/frame"/>` and wait for its first load.
async fn mount(name: &str) -> Mounted {
    start().await;
    answer(Answer::Html(r#"<p id="content">Content</p>"#.into(), None));
    clear();
    let document = web_sys::window().unwrap().document().unwrap();
    let el: web_sys::HtmlElement = document.create_element("div").unwrap().unchecked_into();
    document.body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    root.render(rsx! { <Frame name={name.to_string()} src="/frame"/> });
    root.flush();
    let mounted = Mounted { el, root };
    for _ in 0..200 {
        if mounted.el.query_selector("*").unwrap().is_some() {
            break;
        }
        tick().await;
    }
    assert_eq!(requests().len(), 1, "the frame loads once when mounted");
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

#[wasm_bindgen_test]
async fn forwards_the_frame_name_as_the_target_when_mounting_a_named_frame() {
    let m = mount("details").await;
    let request = &requests()[0];
    assert_eq!(request.target.as_deref(), Some("details"));
    assert_eq!(request.method, "GET");
    assert_eq!(request.url, absolute("/frame"));
    assert_eq!(m.text_of("#content").as_deref(), Some("Content"));
}

#[wasm_bindgen_test]
async fn passes_form_submission_options_to_the_resolver() {
    let _m = mount("submit").await;
    clear();

    frame("submit")
        .reload_async(ReloadOptions {
            src: Some("/save".into()),
            method: Some("post".into()),
            enc_type: Some("Multipart/Form-Data".into()),
            body: fields(&[("name", "Ada"), ("intent", "save")]),
        })
        .await;

    let request = &requests()[0];
    assert_eq!(request.url, absolute("/save"));
    assert_eq!(request.method, "POST");
    assert_eq!(request.enc_type.as_deref(), Some("multipart/form-data"));
    assert_eq!(request.form, fields(&[("name", "Ada"), ("intent", "save")]));
    assert_eq!(request.target.as_deref(), Some("submit"));
}

#[wasm_bindgen_test]
async fn normalizes_an_invalid_encoding_for_the_resolver() {
    let _m = mount("encoding").await;
    clear();

    frame("encoding").reload_async(ReloadOptions { method: Some("post".into()), enc_type: Some("invalid".into()), body: fields(&[("a", "1")]), ..Default::default() }).await;

    assert_eq!(requests()[0].enc_type.as_deref(), Some("application/x-www-form-urlencoded"));
}

#[wasm_bindgen_test]
async fn omits_submission_options_from_plain_reloads() {
    let _m = mount("plain").await;
    clear();

    frame("plain").reload_async(ReloadOptions::default()).await;

    let request = &requests()[0];
    assert_eq!(request.method, "GET");
    assert_eq!(request.form, None);
    assert_eq!(request.enc_type, None);
}

#[wasm_bindgen_test]
async fn reloads_a_get_source_with_query_parameters_without_body_metadata() {
    let _m = mount("get").await;
    clear();

    frame("get").reload_async(ReloadOptions { src: Some("/search".into()), body: fields(&[("q", "rust")]), ..Default::default() }).await;

    let request = &requests()[0];
    assert_eq!(request.url, absolute("/search?q=rust"));
    assert_eq!(request.method, "GET");
    assert_eq!(request.form, None);
    assert_eq!(request.enc_type, None);
}

#[wasm_bindgen_test]
async fn renders_a_redirected_response_without_changing_the_frame_source() {
    let m = mount("redirect").await;
    answer(Answer::Html(r#"<main id="result">Settings overview</main>"#.into(), Some("https://example.com/settings/overview".into())));

    let f = frame("redirect");
    f.reload_async(ReloadOptions { src: Some("https://example.com/settings".into()), ..Default::default() }).await;

    assert_eq!(m.text_of("#result").as_deref(), Some("Settings overview"));
    assert_eq!(f.src(), "https://example.com/settings");
}

#[wasm_bindgen_test]
async fn renders_node_results_from_the_resolver() {
    let m = mount("node").await;
    answer(Answer::Node);

    frame("node").reload_async(ReloadOptions::default()).await;

    assert_eq!(m.text_of("#node").as_deref(), Some("From a node"));
    assert_eq!(m.text_of("#content"), None);
}

#[wasm_bindgen_test]
async fn renders_node_results_when_a_client_frame_src_changes() {
    start().await;
    answer(Answer::Html(r#"<div id="stale">Stale content</div>"#.into(), None));
    let document = web_sys::window().unwrap().document().unwrap();
    let el: web_sys::HtmlElement = document.create_element("div").unwrap().unchecked_into();
    document.body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    let m = Mounted { el, root };
    let page = |src: &str| rsx! { <Frame src={src.to_string()} fallback={rsx! { <p id="fallback">"Loading..."</p> }}/> };
    m.root.render(page("/html"));
    m.root.flush();
    for _ in 0..200 {
        if m.text_of("#stale").is_some() {
            break;
        }
        tick().await;
    }
    assert_eq!(m.text_of("#stale").as_deref(), Some("Stale content"));

    m.root.render(page("/error-node"));
    m.root.flush();
    for _ in 0..200 {
        if m.text_of("#frame-error h2").is_some() {
            break;
        }
        tick().await;
    }
    assert_eq!(m.text_of("#frame-error h2").as_deref(), Some("Frame Error"));
    assert_eq!(m.text_of("#frame-error p").as_deref(), Some("Retry the page."));
    assert_eq!(m.text_of("#stale"), None);
}
