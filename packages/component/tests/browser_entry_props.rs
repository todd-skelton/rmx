//! Element-valued client entry props keep their keys after hydration (Remix 3.1's
//! `frame.test.tsx` cases for #12067): reversing keyed elements or keyed `<Fragment>`s that
//! arrived as props moves the server-rendered nodes instead of recreating them. The server
//! HTML is the content of a client-created `<Frame>` from a custom resolver. Run with
//! `pwsh ./scripts/test-browser.ps1 -Test browser_entry_props`.
#![cfg(all(target_arch = "wasm32", feature = "frames"))]

extern crate rmx_component as rmx;

use std::cell::RefCell;
use std::rc::Rc;

use rmx::{create_root, entry, render_to_string, rsx, Component, Fragment, Frame, FrameContent, FrameRequest, FrameResponse, Handle, Node, RunOptions};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

thread_local! {
    static APP: RefCell<Option<rmx::App>> = const { RefCell::new(None) };
    static REVERSE: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct ItemsProps {
    items: Vec<Node>,
}

/// Renders its `items` prop in a list, reversed on demand.
struct Reversible {
    reversed: bool,
}

impl Component for Reversible {
    type Props = ItemsProps;
    rmx::client_entry!("Reversible");
    fn setup(_: &ItemsProps, h: &Handle<Self>) -> Self {
        let h = h.clone();
        REVERSE.with(|r| {
            *r.borrow_mut() = Some(Box::new(move || {
                let _ = h.with(|this, h| {
                    this.reversed = !this.reversed;
                    h.update();
                });
            }))
        });
        Reversible { reversed: false }
    }
    fn render(&self, props: &ItemsProps, _: &Handle<Self>) -> Node {
        let mut items = props.items.clone();
        if self.reversed {
            items.reverse();
        }
        rsx! { <dl>{items}</dl> }
    }
}

fn content(path: &str) -> String {
    let items = match path {
        "/elements" => vec![rsx! { <dt key="a">"A"</dt> }, rsx! { <dt key="b">"B"</dt> }],
        "/fragments" => vec![
            rsx! { <Fragment key="a"><dt>"A"</dt><dd>"first"</dd></Fragment> },
            rsx! { <Fragment key="b"><dt>"B"</dt><dd>"second"</dd></Fragment> },
        ],
        other => panic!("unexpected frame source {other}"),
    };
    render_to_string(&rsx! { <Reversible items={items}/> })
}

fn resolve(request: FrameRequest) -> rmx::ResolveFuture {
    let html = content(&web_sys::Url::new(&request.url).unwrap().pathname());
    Box::pin(async move { Ok(FrameResponse { content: FrameContent::Html(html), url: None }) })
}

async fn start() {
    if APP.with(|a| a.borrow().is_none()) {
        let app = rmx::run(RunOptions { entries: vec![entry::<Reversible>()], resolve_frame: Some(Rc::new(resolve)), ..Default::default() });
        APP.with(|a| *a.borrow_mut() = Some(app));
    }
    let app = APP.with(|a| a.borrow_mut().take()).expect("app");
    app.ready().await;
    APP.with(|a| *a.borrow_mut() = Some(app));
}

async fn tick() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

/// Mount `<Frame src>` and wait for its hydrated list; the container and root.
async fn mount(src: &'static str) -> (web_sys::Element, rmx::Root) {
    start().await;
    let document = web_sys::window().unwrap().document().unwrap();
    let el = document.create_element("div").unwrap();
    document.body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el.clone().unchecked_into());
    root.render(rsx! { <Frame src={src}/> });
    root.flush();
    for _ in 0..100 {
        if el.query_selector("dl").unwrap().is_some() && REVERSE.with(|r| r.borrow().is_some()) {
            break;
        }
        tick().await;
    }
    (el, root)
}

fn nodes(el: &web_sys::Element, selector: &str) -> Vec<web_sys::Element> {
    let list = el.query_selector_all(selector).unwrap();
    (0..list.length()).map(|i| list.item(i).unwrap().unchecked_into()).collect()
}

fn reverse() {
    REVERSE.with(|r| r.borrow().as_ref().expect("hydrated")());
    APP.with(|a| a.borrow().as_ref().unwrap().flush());
}

#[wasm_bindgen_test]
async fn preserves_element_keys_in_props_after_hydration() {
    let (el, root) = mount("/elements").await;
    let before = nodes(&el, "dt");
    assert_eq!(before.iter().map(|n| n.text_content().unwrap()).collect::<Vec<_>>(), ["A", "B"]);
    reverse();
    let after = nodes(&el, "dt");
    assert!(after[0].is_same_node(Some(&before[1])) && after[1].is_same_node(Some(&before[0])), "keyed elements move");
    REVERSE.with(|r| r.borrow_mut().take());
    root.dispose();
    el.remove();
}

#[wasm_bindgen_test]
async fn preserves_keyed_fragment_props_after_hydration() {
    let (el, root) = mount("/fragments").await;
    let (dts, dds) = (nodes(&el, "dt"), nodes(&el, "dd"));
    reverse();
    assert_eq!(el.query_selector("dl").unwrap().unwrap().inner_html(), "<dt>B</dt><dd>second</dd><dt>A</dt><dd>first</dd>");
    let (after_dts, after_dds) = (nodes(&el, "dt"), nodes(&el, "dd"));
    assert!(after_dts[0].is_same_node(Some(&dts[1])) && after_dds[1].is_same_node(Some(&dds[0])), "keyed fragments move as groups");
    REVERSE.with(|r| r.borrow_mut().take());
    root.dispose();
    el.remove();
}
