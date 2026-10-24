//! Ports of Remix's `vdom.tasks`, `vdom.scheduler`, `vdom.signals`, `vdom.connect` and
//! `vdom.context` tests. Run with `pwsh ./scripts/test-browser.ps1 -Test browser_vdom`.
#![cfg(target_arch = "wasm32")]

extern crate rsmix_component as rsmix;

use std::cell::{Cell, RefCell};

use std::rc::Rc;

use rsmix::{create_root, node_ref, on, rsx, Children, Component, EventHandler, Handle, Node, Signal};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn container() -> web_sys::HtmlElement {
    web_sys::window().unwrap().document().unwrap().create_element("div").unwrap().unchecked_into()
}

/// `await Promise.resolve()`: let pending microtasks run.
async fn microtask() {
    wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&wasm_bindgen::JsValue::UNDEFINED)).await.unwrap();
}

thread_local! {
    /// What the test calls to poke the component under test (`capturedUpdate`).
    static POKE: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
    static POKE_PARENT: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
    static COUNT: Cell<u32> = const { Cell::new(0) };
    static CHILD_COUNT: Cell<u32> = const { Cell::new(0) };
    static SIGNALS: RefCell<Vec<Signal>> = const { RefCell::new(Vec::new()) };
}

fn poke() {
    POKE.with(|p| (p.borrow().as_ref().expect("poke"))());
}

fn set_poke(f: impl Fn() + 'static) {
    POKE.with(|p| *p.borrow_mut() = Some(Box::new(f)));
}

fn reset() {
    POKE.with(|p| p.borrow_mut().take());
    POKE_PARENT.with(|p| p.borrow_mut().take());
    COUNT.with(|c| c.set(0));
    CHILD_COUNT.with(|c| c.set(0));
    SIGNALS.with(|s| s.borrow_mut().clear());
}

fn count() -> u32 {
    COUNT.with(Cell::get)
}

fn signals() -> Vec<Signal> {
    SIGNALS.with(|s| s.borrow().clone())
}

// tasks

struct TaskOnUpdate;

impl Component for TaskOnUpdate {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        let h = h.clone();
        set_poke(move || {
            h.queue_task(|_, _| COUNT.with(|c| c.set(c.get() + 1)));
            h.update();
        });
        TaskOnUpdate
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <div>"Hello, world!"</div> }
    }
}

#[wasm_bindgen_test]
fn runs_update_tasks_after_updates() {
    reset();
    let root = create_root(&container());
    root.render(rsx! { <TaskOnUpdate/> });
    root.flush();
    assert_eq!(count(), 0);
    poke();
    assert_eq!(count(), 0);
    root.flush();
    assert_eq!(count(), 1);
}

struct UpdateSignal;

impl Component for UpdateSignal {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        let h = h.clone();
        set_poke(move || {
            // Like `handle.update().then(...)`: the update is scheduled right away.
            let updated = h.updated();
            wasm_bindgen_futures::spawn_local(async move {
                let signal = updated.await;
                SIGNALS.with(|s| s.borrow_mut().push(signal));
            });
        });
        UpdateSignal
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        Node::Empty
    }
}

#[wasm_bindgen_test]
async fn handle_updated_resolves_with_a_signal() {
    reset();
    let root = create_root(&container());
    root.render(rsx! { <UpdateSignal/> });
    root.flush();
    assert!(signals().is_empty());
    poke();
    assert!(signals().is_empty());
    microtask().await;
    root.flush();
    microtask().await;
    assert_eq!(signals().len(), 1);
    assert!(!signals()[0].aborted());
}

// scheduler

#[derive(Clone, Default)]
struct ParentProps {
    children: Children,
}

struct Parent;

impl Component for Parent {
    type Props = ParentProps;
    fn setup(_: &ParentProps, h: &Handle<Self>) -> Self {
        let h = h.clone();
        POKE_PARENT.with(|p| *p.borrow_mut() = Some(Box::new(move || h.update())));
        Parent
    }
    fn render(&self, props: &ParentProps, _: &Handle<Self>) -> Node {
        COUNT.with(|c| c.set(c.get() + 1));
        rsx! { {&props.children} }
    }
}

struct Child;

impl Component for Child {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        let h = h.clone();
        set_poke(move || h.update());
        Child
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        CHILD_COUNT.with(|c| c.set(c.get() + 1));
        rsx! { <div>"Hello, world!"</div> }
    }
}

#[wasm_bindgen_test]
fn skips_descendant_updates_if_ancestor_is_scheduled() {
    reset();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <Parent><Child/></Parent> });
    assert_eq!(el.inner_html(), "<div>Hello, world!</div>");
    assert_eq!((count(), CHILD_COUNT.with(Cell::get)), (1, 1));

    poke();
    POKE_PARENT.with(|p| (p.borrow().as_ref().unwrap())());
    root.flush();
    assert_eq!((count(), CHILD_COUNT.with(Cell::get)), (2, 2));

    POKE_PARENT.with(|p| (p.borrow().as_ref().unwrap())());
    poke();
    root.flush();
    assert_eq!((count(), CHILD_COUNT.with(Cell::get)), (3, 3));
}

struct TasksOnce;

impl Component for TasksOnce {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        h.queue_task(|_, _| COUNT.with(|c| c.set(c.get() + 1)));
        let h = h.clone();
        set_poke(move || {
            h.queue_task(|_, _| COUNT.with(|c| c.set(c.get() + 1)));
            h.update();
        });
        TasksOnce
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        Node::Empty
    }
}

#[wasm_bindgen_test]
fn only_runs_tasks_once() {
    reset();
    let root = create_root(&container());
    root.render(rsx! { <TasksOnce/> });
    root.flush();
    assert_eq!(count(), 1);
    poke();
    root.flush();
    assert_eq!(count(), 2);
}

// signals

struct MountedSignal;

impl Component for MountedSignal {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        SIGNALS.with(|s| s.borrow_mut().push(Signal::from(h.signal())));
        MountedSignal
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        Node::Empty
    }
}

#[wasm_bindgen_test]
fn provides_mounted_signal_on_handle_signal() {
    reset();
    let root = create_root(&container());
    root.render(rsx! { <MountedSignal/> });
    assert!(!signals()[0].aborted());
    root.render(());
    root.flush();
    assert!(signals()[0].aborted());
}

struct TaskSignal;

impl Component for TaskSignal {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        h.queue_task(|_, h| SIGNALS.with(|s| s.borrow_mut().push(h.render_signal())));
        TaskSignal
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        Node::Empty
    }
}

#[wasm_bindgen_test]
fn provides_render_signal_to_tasks_and_aborts_on_re_render() {
    reset();
    let root = create_root(&container());
    root.render(rsx! { <TaskSignal/> });
    root.flush();
    assert_eq!(signals().len(), 1);
    assert!(!signals()[0].aborted());
    root.render(rsx! { <TaskSignal/> });
    root.flush();
    assert_eq!(signals().len(), 1);
    assert!(signals()[0].aborted());
}

#[wasm_bindgen_test]
async fn aborts_handle_updated_signal_on_next_update() {
    reset();
    let root = create_root(&container());
    root.render(rsx! { <UpdateSignal/> });
    root.flush();
    poke();
    microtask().await;
    root.flush();
    microtask().await;
    let first = signals()[0].clone();
    assert!(!first.aborted());
    poke();
    root.flush();
    assert!(first.aborted());
}

#[wasm_bindgen_test]
fn aborts_queue_task_signal_when_component_is_removed() {
    reset();
    let root = create_root(&container());
    root.render(rsx! { <TaskSignal/> });
    root.flush();
    assert!(!signals()[0].aborted());
    root.render(());
    root.flush();
    assert!(signals()[0].aborted());
}

#[wasm_bindgen_test]
async fn aborts_handle_updated_signal_when_component_is_removed() {
    reset();
    let root = create_root(&container());
    root.render(rsx! { <UpdateSignal/> });
    root.flush();
    poke();
    microtask().await;
    root.flush();
    microtask().await;
    assert!(!signals()[0].aborted());
    root.render(());
    root.flush();
    assert!(signals()[0].aborted());
}

// connect (ref)

thread_local! {
    static NODE: RefCell<Option<web_sys::Element>> = const { RefCell::new(None) };
}

struct RefApp;

impl Component for RefApp {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        let h = h.clone();
        set_poke(move || h.update());
        RefApp
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let capture = node_ref(|node, signal| {
            COUNT.with(|c| c.set(c.get() + 1));
            NODE.with(|n| *n.borrow_mut() = Some(node.element().clone()));
            signal.on_abort(|| NODE.with(|n| *n.borrow_mut() = None));
        });
        rsx! { <div mix={capture}>"Hello, world!"</div> }
    }
}

#[wasm_bindgen_test]
fn connects_host_node_lifecycle_to_component_scope() {
    reset();
    let root = create_root(&container());
    root.render(rsx! { <RefApp/> });
    root.flush();
    assert!(NODE.with(|n| n.borrow().as_ref().is_some_and(|e| e.is_instance_of::<web_sys::HtmlDivElement>())));
    root.render(());
    root.flush();
    assert!(NODE.with(|n| n.borrow().is_none()));
}

#[wasm_bindgen_test]
fn calls_ref_only_once() {
    reset();
    let root = create_root(&container());
    root.render(rsx! { <RefApp/> });
    root.flush();
    assert_eq!(count(), 1);
    poke();
    root.flush();
    assert_eq!(count(), 1);
}

// context

#[derive(Clone, Default)]
struct AppProps {
    children: Children,
}

struct ValueContext {
    value: String,
}

struct ContextApp;

impl Component for ContextApp {
    type Props = AppProps;
    fn setup(_: &AppProps, h: &Handle<Self>) -> Self {
        h.set_context(ValueContext { value: "test".into() });
        let h = h.clone();
        set_poke(move || {
            h.set_context(ValueContext { value: "test2".into() });
            h.update();
        });
        ContextApp
    }
    fn render(&self, props: &AppProps, _: &Handle<Self>) -> Node {
        rsx! { <div>{&props.children}</div> }
    }
}

struct SetupReader {
    value: String,
}

impl Component for SetupReader {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        SetupReader { value: h.context::<ValueContext>().unwrap().value.clone() }
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <main>"Child: " {self.value.clone()}</main> }
    }
}

struct RenderReader;

impl Component for RenderReader {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        RenderReader
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let value = h.context::<ValueContext>().unwrap().value.clone();
        rsx! { <main>"Child: " {value}</main> }
    }
}

#[wasm_bindgen_test]
fn provides_and_reads_context() {
    reset();
    let el = container();
    create_root(&el).render(rsx! { <ContextApp><SetupReader/></ContextApp> });
    assert!(el.inner_html().contains("Child: test"));
}

#[wasm_bindgen_test]
fn provides_context_on_updates() {
    reset();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <ContextApp><RenderReader/></ContextApp> });
    assert!(el.inner_html().contains("Child: test"));
    poke();
    root.flush();
    assert!(el.inner_html().contains("Child: test2"), "{}", el.inner_html());
}

thread_local! {
    static OPTIONS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

struct ListboxContext {
    register: Rc<dyn Fn(&str)>,
}

struct Listbox;

impl Component for Listbox {
    type Props = AppProps;
    fn setup(_: &AppProps, h: &Handle<Self>) -> Self {
        h.set_context(ListboxContext { register: Rc::new(|o| OPTIONS.with(|v| v.borrow_mut().push(o.to_string()))) });
        let h = h.clone();
        set_poke(move || h.update());
        Listbox
    }
    fn render(&self, props: &AppProps, _: &Handle<Self>) -> Node {
        OPTIONS.with(|v| v.borrow_mut().clear());
        rsx! { <div>{&props.children}</div> }
    }
}

#[derive(Clone, Default)]
struct OptionProps {
    value: &'static str,
}

struct ListOption {
    register: Rc<dyn Fn(&str)>,
}

impl Component for ListOption {
    type Props = OptionProps;
    fn setup(_: &OptionProps, h: &Handle<Self>) -> Self {
        ListOption { register: h.context::<ListboxContext>().unwrap().register.clone() }
    }
    fn render(&self, props: &OptionProps, _: &Handle<Self>) -> Node {
        (self.register)(props.value);
        rsx! { <div>"Option"</div> }
    }
}

struct ListApp;

impl Component for ListApp {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        ListApp
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! {
            <Listbox>
                <ListOption value="Option 1"/>
                <ListOption value="Option 2"/>
                <ListOption value="Option 3"/>
            </Listbox>
        }
    }
}

#[wasm_bindgen_test]
fn renders_descendants_in_order_of_appearance() {
    reset();
    let root = create_root(&container());
    root.render(rsx! { <ListApp/> });
    assert_eq!(OPTIONS.with(|o| o.borrow().clone()), ["Option 1", "Option 2", "Option 3"]);
    poke();
    root.flush();
    assert_eq!(OPTIONS.with(|o| o.borrow().clone()), ["Option 1", "Option 2", "Option 3"]);
}

// events

fn counter_click() -> rsmix::Mixin {
    on("click", EventHandler::new(|_| COUNT.with(|c| c.set(c.get() + 1))))
}

fn click(el: &web_sys::HtmlElement, selector: &str) {
    el.query_selector(selector).unwrap().unwrap().unchecked_into::<web_sys::HtmlElement>().click();
}

#[wasm_bindgen_test]
fn attaches_events_via_on_mixin() {
    reset();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <button mix={counter_click()}>"Click me"</button> });
    assert_eq!(el.inner_html(), "<button>Click me</button>");
    root.flush();
    click(&el, "button");
    assert_eq!(count(), 1);
}

struct ClickApp;

impl Component for ClickApp {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        ClickApp
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <button mix={counter_click()}>"Click me"</button> }
    }
}

#[wasm_bindgen_test]
fn updates_on_mixin_listeners_across_rerenders() {
    reset();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <ClickApp/> });
    root.flush();
    click(&el, "button");
    assert_eq!(count(), 1);
    root.render(rsx! { <ClickApp/> });
    root.flush();
    click(&el, "button");
    assert_eq!(count(), 2);
}

#[wasm_bindgen_test]
fn cleans_up_mixin_listeners_when_removed() {
    reset();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <button mix={counter_click()}>"Click me"</button> });
    root.flush();
    let button: web_sys::HtmlElement = el.query_selector("button").unwrap().unwrap().unchecked_into();
    button.click();
    assert_eq!(count(), 1);
    root.render(rsx! { <button>"Click me"</button> });
    root.flush();
    button.click();
    assert_eq!(count(), 1);
}

// inserts and removals (Rust rejects rendering booleans and undefined at compile time)

#[wasm_bindgen_test]
fn renders_text_numbers_and_nothing() {
    let cases: Vec<(Node, &str)> = vec![
        (rsmix::IntoNode::into_node("Hello, world!"), "Hello, world!"),
        (rsmix::IntoNode::into_node(42), "42"),
        (rsmix::IntoNode::into_node(0), "0"),
        (rsmix::IntoNode::into_node(9007199254740991i128), "9007199254740991"),
        (rsmix::IntoNode::into_node(()), ""),
        (rsmix::IntoNode::into_node(None::<Node>), ""),
    ];
    for (node, html) in cases {
        let el = container();
        create_root(&el).render(node);
        assert_eq!(el.inner_html(), html);
    }
}

#[wasm_bindgen_test]
fn removes_text_elements_fragments_and_components() {
    let cases: Vec<(Node, &str)> = vec![
        (rsx! { <div>"Hello, world!"</div> }, "<div>Hello, world!</div>"),
        (rsx! { <div><span>"Hello, world!"</span></div> }, "<div><span>Hello, world!</span></div>"),
        (rsx! { <div><><p>"Hello"</p><p>"world!"</p></></div> }, "<div><p>Hello</p><p>world!</p></div>"),
        (rsx! { <div><ClickApp/></div> }, "<div><button>Click me</button></div>"),
    ];
    for (node, html) in cases {
        let el = container();
        let root = create_root(&el);
        root.render(node);
        assert_eq!(el.inner_html(), html);
        root.render(rsx! { <div/> });
        assert_eq!(el.inner_html(), "<div></div>");
    }
}

#[wasm_bindgen_test]
fn removes_attributes() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <input id="hello" value="world"/> });
    let input: web_sys::HtmlInputElement = el.query_selector("input").unwrap().unwrap().unchecked_into();
    assert_eq!(input.value(), "world");
    assert_eq!(input.get_attribute("id").as_deref(), Some("hello"));
    root.render(rsx! { <input/> });
    root.flush();
    assert_eq!(input.value(), "");
    assert!(!input.has_attribute("id"));
    assert!(!input.has_attribute("value"));
}

#[wasm_bindgen_test]
fn removes_reflected_attributes_without_leaving_empty_values() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div id="hello" class="world">"content"</div> });
    let div = el.query_selector("div").unwrap().unwrap();
    assert_eq!(div.get_attribute("class").as_deref(), Some("world"));
    root.render(rsx! { <div>"content"</div> });
    root.flush();
    assert!(!div.has_attribute("id"));
    assert!(!div.has_attribute("class"));
}
