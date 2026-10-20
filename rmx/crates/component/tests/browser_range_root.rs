//! Ports of Remix's `vdom.range-root.test.tsx`: a root that owns the nodes between two
//! sibling markers. Run with `pwsh ./test-browser.ps1 -Test browser_range_root`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rmx::{create_range_root, rsx, Component, Handle, Node, Root};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn container() -> web_sys::HtmlElement {
    document().create_element("div").unwrap().unchecked_into()
}

/// A container holding `<!--start--><!--end-->`, and those two markers.
fn markers() -> (web_sys::HtmlElement, web_sys::Node, web_sys::Node) {
    let el = container();
    let start: web_sys::Node = document().create_comment("start").into();
    let end: web_sys::Node = document().create_comment("end").into();
    el.append_child(&start).unwrap();
    el.append_child(&end).unwrap();
    (el, start, end)
}

/// A container with `html` and its child nodes.
fn parse(html: &str) -> (web_sys::HtmlElement, Vec<web_sys::Node>) {
    let el = container();
    el.set_inner_html(html);
    let list = el.child_nodes();
    let nodes = (0..list.length()).map(|i| list.get(i).unwrap()).collect();
    (el, nodes)
}

fn find(el: &web_sys::HtmlElement, selector: &str) -> web_sys::Element {
    el.query_selector(selector).unwrap().unwrap_or_else(|| panic!("no {selector}"))
}

fn error_event(message: &str) -> web_sys::ErrorEvent {
    let init = web_sys::ErrorEventInit::new();
    init.set_bubbles(true);
    init.set_error(&js_sys::Error::new(message));
    web_sys::ErrorEvent::new_with_event_init_dict("error", &init).unwrap()
}

fn errors(root: &Root) -> Rc<RefCell<Vec<String>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    root.on_error(move |e| s.borrow_mut().push(e.message.clone()));
    seen
}

// event forwarding

#[wasm_bindgen_test]
fn forwards_bubbling_dom_error_events_to_range_root_listeners() {
    let (el, start, end) = markers();
    let root = create_range_root(&start, &end);
    let seen = errors(&root);
    el.dispatch_event(&error_event("createRangeRoot forwarded error")).unwrap();
    assert_eq!(*seen.borrow(), vec!["createRangeRoot forwarded error".to_string()]);
}

#[wasm_bindgen_test]
fn stops_forwarding_bubbling_dom_error_events_after_dispose() {
    let (el, start, end) = markers();
    let root = create_range_root(&start, &end);
    let seen = errors(&root);
    root.dispose();
    el.dispatch_event(&error_event("after dispose")).unwrap();
    assert!(seen.borrow().is_empty());
}

// basic rendering

#[wasm_bindgen_test]
fn dispose_is_a_no_op_before_first_render() {
    let (el, start, end) = markers();
    let root = create_range_root(&start, &end);
    root.dispose();
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><!--end-->");
}

#[wasm_bindgen_test]
fn renders_content_between_markers() {
    let (el, start, end) = markers();
    let root = create_range_root(&start, &end);
    root.render(rsx! { <div>"Hello"</div> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><div>Hello</div><!--end-->");
}

#[wasm_bindgen_test]
fn renders_text_between_markers() {
    let (el, start, end) = markers();
    let root = create_range_root(&start, &end);
    root.render("Hello, world!");
    root.flush();
    assert_eq!(el.inner_html(), "<!--start-->Hello, world!<!--end-->");
}

#[wasm_bindgen_test]
fn renders_fragments_between_markers() {
    let (el, start, end) = markers();
    let root = create_range_root(&start, &end);
    root.render(rsx! { <><p>"First"</p><p>"Second"</p></> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><p>First</p><p>Second</p><!--end-->");
}

struct Greeting;

impl Component for Greeting {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Greeting
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <span>"Hello!"</span> }
    }
}

#[wasm_bindgen_test]
fn renders_components_between_markers() {
    let (el, start, end) = markers();
    let root = create_range_root(&start, &end);
    root.render(rsx! { <Greeting/> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><span>Hello!</span><!--end-->");
}

// updates

#[wasm_bindgen_test]
fn updates_content_between_markers() {
    let (el, start, end) = markers();
    let root = create_range_root(&start, &end);
    root.render(rsx! { <div>"First"</div> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><div>First</div><!--end-->");
    root.render(rsx! { <div>"Second"</div> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><div>Second</div><!--end-->");
}

#[wasm_bindgen_test]
fn handles_adding_children() {
    let (el, start, end) = markers();
    let root = create_range_root(&start, &end);
    root.render(rsx! { <div></div> });
    root.flush();
    root.render(rsx! { <div><span>"Added"</span></div> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><div><span>Added</span></div><!--end-->");
}

#[wasm_bindgen_test]
fn handles_removing_children() {
    let (el, start, end) = markers();
    let root = create_range_root(&start, &end);
    root.render(rsx! { <div><span>"To remove"</span></div> });
    root.flush();
    root.render(rsx! { <div></div> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><div></div><!--end-->");
}

#[wasm_bindgen_test]
fn replaces_all_fragment_children_with_different_elements() {
    let (el, start, end) = markers();
    let root = create_range_root(&start, &end);
    root.render(rsx! { <><div>"First"</div><div>"Second"</div></> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><div>First</div><div>Second</div><!--end-->");
    root.render(rsx! { <><span>"A"</span><span>"B"</span></> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><span>A</span><span>B</span><!--end-->");
}

#[wasm_bindgen_test]
fn renders_null_then_content() {
    let (el, start, end) = markers();
    let root = create_range_root(&start, &end);
    root.render(());
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><!--end-->");
    root.render(rsx! { <div>"Now visible"</div> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><div>Now visible</div><!--end-->");
}

#[wasm_bindgen_test]
fn renders_content_then_null_then_content() {
    let (el, start, end) = markers();
    let root = create_range_root(&start, &end);
    root.render(rsx! { <div>"Visible"</div> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><div>Visible</div><!--end-->");
    root.render(());
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><!--end-->");
    root.render(rsx! { <span>"Back again"</span> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><span>Back again</span><!--end-->");
}

#[wasm_bindgen_test]
fn changes_fragment_child_count() {
    let (el, start, end) = markers();
    let root = create_range_root(&start, &end);
    root.render(rsx! { <><div>"One"</div><div>"Two"</div><div>"Three"</div></> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><div>One</div><div>Two</div><div>Three</div><!--end-->");
    root.render(rsx! { <div>"Only"</div> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><div>Only</div><!--end-->");
    root.render(rsx! { <><span>"A"</span><span>"B"</span></> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><span>A</span><span>B</span><!--end-->");
}

thread_local! {
    static CONTENT: RefCell<Node> = const { RefCell::new(Node::Empty) };
    static UPDATE: RefCell<Option<Handle<HydratedApp>>> = const { RefCell::new(None) };
}

/// Renders whatever `CONTENT` holds; `update()` re-renders it.
struct HydratedApp;

impl Component for HydratedApp {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        UPDATE.with(|u| *u.borrow_mut() = Some(h.clone()));
        HydratedApp
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        CONTENT.with(|c| c.borrow().clone())
    }
}

fn show(content: Node) {
    CONTENT.with(|c| *c.borrow_mut() = content);
    UPDATE.with(|u| u.borrow().as_ref().expect("app set up").update());
}

#[wasm_bindgen_test]
fn keeps_component_fragment_updates_inside_hydration_range_markers() {
    let (el, nodes) = parse("<!--rmx:h:c1--><div>1</div><!--/rmx:h-->");
    CONTENT.with(|c| *c.borrow_mut() = rsx! { <div>"1"</div> });
    let root = create_range_root(&nodes[0], &nodes[2]);
    root.render(rsx! { <HydratedApp/> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--rmx:h:c1--><div>1</div><!--/rmx:h-->");

    // Wrap content in a fragment.
    show(rsx! { <><div>"1"</div></> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--rmx:h:c1--><div>1</div><!--/rmx:h-->");

    // Add a new element before the existing content.
    show(rsx! { <><div>"0"</div><div>"1"</div></> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--rmx:h:c1--><div>0</div><div>1</div><!--/rmx:h-->");
}

// events

#[wasm_bindgen_test]
fn attaches_event_handlers() {
    let (el, start, end) = markers();
    let clicked = Rc::new(Cell::new(false));
    let c = clicked.clone();
    let root = create_range_root(&start, &end);
    root.render(rsx! { <button mix={rmx::on("click", rmx::EventHandler::new(move |_| c.set(true)))}>"Click me"</button> });
    root.flush();
    find(&el, "button").unchecked_into::<web_sys::HtmlElement>().click();
    assert!(clicked.get());
}

// multiple range roots

#[wasm_bindgen_test]
fn supports_multiple_ranges_in_same_container() {
    let (el, nodes) = parse("<!--a--><div>A</div><!--/a--><p>static</p><!--b--><div>B</div><!--/b-->");
    let existing_a = el.query_selector_all("div").unwrap().get(0).unwrap();
    let existing_b = el.query_selector_all("div").unwrap().get(1).unwrap();

    let root_a = create_range_root(&nodes[0], &nodes[2]);
    let root_b = create_range_root(&nodes[4], &nodes[6]);
    root_a.render(rsx! { <div>"A updated"</div> });
    root_b.render(rsx! { <div>"B updated"</div> });
    root_a.flush();
    root_b.flush();

    assert_eq!(el.inner_html(), "<!--a--><div>A updated</div><!--/a--><p>static</p><!--b--><div>B updated</div><!--/b-->");
    let divs = el.query_selector_all("div").unwrap();
    assert!(divs.get(0).unwrap().is_same_node(Some(&existing_a)));
    assert!(divs.get(1).unwrap().is_same_node(Some(&existing_b)));
}

#[wasm_bindgen_test]
fn ranges_are_independent() {
    let el = container();
    let marker = |text: &str| -> web_sys::Node {
        let node: web_sys::Node = document().create_comment(text).into();
        el.append_child(&node).unwrap();
        node
    };
    let (start_a, end_a, start_b, end_b) = (marker("a"), marker("/a"), marker("b"), marker("/b"));

    let root_a = create_range_root(&start_a, &end_a);
    let root_b = create_range_root(&start_b, &end_b);
    root_a.render(rsx! { <span>"A"</span> });
    root_a.flush();
    assert_eq!(el.inner_html(), "<!--a--><span>A</span><!--/a--><!--b--><!--/b-->");

    root_b.render(rsx! { <span>"B"</span> });
    root_b.flush();
    assert_eq!(el.inner_html(), "<!--a--><span>A</span><!--/a--><!--b--><span>B</span><!--/b-->");
}

// boundary handling

#[wasm_bindgen_test]
#[should_panic(expected = "Boundaries must share parent")]
fn panics_when_start_and_end_markers_do_not_share_a_parent_node() {
    let (a, b) = (container(), container());
    let start: web_sys::Node = document().create_comment("start").into();
    let end: web_sys::Node = document().create_comment("end").into();
    a.append_child(&start).unwrap();
    b.append_child(&end).unwrap();
    create_range_root(&start, &end);
}

#[wasm_bindgen_test]
fn does_not_affect_content_before_start_marker() {
    let (el, nodes) = parse("<header>Before</header><!--start--><!--end-->");
    let root = create_range_root(&nodes[1], &nodes[2]);
    root.render(rsx! { <div>"Inside"</div> });
    root.flush();
    assert_eq!(el.inner_html(), "<header>Before</header><!--start--><div>Inside</div><!--end-->");
}

#[wasm_bindgen_test]
fn does_not_affect_content_after_end_marker() {
    let (el, nodes) = parse("<!--start--><div>Old</div><!--end--><footer>After</footer>");
    let root = create_range_root(&nodes[0], &nodes[2]);
    root.render(rsx! { <div>"New"</div> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><div>New</div><!--end--><footer>After</footer>");
}

#[wasm_bindgen_test]
fn handles_empty_ranges_with_content_after_end_marker() {
    let (el, nodes) = parse("<!--start--><!--end--><footer>After</footer>");
    let footer = find(&el, "footer");
    let root = create_range_root(&nodes[0], &nodes[1]);
    root.render(rsx! { <div>"New content"</div> });
    root.flush();
    assert_eq!(el.inner_html(), "<!--start--><div>New content</div><!--end--><footer>After</footer>");
    assert!(find(&el, "footer").is_same_node(Some(&footer)));
}

#[wasm_bindgen_test]
fn preserves_surrounding_content_during_updates() {
    let (el, nodes) = parse("<header>H</header><!--start--><p>Old</p><!--end--><footer>F</footer>");
    let root = create_range_root(&nodes[1], &nodes[3]);
    root.render(rsx! { <p>"New"</p> });
    root.flush();
    assert_eq!(el.inner_html(), "<header>H</header><!--start--><p>New</p><!--end--><footer>F</footer>");
    root.render(rsx! { <p>"Updated again"</p> });
    root.flush();
    assert_eq!(el.inner_html(), "<header>H</header><!--start--><p>Updated again</p><!--end--><footer>F</footer>");
}

// stateful components

#[derive(Clone, Default)]
struct CounterProps {
    initial_count: i32,
}

struct Counter {
    count: i32,
}

impl Component for Counter {
    type Props = CounterProps;
    fn setup(props: &CounterProps, _: &Handle<Self>) -> Self {
        Counter { count: props.initial_count }
    }
    fn render(&self, _: &CounterProps, h: &Handle<Self>) -> Node {
        let click = h.on(|this, h| {
            this.count += 1;
            h.update();
        });
        rsx! { <button mix={rmx::on("click", click)}>{self.count}</button> }
    }
}

#[wasm_bindgen_test]
fn maintains_component_state_across_renders() {
    let (el, start, end) = markers();
    let root = create_range_root(&start, &end);
    root.render(rsx! { <Counter initial_count={0}/> });
    root.flush();
    let button: web_sys::HtmlElement = find(&el, "button").unchecked_into();
    assert_eq!(button.text_content().as_deref(), Some("0"));
    button.click();
    root.flush();
    assert_eq!(button.text_content().as_deref(), Some("1"));
    button.click();
    root.flush();
    assert_eq!(button.text_content().as_deref(), Some("2"));
}
