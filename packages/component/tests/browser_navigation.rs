//! Ports of Remix's `navigation.test.ts` decisions: which navigations the runtime
//! intercepts and which it leaves to the browser. Remix dispatches synthetic events at a
//! stub `navigation`; here real clicks, form submissions and `navigation.navigate` calls go
//! through the real Navigation API. A spy on `NavigateEvent.prototype.intercept` records
//! the runtime's decision, and a listener added after the runtime's cancels every
//! navigation it did not intercept, so nothing leaves the test page. Intercepted
//! navigations only target named frames (a top frame load would replace the test page)
//! and resolve through a custom resolver. Run with
//! `pwsh ./scripts/test-browser.ps1 -Test browser_navigation`.
#![cfg(all(target_arch = "wasm32", feature = "navigation"))]

extern crate rsmix_component as rsmix;

use std::cell::RefCell;
use std::rc::Rc;

use rsmix::{create_root, navigate, rsx, Frame, FrameContent, FrameRequest, FrameResponse, Frames, NavigateOptions, Root, RunOptions};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

type Resolver = Box<dyn FnOnce(FrameRequest) -> rsmix::ResolveFuture>;

thread_local! {
    static APP: RefCell<Option<rsmix::App>> = const { RefCell::new(None) };
    static GUARD: RefCell<Option<JsValue>> = const { RefCell::new(None) };
    static REQUESTS: RefCell<Vec<FrameRequest>> = const { RefCell::new(Vec::new()) };
    /// When set, the next resolve waits for this promise.
    static HOLD: RefCell<Option<js_sys::Promise>> = const { RefCell::new(None) };
    /// When set, the next request is resolved by this function instead.
    static RESOLVE_NEXT: RefCell<Option<Resolver>> = const { RefCell::new(None) };
}

fn resolve(request: FrameRequest) -> rsmix::ResolveFuture {
    REQUESTS.with(|r| r.borrow_mut().push(request.clone()));
    if let Some(next) = RESOLVE_NEXT.with(|n| n.borrow_mut().take()) {
        return next(request);
    }
    let hold = HOLD.with(|h| h.borrow_mut().take());
    Box::pin(async move {
        if let Some(hold) = hold {
            let _ = JsFuture::from(hold).await;
        }
        Ok(FrameResponse { content: FrameContent::Html(r#"<p id="loaded">Loaded</p>"#.into()), url: None })
    })
}

fn window() -> web_sys::Window {
    web_sys::window().unwrap()
}

fn document() -> web_sys::Document {
    window().document().unwrap()
}

fn get(target: &JsValue, key: &str) -> JsValue {
    js_sys::Reflect::get(target, &key.into()).unwrap_or(JsValue::UNDEFINED)
}

fn navigation() -> JsValue {
    get(&window(), "navigation")
}

/// Start the runtime and the guard once for the page.
async fn start() {
    if APP.with(|a| a.borrow().is_none()) {
        let app = rsmix::run(RunOptions { resolve_frame: Some(Rc::new(resolve)), ..Default::default() });
        APP.with(|a| *a.borrow_mut() = Some(app));
        let install = js_sys::Function::new_with_args(
            "",
            "const proto = NavigateEvent.prototype;
             const intercept = proto.intercept;
             const scroll = proto.scroll;
             const log = { intercepted: new WeakMap(), events: [], scrolls: [] };
             proto.intercept = function (options) { log.intercepted.set(this, options || {}); return intercept.call(this, options) };
             proto.scroll = function () { log.scrolls.push(this.destination.url); return scroll.call(this) };
             navigation.addEventListener('navigate', (event) => {
               const options = log.intercepted.get(event);
               const intercepted = options !== undefined;
               log.events.push({ intercepted, url: event.destination.url, type: event.navigationType,
                 scroll: options && options.scroll, focusReset: options && options.focusReset });
               if (!intercepted && event.cancelable && !log.allow) event.preventDefault();
             });
             return log;",
        );
        let log = install.call0(&JsValue::NULL).unwrap();
        GUARD.with(|g| *g.borrow_mut() = Some(log));
    }
    let app = APP.with(|a| a.borrow_mut().take()).expect("app");
    app.ready().await;
    APP.with(|a| *a.borrow_mut() = Some(app));
}

fn events() -> js_sys::Array {
    GUARD.with(|g| get(g.borrow().as_ref().unwrap(), "events")).unchecked_into()
}

/// The destination URLs of the navigations whose `scroll()` ran since [`clear`].
fn scrolls() -> Vec<String> {
    let scrolls: js_sys::Array = GUARD.with(|g| get(g.borrow().as_ref().unwrap(), "scrolls")).unchecked_into();
    scrolls.iter().map(|s| s.as_string().unwrap()).collect()
}

/// Forget the navigations recorded so far.
fn clear() {
    events().set_length(0);
    GUARD.with(|g| get(g.borrow().as_ref().unwrap(), "scrolls")).unchecked_into::<js_sys::Array>().set_length(0);
    REQUESTS.with(|r| r.borrow_mut().clear());
}

/// Whether each navigation since [`clear`] was intercepted, in order.
fn decisions() -> Vec<bool> {
    events().iter().map(|e| get(&e, "intercepted").as_bool().unwrap()).collect()
}

async fn tick() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        window().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

/// Wait for the current navigation transition, if any, to finish.
async fn settle() {
    tick().await;
    let finished = get(&get(&navigation(), "transition"), "finished");
    if let Ok(finished) = finished.dyn_into::<js_sys::Promise>() {
        let _ = JsFuture::from(finished).await;
    }
    tick().await;
}

/// A mounted `<Frame name="details">` (loaded once through the resolver), plus a scratch
/// container for links and forms. Dropping it unmounts both.
struct Page {
    el: web_sys::HtmlElement,
    root: Root,
}

impl Drop for Page {
    fn drop(&mut self) {
        self.root.dispose();
        self.el.remove();
    }
}

impl Page {
    async fn new() -> Page {
        start().await;
        let el: web_sys::HtmlElement = document().create_element("div").unwrap().unchecked_into();
        document().body().unwrap().append_child(&el).unwrap();
        let root = create_root(&el);
        root.render(rsx! { <section><Frame name="details" src="/initial-frame"/></section> });
        root.flush();
        for _ in 0..200 {
            if el.query_selector("#loaded").unwrap().is_some() {
                break;
            }
            tick().await;
        }
        clear();
        Page { el, root }
    }

    /// Add `html` to the page and return its first element.
    fn add(&self, html: &str) -> web_sys::Element {
        let holder = document().create_element("div").unwrap();
        holder.set_inner_html(html);
        let first = holder.first_element_child().unwrap();
        self.el.append_child(&holder).unwrap();
        first
    }

    /// Click the first element `html` adds.
    async fn click(&self, html: &str) {
        let el = self.add(html);
        el.dispatch_event(&web_sys::MouseEvent::new_with_mouse_event_init_dict("click", &{
            let init = web_sys::MouseEventInit::new();
            init.set_bubbles(true);
            init.set_cancelable(true);
            init
        }).unwrap())
        .unwrap();
        settle().await;
    }

    /// Submit the form `html` adds, with the submitter matching `submitter` if given.
    async fn submit(&self, html: &str, submitter: Option<&str>) {
        let form: web_sys::HtmlFormElement = self.add(html).unchecked_into();
        match submitter {
            Some(selector) => {
                let button: web_sys::HtmlElement = form.query_selector(selector).unwrap().unwrap().unchecked_into();
                form.request_submit_with_submitter(Some(&button)).unwrap();
            }
            None => form.request_submit().unwrap(),
        }
        settle().await;
    }
}

/// A same-origin URL on the test page with `?case=name`.
fn page_url(case: &str) -> String {
    let url = web_sys::Url::new(&window().location().href().unwrap()).unwrap();
    url.set_hash("");
    url.set_search(&format!("?case={case}"));
    url.href()
}

/// The `scroll` and `focusReset` options of each intercepted navigation since [`clear`].
fn resets() -> Vec<(String, String)> {
    events()
        .iter()
        .filter(|e: &JsValue| get(e, "intercepted").as_bool() == Some(true))
        .map(|e| (get(&e, "scroll").as_string().unwrap_or_default(), get(&e, "focusReset").as_string().unwrap_or_default()))
        .collect()
}

fn entry_count() -> u32 {
    let entries_fn: js_sys::Function = get(&navigation(), "entries").unchecked_into();
    let entries: js_sys::Array = entries_fn.call0(&navigation()).unwrap().unchecked_into();
    entries.length()
}

fn current_url() -> String {
    window().location().href().unwrap()
}

fn resets_of(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(s, f)| (s.to_string(), f.to_string())).collect()
}

fn requests() -> Vec<FrameRequest> {
    REQUESTS.with(|r| r.borrow().clone())
}

/// A link to `page_url(case)` with the given extra attributes.
fn link(case: &str, attrs: &str) -> String {
    format!(r#"<a href="{}" {attrs}>Go</a>"#, page_url(case))
}

#[wasm_bindgen_test]
async fn reloads_only_the_target_frame_for_a_targeted_link() {
    let page = Page::new().await;
    page.click(&link("targeted", r#"data-rmx-target="details" data-rmx-src="/partial""#)).await;

    assert_eq!(decisions(), [true]);
    let requests = requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].url.ends_with("/partial"), "{}", requests[0].url);
    assert_eq!(requests[0].target.as_deref(), Some("details"));
    assert_eq!(Frames.get("details").unwrap().src(), "/partial");
    assert_eq!(Frames.top().src(), page_url("targeted"));
    assert_eq!(window().location().href().unwrap(), page_url("targeted"));
}

#[wasm_bindgen_test]
async fn intercepts_anchors_inside_svg() {
    let page = Page::new().await;
    let svg = page.add(&format!(
        r#"<svg><a href="{}" data-rmx-target="details" data-rmx-src="/svg-partial"><text>Go</text></a></svg>"#,
        page_url("svg")
    ));
    let text = svg.query_selector("text").unwrap().unwrap();
    let init = web_sys::MouseEventInit::new();
    init.set_bubbles(true);
    init.set_cancelable(true);
    text.dispatch_event(&web_sys::MouseEvent::new_with_mouse_event_init_dict("click", &init).unwrap()).unwrap();
    settle().await;
    assert_eq!(decisions(), [true]);
    assert_eq!(Frames.get("details").unwrap().src(), "/svg-partial");
}

#[wasm_bindgen_test]
async fn does_not_intercept_anchors_marked_for_document_navigation() {
    let page = Page::new().await;
    page.click(&link("document", r#"data-rmx-target="details" data-rmx-document"#)).await;
    assert_eq!(decisions(), [false]);
    assert!(requests().is_empty());
}

#[wasm_bindgen_test]
async fn does_not_intercept_navigations_to_a_cross_origin_destination() {
    let page = Page::new().await;
    page.click(r#"<a href="https://frames.example/next" data-rmx-target="details">Go</a>"#).await;
    assert_eq!(decisions(), [false]);
}

#[wasm_bindgen_test]
async fn does_not_intercept_hash_only_navigations() {
    let page = Page::new().await;
    page.click(r##"<a href="#details" data-rmx-target="details">Go</a>"##).await;
    assert_eq!(decisions(), [false]);
    assert!(requests().is_empty());
}

// navigation.test.ts: does not intercept dialog forms or forms submitted to a new browsing
// context. A real dialog submission fires no navigation and a `_blank` one navigates
// another window, so, as in Remix, synthetic navigate events reach the runtime's listener.
#[wasm_bindgen_test]
async fn does_not_intercept_dialog_forms_or_forms_submitted_to_a_new_browsing_context() {
    let page = Page::new().await;
    let dialog = page.add(&format!(r#"<form method="dialog" action="{}"><button>Close</button></form>"#, page_url("dialog")));
    let blank = page.add(&format!(r#"<form method="post" target="_blank" data-rmx-target="details" action="{}"><button>Open</button></form>"#, page_url("blank")));
    let submitter_blank = page.add(&format!(r#"<form method="post" data-rmx-target="details" action="{}"><button formtarget="_blank">Open</button></form>"#, page_url("formtarget")));
    let dispatch = js_sys::Function::new_with_args(
        "form, url",
        "const submitter = form.querySelector('button');
         form.dispatchEvent(new SubmitEvent('submit', { submitter, bubbles: true, cancelable: true }));
         const calls = [];
         const event = Object.assign(new Event('navigate'), {
           canIntercept: true, hashChange: false, downloadRequest: null, navigationType: 'push',
           sourceElement: form, formData: new FormData(form), info: undefined,
           signal: new AbortController().signal,
           destination: { url, getState: () => undefined },
           intercept: (options) => calls.push(options),
         });
         navigation.dispatchEvent(event);
         return calls.length;",
    );
    let mut intercepted = Vec::new();
    for (form, case) in [(&dialog, "dialog"), (&blank, "blank"), (&submitter_blank, "formtarget")] {
        let count = dispatch.call2(&JsValue::NULL, form, &page_url(case).into()).unwrap();
        intercepted.push(count.as_f64().unwrap());
    }
    tick().await;
    assert_eq!(intercepted, [0.0, 0.0, 0.0]);
    assert!(requests().is_empty());
}

// navigation.test.ts: leaves invalid forms to native constraint validation
#[wasm_bindgen_test]
async fn leaves_invalid_forms_to_native_constraint_validation() {
    let page = Page::new().await;
    js("window.__submits = 0; return 0");
    page.submit(
        &format!(r#"<form method="post" data-rmx-target="details" action="{}" onsubmit="window.__submits++"><input required name="title"></form>"#, page_url("invalid")),
        None,
    )
    .await;
    let invalid = js("return document.querySelector('form[action$=\"case=invalid\"] input').matches(':invalid')");
    assert_eq!(js("return window.__submits").as_f64(), Some(0.0), "no submit event");
    assert_eq!(invalid, JsValue::TRUE);
    assert!(decisions().is_empty(), "no navigation started");
    assert!(requests().is_empty());
}

#[wasm_bindgen_test]
async fn does_not_intercept_anchors_marked_for_download() {
    let page = Page::new().await;
    page.click(&link("download", r#"download="notes.txt" data-rmx-target="details""#)).await;
    page.click(&link("download-empty", r#"download data-rmx-target="details""#)).await;
    assert_eq!(decisions(), [false, false]);
    assert!(requests().is_empty());
}

#[wasm_bindgen_test]
async fn leaves_links_with_a_missing_named_target_to_document_navigation() {
    let page = Page::new().await;
    page.click(&link("missing", r#"data-rmx-target="missing" data-rmx-src="/partial""#)).await;
    assert_eq!(decisions(), [false]);
    assert!(requests().is_empty());
}

/// A targeted (or untargeted) link whose `data-rmx-src` is `src` is left to the browser.
async fn expect_document_navigation(src: &str, target: Option<&str>) {
    let page = Page::new().await;
    let target = target.map(|t| format!(r#"data-rmx-target="{t}""#)).unwrap_or_default();
    page.click(&link("unsafe", &format!(r#"{target} data-rmx-src="{src}""#))).await;
    assert_eq!(decisions(), [false], "src {src}");
    assert!(requests().is_empty(), "src {src}");
    assert_eq!(Frames.get("details").unwrap().src(), "/initial-frame");
}

#[wasm_bindgen_test]
async fn leaves_cross_origin_named_frame_link_sources_to_document_navigation() {
    expect_document_navigation("https://frames.example/partial", Some("details")).await;
}

#[wasm_bindgen_test]
async fn leaves_protocol_relative_cross_origin_sources_to_document_navigation() {
    expect_document_navigation("//frames.example/partial", Some("details")).await;
}

#[wasm_bindgen_test]
async fn leaves_malformed_named_frame_sources_to_document_navigation() {
    expect_document_navigation("https://[", Some("details")).await;
}

#[wasm_bindgen_test]
async fn leaves_cross_origin_sources_without_a_target_to_document_navigation() {
    expect_document_navigation("https://frames.example/partial", None).await;
}

#[wasm_bindgen_test]
async fn leaves_malformed_sources_with_a_missing_target_to_document_navigation() {
    expect_document_navigation("https://[", Some("missing")).await;
}

#[wasm_bindgen_test]
async fn leaves_data_url_frame_sources_to_document_navigation() {
    expect_document_navigation("data:text/html,&lt;p&gt;Frame&lt;/p&gt;", Some("details")).await;
}

#[wasm_bindgen_test]
async fn leaves_javascript_url_frame_sources_to_document_navigation() {
    expect_document_navigation("javascript:void(0)", Some("details")).await;
}

#[wasm_bindgen_test]
async fn checks_relative_named_frame_sources_against_the_document_base_url() {
    let base = document().create_element("base").unwrap();
    base.set_attribute("href", "https://frames.example/partials/").unwrap();
    document().head().unwrap().prepend_with_node_1(&base).unwrap();
    expect_document_navigation("details", Some("details")).await;
    base.remove();
}

#[wasm_bindgen_test]
async fn checks_sources_supplied_by_programmatic_navigate_calls() {
    let _page = Page::new().await;
    navigate(&page_url("programmatic-src"), NavigateOptions { target: Some("details".into()), src: Some("https://frames.example/partial".into()), ..Default::default() });
    settle().await;
    assert_eq!(decisions(), [false]);
    assert!(requests().is_empty());
}

#[wasm_bindgen_test]
async fn leaves_programmatic_navigations_with_a_missing_named_target_to_document_navigation() {
    let _page = Page::new().await;
    navigate(&page_url("programmatic-missing"), NavigateOptions { target: Some("missing".into()), ..Default::default() });
    settle().await;
    assert_eq!(decisions(), [false]);
    assert!(requests().is_empty());
}

#[wasm_bindgen_test]
async fn loads_a_fresh_document_without_intercepting_the_replacement_navigation() {
    let _page = Page::new().await;
    let options = js_sys::Object::new();
    js_sys::Reflect::set(&options, &"history".into(), &"replace".into()).unwrap();
    js_sys::Reflect::set(&options, &"info".into(), &"remix-document-reload".into()).unwrap();
    let state = js_sys::JSON::parse(r#"{"$rmx":true,"target":"details","src":"/partial"}"#).unwrap();
    js_sys::Reflect::set(&options, &"state".into(), &state).unwrap();
    let navigate: js_sys::Function = get(&navigation(), "navigate").unchecked_into();
    let _ = navigate.call2(&navigation(), &page_url("document-reload").into(), &options);
    settle().await;
    assert_eq!(decisions(), [false]);
    assert!(requests().is_empty());
}

#[wasm_bindgen_test]
async fn reloads_a_targeted_frame_with_submitter_overridden_submission_metadata() {
    let page = Page::new().await;
    page.submit(
        &format!(
            r#"<form action="{}" method="get" data-rmx-target="missing"><input name="name" value="Ada"><button name="intent" value="save" formmethod="post" formenctype="multipart/form-data" data-rmx-target="details">Save</button></form>"#,
            page_url("submitter")
        ),
        Some("button"),
    )
    .await;
    assert_eq!(decisions(), [true]);
    let requests = requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].enc_type.as_deref(), Some("multipart/form-data"));
    assert_eq!(requests[0].target.as_deref(), Some("details"));
    let form = requests[0].form.clone().unwrap();
    assert_eq!(form, [("name".to_string(), "Ada".to_string()), ("intent".to_string(), "save".to_string())]);
}

#[wasm_bindgen_test]
async fn reloads_get_form_navigations_like_link_navigations() {
    let page = Page::new().await;
    page.submit(&format!(r#"<form action="{}" method="get" data-rmx-target="details"><input name="q" value="rust"></form>"#, page_url("get-form")), None).await;
    assert_eq!(decisions(), [true]);
    let requests = requests();
    assert_eq!(requests[0].method, "GET");
    assert!(requests[0].url.contains("q=rust"), "{}", requests[0].url);
    assert_eq!(requests[0].form, None);
}

#[wasm_bindgen_test]
async fn leaves_post_submissions_with_a_missing_named_target_to_document_navigation() {
    let page = Page::new().await;
    page.submit(&format!(r#"<form action="{}" method="post" data-rmx-target="missing"><input name="a" value="1"></form>"#, page_url("post-missing")), None).await;
    assert_eq!(decisions(), [false]);
    assert!(requests().is_empty());
}

#[wasm_bindgen_test]
async fn does_not_intercept_forms_that_opt_into_document_navigation() {
    let page = Page::new().await;
    page.submit(&format!(r#"<form action="{}" method="post" data-rmx-target="details" data-rmx-document></form>"#, page_url("form-document")), None).await;
    page.submit(
        &format!(r#"<form action="{}" method="post" data-rmx-target="details"><button data-rmx-document>Go</button></form>"#, page_url("submitter-document")),
        Some("button"),
    )
    .await;
    assert_eq!(decisions(), [false, false]);
    assert!(requests().is_empty());
}

#[wasm_bindgen_test]
async fn checks_form_sources_before_intercepting_a_post_submission() {
    let page = Page::new().await;
    page.submit(
        &format!(r#"<form action="{}" method="post" data-rmx-target="details" data-rmx-src="https://frames.example/save"></form>"#, page_url("post-src")),
        None,
    )
    .await;
    assert_eq!(decisions(), [false]);
    assert!(requests().is_empty());
}

#[wasm_bindgen_test]
async fn checks_get_form_sources_overridden_by_the_submitter() {
    let page = Page::new().await;
    page.submit(
        &format!(
            r#"<form action="{}" method="get" data-rmx-target="details" data-rmx-src="/safe"><button data-rmx-src="https://frames.example/search">Go</button></form>"#,
            page_url("get-src")
        ),
        Some("button"),
    )
    .await;
    assert_eq!(decisions(), [false]);
    assert!(requests().is_empty());
}

#[wasm_bindgen_test]
async fn reloads_the_destination_document_when_traversing_to_a_missing_named_target() {
    let page = Page::new().await;
    page.click(&link("traverse-a", r#"data-rmx-target="details" data-rmx-src="/a""#)).await;
    page.click(&link("traverse-b", r#"data-rmx-target="details" data-rmx-src="/b""#)).await;
    assert_eq!(decisions(), [true, true]);
    drop(page);
    clear();

    // Record reloads instead of reloading the test page.
    let reloads = js_sys::Array::new();
    let stub = js_sys::Function::new_with_args("log", "return function (options) { log.push(options && options.info); return { committed: Promise.resolve(), finished: Promise.resolve() } }");
    let reload = stub.call1(&JsValue::NULL, &reloads).unwrap();
    js_sys::Reflect::set(&navigation(), &"reload".into(), &reload).unwrap();

    let back: js_sys::Function = get(&navigation(), "back").unchecked_into();
    let result = back.call0(&navigation()).unwrap();
    let _ = JsFuture::from(get(&result, "finished").unchecked_into::<js_sys::Promise>()).await;
    settle().await;
    js_sys::Reflect::delete_property(navigation().unchecked_ref::<js_sys::Object>(), &"reload".into()).unwrap();

    assert_eq!(decisions(), [true], "the traversal is intercepted");
    assert_eq!(reloads.to_vec(), [JsValue::from_str("remix-document-reload")]);
    assert!(requests().is_empty());
}

const MANUAL: (&str, &str) = ("manual", "manual");
const AFTER: &str = "after-transition";

#[wasm_bindgen_test]
async fn opts_out_of_browser_scrolling_when_data_rmx_reset_scroll_is_false() {
    let page = Page::new().await;
    page.click(&link("reset-scroll-false", r#"data-rmx-target="details" data-rmx-reset-scroll="false""#)).await;
    assert_eq!(resets(), resets_of(&[("manual", AFTER)]));
}

#[wasm_bindgen_test]
async fn opts_out_of_browser_focus_reset_when_data_rmx_reset_focus_is_false() {
    let page = Page::new().await;
    page.click(&link("reset-focus-false", r#"data-rmx-target="details" data-rmx-reset-focus="false""#)).await;
    assert_eq!(resets(), resets_of(&[(AFTER, "manual")]));
}

#[wasm_bindgen_test]
async fn opts_out_of_scrolling_and_focus_reset_for_manual_link_attributes() {
    let page = Page::new().await;
    page.click(&link("manual", r#"data-rmx-target="details" data-rmx-reset-scroll="manual" data-rmx-reset-focus="manual""#)).await;
    assert_eq!(resets(), resets_of(&[MANUAL]));
}

#[wasm_bindgen_test]
async fn uses_default_scrolling_and_focus_reset_for_after_transition_link_attributes() {
    let page = Page::new().await;
    page.click(&link("after", r#"data-rmx-target="details" data-rmx-reset-scroll="after-transition" data-rmx-reset-focus="after-transition""#)).await;
    page.click(&link("default", r#"data-rmx-target="details""#)).await;
    assert_eq!(resets(), resets_of(&[(AFTER, AFTER), (AFTER, AFTER)]));
}

#[wasm_bindgen_test]
async fn accepts_manual_scroll_and_focus_attributes_on_get_forms() {
    let page = Page::new().await;
    page.submit(
        &format!(r#"<form action="{}" data-rmx-target="details" data-rmx-reset-scroll="manual" data-rmx-reset-focus="false"></form>"#, page_url("get-manual")),
        None,
    )
    .await;
    assert_eq!(resets(), resets_of(&[MANUAL]));
}

#[wasm_bindgen_test]
async fn lets_after_transition_submitters_override_manual_post_form_attributes() {
    let page = Page::new().await;
    page.submit(
        &format!(
            r#"<form action="{}" method="post" data-rmx-target="details" data-rmx-reset-scroll="manual" data-rmx-reset-focus="manual"><button data-rmx-reset-scroll="after-transition" data-rmx-reset-focus="after-transition">Save</button></form>"#,
            page_url("post-override")
        ),
        Some("button"),
    )
    .await;
    assert_eq!(resets(), resets_of(&[(AFTER, AFTER)]));
}

#[wasm_bindgen_test]
async fn lets_submitters_disable_browser_focus_reset_for_post_forms() {
    let page = Page::new().await;
    page.submit(
        &format!(r#"<form action="{}" method="post" data-rmx-target="details"><button data-rmx-reset-focus="false">Save</button></form>"#, page_url("post-focus")),
        Some("button"),
    )
    .await;
    assert_eq!(resets(), resets_of(&[(AFTER, "manual")]));
}

#[wasm_bindgen_test]
async fn replaces_same_location_post_submission_history() {
    let page = Page::new().await;
    let entries = entry_count();
    let url = current_url();
    let form: web_sys::HtmlFormElement = page
        .add(&format!(r#"<form action="{url}" method="post" data-rmx-target="details" data-rmx-reset-focus="false"><input name="displayName" value="Ada"></form>"#))
        .unchecked_into();
    let input: web_sys::HtmlElement = form.query_selector("input").unwrap().unwrap().unchecked_into();
    input.focus().unwrap();
    form.request_submit().unwrap();
    settle().await;

    assert_eq!(entry_count(), entries, "the entry is replaced");
    assert_eq!(current_url(), url);
    assert_eq!(document().active_element().as_ref(), Some(input.as_ref()), "focus stays");
    let requests = requests();
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].form, Some(vec![("displayName".to_string(), "Ada".to_string())]));
}

#[wasm_bindgen_test]
async fn pushes_same_location_post_history_when_data_rmx_history_is_push() {
    let page = Page::new().await;
    let entries = entry_count();
    let url = current_url();
    page.submit(&format!(r#"<form action="{url}" method="post" data-rmx-target="details" data-rmx-history="push"></form>"#), None).await;

    assert_eq!(entry_count(), entries + 1);
    assert_eq!(current_url(), url);
    assert_eq!(requests()[0].method, "POST");
}

#[wasm_bindgen_test]
async fn pushes_post_submission_history_for_a_different_location() {
    let page = Page::new().await;
    let entries = entry_count();
    let destination = page_url("post-elsewhere");
    assert_ne!(destination, current_url());
    page.submit(&format!(r#"<form action="{destination}" method="post" data-rmx-target="details"></form>"#), None).await;

    assert_eq!(entry_count(), entries + 1);
    assert_eq!(current_url(), destination);
    assert_eq!(requests()[0].method, "POST");
}

#[wasm_bindgen_test]
async fn replaces_marked_get_form_history_without_submission_metadata() {
    let page = Page::new().await;
    let entries = entry_count();
    page.submit(
        &format!(r#"<form action="{}" method="get" data-rmx-target="details" data-rmx-history="replace"><input name="query" value="replace-frames"></form>"#, page_url("get-replace")),
        None,
    )
    .await;

    assert_eq!(entry_count(), entries, "the entry is replaced");
    assert!(current_url().contains("query=replace-frames"), "{}", current_url());
    // The push is cancelled and replayed as a replace navigation (dispatched inside the
    // first one's listener, so it is recorded first), which is intercepted.
    assert_eq!(decisions(), [true, false]);
    let requests = requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].form, None);
}

#[wasm_bindgen_test]
async fn reloads_a_targeted_frame_when_a_temporary_submitter_is_removed_during_submission() {
    let page = Page::new().await;
    let form: web_sys::HtmlFormElement = page.add(&format!(r#"<form action="{}" method="post"><input name="name" value="Ada"></form>"#, page_url("temp-submitter"))).unchecked_into();
    let button: web_sys::HtmlElement = document().create_element("button").unwrap().unchecked_into();
    button.set_attribute("name", "intent").unwrap();
    button.set_attribute("value", "save").unwrap();
    button.set_attribute("data-rmx-target", "details").unwrap();
    form.append_child(&button).unwrap();
    form.request_submit_with_submitter(Some(&button)).unwrap();
    button.remove();
    settle().await;

    assert_eq!(decisions(), [true]);
    let requests = requests();
    assert_eq!(requests[0].target.as_deref(), Some("details"));
    assert_eq!(requests[0].form, Some(vec![("name".to_string(), "Ada".to_string()), ("intent".to_string(), "save".to_string())]));
}

/// Remix's version interrupts a top frame load; here the load targets the named frame. The
/// fragment is not `#details`, which `does_not_intercept_hash_only_navigations` links to.
#[wasm_bindgen_test]
async fn finishes_loading_the_destination_when_a_hash_navigation_interrupts_its_frame_load() {
    let _page = Page::new().await;
    let mut release = None;
    let hold = js_sys::Promise::new(&mut |resolve, _| release = Some(resolve));
    HOLD.with(|h| *h.borrow_mut() = Some(hold));
    let destination = page_url("pending-hash");
    let target = || NavigateOptions { target: Some("details".into()), ..Default::default() };
    navigate(&destination, target());
    for _ in 0..3 {
        tick().await;
    }
    assert_eq!(requests().len(), 1, "the first load is pending");
    assert_eq!(current_url(), destination);

    navigate(&format!("{destination}#interrupt"), target());
    for _ in 0..3 {
        tick().await;
    }
    release.unwrap().call0(&JsValue::NULL).unwrap();
    settle().await;
    assert_eq!(decisions(), [true, true], "the hash navigation is routed while the frame load is pending");
    assert_eq!(requests().len(), 2, "the destination loads again");
    assert_eq!(current_url(), format!("{destination}#interrupt"));
    assert!(Frames.get("details").unwrap().src().ends_with("#interrupt"));

    navigate(&format!("{destination}#other"), target());
    settle().await;
    assert_eq!(decisions(), [true, true, false], "later hash navigations are left to the browser");
    assert_eq!(requests().len(), 2);
}

/// Start a navigation that loads the `details` frame, wait until its request reached the
/// resolver, then abort the navigation with `window.stop()`.
async fn navigate_then_stop(case: &str) -> FrameRequest {
    navigate(&page_url(case), NavigateOptions { target: Some("details".into()), ..Default::default() });
    for _ in 0..20 {
        if !requests().is_empty() {
            break;
        }
        tick().await;
    }
    assert_eq!(requests().len(), 1, "the frame load started");
    window().stop().unwrap();
    settle().await;
    for _ in 0..3 {
        tick().await;
    }
    requests().remove(0)
}

#[wasm_bindgen_test]
async fn aborts_the_active_resolver_when_the_navigation_is_aborted() {
    let page = Page::new().await;
    // Like Remix's case, the resolver answers only once its signal aborts; an aborted load
    // must not apply that answer.
    RESOLVE_NEXT.with(|n| {
        *n.borrow_mut() = Some(Box::new(|request: FrameRequest| {
            let signal = request.signal.clone();
            let aborted = js_sys::Promise::new(&mut |resolve, _| signal.on_abort(move || drop(resolve.call0(&JsValue::NULL))));
            Box::pin(async move {
                let _ = JsFuture::from(aborted).await;
                Ok(FrameResponse { content: FrameContent::Html(r#"<p id="aborted">Aborted</p>"#.into()), url: None })
            })
        }))
    });
    let request = navigate_then_stop("abort-resolver").await;
    assert!(request.signal.aborted(), "the resolver's signal aborts with the navigation");
    assert!(page.el.query_selector("#aborted").unwrap().is_none(), "{}", page.el.inner_html());
    assert!(page.el.query_selector("#loaded").unwrap().is_some(), "{}", page.el.inner_html());
}

#[wasm_bindgen_test]
async fn stops_reading_the_active_response_stream_when_the_navigation_is_aborted() {
    let page = Page::new().await;
    let source = js("const log = { pulls: 0, cancelled: false };
        log.stream = new ReadableStream({ pull() { log.pulls++; return new Promise(() => {}) }, cancel() { log.cancelled = true } });
        return log;");
    let stream: web_sys::ReadableStream = get(&source, "stream").unchecked_into();
    RESOLVE_NEXT.with(|n| {
        *n.borrow_mut() = Some(Box::new(move |_| Box::pin(async move { Ok(FrameResponse { content: FrameContent::Stream(stream), url: None }) })))
    });
    navigate(&page_url("abort-stream"), NavigateOptions { target: Some("details".into()), ..Default::default() });
    for _ in 0..20 {
        if get(&source, "pulls").as_f64().unwrap_or(0.0) > 0.0 {
            break;
        }
        tick().await;
    }
    assert!(get(&source, "pulls").as_f64().unwrap_or(0.0) > 0.0, "the stream is being read");
    window().stop().unwrap();
    settle().await;
    for _ in 0..3 {
        tick().await;
    }
    assert!(requests()[0].signal.aborted(), "the load's signal aborts with the navigation");
    assert_eq!(get(&source, "cancelled"), JsValue::TRUE, "the stream is cancelled");
    assert!(page.el.query_selector("#loaded").unwrap().is_some(), "{}", page.el.inner_html());
}

/// Run a JavaScript function body and return its value.
fn js(body: &str) -> JsValue {
    js_sys::Function::new_no_args(body).call0(&JsValue::NULL).unwrap()
}

/// Navigate with `navigation.navigate(url, options)`, `options` given as JavaScript.
fn navigate_raw(url: &str, options: &str) {
    let options = js(&format!("return {options}"));
    let navigate: js_sys::Function = get(&navigation(), "navigate").unchecked_into();
    let _ = navigate.call2(&navigation(), &url.into(), &options);
}

fn current_state() -> JsValue {
    let entry = get(&navigation(), "currentEntry");
    let get_state: js_sys::Function = get(&entry, "getState").unchecked_into();
    get_state.call0(&entry).unwrap()
}

async fn back() {
    let back: js_sys::Function = get(&navigation(), "back").unchecked_into();
    let result = back.call0(&navigation()).unwrap();
    let _ = JsFuture::from(get(&result, "finished").unchecked_into::<js_sys::Promise>()).await;
    settle().await;
}

#[wasm_bindgen_test]
async fn passes_runtime_state_via_navigate_history_state() {
    let _page = Page::new().await;
    let url = page_url("state");
    navigate(&url, NavigateOptions { target: Some("details".into()), src: Some("/state-partial".into()), ..Default::default() });
    settle().await;
    let state = current_state();
    assert_eq!(get(&state, "$rmx"), JsValue::TRUE);
    assert_eq!(get(&state, "target").as_string().as_deref(), Some("details"));
    assert_eq!(get(&state, "src").as_string().as_deref(), Some("/state-partial"));
    assert_eq!(get(&state, "resetScroll").as_string().as_deref(), Some(AFTER), "resets after the transition by default");
    assert_eq!(get(&state, "resetFocus").as_string().as_deref(), Some(AFTER));
    assert_eq!(resets(), resets_of(&[(AFTER, AFTER)]));
}

/// Remix's "normalizes resetScroll=false" and "resetFocus=false to manual history state"
/// and "accepts manual scroll and focus options": `false` converts to `Reset::Manual`.
#[wasm_bindgen_test]
async fn normalizes_reset_options_to_manual_history_state() {
    let _page = Page::new().await;
    let options = NavigateOptions { target: Some("details".into()), reset_scroll: Some(false.into()), reset_focus: Some(rsmix::Reset::Manual), ..Default::default() };
    navigate(&page_url("manual-state"), options);
    settle().await;
    let state = current_state();
    assert_eq!(get(&state, "resetScroll").as_string().as_deref(), Some("manual"));
    assert_eq!(get(&state, "resetFocus").as_string().as_deref(), Some("manual"));
    assert_eq!(resets(), resets_of(&[MANUAL]));
}

#[wasm_bindgen_test]
async fn uses_browser_scrolling_after_the_frame_content_commits() {
    let page = Page::new().await;
    let url = page_url("commit-scroll");
    page.click(&format!(r#"<a href="{url}" data-rmx-target="details">Go</a>"#)).await;
    assert_eq!(resets(), resets_of(&[(AFTER, AFTER)]));
    assert_eq!(scrolls(), [url], "scrolled once, when the content committed");
}

#[wasm_bindgen_test]
async fn does_not_scroll_when_reset_scroll_is_manual() {
    let page = Page::new().await;
    page.click(&link("manual-scroll", r#"data-rmx-target="details" data-rmx-reset-scroll="manual""#)).await;
    assert_eq!(decisions(), [true]);
    assert!(scrolls().is_empty());
}

#[wasm_bindgen_test]
async fn does_not_scroll_for_a_superseded_frame_reload() {
    let _page = Page::new().await;
    let mut release = None;
    let hold = js_sys::Promise::new(&mut |resolve, _| release = Some(resolve));
    HOLD.with(|h| *h.borrow_mut() = Some(hold));
    navigate(&page_url("superseded"), NavigateOptions { target: Some("details".into()), ..Default::default() });
    for _ in 0..3 {
        tick().await;
    }
    assert_eq!(requests().len(), 1, "the navigation's load is pending");
    Frames.get("details").unwrap().reload();
    for _ in 0..3 {
        tick().await;
    }
    release.unwrap().call0(&JsValue::NULL).unwrap();
    settle().await;
    assert_eq!(decisions(), [true]);
    assert_eq!(requests().len(), 2);
    assert!(scrolls().is_empty(), "the navigation's load was superseded before it committed");
}

/// Pretend to be Safari: a WebKit user agent, a stubbed scroll position, and a
/// `scrollTo` that records its calls and moves the position to `then` (or to the requested
/// top when `then` is `null`). Returns the recorded calls; `restore_webkit` undoes it.
fn stub_webkit(then: &str) -> js_sys::Array {
    js(&format!(
        "const saved = {{ x: Object.getOwnPropertyDescriptor(window, 'scrollX'), y: Object.getOwnPropertyDescriptor(window, 'scrollY'), scrollTo: window.scrollTo }};
         window.__webkit = saved;
         Object.defineProperty(navigator, 'userAgent', {{ configurable: true, value: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 Version/18.6 Safari/605.1.15' }});
         let y = 0;
         Object.defineProperty(window, 'scrollX', {{ configurable: true, get: () => 0 }});
         Object.defineProperty(window, 'scrollY', {{ configurable: true, get: () => y }});
         const calls = [];
         window.scrollTo = (options) => {{ calls.push(options.behavior + ' ' + options.left + ' ' + options.top); const then = {then}; y = then === null ? options.top : then }};
         return calls;"
    ))
    .unchecked_into()
}

fn restore_webkit() {
    js("const saved = window.__webkit;
        delete navigator.userAgent;
        Object.defineProperty(window, 'scrollX', saved.x);
        Object.defineProperty(window, 'scrollY', saved.y);
        window.scrollTo = saved.scrollTo;
        delete window.__webkit;");
}

/// Wait for two animation frames.
async fn frames() {
    for _ in 0..2 {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            window().request_animation_frame(&resolve).unwrap();
        });
        let _ = JsFuture::from(promise).await;
    }
}

fn strings(array: &js_sys::Array) -> Vec<String> {
    array.iter().map(|v| v.as_string().unwrap()).collect()
}

#[wasm_bindgen_test]
async fn resynchronizes_webkit_scroll_state_after_a_navigation_scroll_reset() {
    let page = Page::new().await;
    let calls = stub_webkit("null");
    page.click(&link("webkit", r#"data-rmx-target="details""#)).await;
    frames().await;
    restore_webkit();
    assert_eq!(decisions(), [true]);
    assert_eq!(strings(&calls), ["instant 0 1", "instant 0 0"]);
}

#[wasm_bindgen_test]
async fn preserves_scrolling_that_occurs_after_webkit_scroll_resynchronization() {
    let page = Page::new().await;
    let calls = stub_webkit("200");
    page.click(&link("webkit-scrolled", r#"data-rmx-target="details""#)).await;
    frames().await;
    restore_webkit();
    assert_eq!(strings(&calls), ["instant 0 1"], "a scroll after the nudge is kept");
}

#[wasm_bindgen_test]
async fn does_not_resynchronize_webkit_scroll_state_for_fragment_navigations() {
    let page = Page::new().await;
    let calls = stub_webkit("null");
    page.click(&format!(r#"<a href="{}#section" data-rmx-target="details">Go</a>"#, page_url("webkit-fragment"))).await;
    frames().await;
    restore_webkit();
    assert_eq!(decisions(), [true]);
    assert!(calls.length() == 0);
}

#[wasm_bindgen_test]
async fn does_not_resynchronize_scroll_state_outside_webkit() {
    let page = Page::new().await;
    let calls = js("const calls = []; window.__scrollTo = window.scrollTo; window.scrollTo = (o) => calls.push(o); return calls;").unchecked_into::<js_sys::Array>();
    page.click(&link("chromium", r#"data-rmx-target="details""#)).await;
    frames().await;
    js("window.scrollTo = window.__scrollTo; delete window.__scrollTo;");
    assert_eq!(calls.length(), 0);
}

/// Pretend to be iOS Safari (Remix `setupWebKitScrollReset`): an iPhone user agent, a
/// stubbed scroll position that `scrollTo` moves and, when `reset` is set, that
/// `NavigateEvent.scroll()` resets to the top, plus a spacer whose height sets the page's.
fn stub_ios(reset: bool) {
    js(&format!(
        "const saved = {{ x: Object.getOwnPropertyDescriptor(window, 'scrollX'), y: Object.getOwnPropertyDescriptor(window, 'scrollY'), scrollTo: window.scrollTo, scroll: NavigateEvent.prototype.scroll }};
         window.__ios = saved;
         Object.defineProperty(navigator, 'userAgent', {{ configurable: true, value: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.6.1 Mobile/15E148 Safari/604.1' }});
         let y = 0;
         saved.setY = (top) => {{ y = top }};
         Object.defineProperty(window, 'scrollX', {{ configurable: true, get: () => 0 }});
         Object.defineProperty(window, 'scrollY', {{ configurable: true, get: () => y }});
         window.scrollTo = (options) => {{ y = options.top }};
         if ({reset}) NavigateEvent.prototype.scroll = function () {{
             y = 0;
             const after = saved.afterScroll;
             if (after) {{ saved.afterScroll = null; queueMicrotask(after) }}
             return saved.scroll.call(this)
         }};
         saved.spacer = document.createElement('div');
         document.body.append(saved.spacer);"
    ));
}

fn restore_ios() {
    // Stop any workaround still listening, then put the real window back.
    window().dispatch_event(&web_sys::Event::new("pointerdown").unwrap()).unwrap();
    js("const saved = window.__ios;
        delete navigator.userAgent;
        Object.defineProperty(window, 'scrollX', saved.x);
        Object.defineProperty(window, 'scrollY', saved.y);
        window.scrollTo = saved.scrollTo;
        NavigateEvent.prototype.scroll = saved.scroll;
        saved.spacer.remove();
        delete window.__ios;");
}

fn set_scroll(y: f64) {
    js(&format!("window.__ios.setY({y})"));
}

/// Move the page as Safari's restoration or app code would, including the scroll event.
fn scroll_page(y: f64) {
    set_scroll(y);
    window().dispatch_event(&web_sys::Event::new("scroll").unwrap()).unwrap();
}

fn scroll_y() -> f64 {
    window().scroll_y().unwrap()
}

fn set_page_height(px: u32) {
    js(&format!("window.__ios.spacer.style.height = '{px}px'"));
}

fn max_scroll_y() -> f64 {
    js("return document.documentElement.scrollHeight - window.innerHeight").as_f64().unwrap()
}

async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        window().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

// navigation.test.ts: undoes an iOS restoration of the previous position after the scroll reset
#[wasm_bindgen_test]
async fn undoes_an_ios_restoration_of_the_previous_position_after_the_scroll_reset() {
    let page = Page::new().await;
    stub_ios(true);
    set_scroll(2400.0);
    page.click(&link("ios-previous", r#"data-rmx-target="details""#)).await;
    scroll_page(2400.0);
    let y = scroll_y();
    restore_ios();
    assert_eq!(decisions(), [true]);
    assert_eq!(y, 0.0);
}

// navigation.test.ts: undoes an iOS restoration at the bottom the page had before it grew
#[wasm_bindgen_test]
async fn undoes_an_ios_restoration_at_the_bottom_the_page_had_before_it_grew() {
    let page = Page::new().await;
    stub_ios(true);
    set_page_height(3000);
    // Add the link first: here it is part of the page whose bottom is measured.
    let anchor: web_sys::HtmlElement = page.add(&link("ios-bottom", r#"data-rmx-target="details""#)).unchecked_into();
    let bottom = max_scroll_y();
    set_scroll(bottom + 1000.0);
    // Right after the scroll reset, before the navigation finishes, the page grows and
    // Safari restores the old bottom.
    js(&format!(
        "window.__ios.afterScroll = () => {{
             window.__ios.spacer.style.height = '3100px';
             window.__ios.setY({bottom});
             window.dispatchEvent(new Event('scroll'));
             window.__ios.seen = window.scrollY;
         }}"
    ));
    anchor.click();
    settle().await;
    let y = js("return window.__ios.seen").as_f64().expect("the restoration ran");
    restore_ios();
    assert!(bottom > 0.0, "{bottom}");
    assert_eq!(decisions(), [true]);
    assert_eq!(y, 0.0, "bottom {bottom}");
}

// navigation.test.ts: undoes an iOS restoration that is still in place when a frame redirect finishes
#[wasm_bindgen_test]
async fn undoes_an_ios_restoration_still_in_place_when_a_frame_redirect_finishes() {
    let _page = Page::new().await;
    stub_ios(false);
    set_scroll(2400.0);
    navigate_raw(&page_url("ios-redirected"), "{ history: 'replace', info: { type: 'frame-redirect', resetScroll: 'after-transition', resetFocus: 'after-transition' } }");
    settle().await;
    frames().await;
    let y = scroll_y();
    restore_ios();
    assert_eq!(decisions(), [true]);
    assert_eq!(y, 0.0);
}

// navigation.test.ts: leaves the previous page in place until the new page resets scroll
#[wasm_bindgen_test]
async fn leaves_the_previous_page_in_place_until_the_new_page_resets_scroll() {
    let _page = Page::new().await;
    stub_ios(true);
    set_scroll(2400.0);
    let mut release = None;
    let hold = js_sys::Promise::new(&mut |resolve, _| release = Some(resolve));
    HOLD.with(|h| *h.borrow_mut() = Some(hold));
    navigate(&page_url("ios-pending"), NavigateOptions { target: Some("details".into()), ..Default::default() });
    for _ in 0..3 {
        tick().await;
    }
    scroll_page(2400.0);
    let y = scroll_y();
    release.unwrap().call0(&JsValue::NULL).unwrap();
    settle().await;
    restore_ios();
    assert_eq!(y, 2400.0);
}

// navigation.test.ts: leaves positions other code scrolls to after the scroll reset
#[wasm_bindgen_test]
async fn leaves_positions_other_code_scrolls_to_after_the_scroll_reset() {
    let page = Page::new().await;
    stub_ios(true);
    set_scroll(2400.0);
    page.click(&link("ios-other", r#"data-rmx-target="details""#)).await;
    scroll_page(500.0);
    let y = scroll_y();
    restore_ios();
    assert_eq!(y, 500.0);
}

// navigation.test.ts: stops undoing iOS restorations once the user interacts with the page
#[wasm_bindgen_test]
async fn stops_undoing_ios_restorations_once_the_user_interacts_with_the_page() {
    let page = Page::new().await;
    stub_ios(true);
    set_scroll(2400.0);
    page.click(&link("ios-interact", r#"data-rmx-target="details""#)).await;
    window().dispatch_event(&web_sys::Event::new("pointerdown").unwrap()).unwrap();
    scroll_page(2400.0);
    let y = scroll_y();
    restore_ios();
    assert_eq!(y, 2400.0);
}

// navigation.test.ts: stops undoing iOS restorations when the next navigation starts (the next
// navigation keeps its scroll, so it starts no workaround of its own)
#[wasm_bindgen_test]
async fn stops_undoing_ios_restorations_when_the_next_navigation_starts() {
    let page = Page::new().await;
    stub_ios(true);
    set_scroll(2400.0);
    page.click(&link("ios-first", r#"data-rmx-target="details""#)).await;
    page.click(&link("ios-next", r#"data-rmx-target="details" data-rmx-reset-scroll="manual""#)).await;
    scroll_page(2400.0);
    let y = scroll_y();
    restore_ios();
    assert_eq!(decisions(), [true, true]);
    assert_eq!(y, 2400.0);
}

// navigation.test.ts: stops undoing iOS restorations shortly after the navigation finishes
#[wasm_bindgen_test]
async fn stops_undoing_ios_restorations_shortly_after_the_navigation_finishes() {
    let page = Page::new().await;
    stub_ios(true);
    set_scroll(2400.0);
    page.click(&link("ios-later", r#"data-rmx-target="details""#)).await;
    sleep(550).await;
    scroll_page(2400.0);
    let y = scroll_y();
    restore_ios();
    assert_eq!(y, 2400.0);
}

// navigation.test.ts: stops undoing iOS restorations when the navigation fails
#[wasm_bindgen_test]
async fn stops_undoing_ios_restorations_when_the_navigation_fails() {
    let page = Page::new().await;
    stub_ios(true);
    set_scroll(2400.0);
    RESOLVE_NEXT.with(|n| *n.borrow_mut() = Some(Box::new(|_| Box::pin(async { Err("offline".into()) }))));
    page.click(&link("ios-failed", r#"data-rmx-target="details""#)).await;
    scroll_page(2400.0);
    let y = scroll_y();
    restore_ios();
    assert_eq!(y, 2400.0);
}

async fn next_frame() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        window().request_animation_frame(&resolve).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

fn adopted_sheets() -> u32 {
    js("return document.adoptedStyleSheets.length").as_f64().unwrap() as u32
}

fn computed(element: &str, property: &str) -> String {
    js(&format!("return getComputedStyle(document.{element}).getPropertyValue('{property}')")).as_string().unwrap()
}

// navigation.test.ts: leaves traversal restoration to the browser after frame reconciliation
// (the scroll styles: the starting scroll range and no anchoring while the traversal runs,
// only the anchoring suppression once it finishes, nothing from the second frame on)
#[wasm_bindgen_test]
async fn keeps_scroll_anchoring_off_until_after_a_traversal_paints() {
    let page = Page::new().await;
    page.click(&link("anchoring-a", r#"data-rmx-target="details" data-rmx-src="/a""#)).await;
    page.click(&link("anchoring-b", r#"data-rmx-target="details" data-rmx-src="/b""#)).await;
    clear();
    // Let the push navigations' own anchoring styles go first.
    next_frame().await;
    next_frame().await;
    let count = adopted_sheets();
    let starting_min_height = computed("documentElement", "min-height");
    let expected_min_height = js("let r = document.documentElement; return r.scrollHeight + r.clientHeight + 'px'").as_string().unwrap();

    let mut release = None;
    let hold = js_sys::Promise::new(&mut |resolve, _| release = Some(resolve));
    HOLD.with(|h| *h.borrow_mut() = Some(hold));
    let back: js_sys::Function = get(&navigation(), "back").unchecked_into();
    let result = back.call0(&navigation()).unwrap();
    for _ in 0..50 {
        if !requests().is_empty() {
            break;
        }
        tick().await;
    }
    let during = (adopted_sheets(), computed("documentElement", "min-height"), computed("body", "overflow-anchor"));
    release.unwrap().call0(&JsValue::NULL).unwrap();
    let _ = JsFuture::from(get(&result, "finished").unchecked_into::<js_sys::Promise>()).await;
    JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await.unwrap();
    let finished = (adopted_sheets(), computed("documentElement", "min-height"), computed("documentElement", "overflow-anchor"), computed("body", "overflow-anchor"));
    next_frame().await;
    let first_frame = adopted_sheets();
    next_frame().await;
    let second_frame = adopted_sheets();
    drop(page);

    assert_eq!(decisions(), [true]);
    assert_eq!(during, (count + 1, expected_min_height, "none".to_string()));
    assert_eq!(finished, (count + 1, starting_min_height, "none".to_string(), "none".to_string()));
    assert_eq!(first_frame, count + 1);
    assert_eq!(second_frame, count);
}

/// Hide the Navigation API (`what` = "navigation") or `NavigateEvent.sourceElement`
/// (`what` = "sourceElement") until the returned function runs.
fn hide(what: &str) -> js_sys::Function {
    let script = match what {
        // Window is a global interface: `navigation` is an own property of `window`.
        "navigation" => "const d = Object.getOwnPropertyDescriptor(window, 'navigation');
             Object.defineProperty(window, 'navigation', { configurable: true, value: undefined });
             return () => Object.defineProperty(window, 'navigation', d)",
        _ => "const proto = NavigateEvent.prototype;
             const d = Object.getOwnPropertyDescriptor(proto, 'sourceElement');
             delete proto.sourceElement;
             return () => Object.defineProperty(proto, 'sourceElement', d)",
    };
    js(script).unchecked_into()
}

async fn hash_changed() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let once = web_sys::AddEventListenerOptions::new();
        once.set_once(true);
        window().add_event_listener_with_callback_and_add_event_listener_options("hashchange", &resolve, &once).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

async fn popstate_back() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let once = web_sys::AddEventListenerOptions::new();
        once.set_once(true);
        window().add_event_listener_with_callback_and_add_event_listener_options("popstate", &resolve, &once).unwrap();
    });
    window().history().unwrap().back().unwrap();
    let _ = JsFuture::from(promise).await;
}

/// `navigate()` without interception support: a document navigation (here to a fragment,
/// so the test page stays), pushing an entry or replacing the current one.
async fn falls_back_to_document_navigation(hidden: &str, replace: bool) {
    let _page = Page::new().await;
    let original = current_url();
    let destination = format!("{}#fallback-{hidden}-{replace}", original.split('#').next().unwrap());
    let index = || get(&get(&navigation(), "currentEntry"), "index").as_f64().unwrap();
    let start_index = index();
    let restore = hide(hidden);
    let guard = GUARD.with(|g| g.borrow().clone().unwrap());
    js_sys::Reflect::set(&guard, &"allow".into(), &JsValue::TRUE).unwrap();
    let history = replace.then_some(rsmix::History::Replace);
    navigate(&destination, NavigateOptions { history, ..Default::default() });
    hash_changed().await;
    let (url, seen) = (current_url(), decisions());
    restore.call0(&JsValue::NULL).unwrap();
    let new_index = index();
    // A fragment navigation carries the previous entry's state, if any; the runtime would
    // have stored the destination as the entry's `src`.
    let state = current_state();
    // Leave the fragment again (the guard would cancel the traversal).
    if replace {
        window().history().unwrap().replace_state_with_url(&JsValue::NULL, "", Some(&original)).unwrap();
    } else {
        popstate_back().await;
    }
    js_sys::Reflect::set(&guard, &"allow".into(), &JsValue::FALSE).unwrap();
    assert!(navigation().is_object(), "the Navigation API is back");
    assert_eq!(new_index, if replace { start_index } else { start_index + 1.0 }, "replace keeps the entry, push adds one");
    assert_eq!(current_url(), original);
    assert_eq!(url, destination);
    assert_eq!(seen, [false], "the runtime did not intercept it");
    assert_ne!(get(&state, "src").as_string().as_deref(), Some(destination.as_str()), "no runtime state for the destination: {state:?}");
}

// navigation.test.ts: falls back to document navigation when the Navigation API is unavailable
#[wasm_bindgen_test]
async fn falls_back_to_document_navigation_when_the_navigation_api_is_unavailable() {
    falls_back_to_document_navigation("navigation", false).await;
}

// navigation.test.ts: falls back to document navigation when sourceElement is unavailable
#[wasm_bindgen_test]
async fn falls_back_to_document_navigation_when_source_element_is_unavailable() {
    falls_back_to_document_navigation("sourceElement", false).await;
}

// navigation.test.ts: replaces document history when the Navigation API is unavailable
#[wasm_bindgen_test]
async fn replaces_document_history_when_the_navigation_api_is_unavailable() {
    falls_back_to_document_navigation("navigation", true).await;
}

#[wasm_bindgen_test]
async fn preserves_manual_scrolling_and_focus_across_frame_redirects() {
    let _page = Page::new().await;
    navigate_raw(&page_url("redirected"), "{ history: 'replace', info: { type: 'frame-redirect', resetScroll: 'manual', resetFocus: 'manual' } }");
    settle().await;
    assert_eq!(decisions(), [true]);
    assert_eq!(resets(), resets_of(&[MANUAL]));
    assert!(requests().is_empty(), "the content is already in the page");
}

#[wasm_bindgen_test]
async fn does_not_scroll_again_when_synchronizing_a_frame_redirect_url() {
    let _page = Page::new().await;
    navigate_raw(&page_url("redirect-sync"), "{ history: 'replace', info: { type: 'frame-redirect', resetScroll: 'after-transition', resetFocus: 'after-transition' } }");
    settle().await;
    assert_eq!(resets(), resets_of(&[("manual", AFTER)]));
    assert!(scrolls().is_empty());
    assert!(requests().is_empty());
}

/// Remix's "ignores legacy boolean scroll and focus settings on traverse navigations" and
/// "ignores manual scroll and focus settings on traverse navigations": state written with
/// `resetScroll: false` is manual when pushed, but a traversal back to it resets.
#[wasm_bindgen_test]
async fn ignores_legacy_boolean_and_manual_settings_on_traverse_navigations() {
    let _page = Page::new().await;
    let state = r#"{ $rmx: true, target: 'details', src: '/legacy', resetScroll: false, resetFocus: false }"#;
    navigate_raw(&page_url("legacy"), &format!("{{ state: {state} }}"));
    settle().await;
    navigate(&page_url("legacy-next"), NavigateOptions { target: Some("details".into()), ..Default::default() });
    settle().await;
    back().await;
    assert_eq!(decisions(), [true, true, true]);
    assert_eq!(resets(), resets_of(&[MANUAL, (AFTER, AFTER), (AFTER, AFTER)]));
    let requests = requests();
    assert!(requests[2].url.ends_with("/legacy"), "{}", requests[2].url);
}

#[wasm_bindgen_test]
async fn defaults_missing_reset_settings_when_safari_reads_traversal_state_from_entries() {
    let page = Page::new().await;
    page.click(&link("entries-a", r#"data-rmx-target="details" data-rmx-src="/entries-a""#)).await;
    page.click(&link("entries-b", r#"data-rmx-target="details" data-rmx-src="/entries-b""#)).await;
    clear();
    // Safari returns null from destination.getState() for traversals.
    js("window.__getState = NavigationDestination.prototype.getState; NavigationDestination.prototype.getState = function () { return null };");
    back().await;
    js("NavigationDestination.prototype.getState = window.__getState; delete window.__getState;");
    assert_eq!(decisions(), [true]);
    assert_eq!(resets(), resets_of(&[(AFTER, AFTER)]));
    assert!(requests()[0].url.ends_with("/entries-a"), "{}", requests()[0].url);
    assert_eq!(scrolls().len(), 1);
}

#[wasm_bindgen_test]
async fn does_not_reconcile_unsafe_frame_sources_restored_from_navigation_history() {
    let page = Page::new().await;
    page.click(&link("unsafe-a", r#"data-rmx-target="details" data-rmx-src="/unsafe-a""#)).await;
    js("navigation.updateCurrentEntry({ state: { $rmx: true, target: 'details', src: 'https://frames.example/unsafe' } })");
    page.click(&link("unsafe-b", r#"data-rmx-target="details" data-rmx-src="/unsafe-b""#)).await;
    clear();
    let reloads = js_sys::Array::new();
    let stub = js_sys::Function::new_with_args("log", "return function (options) { log.push(options && options.info); return { committed: Promise.resolve(), finished: Promise.resolve() } }");
    js_sys::Reflect::set(&navigation(), &"reload".into(), &stub.call1(&JsValue::NULL, &reloads).unwrap()).unwrap();
    back().await;
    js_sys::Reflect::delete_property(navigation().unchecked_ref::<js_sys::Object>(), &"reload".into()).unwrap();
    assert_eq!(reloads.to_vec(), [JsValue::from_str("remix-document-reload")]);
    assert_eq!(resets(), resets_of(&[("manual", "")]), "Remix's traversal case: no scroll before the reload");
    assert!(requests().is_empty(), "the unsafe source is not loaded");
}

#[wasm_bindgen_test]
async fn opts_out_of_browser_focus_reset_for_get_forms() {
    let page = Page::new().await;
    page.submit(&format!(r#"<form action="{}" data-rmx-target="details" data-rmx-reset-focus="false"></form>"#, page_url("get-focus")), None).await;
    assert_eq!(resets(), resets_of(&[(AFTER, "manual")]));
}

#[wasm_bindgen_test]
async fn lets_submitters_restore_browser_focus_reset_for_forms() {
    let page = Page::new().await;
    page.submit(
        &format!(r#"<form action="{}" method="post" data-rmx-target="details" data-rmx-reset-focus="false"><button data-rmx-reset-focus="true">Save</button></form>"#, page_url("focus-restore")),
        Some("button"),
    )
    .await;
    assert_eq!(resets(), resets_of(&[(AFTER, AFTER)]));
}

/// Also Remix's "replays a POST submission in the frame selected before cancelling it": the
/// replay loads the frame chosen for the cancelled push.
#[wasm_bindgen_test]
async fn replays_same_location_post_submissions_as_replace_navigations_without_precommit_support() {
    let page = Page::new().await;
    js("window.__precommit = window.NavigationPrecommitController; delete window.NavigationPrecommitController;");
    let entries = entry_count();
    let url = current_url();
    let form: web_sys::HtmlFormElement = page
        .add(&format!(r#"<form action="{url}" method="post" data-rmx-target="details" data-rmx-reset-focus="false"><input name="displayName" value="Ada"></form>"#))
        .unchecked_into();
    let input: web_sys::HtmlElement = form.query_selector("input").unwrap().unwrap().unchecked_into();
    input.focus().unwrap();
    form.request_submit().unwrap();
    settle().await;
    js("window.NavigationPrecommitController = window.__precommit; delete window.__precommit;");

    // The push is cancelled and replayed as a replace navigation (dispatched inside the
    // first one's listener, so it is recorded first).
    assert_eq!(decisions(), [true, false]);
    assert_eq!(entry_count(), entries, "the entry is replaced");
    assert_eq!(current_url(), url);
    assert_eq!(document().active_element().as_ref(), Some(input.as_ref()), "focus stays");
    let requests = requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].target.as_deref(), Some("details"));
    assert_eq!(requests[0].form, Some(vec![("displayName".to_string(), "Ada".to_string())]));
}

// navigation.test.ts: accepts after-transition scroll and focus options
#[wasm_bindgen_test]
async fn accepts_after_transition_scroll_and_focus_options() {
    let _page = Page::new().await;
    let after = Some(rsmix::Reset::AfterTransition);
    navigate(&page_url("after-state"), NavigateOptions { target: Some("details".into()), reset_scroll: after, reset_focus: after, ..Default::default() });
    settle().await;
    let state = current_state();
    assert_eq!(get(&state, "resetScroll").as_string().as_deref(), Some(AFTER));
    assert_eq!(get(&state, "resetFocus").as_string().as_deref(), Some(AFTER));
    assert_eq!(resets(), resets_of(&[(AFTER, AFTER)]));
}

/// A promise the next resolve waits for, and the function that releases it.
fn hold_next_resolve() -> js_sys::Function {
    let mut release = None;
    let hold = js_sys::Promise::new(&mut |resolve, _| release = Some(resolve));
    HOLD.with(|h| *h.borrow_mut() = Some(hold));
    release.unwrap()
}

async fn wait_for_request() {
    for _ in 0..50 {
        if !requests().is_empty() {
            break;
        }
        tick().await;
    }
    assert!(!requests().is_empty(), "the frame load started");
}

// navigation.test.ts: resets focus after the transition by default
#[wasm_bindgen_test]
async fn resets_focus_after_the_transition_by_default() {
    let page = Page::new().await;
    let input: web_sys::HtmlElement = page.add("<input>").unchecked_into();
    input.focus().unwrap();
    let release = hold_next_resolve();
    let options = NavigateOptions { target: Some("details".into()), history: Some(rsmix::History::Replace), reset_scroll: Some(rsmix::Reset::Manual), ..Default::default() };
    navigate(&page_url("focus-default"), options);
    wait_for_request().await;
    let during = document().active_element();
    release.call0(&JsValue::NULL).unwrap();
    settle().await;
    let after = document().active_element();
    assert_eq!(during.as_ref(), Some(input.as_ref()), "focus stays while the frame loads");
    assert_eq!(after, document().body().map(Into::into), "focus resets once the navigation finishes");
}

/// Start the navigation listeners with `what` hidden (see [`hide`]) and return the
/// listener registrations and entry updates they made.
fn start_listeners_without(what: &str) -> Vec<String> {
    let spies = js("const calls = [];
        const restores = [];
        const spy = (target, name) => {
          target[name] = function (...args) {
            calls.push(name + ' ' + (typeof args[0] === 'string' ? args[0] : ''));
            return Object.getPrototypeOf(target)[name].apply(this, args);
          };
          restores.push(() => delete target[name]);
        };
        spy(document, 'addEventListener');
        spy(navigation, 'addEventListener');
        spy(navigation, 'updateCurrentEntry');
        return { calls, restore: () => restores.forEach((restore) => restore()) };");
    let restore = hide(what);
    rsmix::test::start_navigation_listener();
    restore.call0(&JsValue::NULL).unwrap();
    get(&spies, "restore").unchecked_into::<js_sys::Function>().call0(&JsValue::NULL).unwrap();
    get(&spies, "calls").unchecked_into::<js_sys::Array>().iter().map(|c| c.as_string().unwrap()).collect()
}

// navigation.test.ts: skips navigation listeners when the Navigation API is unavailable
#[wasm_bindgen_test]
async fn skips_navigation_listeners_when_the_navigation_api_is_unavailable() {
    start().await;
    assert_eq!(start_listeners_without("navigation"), Vec::<String>::new());
}

// navigation.test.ts: skips navigation listeners when sourceElement is unavailable
#[wasm_bindgen_test]
async fn skips_navigation_listeners_when_source_element_is_unavailable() {
    start().await;
    assert_eq!(start_listeners_without("sourceElement"), Vec::<String>::new());
}

// navigation.test.ts: suppresses Chromium scroll anchoring until after a push navigation paints
#[wasm_bindgen_test]
async fn suppresses_chromium_scroll_anchoring_until_after_a_push_navigation_paints() {
    let page = Page::new().await;
    next_frame().await;
    next_frame().await;
    let count = adopted_sheets();
    let starting_min_height = computed("documentElement", "min-height");
    let release = hold_next_resolve();
    let a: web_sys::HtmlElement = page.add(&link("anchoring-push", r#"data-rmx-target="details""#)).unchecked_into();
    a.click();
    wait_for_request().await;
    let during = (adopted_sheets(), computed("documentElement", "overflow-anchor"), computed("body", "overflow-anchor"), computed("documentElement", "min-height"));
    let finished = get(&get(&navigation(), "transition"), "finished").unchecked_into::<js_sys::Promise>();
    release.call0(&JsValue::NULL).unwrap();
    let _ = JsFuture::from(finished).await;
    JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await.unwrap();
    let after_finish = adopted_sheets();
    next_frame().await;
    let first_frame = adopted_sheets();
    next_frame().await;
    let second_frame = adopted_sheets();
    drop(page);

    assert_eq!(decisions(), [true]);
    assert_eq!(during, (count + 1, "none".to_string(), "none".to_string(), starting_min_height), "only the anchoring rule");
    assert_eq!((after_finish, first_frame, second_frame), (count + 1, count + 1, count));
}

// navigation.test.ts: removes traversal scroll styles as soon as the navigation fails
#[wasm_bindgen_test]
async fn removes_traversal_scroll_styles_as_soon_as_the_navigation_fails() {
    let page = Page::new().await;
    page.click(&link("fail-a", r#"data-rmx-target="details" data-rmx-src="/fail-a""#)).await;
    page.click(&link("fail-b", r#"data-rmx-target="details" data-rmx-src="/fail-b""#)).await;
    clear();
    next_frame().await;
    next_frame().await;
    let count = adopted_sheets();
    // With an error listener a failed load fails the navigation (instead of loading the
    // document).
    let app = APP.with(|a| a.borrow().clone()).expect("app");
    let listener = app.on_error(|_| {});
    let release = hold_next_resolve();
    RESOLVE_NEXT.with(|n| {
        *n.borrow_mut() = Some(Box::new(|_| {
            let hold = HOLD.with(|h| h.borrow_mut().take()).unwrap();
            Box::pin(async move {
                let _ = JsFuture::from(hold).await;
                Err("offline".into())
            })
        }))
    });
    let back: js_sys::Function = get(&navigation(), "back").unchecked_into();
    let result = back.call0(&navigation()).unwrap();
    wait_for_request().await;
    let during = adopted_sheets();
    release.call0(&JsValue::NULL).unwrap();
    let failed = JsFuture::from(get(&result, "finished").unchecked_into::<js_sys::Promise>()).await.is_err();
    JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await.unwrap();
    let after = adopted_sheets();
    app.remove_error_listener(listener);
    settle().await;

    assert!(failed, "the traversal failed");
    assert_eq!(during, count + 1, "the traversal styles while it runs");
    assert_eq!(after, count, "removed without waiting for a frame");
}

/// Dispatch a synthetic `navigate` event from `source` to `url` with `extra` fields (as
/// Remix's tests do) and return how many times the runtime intercepted it.
fn dispatch_navigate(source: &JsValue, url: &str, extra: &str) -> f64 {
    let dispatch = js_sys::Function::new_with_args(
        "source, url",
        &format!(
            "const calls = [];
             const controller = new AbortController();
             const event = Object.assign(new Event('navigate'), {{
               canIntercept: true, hashChange: false, downloadRequest: null, navigationType: 'push',
               sourceElement: source, formData: null, info: undefined, signal: controller.signal,
               destination: {{ url, getState: () => undefined }},
               intercept: (options) => calls.push(options),
             }}, {extra});
             navigation.dispatchEvent(event);
             controller.abort();
             return calls.length;"
        ),
    );
    dispatch.call2(&JsValue::NULL, source, &url.into()).unwrap().as_f64().unwrap()
}

// navigation.test.ts: does not intercept download requests with filenames, and with empty
// filenames (a plain anchor, so only the event's downloadRequest marks the download)
#[wasm_bindgen_test]
async fn does_not_intercept_download_requests_with_or_without_filenames() {
    let page = Page::new().await;
    let anchor = page.add(&link("download-request", r#"data-rmx-target="details""#));
    let url = page_url("download-request");
    let named = dispatch_navigate(&anchor, &url, "{ downloadRequest: 'report.csv' }");
    let empty = dispatch_navigate(&anchor, &url, "{ downloadRequest: '' }");
    tick().await;
    assert_eq!((named, empty), (0.0, 0.0));
    assert!(requests().is_empty());
}

// navigation.test.ts: replaces marked anchor history without precommit support
#[wasm_bindgen_test]
async fn replaces_marked_anchor_history_without_precommit_support() {
    let page = Page::new().await;
    js("window.__precommit = window.NavigationPrecommitController; delete window.NavigationPrecommitController;");
    let entries = entry_count();
    let anchor: web_sys::HtmlElement = page
        .add(&link("anchor-replace", r#"data-rmx-target="details" data-rmx-history="replace" data-rmx-reset-focus="false""#))
        .unchecked_into();
    anchor.focus().unwrap();
    anchor.click();
    settle().await;
    js("window.NavigationPrecommitController = window.__precommit; delete window.__precommit;");

    // The push is cancelled and replayed as a replace navigation (recorded first).
    assert_eq!(decisions(), [true, false]);
    assert_eq!(entry_count(), entries, "the entry is replaced");
    assert_eq!(current_url(), page_url("anchor-replace"));
    assert_eq!(requests().len(), 1);
    assert_eq!(document().active_element().as_ref(), Some(anchor.as_ref()), "focus stays");
}

fn absolute(path: &str) -> String {
    web_sys::Url::new_with_base(path, &current_url()).unwrap().href()
}

// navigation.test.ts: does not replace the navigation URL when a non-top frame reload
// follows a redirect
#[wasm_bindgen_test]
async fn does_not_replace_the_navigation_url_when_a_named_frame_load_follows_a_redirect() {
    let _page = Page::new().await;
    let entries = entry_count();
    let redirected = absolute("/redirected-frame");
    RESOLVE_NEXT.with(|n| {
        *n.borrow_mut() = Some(Box::new(move |_| {
            Box::pin(async move { Ok(FrameResponse { content: FrameContent::Html(r#"<p id="loaded">Redirected</p>"#.into()), url: Some(redirected) }) })
        }))
    });
    let requested = absolute("/requested-frame");
    navigate(&page_url("named-redirect"), NavigateOptions { target: Some("details".into()), src: Some(requested.clone()), ..Default::default() });
    settle().await;
    let state = current_state();

    assert_eq!(decisions(), [true], "no frame-redirect navigation");
    assert_eq!(requests().len(), 1);
    assert_eq!(entry_count(), entries + 1);
    assert_eq!(current_url(), page_url("named-redirect"));
    assert_eq!(get(&state, "target").as_string().as_deref(), Some("details"));
    assert_eq!(get(&state, "src").as_string(), Some(requested));
    assert_eq!(get(&state, "resetScroll").as_string().as_deref(), Some(AFTER));
    assert_eq!(get(&state, "resetFocus").as_string().as_deref(), Some(AFTER));
}

// navigation.test.ts: preserves same-origin relative sources for custom frame resolvers
#[wasm_bindgen_test]
async fn preserves_same_origin_relative_sources_for_custom_frame_resolvers() {
    let page = Page::new().await;
    let base = document().create_element("base").unwrap();
    base.set_attribute("href", &absolute("/partials/")).unwrap();
    document().head().unwrap().prepend_with_node_1(&base).unwrap();
    page.click(&link("relative-src", r#"data-rmx-target="details" data-rmx-src="../details?tab=one#summary""#)).await;
    base.remove();
    assert_eq!(decisions(), [true]);
    assert_eq!(Frames.get("details").unwrap().src(), "../details?tab=one#summary");
    let requests = requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].url.starts_with(&absolute("/details?tab=one")), "{}", requests[0].url);
}

// navigation.test.ts: honors legacy true settings on traverse navigations
#[wasm_bindgen_test]
async fn honors_legacy_true_settings_on_traverse_navigations() {
    let _page = Page::new().await;
    let state = r#"{ $rmx: true, target: 'details', src: '/legacy-true', resetScroll: true, resetFocus: true }"#;
    navigate_raw(&page_url("legacy-true"), &format!("{{ state: {state} }}"));
    settle().await;
    navigate(&page_url("legacy-true-next"), NavigateOptions { target: Some("details".into()), ..Default::default() });
    settle().await;
    back().await;
    assert_eq!(decisions(), [true, true, true]);
    assert_eq!(resets(), resets_of(&[(AFTER, AFTER), (AFTER, AFTER), (AFTER, AFTER)]));
    assert_eq!(scrolls().len(), 3);
    let requests = requests();
    assert!(requests[2].url.ends_with("/legacy-true"), "{}", requests[2].url);
}
