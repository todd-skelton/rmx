//! Ports of the hydration cases of Remix's `mixin-ssr.test.tsx`: browser-only mixins
//! start during hydration and keep their lifecycle, and `on` with `node_ref` work on
//! adopted server elements. Run with `pwsh ./scripts/test-browser.ps1 -Test browser_mixin_ssr`.
#![cfg(target_arch = "wasm32")]

extern crate rsmix_component as rsmix;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rsmix::{create_root, mixin_with, node_ref, on, render_to_string, rsx, Element, EventHandler, HostMixin, MixinHandle, MixinOptions, Node};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn container(html: &str) -> web_sys::HtmlElement {
    let el: web_sys::HtmlElement = web_sys::window().unwrap().document().unwrap().create_element("div").unwrap().unchecked_into();
    el.set_inner_html(html);
    el
}

thread_local! {
    static SETUPS: Cell<u32> = const { Cell::new(0) };
    static CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn call(s: impl Into<String>) {
    CALLS.with(|c| c.borrow_mut().push(s.into()));
}

fn calls() -> Vec<String> {
    CALLS.with(|c| c.borrow().clone())
}

/// A browser-only (`ssr: false` option) mixin that logs clicks with its label.
struct Clicks;

impl HostMixin for Clicks {
    type Args = &'static str;

    fn setup(h: &MixinHandle<Self>, tag: &'static str) -> Self {
        SETUPS.with(|s| s.set(s.get() + 1));
        assert_eq!(tag, "button");
        h.on_insert(|_, _, _| call("insert"));
        h.on_remove(|_, _| call("remove"));
        Clicks
    }

    fn render(&mut self, label: &&'static str, host: &mut Element, _: &MixinHandle<Self>) {
        let label = *label;
        host.mix(on("click", EventHandler::new(move |_| call(label))));
    }
}

fn clicks(label: &'static str) -> Node {
    let browser = mixin_with::<Clicks>(label, MixinOptions { ssr: Some(false) });
    rsx! { <button mix={browser}>"Click"</button> }
}

#[wasm_bindgen_test]
fn initializes_browser_only_mixins_during_hydration_and_keeps_their_lifecycle() {
    SETUPS.with(|s| s.set(0));
    CALLS.with(|c| c.borrow_mut().clear());
    let el = container(&render_to_string(&clicks("first")));
    assert_eq!(SETUPS.with(Cell::get), 0);
    let button: web_sys::HtmlElement = el.query_selector("button").unwrap().unwrap().unchecked_into();
    let root = create_root(&el);
    root.render(clicks("first"));
    root.flush();
    let hydrated: web_sys::HtmlElement = el.query_selector("button").unwrap().unwrap().unchecked_into();
    assert_eq!(hydrated, button, "the server button is adopted");
    button.click();

    root.render(clicks("second"));
    root.flush();
    button.click();

    assert_eq!(SETUPS.with(Cell::get), 1);
    assert_eq!(calls(), ["insert", "first", "second"]);
    root.dispose();
    assert_eq!(calls(), ["insert", "first", "second", "remove"]);
}

#[wasm_bindgen_test]
fn keeps_on_and_ref_working_together_after_hydration() {
    let clicked = Rc::new(Cell::new(0));
    let nodes: Rc<RefCell<Vec<web_sys::Element>>> = Rc::default();
    let button = {
        let (clicked, nodes) = (clicked.clone(), nodes.clone());
        move || {
            let (clicked, nodes) = (clicked.clone(), nodes.clone());
            rsx! {
                <button mix={[
                    on("click", EventHandler::new(move |_| clicked.set(clicked.get() + 1))),
                    node_ref(move |node, _| nodes.borrow_mut().push(node.element().clone())),
                ]}>"Click"</button>
            }
        }
    };
    let el = container(&render_to_string(&button()));
    assert!(nodes.borrow().is_empty());
    let root = create_root(&el);
    root.render(button());
    root.flush();
    let node = el.query_selector("button").unwrap().unwrap();
    assert_eq!(*nodes.borrow(), std::slice::from_ref(&node));
    node.unchecked_ref::<web_sys::HtmlElement>().click();
    assert_eq!(clicked.get(), 1);
    root.dispose();
}
