//! Ports of Remix's `form-navigation.test.ts`: matching a `navigate` event to the form
//! submission behind it, with submitter overrides, detached or reused submitters and
//! canceled duplicate submits. Run with
//! `pwsh ./scripts/test-browser.ps1 -Test browser_form_navigation`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rmx::test::{FormNavigation, FormNavigationResolver, FormSubmission};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn get(target: &JsValue, key: &str) -> JsValue {
    js_sys::Reflect::get(target, &key.into()).unwrap_or(JsValue::UNDEFINED)
}

fn call(target: &JsValue, method: &str, args: &[&JsValue]) -> JsValue {
    let f: js_sys::Function = get(target, method).unchecked_into();
    f.apply(target, &args.iter().copied().collect::<js_sys::Array>()).unwrap()
}

fn navigation() -> JsValue {
    get(&web_sys::window().unwrap(), "navigation")
}

fn href() -> String {
    web_sys::window().unwrap().location().href().unwrap()
}

/// `href` with `?form-navigation=value`.
fn destination(value: &str) -> String {
    let url = web_sys::Url::new(&href()).unwrap();
    url.search_params().set("form-navigation", value);
    url.href()
}

fn entries(form_data: Option<&web_sys::FormData>) -> Vec<(String, String)> {
    let Some(form_data) = form_data else { return Vec::new() };
    js_sys::try_iter(form_data)
        .unwrap()
        .unwrap()
        .map(|pair| {
            let pair: js_sys::Array = pair.unwrap().unchecked_into();
            (pair.get(0).as_string().unwrap(), pair.get(1).as_string().unwrap())
        })
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn create<T: JsCast>(tag: &str, attrs: &[(&str, &str)]) -> T {
    let el = document().create_element(tag).unwrap();
    for (k, v) in attrs {
        el.set_attribute(k, v).unwrap();
    }
    el.unchecked_into()
}

fn form(attrs: &[(&str, &str)]) -> web_sys::HtmlFormElement {
    create("form", attrs)
}

fn button(attrs: &[(&str, &str)]) -> web_sys::HtmlButtonElement {
    create("button", attrs)
}

fn input(name: &str, value: &str) -> web_sys::HtmlInputElement {
    let el: web_sys::HtmlInputElement = create("input", &[("name", name)]);
    el.set_value(value);
    el
}

fn append(parent: &web_sys::Node, children: &[&web_sys::Node]) {
    for child in children {
        parent.append_child(child).unwrap();
    }
}

fn submit(form: &web_sys::HtmlFormElement, submitter: &web_sys::HtmlButtonElement) {
    form.request_submit_with_submitter(Some(submitter)).unwrap();
}

fn body() -> web_sys::Node {
    document().body().unwrap().into()
}

/// Call `handler` on every `type` event of `target` until the closure is dropped (leaked
/// for the test's lifetime: targets are removed at the end of each test).
fn listen(target: &web_sys::EventTarget, kind: &str, handler: impl Fn(web_sys::Event) + 'static) {
    let closure = Closure::<dyn Fn(web_sys::Event)>::new(handler);
    target.add_event_listener_with_callback(kind, closure.as_ref().unchecked_ref()).unwrap();
    closure.forget();
}

fn prevent_submit(form: &web_sys::HtmlFormElement, once: bool) {
    let closure = Closure::<dyn Fn(web_sys::Event)>::new(|e: web_sys::Event| e.prevent_default()).into_js_value();
    let options = web_sys::AddEventListenerOptions::new();
    options.set_once(once);
    form.add_event_listener_with_callback_and_add_event_listener_options("submit", closure.unchecked_ref(), &options).unwrap();
}

/// A resolver plus a `navigate` listener that intercepts every navigation with it, like
/// the Remix tests' setup. Dropping it stops both.
struct Harness {
    controller: web_sys::AbortController,
    /// The last navigation's form metadata.
    last: Rc<RefCell<Option<FormNavigation>>>,
    /// The submission the intercept handler resolved (with `resolve_in_handler`).
    submission: Rc<RefCell<Option<FormSubmission>>>,
    destination: Rc<RefCell<Option<String>>>,
    initial_key: JsValue,
    _listener: Closure<dyn Fn(JsValue)>,
}

impl Harness {
    fn new(resolve_in_handler: bool) -> Harness {
        Harness::with_hook(resolve_in_handler, || {})
    }

    /// `before` runs at the start of each `navigate` event, before the resolver.
    fn with_hook(resolve_in_handler: bool, before: impl Fn() + 'static) -> Harness {
        let resolver = Rc::new(FormNavigationResolver::new());
        let last: Rc<RefCell<Option<FormNavigation>>> = Rc::default();
        let submission: Rc<RefCell<Option<FormSubmission>>> = Rc::default();
        let destination: Rc<RefCell<Option<String>>> = Rc::default();
        let (l, s, d) = (last.clone(), submission.clone(), destination.clone());
        let listener = Closure::<dyn Fn(JsValue)>::new(move |event: JsValue| {
            before();
            *d.borrow_mut() = get(&get(&event, "destination"), "url").as_string();
            let navigation = resolver.resolve(&event);
            let pending = navigation.as_ref().and_then(|n| n.submission()).filter(|_| resolve_in_handler);
            *l.borrow_mut() = navigation;
            let s = s.clone();
            let handler = Closure::once_into_js(move || -> js_sys::Promise {
                if let Some(pending) = pending {
                    *s.borrow_mut() = Some(pending.get());
                }
                js_sys::Promise::resolve(&JsValue::UNDEFINED)
            });
            let options = js_sys::Object::new();
            js_sys::Reflect::set(&options, &"handler".into(), &handler).unwrap();
            call(&event, "intercept", &[&options]);
        });
        let controller = web_sys::AbortController::new().unwrap();
        let options = js_sys::Object::new();
        js_sys::Reflect::set(&options, &"signal".into(), &controller.signal()).unwrap();
        call(&navigation(), "addEventListener", &[&"navigate".into(), listener.as_ref(), &options]);
        let initial_key = get(&get(&navigation(), "currentEntry"), "key");
        Harness { controller, last, submission, destination, initial_key, _listener: listener }
    }

    /// Resolves on the next `navigatesuccess`.
    fn next_navigation(&self) -> JsFuture {
        let make = js_sys::Function::new_with_args(
            "nav,signal",
            "return new Promise(r => nav.addEventListener('navigatesuccess', () => r(), { once: true, signal }))",
        );
        let promise: js_sys::Promise = make.call2(&JsValue::NULL, &navigation(), &self.controller.signal()).unwrap().unchecked_into();
        JsFuture::from(promise)
    }

    fn attribute(&self, name: &str, submitter_name: Option<&str>) -> Option<String> {
        self.last.borrow().as_ref().expect("a form navigation").get_attribute(name, submitter_name)
    }

    fn has_submission(&self) -> bool {
        self.last.borrow().as_ref().expect("a form navigation").submission().is_some()
    }

    fn take_submission(&self) -> FormSubmission {
        self.submission.borrow_mut().take().expect("a resolved submission")
    }

    /// Stop listening and go back to the entry the test started on.
    async fn finish(self) {
        self.controller.abort();
        let result = call(&navigation(), "traverseTo", &[&self.initial_key]);
        let _ = JsFuture::from(get(&result, "finished").unchecked_into::<js_sys::Promise>()).await;
    }
}

#[wasm_bindgen_test]
async fn resolves_browser_form_data_and_submitter_overrides() {
    let h = Harness::new(true);
    let form = form(&[("method", "get"), ("enctype", "application/x-www-form-urlencoded"), ("data-target", "form")]);
    form.set_action(&destination("submitter-overrides"));
    let count = Rc::new(Cell::new(0));
    let c = count.clone();
    listen(&form, "formdata", move |_| c.set(c.get() + 1));
    let button = button(&[("name", "intent"), ("value", "save"), ("formmethod", "post"), ("formenctype", "multipart/form-data"), ("data-submit-target", "button")]);
    append(&form, &[&input("name", "Ada"), &button]);
    append(&body(), &[&form]);

    let done = h.next_navigation();
    submit(&form, &button);
    done.await.unwrap();

    assert!(h.last.borrow().as_ref().unwrap().has_attribute("data-target"));
    assert_eq!(h.attribute("data-target", Some("data-submit-target")).as_deref(), Some("button"));
    let submission = h.take_submission();
    assert_eq!(submission.method, "post");
    assert_eq!(submission.enc_type, "multipart/form-data");
    assert_eq!(entries(submission.form_data.as_ref()), pairs(&[("name", "Ada"), ("intent", "save")]));
    assert_eq!(count.get(), 1);
    form.remove();
    h.finish().await;
}

#[wasm_bindgen_test]
async fn resolves_an_external_submit_button_associated_by_its_form_attribute() {
    let h = Harness::new(true);
    let container = document().create_element("div").unwrap();
    container.set_inner_html(
        r#"<form id="external-submitter-form" action="" method="get" enctype="application/x-www-form-urlencoded" data-rmx-target="form-frame">
            <input name="name" value="Ada" />
          </form>
          <button type="submit" form="external-submitter-form" name="intent" value="save" formmethod="post"
            formenctype="multipart/form-data" data-rmx-target="button-frame">Save</button>"#,
    );
    append(&body(), &[&container]);
    let button: web_sys::HtmlElement = container.query_selector("button").unwrap().unwrap().unchecked_into();

    let done = h.next_navigation();
    button.click();
    done.await.unwrap();

    assert_eq!(h.attribute("data-rmx-target", None).as_deref(), Some("button-frame"));
    let submission = h.take_submission();
    assert_eq!(submission.method, "post");
    assert_eq!(submission.enc_type, "multipart/form-data");
    assert_eq!(entries(submission.form_data.as_ref()), pairs(&[("name", "Ada"), ("intent", "save")]));
    container.remove();
    h.finish().await;
}

#[wasm_bindgen_test]
async fn preserves_a_reused_submitter_when_its_previous_form_submits_again() {
    let button = button(&[("name", "filter"), ("value", "example")]);
    let submitter_form: Rc<RefCell<Option<Option<web_sys::HtmlFormElement>>>> = Rc::default();
    let (b, sf) = (button.clone(), submitter_form.clone());
    let h = Harness::with_hook(false, move || *sf.borrow_mut() = Some(b.form()));
    let previous_form = form(&[]);
    prevent_submit(&previous_form, false);
    let form = form(&[("method", "get"), ("data-rmx-target", "items")]);
    form.set_action(&href());
    append(&previous_form, &[&button]);
    append(&body(), &[&previous_form, &form]);

    let done = h.next_navigation();
    submit(&previous_form, &button);
    form.append_child(&button).unwrap();
    submit(&form, &button);
    button.remove();
    previous_form.request_submit().unwrap();
    done.await.unwrap();

    assert_eq!(submitter_form.borrow().clone(), Some(None));
    assert_eq!(h.attribute("data-rmx-target", None).as_deref(), Some("items"));
    assert!(!h.has_submission());
    let url = web_sys::Url::new(&href()).unwrap();
    assert_eq!(url.search_params().get("filter").as_deref(), Some("example"));
    previous_form.remove();
    form.remove();
    h.finish().await;
}

#[wasm_bindgen_test]
async fn preserves_a_newer_submission_on_the_same_form_when_an_older_post_callback_completes() {
    let h = Harness::new(false);
    let form = form(&[("method", "post"), ("data-rmx-target", "form")]);
    form.set_action(&href());
    let old_button = button(&[("name", "intent"), ("value", "first")]);
    let new_button = button(&[("name", "intent"), ("value", "second"), ("data-rmx-target", "new-submitter")]);
    append(&form, &[&old_button, &new_button]);
    append(&body(), &[&form]);

    let first = h.next_navigation();
    submit(&form, &old_button);
    first.await.unwrap();
    let finish_old_post = h.last.borrow().as_ref().unwrap().submission().expect("a POST submission");

    let second = h.next_navigation();
    submit(&form, &new_button);
    new_button.remove();
    // Complete the old callback after the new submit, but before its navigate event.
    let first_submission = finish_old_post.get();
    second.await.unwrap();

    assert!(new_button.form().is_none());
    assert_eq!(h.attribute("data-rmx-target", None).as_deref(), Some("new-submitter"));
    let second_submission = h.last.borrow().as_ref().unwrap().submission().unwrap().get();
    assert_eq!(entries(first_submission.form_data.as_ref()), pairs(&[("intent", "first")]));
    assert_eq!(entries(second_submission.form_data.as_ref()), pairs(&[("intent", "second")]));
    form.remove();
    h.finish().await;
}

#[wasm_bindgen_test]
async fn preserves_a_submitter_reused_by_another_form_when_an_older_post_callback_completes() {
    let h = Harness::new(false);
    let previous_form = form(&[("method", "post")]);
    previous_form.set_action(&href());
    let form = form(&[("method", "post"), ("data-rmx-target", "new-form")]);
    form.set_action(&href());
    let button = button(&[("name", "intent"), ("value", "first")]);
    append(&previous_form, &[&button]);
    append(&body(), &[&previous_form, &form]);

    let first = h.next_navigation();
    submit(&previous_form, &button);
    first.await.unwrap();
    let finish_old_post = h.last.borrow().as_ref().unwrap().submission().expect("a POST submission");

    form.append_child(&button).unwrap();
    button.set_value("second");
    let second = h.next_navigation();
    submit(&form, &button);
    button.remove();
    let first_submission = finish_old_post.get();
    second.await.unwrap();

    assert!(button.form().is_none());
    assert_eq!(h.attribute("data-rmx-target", None).as_deref(), Some("new-form"));
    let second_submission = h.last.borrow().as_ref().unwrap().submission().unwrap().get();
    assert_eq!(entries(first_submission.form_data.as_ref()), pairs(&[("intent", "first")]));
    assert_eq!(entries(second_submission.form_data.as_ref()), pairs(&[("intent", "second")]));
    previous_form.remove();
    form.remove();
    h.finish().await;
}

#[wasm_bindgen_test]
async fn preserves_original_post_data_across_unrelated_form_data_snapshots() {
    let h = Harness::new(true);
    let form = form(&[("method", "post")]);
    form.set_action(&href());
    let input = input("name", "draft");
    let button = button(&[("name", "intent"), ("value", "save")]);
    append(&form, &[&input, &button]);
    append(&body(), &[&form]);
    let submit_snapshot: Rc<RefCell<Option<web_sys::FormData>>> = Rc::default();
    let (f, i, snap) = (form.clone(), input.clone(), submit_snapshot.clone());
    listen(&form, "submit", move |_| {
        *snap.borrow_mut() = Some(web_sys::FormData::new_with_form(&f).unwrap());
        i.set_value("Ada");
    });

    let done = h.next_navigation();
    submit(&form, &button);
    button.remove();
    input.set_value("Grace");
    let later_snapshot = web_sys::FormData::new_with_form(&form).unwrap();
    done.await.unwrap();

    assert_eq!(submit_snapshot.borrow().as_ref().unwrap().get("name").as_string().as_deref(), Some("draft"));
    assert_eq!(later_snapshot.get("name").as_string().as_deref(), Some("Grace"));
    let submission = h.take_submission();
    assert_eq!(submission.method, "post");
    assert_eq!(entries(submission.form_data.as_ref()), pairs(&[("name", "Ada"), ("intent", "save")]));
    form.remove();
    h.finish().await;
}

#[wasm_bindgen_test]
async fn preserves_the_original_submission_when_a_reused_submitter_has_a_later_canceled_submit() {
    let h = Harness::new(true);
    let url = destination("original-submission");
    let form = form(&[("method", "post"), ("data-rmx-target", "original-frame")]);
    form.set_action(&url);
    let canceled_form = form_with_cancel();
    let button = button(&[("name", "intent"), ("value", "save")]);
    append(&form, &[&input("name", "Ada"), &button]);
    append(&body(), &[&form, &canceled_form]);

    let done = h.next_navigation();
    submit(&form, &button);
    canceled_form.append_child(&button).unwrap();
    submit(&canceled_form, &button);
    let _ = web_sys::FormData::new_with_form(&canceled_form).unwrap();
    button.remove();
    done.await.unwrap();

    assert_eq!(h.destination.borrow().as_deref(), Some(url.as_str()));
    assert_eq!(h.attribute("data-rmx-target", None).as_deref(), Some("original-frame"));
    let submission = h.take_submission();
    assert_eq!(submission.method, "post");
    assert_eq!(entries(submission.form_data.as_ref()), pairs(&[("name", "Ada"), ("intent", "save")]));
    form.remove();
    canceled_form.remove();
    h.finish().await;
}

fn form_with_cancel() -> web_sys::HtmlFormElement {
    let canceled = form(&[("method", "get"), ("data-rmx-target", "canceled-frame")]);
    prevent_submit(&canceled, false);
    canceled
}

#[wasm_bindgen_test]
async fn preserves_a_pending_get_when_the_same_form_has_a_canceled_duplicate_submit() {
    let h = Harness::new(false);
    let form = form(&[("method", "get")]);
    form.set_action(&href());
    let button = button(&[("name", "intent"), ("value", "search"), ("data-rmx-target", "original-frame")]);
    append(&form, &[&button]);
    append(&body(), &[&form]);

    let done = h.next_navigation();
    submit(&form, &button);
    prevent_submit(&form, true);
    submit(&form, &button);
    button.remove();
    done.await.unwrap();

    assert!(button.form().is_none());
    assert_eq!(h.attribute("data-rmx-target", None).as_deref(), Some("original-frame"));
    assert!(!h.has_submission());
    let destination = h.destination.borrow().clone().expect("a destination");
    assert_eq!(web_sys::Url::new(&destination).unwrap().search_params().get("intent").as_deref(), Some("search"));
    form.remove();
    h.finish().await;
}

#[wasm_bindgen_test]
async fn preserves_a_pending_post_when_the_same_form_has_a_canceled_submit_with_another_button() {
    let h = Harness::new(true);
    let form = form(&[("method", "post")]);
    form.set_action(&href());
    let input = input("name", "Ada");
    let button = button(&[("name", "intent"), ("value", "save"), ("formenctype", "multipart/form-data"), ("data-rmx-target", "original-frame")]);
    let canceled_button = button_canceled();
    append(&form, &[&input, &button, &canceled_button]);
    append(&body(), &[&form]);

    let done = h.next_navigation();
    submit(&form, &button);
    input.set_value("Grace");
    prevent_submit(&form, true);
    submit(&form, &canceled_button);
    button.remove();
    let _ = web_sys::FormData::new_with_form(&form).unwrap();
    done.await.unwrap();

    assert!(button.form().is_none());
    assert_eq!(h.attribute("data-rmx-target", None).as_deref(), Some("original-frame"));
    let submission = h.take_submission();
    assert_eq!(submission.method, "post");
    assert_eq!(submission.enc_type, "multipart/form-data");
    assert_eq!(entries(submission.form_data.as_ref()), pairs(&[("name", "Ada"), ("intent", "save")]));
    form.remove();
    h.finish().await;
}

fn button_canceled() -> web_sys::HtmlButtonElement {
    button(&[("name", "intent"), ("value", "search"), ("formmethod", "get"), ("data-rmx-target", "canceled-frame")])
}

#[wasm_bindgen_test]
async fn preserves_browser_form_data_when_the_submitter_is_detached_before_navigate_fires() {
    let temp_button = button(&[("name", "intent"), ("value", "save")]);
    let submitter_form: Rc<RefCell<Option<Option<web_sys::HtmlFormElement>>>> = Rc::default();
    let (b, sf) = (temp_button.clone(), submitter_form.clone());
    let h = Harness::with_hook(true, move || *sf.borrow_mut() = Some(b.form()));
    let form = form(&[("method", "post")]);
    form.set_action(&destination("detached-submitter"));
    let count = Rc::new(Cell::new(0));
    let c = count.clone();
    listen(&form, "formdata", move |event| {
        c.set(c.get() + 1);
        let form_data: web_sys::FormData = get(&event, "formData").unchecked_into();
        form_data.set_with_str("token", "native").unwrap();
    });
    append(&body(), &[&form]);
    append(&form, &[&input("name", "Ada"), &temp_button]);

    let done = h.next_navigation();
    submit(&form, &temp_button);
    temp_button.remove();
    done.await.unwrap();

    assert_eq!(submitter_form.borrow().clone(), Some(None));
    let submission = h.take_submission();
    assert_eq!(submission.method, "post");
    assert_eq!(submission.enc_type, "application/x-www-form-urlencoded");
    assert_eq!(entries(submission.form_data.as_ref()), pairs(&[("name", "Ada"), ("intent", "save"), ("token", "native")]));
    assert_eq!(count.get(), 1);
    form.remove();
    h.finish().await;
}
