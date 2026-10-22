//! Ports of Remix's `vdom.keys.test.tsx`. Run with `pwsh ./scripts/test-browser.ps1 -Test browser_keys`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::cell::RefCell;
use std::rc::Rc;

use rmx::{create_root, rsx, Component, Fragment, Handle, Node};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn container() -> web_sys::HtmlElement {
    web_sys::window().unwrap().document().unwrap().create_element("div").unwrap().unchecked_into()
}

fn find(el: &web_sys::HtmlElement, selector: &str) -> web_sys::Element {
    el.query_selector(selector).unwrap().unwrap_or_else(|| panic!("no {selector}"))
}

fn all(el: &web_sys::HtmlElement, selector: &str) -> Vec<web_sys::Element> {
    let list = el.query_selector_all(selector).unwrap();
    (0..list.length()).map(|i| list.item(i).unwrap().unchecked_into()).collect()
}

fn text(el: &web_sys::HtmlElement) -> String {
    el.text_content().unwrap_or_default()
}

fn same(a: &web_sys::Element, b: &web_sys::Element) -> bool {
    a.is_same_node(Some(b))
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

// keyed list with non-keyed sibling

#[derive(Clone, Default, PartialEq)]
struct CardData {
    id: &'static str,
    title: &'static str,
}

#[derive(Clone, Default)]
struct CardProps {
    card: CardData,
}

struct Card;

impl Component for Card {
    type Props = CardProps;
    fn setup(_: &CardProps, _: &Handle<Self>) -> Self {
        Card
    }
    fn render(&self, p: &CardProps, _: &Handle<Self>) -> Node {
        rsx! { <div data-id={p.card.id}>{p.card.title}</div> }
    }
}

#[derive(Clone, Default)]
struct ColumnProps {
    cards: Vec<CardData>,
    is_adding_card: bool,
}

struct Column;

impl Component for Column {
    type Props = ColumnProps;
    fn setup(_: &ColumnProps, _: &Handle<Self>) -> Self {
        Column
    }
    fn render(&self, p: &ColumnProps, _: &Handle<Self>) -> Node {
        let cards: Vec<Node> = p.cards.iter().map(|card| rsx! { <Card key={card.id} card={card.clone()}/> }).collect();
        let tail = if p.is_adding_card { rsx! { <div id="form">"Form"</div> } } else { rsx! { <button>"Add"</button> } };
        rsx! { <div>{cards}{tail}</div> }
    }
}

#[wasm_bindgen_test]
fn appends_keyed_component_before_non_keyed_sibling() {
    let el = container();
    let root = create_root(&el);
    let mut cards = vec![CardData { id: "1", title: "Card 1" }, CardData { id: "2", title: "Card 2" }];
    root.render(rsx! { <Column cards={cards.clone()} is_adding_card={false}/> });
    let col = find(&el, "div");
    assert_eq!(col.inner_html(), r#"<div data-id="1">Card 1</div><div data-id="2">Card 2</div><button>Add</button>"#);
    root.render(rsx! { <Column cards={cards.clone()} is_adding_card={true}/> });
    assert_eq!(col.inner_html(), r#"<div data-id="1">Card 1</div><div data-id="2">Card 2</div><div id="form">Form</div>"#);
    cards.push(CardData { id: "3", title: "Card 3" });
    root.render(rsx! { <Column cards={cards.clone()} is_adding_card={true}/> });
    assert_eq!(
        col.inner_html(),
        r#"<div data-id="1">Card 1</div><div data-id="2">Card 2</div><div data-id="3">Card 3</div><div id="form">Form</div>"#
    );
}

// basic keyed list operations

#[derive(Clone, Default)]
struct ListProps {
    values: Vec<&'static str>,
}

struct List;

impl Component for List {
    type Props = ListProps;
    fn setup(_: &ListProps, _: &Handle<Self>) -> Self {
        List
    }
    fn render(&self, p: &ListProps, _: &Handle<Self>) -> Node {
        let items: Vec<Node> = p.values.iter().map(|v| rsx! { <li key={*v} data-id={*v}>{*v}</li> }).collect();
        rsx! { <ul>{items}</ul> }
    }
}

/// Render `before`, then `after`, and return the `li`s before and after.
fn relist(before: &[&'static str], after: &[&'static str]) -> (web_sys::HtmlElement, Vec<web_sys::Element>, Vec<web_sys::Element>) {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <List values={before.to_vec()}/> });
    assert_eq!(text(&el), before.concat());
    let first = all(&el, "li");
    root.render(rsx! { <List values={after.to_vec()}/> });
    assert_eq!(text(&el), after.concat());
    let second = all(&el, "li");
    (el, first, second)
}

#[wasm_bindgen_test]
fn handles_prepending_items_with_keys() {
    let (_, first, items) = relist(&["b", "c"], &["a", "b", "c"]);
    let ids: Vec<String> = items.iter().map(|e| e.get_attribute("data-id").unwrap()).collect();
    assert_eq!(ids, ["a", "b", "c"]);
    assert!(same(&items[1], &first[0]) && same(&items[2], &first[1]));
}

#[wasm_bindgen_test]
fn handles_appending_items_with_keys() {
    let (_, first, items) = relist(&["a", "b"], &["a", "b", "c"]);
    assert!(same(&items[0], &first[0]) && same(&items[1], &first[1]));
    assert_eq!(items[2].get_attribute("data-id").as_deref(), Some("c"));
}

#[wasm_bindgen_test]
fn handles_removing_items_with_keys() {
    let (el, first, items) = relist(&["a", "b", "c", "d"], &["a", "c", "d"]);
    assert!(same(&items[0], &first[0]));
    assert_eq!(items[1].get_attribute("data-id").as_deref(), Some("c"));
    assert!(same(&items[2], &first[3]));
    assert!(el.query_selector("[data-id=b]").unwrap().is_none());
}

#[wasm_bindgen_test]
fn handles_inserting_items_with_keys() {
    let (_, first, items) = relist(&["a", "c"], &["a", "b", "c"]);
    assert!(same(&items[0], &first[0]));
    assert_eq!(items[1].get_attribute("data-id").as_deref(), Some("b"));
    assert!(same(&items[2], &first[1]));
}

#[wasm_bindgen_test]
fn handles_swapping_adjacent_items_with_keys() {
    let (_, first, items) = relist(&["a", "b"], &["b", "a"]);
    assert!(same(&items[0], &first[1]) && same(&items[1], &first[0]));
}

#[wasm_bindgen_test]
fn handles_reversing_list_order_with_keys() {
    let (_, first, items) = relist(&["a", "b", "c", "d"], &["d", "c", "b", "a"]);
    for i in 0..4 {
        assert!(same(&items[i], &first[3 - i]), "item {i}");
    }
}

#[wasm_bindgen_test]
fn handles_complex_reordering_with_keys() {
    let (_, first, items) = relist(&["a", "b", "c", "d", "e", "f"], &["a", "e", "b", "f", "c", "d"]);
    for (i, from) in [0, 4, 1, 5, 2, 3].into_iter().enumerate() {
        assert!(same(&items[i], &first[from]), "item {i}");
    }
}

// key semantics

#[wasm_bindgen_test]
fn replaces_nodes_when_keys_match_but_type_differs() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div><span key="x" id="x">"X"</span></div> });
    let first = find(&el, "#x");
    assert_eq!(el.inner_html(), r#"<div><span id="x">X</span></div>"#);
    root.render(rsx! { <div><p key="x" id="x">"Y"</p></div> });
    let second = find(&el, "#x");
    assert_eq!(el.inner_html(), r#"<div><p id="x">Y</p></div>"#);
    assert!(!same(&first, &second));
}

#[derive(Clone, Default)]
struct ItemProps {
    label: &'static str,
}

struct Item;

impl Component for Item {
    type Props = ItemProps;
    fn setup(_: &ItemProps, _: &Handle<Self>) -> Self {
        Item
    }
    fn render(&self, p: &ItemProps, _: &Handle<Self>) -> Node {
        rsx! { <li>{p.label}</li> }
    }
}

#[wasm_bindgen_test]
fn handles_mixed_keyed_and_unkeyed_children() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! {
        <ul>
            <Item key="a" label="A"/>
            <Item label="unkeyed-1"/>
            <Item key="b" label="B"/>
            <Item label="unkeyed-2"/>
        </ul>
    });
    assert_eq!(text(&el), "Aunkeyed-1Bunkeyed-2");
    root.render(rsx! {
        <ul>
            <Item label="unkeyed-1"/>
            <Item key="b" label="B"/>
            <Item label="unkeyed-2"/>
            <Item key="a" label="A"/>
        </ul>
    });
    assert_eq!(text(&el), "unkeyed-1Bunkeyed-2A");
}

#[derive(Clone, Default)]
struct DupProps {
    labels: Vec<&'static str>,
}

struct DupList;

impl Component for DupList {
    type Props = DupProps;
    fn setup(_: &DupProps, _: &Handle<Self>) -> Self {
        DupList
    }
    fn render(&self, p: &DupProps, _: &Handle<Self>) -> Node {
        let items: Vec<Node> =
            p.labels.iter().enumerate().map(|(i, l)| rsx! { <li key="dup" data-index={i.to_string()}>{*l}</li> }).collect();
        rsx! { <ul>{items}</ul> }
    }
}

#[wasm_bindgen_test]
fn handles_duplicate_keys_last_one_wins() {
    let el = container();
    let root = create_root(&el);
    let warnings = capture_warnings(|| {
        root.render(rsx! { <DupList labels={vec!["first", "second"]}/> });
        assert_eq!(text(&el), "firstsecond");
        root.render(rsx! { <DupList labels={vec!["only"]}/> });
    });
    assert_eq!(text(&el), "only");
    let items = all(&el, "li");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].get_attribute("data-index").as_deref(), Some("0"));
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("Duplicate keys detected in siblings"), "{}", warnings[0]);
    assert!(warnings[0].contains(r#""dup""#), "{}", warnings[0]);
}

#[wasm_bindgen_test]
fn allows_integer_and_string_keys() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <ul><li key={1}>"one"</li><li key="two">"two"</li><li key={3u64}>"three"</li></ul> });
    assert_eq!(text(&el), "onetwothree");
    root.render(rsx! { <ul><li key={3u64}>"three*"</li><li key={1}>"one*"</li><li key="two">"two*"</li></ul> });
    assert_eq!(text(&el), "three*one*two*");
}

fn fragments(ids: &[&'static str]) -> Node {
    let groups: Vec<Node> = ids
        .iter()
        .map(|id| rsx! { <Fragment key={*id}><span>{*id}</span><button>{format!("{id}-btn")}</button></Fragment> })
        .collect();
    rsx! { <div>{groups}</div> }
}

#[wasm_bindgen_test]
fn reorders_keyed_fragments_correctly() {
    let el = container();
    let root = create_root(&el);
    root.render(fragments(&["a", "b", "c"]));
    assert_eq!(
        el.inner_html(),
        "<div><span>a</span><button>a-btn</button><span>b</span><button>b-btn</button><span>c</span><button>c-btn</button></div>"
    );
    let spans = all(&el, "span");
    root.render(fragments(&["c", "b", "a"]));
    assert_eq!(
        el.inner_html(),
        "<div><span>c</span><button>c-btn</button><span>b</span><button>b-btn</button><span>a</span><button>a-btn</button></div>"
    );
    let after = all(&el, "span");
    assert!(same(&after[0], &spans[2]) && same(&after[2], &spans[0]), "fragment DOM was recreated");
    assert_eq!(all(&el, "button").len(), 3);
}

#[derive(Clone, Default)]
struct PairProps {
    id: &'static str,
    label: &'static str,
}

struct Pair;

impl Component for Pair {
    type Props = PairProps;
    fn setup(_: &PairProps, _: &Handle<Self>) -> Self {
        Pair
    }
    fn render(&self, p: &PairProps, _: &Handle<Self>) -> Node {
        rsx! {
            <>
                <span key={format!("{}-label", p.id)} data-id={p.id}>{p.label}</span>
                <button key={format!("{}-button", p.id)} data-id={format!("{}-btn", p.id)}>"click"</button>
            </>
        }
    }
}

#[wasm_bindgen_test]
fn handles_keys_in_fragments_without_breaking_updates() {
    let el = container();
    let root = create_root(&el);
    root.render(rsx! { <div><Pair id="a" label="A"/><Pair id="b" label="B"/></div> });
    assert_eq!(text(&el), "AclickBclick");
    root.render(rsx! { <div><Pair id="b" label="B"/><Pair id="a" label="A"/></div> });
    assert!(text(&el) == "BclickAclick" || text(&el) == "AclickBclick", "{}", text(&el));
    assert_eq!(all(&el, "span").len(), 2);
    assert_eq!(all(&el, "button").len(), 2);
}
