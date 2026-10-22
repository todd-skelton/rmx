//! Ports of Remix's `vdom.errors.test.tsx`: how a root reports component errors.
//! Run with `pwsh ./scripts/test-browser.ps1 -Test browser_errors`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rmx::{create_root, on, rsx, Component, EventHandler, Handle, Node, Root};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn container() -> web_sys::HtmlElement {
    web_sys::window().unwrap().document().unwrap().create_element("div").unwrap().unchecked_into()
}

/// Collect the root's error messages.
fn errors(root: &Root) -> Rc<RefCell<Vec<String>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    root.on_error(move |e| s.borrow_mut().push(e.message.clone()));
    seen
}

/// Wait `ms` milliseconds (lets the runtime's per-turn counters reset).
async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms).unwrap();
    });
    wasm_bindgen_futures::JsFuture::from(promise).await.unwrap();
}

/// Replace `console.warn` while `f` runs and return what it was called with.
fn capture_warnings(f: impl FnOnce()) -> Vec<String> {
    let console = js_sys::Reflect::get(&js_sys::global(), &"console".into()).unwrap();
    let original = js_sys::Reflect::get(&console, &"warn".into()).unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    let spy = Closure::<dyn Fn(JsValue)>::new(move |v: JsValue| s.borrow_mut().push(v.as_string().unwrap_or_default()));
    js_sys::Reflect::set(&console, &"warn".into(), spy.as_ref()).unwrap();
    f();
    js_sys::Reflect::set(&console, &"warn".into(), &original).unwrap();
    let out = seen.borrow().clone();
    out
}

fn error_event(message: &str) -> web_sys::ErrorEvent {
    let init = web_sys::ErrorEventInit::new();
    init.set_bubbles(true);
    init.set_error(&js_sys::Error::new(message));
    web_sys::ErrorEvent::new_with_event_init_dict("error", &init).unwrap()
}

// root event forwarding

#[wasm_bindgen_test]
fn forwards_bubbling_dom_error_events_to_root_listeners() {
    let el = container();
    let root = create_root(&el);
    let seen = errors(&root);
    el.dispatch_event(&error_event("createRoot forwarded error")).unwrap();
    assert_eq!(*seen.borrow(), vec!["createRoot forwarded error".to_string()]);
}

#[wasm_bindgen_test]
fn stops_forwarding_bubbling_dom_error_events_after_dispose() {
    let el = container();
    let root = create_root(&el);
    let seen = errors(&root);
    root.dispose();
    el.dispatch_event(&error_event("after dispose")).unwrap();
    assert!(seen.borrow().is_empty());
}

#[wasm_bindgen_test]
fn dispose_is_a_no_op_before_first_render() {
    let el = container();
    let root = create_root(&el);
    root.dispose();
    root.flush();
    assert_eq!(el.inner_html(), "");
}

// setup errors

struct SetupUpdate;

impl Component for SetupUpdate {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        h.update();
        SetupUpdate
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <div>"ok"</div> }
    }
}

#[wasm_bindgen_test]
fn warns_and_ignores_handle_update_calls_during_setup() {
    let el = container();
    let root = create_root(&el);
    let seen = errors(&root);
    let warnings = capture_warnings(|| {
        root.render(rsx! { <SetupUpdate/> });
        root.flush();
    });
    assert!(seen.borrow().is_empty());
    assert_eq!(
        warnings,
        vec!["Ignored handle.update() while SetupUpdate is running its setup function. The initial render includes setup changes.".to_string()]
    );
    assert_eq!(el.inner_html(), "<div>ok</div>");
}

#[derive(Clone, Default)]
struct ChildProps {
    update_parent: Option<Rc<dyn Fn()>>,
}

struct Child;

impl Component for Child {
    type Props = ChildProps;
    fn setup(props: &ChildProps, _: &Handle<Self>) -> Self {
        if let Some(f) = &props.update_parent {
            f();
        }
        Child
    }
    fn render(&self, _: &ChildProps, _: &Handle<Self>) -> Node {
        Node::Empty
    }
}

struct Parent;

impl Component for Parent {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Parent
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let h = h.clone();
        rsx! { <Child update_parent={Some(Rc::new(move || h.update()) as Rc<dyn Fn()>)}/> }
    }
}

#[wasm_bindgen_test]
fn reports_updates_to_a_parent_before_the_parents_initial_render_commits() {
    let el = container();
    let root = create_root(&el);
    let seen = errors(&root);
    root.render(rsx! { <Parent/> });
    assert_eq!(
        *seen.borrow(),
        vec!["Cannot call handle.update() before Parent's initial render commits. Call it from an event handler or handle.queueTask() instead.".to_string()]
    );
}

// render errors

struct RenderUpdate;

impl Component for RenderUpdate {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        RenderUpdate
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        h.update();
        rsx! { <div>"ok"</div> }
    }
}

#[wasm_bindgen_test]
fn reports_handle_update_calls_during_render() {
    let el = container();
    let root = create_root(&el);
    let seen = errors(&root);
    root.render(rsx! { <RenderUpdate/> });
    assert_eq!(
        *seen.borrow(),
        vec!["Cannot call handle.update() while RenderUpdate is running its render function. Call it from an event handler or handle.queueTask() instead.".to_string()]
    );
    assert_eq!(el.inner_html(), "<div>ok</div>");
}

// failed setup and render: `Handle::fail` stands in for throwing

/// Setup cannot fail in Rust; a component keeps the failure as state and fails its render.
struct BadSetup {
    error: Option<String>,
}

impl Component for BadSetup {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        BadSetup { error: Some("setup error".into()) }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        match &self.error {
            Some(e) => h.fail(e),
            None => rsx! { <div>"ok"</div> },
        }
    }
}

#[wasm_bindgen_test]
fn reports_a_failed_setup_and_leaves_the_dom_empty() {
    let el = container();
    let root = create_root(&el);
    let seen = errors(&root);
    root.render(rsx! { <BadSetup/> });
    root.flush();
    assert_eq!(*seen.borrow(), vec!["setup error".to_string()]);
    assert_eq!(el.inner_html(), "");
}

struct BadChildParent;

impl Component for BadChildParent {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        BadChildParent
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <div><BadSetup/></div> }
    }
}

#[wasm_bindgen_test]
fn reports_a_failed_nested_component_setup() {
    let el = container();
    let root = create_root(&el);
    let seen = errors(&root);
    root.render(rsx! { <BadChildParent/> });
    root.flush();
    assert_eq!(*seen.borrow(), vec!["setup error".to_string()]);
    assert_eq!(el.inner_html(), "<div></div>");
}

/// Fails a render partway through building its output, like a throw inside JSX.
struct BadRender;

impl Component for BadRender {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        BadRender
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        rsx! { <div>"partial" {h.fail("render error")}</div> }
    }
}

#[wasm_bindgen_test]
fn reports_a_failed_render_and_leaves_the_dom_empty() {
    let el = container();
    let root = create_root(&el);
    let seen = errors(&root);
    root.render(rsx! { <section><BadRender/></section> });
    root.flush();
    assert_eq!(*seen.borrow(), vec!["render error".to_string()]);
    assert_eq!(el.inner_html(), "<section></section>");
}

thread_local! {
    static FAIL_RENDER: Cell<bool> = const { Cell::new(false) };
    static RENDER_COUNT: Cell<u32> = const { Cell::new(0) };
    static UPDATE: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
}

struct Flaky;

impl Component for Flaky {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        let h = h.clone();
        UPDATE.with(|u| *u.borrow_mut() = Some(Box::new(move || h.update())));
        Flaky
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let n = RENDER_COUNT.with(|c| {
            c.set(c.get() + 1);
            c.get()
        });
        if FAIL_RENDER.with(Cell::get) {
            return h.fail("render update error");
        }
        rsx! { <div>"ok " {n}</div> }
    }
}

fn update_flaky() {
    UPDATE.with(|u| (u.borrow().as_ref().unwrap())());
}

#[wasm_bindgen_test]
fn reports_a_failed_update_and_keeps_the_previous_dom() {
    let el = container();
    let root = create_root(&el);
    let seen = errors(&root);
    FAIL_RENDER.with(|f| f.set(false));
    RENDER_COUNT.with(|c| c.set(0));
    root.render(rsx! { <Flaky/> });
    root.flush();
    assert_eq!(el.inner_html(), "<div>ok 1</div>");
    assert!(seen.borrow().is_empty());

    FAIL_RENDER.with(|f| f.set(true));
    update_flaky();
    root.flush();
    assert_eq!(*seen.borrow(), vec!["render update error".to_string()]);
    assert_eq!(el.inner_html(), "<div>ok 1</div>", "the previous DOM is kept");

    // The next successful render patches as usual.
    FAIL_RENDER.with(|f| f.set(false));
    update_flaky();
    root.flush();
    assert_eq!(el.inner_html(), "<div>ok 3</div>");
    assert_eq!(seen.borrow().len(), 1);
}

// event handlers

fn click(el: &web_sys::HtmlElement) {
    el.query_selector("button").unwrap().unwrap().unchecked_into::<web_sys::HtmlElement>().click();
}

#[wasm_bindgen_test]
fn runs_sync_event_handlers_attached_with_the_on_mixin_and_keeps_the_dom() {
    let el = container();
    let root = create_root(&el);
    let clicks = Rc::new(Cell::new(0));
    let c = clicks.clone();
    root.render(rsx! { <button mix={on("click", EventHandler::new(move |_| c.set(c.get() + 1)))}>"Click"</button> });
    root.flush();
    assert_eq!(el.inner_html(), "<button>Click</button>");
    click(&el);
    assert_eq!(clicks.get(), 1);
    assert_eq!(el.inner_html(), "<button>Click</button>");
}

#[wasm_bindgen_test]
async fn runs_async_event_handlers_attached_with_the_on_mixin() {
    let el = container();
    let root = create_root(&el);
    let calls = Rc::new(Cell::new(0));
    let c = calls.clone();
    root.render(rsx! {
        <button mix={on("click", EventHandler::new(move |_| {
            let c = c.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&JsValue::NULL)).await;
                c.set(c.get() + 1);
            });
        }))}>"Click"</button>
    });
    root.flush();
    click(&el);
    sleep(0).await;
    assert_eq!(calls.get(), 1);
}

// task errors: Rust tasks report failures instead of throwing

struct UpdateTask;

impl Component for UpdateTask {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        let h = h.clone();
        UPDATE.with(|u| {
            *u.borrow_mut() = Some(Box::new(move || {
                h.queue_task(|_, h| h.report_error("update task error"));
                h.update();
            }))
        });
        UpdateTask
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <div>"ok"</div> }
    }
}

#[wasm_bindgen_test]
fn reports_errors_from_a_task_queued_with_an_update() {
    let el = container();
    let root = create_root(&el);
    let seen = errors(&root);
    root.render(rsx! { <UpdateTask/> });
    root.flush();
    assert!(seen.borrow().is_empty());
    update_flaky();
    root.flush();
    assert_eq!(*seen.borrow(), vec!["update task error".to_string()]);
}

struct Bad;

impl Component for Bad {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        h.queue_task(|_, h| h.report_error("bad task"));
        Bad
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <div>"bad"</div> }
    }
}

thread_local! {
    static TASK_RAN: Cell<bool> = const { Cell::new(false) };
}

struct Good;

impl Component for Good {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        h.queue_task(|_, _| TASK_RAN.with(|r| r.set(true)));
        Good
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <div>"good"</div> }
    }
}

#[wasm_bindgen_test]
fn reported_task_errors_reach_the_root_and_other_tasks_still_run() {
    let el = container();
    let root = create_root(&el);
    let seen = errors(&root);
    TASK_RAN.with(|r| r.set(false));
    root.render(rsx! { <Bad/><Good/> });
    root.flush();
    assert_eq!(*seen.borrow(), vec!["bad task".to_string()]);
    assert!(TASK_RAN.with(Cell::get));
    assert_eq!(el.inner_html(), "<div>bad</div><div>good</div>");
}

// cascading updates protection

thread_local! {
    static RENDERS: Cell<u32> = const { Cell::new(0) };
    static SHOULD_LOOP: Cell<bool> = const { Cell::new(false) };
    static TRIGGER: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
}

struct InfiniteLoop;

impl Component for InfiniteLoop {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        let h = h.clone();
        TRIGGER.with(|t| *t.borrow_mut() = Some(Box::new(move || h.update())));
        InfiniteLoop
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let count = RENDERS.with(|r| {
            r.set(r.get() + 1);
            r.get()
        });
        if SHOULD_LOOP.with(Cell::get) {
            h.queue_task(|_, h| h.update());
        }
        rsx! { <div>"count: " {count}</div> }
    }
}

#[wasm_bindgen_test]
async fn reports_a_cascading_queued_update_loop() {
    sleep(0).await;
    let el = container();
    let root = create_root(&el);
    let seen = errors(&root);
    RENDERS.with(|r| r.set(0));
    SHOULD_LOOP.with(|l| l.set(false));
    root.render(rsx! { <InfiniteLoop/> });
    root.flush();
    assert_eq!(el.inner_html(), "<div>count: 1</div>");

    SHOULD_LOOP.with(|l| l.set(true));
    let warnings = capture_warnings(|| TRIGGER.with(|t| (t.borrow().as_ref().unwrap())()));
    assert!(warnings.is_empty());
    sleep(10).await;
    SHOULD_LOOP.with(|l| l.set(false));

    let seen = seen.borrow();
    assert!(!seen.is_empty(), "no error reported");
    assert!(seen[0].contains("infinite loop detected"), "{}", seen[0]);
    assert!(seen[0].contains("InfiniteLoop"), "{}", seen[0]);
    assert!(RENDERS.with(Cell::get) < 100);
    root.dispose();
}

thread_local! {
    static COUNT: Cell<u32> = const { Cell::new(0) };
    static UPDATES: RefCell<Vec<Box<dyn Fn()>>> = RefCell::new(Vec::new());
    static COUNTS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Default)]
struct CounterProps {
    index: usize,
}

struct Counter;

impl Component for Counter {
    type Props = CounterProps;
    fn setup(_: &CounterProps, h: &Handle<Self>) -> Self {
        let h = h.clone();
        UPDATES.with(|u| u.borrow_mut().push(Box::new(move || h.update())));
        Counter
    }
    fn render(&self, props: &CounterProps, _: &Handle<Self>) -> Node {
        let count = COUNTS.with(|c| c.borrow().get(props.index).copied()).unwrap_or_else(|| COUNT.with(Cell::get));
        rsx! { <div data-index={props.index.to_string()}>"count: " {count}</div> }
    }
}

#[wasm_bindgen_test]
async fn allows_legitimate_multiple_updates_within_the_same_event_loop_turn() {
    sleep(0).await;
    let el = container();
    let root = create_root(&el);
    let seen = errors(&root);
    UPDATES.with(|u| u.borrow_mut().clear());
    COUNTS.with(|c| c.borrow_mut().clear());
    COUNT.with(|c| c.set(0));
    root.render(rsx! { <Counter index={usize::MAX}/> });
    root.flush();
    for _ in 0..3 {
        COUNT.with(|c| c.set(c.get() + 1));
        UPDATES.with(|u| (u.borrow()[0])());
        root.flush();
    }
    assert_eq!(el.inner_html(), format!("<div data-index=\"{}\">count: 3</div>", usize::MAX));
    assert!(seen.borrow().is_empty());
    root.dispose();
}

#[wasm_bindgen_test]
async fn warns_instead_of_erroring_when_many_component_instances_update_in_one_turn() {
    sleep(0).await;
    let el = container();
    let root = create_root(&el);
    let seen = errors(&root);
    UPDATES.with(|u| u.borrow_mut().clear());
    COUNTS.with(|c| *c.borrow_mut() = vec![0; 60]);
    let list: Vec<Node> = (0..60).map(|index| rsx! { <Counter key={index} index={index}/> }).collect();
    root.render(list);
    root.flush();
    let warnings = capture_warnings(|| {
        for index in 0..60 {
            COUNTS.with(|c| c.borrow_mut()[index] += 1);
            UPDATES.with(|u| (u.borrow()[index])());
            root.flush();
        }
    });
    assert!(seen.borrow().is_empty(), "{:?}", seen.borrow());
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("50 cascading component updates"), "{}", warnings[0]);
    assert!(warnings[0].contains("Counter x50"), "{}", warnings[0]);
    let last = el.query_selector("[data-index=\"59\"]").unwrap().unwrap();
    assert_eq!(last.text_content().unwrap(), "count: 1");
    root.dispose();
}
