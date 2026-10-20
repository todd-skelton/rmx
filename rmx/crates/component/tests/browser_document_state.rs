//! Ports of Remix's `document-state.test.ts`: focus and text selection survive DOM moves
//! made during an update. Plus a keyed reorder through the runtime. Run with
//! `pwsh ./test-browser.ps1 -Test browser_document_state`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use rmx::test::DocumentState;
use std::cell::RefCell;
use std::rc::Rc;

use rmx::{create_root, flush, rsx, Component, Handle, Node};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

/// A container attached to the body, removed when dropped.
struct Container(web_sys::HtmlElement);

impl Container {
    fn new() -> Container {
        let el: web_sys::HtmlElement = document().create_element("div").unwrap().unchecked_into();
        document().body().unwrap().append_child(&el).unwrap();
        Container(el)
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        self.0.remove();
    }
}

fn input(kind: &str, value: &str) -> web_sys::HtmlInputElement {
    let input: web_sys::HtmlInputElement = document().create_element("input").unwrap().unchecked_into();
    input.set_type(kind);
    input.set_value(value);
    input
}

fn active_is(el: &web_sys::Element) -> bool {
    document().active_element().is_some_and(|a| a.is_same_node(Some(el)))
}

fn selection(el: &web_sys::HtmlInputElement) -> (Option<u32>, Option<u32>) {
    (el.selection_start().unwrap(), el.selection_end().unwrap())
}

/// Detach and re-append `el` to `parent`, which drops focus like a DOM move does.
fn move_to(parent: &web_sys::Element, el: &web_sys::Element) {
    el.remove();
    parent.append_child(el).unwrap();
}

#[wasm_bindgen_test]
fn restores_focus_and_selection_after_moving_a_text_input() {
    let c = Container::new();
    let el = input("text", "Hello World");
    c.0.append_child(&el).unwrap();
    el.focus().unwrap();
    el.set_selection_range(6, 11).unwrap();

    let state = DocumentState::capture();
    move_to(&c.0, &el);
    assert!(active_is(&document().body().unwrap()));
    state.restore();

    assert!(active_is(&el));
    assert_eq!(selection(&el), (Some(6), Some(11)));
}

#[wasm_bindgen_test]
fn restores_focus_and_selection_after_reordering_nodes() {
    let c = Container::new();
    let inputs: Vec<_> = ["First", "Second", "Third"].iter().map(|v| input("text", v)).collect();
    inputs.iter().for_each(|i| {
        c.0.append_child(i).unwrap();
    });
    inputs[1].focus().unwrap();
    inputs[1].set_selection_range(0, 6).unwrap();

    let state = DocumentState::capture();
    move_to(&c.0, &inputs[1]);
    state.restore();

    assert!(active_is(&inputs[1]));
    assert_eq!(selection(&inputs[1]), (Some(0), Some(6)));
}

#[wasm_bindgen_test]
fn restores_focus_and_selection_after_moving_textarea() {
    let c = Container::new();
    let textarea: web_sys::HtmlTextAreaElement = document().create_element("textarea").unwrap().unchecked_into();
    textarea.set_value("Hello\nWorld\nTest");
    c.0.append_child(&textarea).unwrap();
    textarea.focus().unwrap();
    textarea.set_selection_range(6, 11).unwrap();

    let state = DocumentState::capture();
    move_to(&c.0, &textarea);
    state.restore();

    assert!(active_is(&textarea));
    assert_eq!((textarea.selection_start().unwrap(), textarea.selection_end().unwrap()), (Some(6), Some(11)));
}

#[wasm_bindgen_test]
fn restores_focus_and_selection_for_different_input_types_after_move() {
    let c = Container::new();
    for kind in ["text", "search", "tel", "url", "password"] {
        let el = input(kind, "test value");
        c.0.append_child(&el).unwrap();
        el.focus().unwrap();
        el.set_selection_range(0, 4).unwrap();

        let state = DocumentState::capture();
        move_to(&c.0, &el);
        state.restore();

        assert!(active_is(&el), "{kind}");
        assert_eq!(selection(&el), (Some(0), Some(4)), "{kind}");
        el.remove();
    }
}

#[wasm_bindgen_test]
fn restores_focus_without_selection_when_element_had_no_selection() {
    let c = Container::new();
    let el = input("text", "Hello World");
    c.0.append_child(&el).unwrap();
    el.focus().unwrap();
    let state = DocumentState::capture();
    move_to(&c.0, &el);
    state.restore();
    assert!(active_is(&el));
}

#[wasm_bindgen_test]
fn restores_focus_after_moving_element_inside_a_scroller() {
    let c = Container::new();
    let scrollable: web_sys::HtmlElement = document().create_element("div").unwrap().unchecked_into();
    scrollable.set_attribute("style", "width: 100px; height: 100px; overflow: auto").unwrap();
    let inner = document().create_element("div").unwrap();
    inner.set_attribute("style", "width: 200px; height: 200px").unwrap();
    scrollable.append_child(&inner).unwrap();
    c.0.append_child(&scrollable).unwrap();
    let el = input("text", "");
    scrollable.append_child(&el).unwrap();
    el.focus().unwrap();

    let state = DocumentState::capture();
    move_to(&scrollable, &el);
    state.restore();
    assert!(active_is(&el));
}

#[wasm_bindgen_test]
fn restores_focus_for_non_selectable_element_after_move() {
    let c = Container::new();
    let div: web_sys::HtmlElement = document().create_element("div").unwrap().unchecked_into();
    div.set_tab_index(-1);
    c.0.append_child(&div).unwrap();
    div.focus().unwrap();
    let state = DocumentState::capture();
    move_to(&c.0, &div);
    state.restore();
    assert!(active_is(&div));
}

#[wasm_bindgen_test]
fn restores_focus_for_content_editable_element_after_move() {
    let c = Container::new();
    let div: web_sys::HtmlElement = document().create_element("div").unwrap().unchecked_into();
    div.set_content_editable("true");
    div.set_text_content(Some("Hello World"));
    c.0.append_child(&div).unwrap();
    div.focus().unwrap();
    let state = DocumentState::capture();
    move_to(&c.0, &div);
    state.restore();
    assert!(active_is(&div));
}

#[wasm_bindgen_test]
fn does_not_restore_if_element_is_removed_from_document() {
    let c = Container::new();
    let el = input("text", "Hello World");
    c.0.append_child(&el).unwrap();
    el.focus().unwrap();
    el.set_selection_range(0, 5).unwrap();
    let state = DocumentState::capture();
    el.remove();
    state.restore();
    assert!(!active_is(&el));
}

#[wasm_bindgen_test]
fn handles_selection_end_beyond_value_length_after_move() {
    let c = Container::new();
    let el = input("text", "Hello");
    c.0.append_child(&el).unwrap();
    el.focus().unwrap();
    el.set_selection_range(0, 10).unwrap();
    let state = DocumentState::capture();
    move_to(&c.0, &el);
    state.restore();
    assert!(active_is(&el));
    assert!(el.selection_end().unwrap().unwrap() <= el.value().len() as u32);
}

#[wasm_bindgen_test]
fn restores_focus_when_element_is_moved_to_different_parent() {
    let c = Container::new();
    let parent1 = document().create_element("div").unwrap();
    let parent2 = document().create_element("div").unwrap();
    c.0.append_child(&parent1).unwrap();
    c.0.append_child(&parent2).unwrap();
    let el = input("text", "Hello World");
    parent1.append_child(&el).unwrap();
    el.focus().unwrap();
    el.set_selection_range(0, 5).unwrap();
    let state = DocumentState::capture();
    move_to(&parent2, &el);
    state.restore();
    assert!(active_is(&el));
    assert_eq!(selection(&el), (Some(0), Some(5)));
}

fn rows(order: &[&'static str]) -> Node {
    let items: Vec<Node> = order.iter().map(|id| rsx! { <li key={*id}><input id={*id} value={*id}/></li> }).collect();
    rsx! { <ul>{items}</ul> }
}

#[wasm_bindgen_test]
fn keyed_reorder_keeps_focus_and_selection_on_a_moved_input() {
    let c = Container::new();
    let root = create_root(&c.0);
    root.render(rows(&["first", "second", "third"]));
    root.flush();
    let el: web_sys::HtmlInputElement = c.0.query_selector("#first").unwrap().unwrap().unchecked_into();
    el.focus().unwrap();
    el.set_selection_range(1, 3).unwrap();

    root.render(rows(&["second", "third", "first"]));
    root.flush();

    let now = c.0.query_selector("li:last-child input").unwrap().unwrap();
    assert!(now.is_same_node(Some(&el)));
    assert!(active_is(&el));
    assert_eq!(selection(&el), (Some(1), Some(3)));
    root.dispose();
}

thread_local! {
    static REORDER: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
}

/// Renders three keyed inputs; `REORDER` moves the first to the end with `update()`.
struct Reorder {
    order: Rc<RefCell<Vec<&'static str>>>,
}

impl Component for Reorder {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        let order = Rc::new(RefCell::new(vec!["first", "second", "third"]));
        let (h, o) = (h.clone(), order.clone());
        REORDER.with(|r| {
            *r.borrow_mut() = Some(Box::new(move || {
                o.borrow_mut().rotate_left(1);
                h.update();
            }))
        });
        Reorder { order }
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rows(&self.order.borrow())
    }
}

#[wasm_bindgen_test]
fn component_update_that_moves_the_focused_input_keeps_focus_and_selection() {
    let c = Container::new();
    let root = create_root(&c.0);
    root.render(rsx! { <Reorder/> });
    root.flush();
    let el: web_sys::HtmlInputElement = c.0.query_selector("#first").unwrap().unwrap().unchecked_into();
    el.focus().unwrap();
    el.set_selection_range(2, 4).unwrap();

    REORDER.with(|r| (r.borrow().as_ref().unwrap())());
    flush();

    assert!(c.0.query_selector("li:last-child input").unwrap().unwrap().is_same_node(Some(&el)));
    assert!(active_is(&el));
    assert_eq!(selection(&el), (Some(2), Some(4)));
    root.dispose();
}
