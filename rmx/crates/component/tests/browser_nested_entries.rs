//! Ports of Remix's `nested-client-entry.test.tsx`: a client entry rendered by another
//! client entry hydrates once (one listener, the same element after the outer one
//! updates) wherever it sits, and removal waits for `persist_node` work in nested entries
//! and plain children alike. Remix puts the server HTML in `document.body`; here it is the
//! content of a client-created `<Frame>` from a custom resolver, which hydrates the same
//! way and leaves the test runner's page alone. Run with
//! `pwsh ./test-browser.ps1 -Test browser_nested_entries`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rmx::{
    create_root, entry, mixin, on, render_to_string, rsx, Children, Component, Element, Frame, FrameContent, FrameRequest, FrameResponse,
    Handle, HostMixin, MixinHandle, Node, Root, RunOptions,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

thread_local! {
    static APP: RefCell<Option<rmx::App>> = const { RefCell::new(None) };
    static CLICKS: Cell<u32> = const { Cell::new(0) };
    static UPDATE_OUTER: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
    static HIDE_INNER: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
    static BEFORE_REMOVE: Cell<u32> = const { Cell::new(0) };
    static REMOVED: Cell<u32> = const { Cell::new(0) };
    static RELEASE: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct NoProps {}

/// The imported entry: a button that counts clicks.
struct Inner;

impl Component for Inner {
    type Props = NoProps;
    rmx::client_entry!("Inner");
    fn setup(_: &NoProps, _: &Handle<Self>) -> Self {
        Inner
    }
    fn render(&self, _: &NoProps, _: &Handle<Self>) -> Node {
        let click = rmx::EventHandler::new(|_| CLICKS.with(|c| c.set(c.get() + 1)));
        rsx! { <button mix={[on("click", click)]}>"Inner"</button> }
    }
}

/// A plain component that wraps its children.
#[derive(Clone, Default)]
struct WrapperProps {
    children: Children,
}

struct Wrapper;

impl Component for Wrapper {
    type Props = WrapperProps;
    fn setup(_: &WrapperProps, _: &Handle<Self>) -> Self {
        Wrapper
    }
    fn render(&self, props: &WrapperProps, _: &Handle<Self>) -> Node {
        rsx! { <div class="wrapper">{&props.children}</div> }
    }
}

fn register_update<C: Component>(h: &Handle<C>) {
    let h = h.clone();
    UPDATE_OUTER.with(|u| *u.borrow_mut() = Some(Box::new(move || h.update())));
}

macro_rules! outer {
    ($name:ident, $entry:literal, $body:expr) => {
        struct $name;

        impl Component for $name {
            type Props = NoProps;
            rmx::client_entry!($entry);
            fn setup(_: &NoProps, h: &Handle<Self>) -> Self {
                register_update(h);
                $name
            }
            fn render(&self, _: &NoProps, _: &Handle<Self>) -> Node {
                $body
            }
        }
    };
}

outer!(InHost, "InHost", rsx! { <section><Inner/></section> });
outer!(Direct, "Direct", rsx! { <Inner/> });
outer!(InFragment, "InFragment", rsx! { <><Inner/></> });
outer!(InWrapper, "InWrapper", rsx! { <Wrapper><Inner/></Wrapper> });

/// Keeps its host in the document until the test releases it, and counts lifecycle calls.
struct DeferredRemove;

impl HostMixin for DeferredRemove {
    type Args = ();
    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        h.on_before_remove(|_, _, event| {
            BEFORE_REMOVE.with(|c| c.set(c.get() + 1));
            event.persist_node(|_signal: web_sys::AbortSignal| async move {
                let pending = js_sys::Promise::new(&mut |resolve, _| RELEASE.with(|r| *r.borrow_mut() = Some(resolve)));
                let _ = JsFuture::from(pending).await;
            });
        });
        h.on_remove(|_, _| REMOVED.with(|c| c.set(c.get() + 1)));
        DeferredRemove
    }
    fn render(&mut self, _: &(), _: &mut Element, _: &MixinHandle<Self>) {}
}

fn deferred_content() -> Node {
    rsx! { <div id="deferred-remove" key="inner" mix={[mixin::<DeferredRemove>(())]}>"Inner"</div> }
}

/// The content as a nested client entry.
struct DeferredEntry;

impl Component for DeferredEntry {
    type Props = NoProps;
    rmx::client_entry!("DeferredEntry");
    fn setup(_: &NoProps, _: &Handle<Self>) -> Self {
        DeferredEntry
    }
    fn render(&self, _: &NoProps, _: &Handle<Self>) -> Node {
        deferred_content()
    }
}

/// The content as an ordinary child component.
struct DeferredPlain;

impl Component for DeferredPlain {
    type Props = NoProps;
    fn setup(_: &NoProps, _: &Handle<Self>) -> Self {
        DeferredPlain
    }
    fn render(&self, _: &NoProps, _: &Handle<Self>) -> Node {
        deferred_content()
    }
}

macro_rules! hiding_outer {
    ($name:ident, $entry:literal, $inner:ident) => {
        struct $name {
            visible: bool,
        }

        impl Component for $name {
            type Props = NoProps;
            rmx::client_entry!($entry);
            fn setup(_: &NoProps, h: &Handle<Self>) -> Self {
                let h = h.clone();
                HIDE_INNER.with(|f| {
                    *f.borrow_mut() = Some(Box::new(move || {
                        let _ = h.with(|this, h| {
                            this.visible = false;
                            h.update();
                        });
                    }))
                });
                $name { visible: true }
            }
            fn render(&self, _: &NoProps, _: &Handle<Self>) -> Node {
                rsx! { <section>{self.visible.then(|| rsx! { <$inner/> })}</section> }
            }
        }
    };
}

hiding_outer!(HidesEntry, "HidesEntry", DeferredEntry);
hiding_outer!(HidesPlain, "HidesPlain", DeferredPlain);

/// Server HTML for each case, by frame source.
fn content(path: &str) -> String {
    let node = match path {
        "/in-host" => rsx! { <InHost/> },
        "/direct" => rsx! { <Direct/> },
        "/in-fragment" => rsx! { <InFragment/> },
        "/in-wrapper" => rsx! { <InWrapper/> },
        "/hides-entry" => rsx! { <HidesEntry/> },
        "/hides-plain" => rsx! { <HidesPlain/> },
        other => panic!("unexpected frame source {other}"),
    };
    render_to_string(&node)
}

fn resolve(request: FrameRequest) -> rmx::ResolveFuture {
    let path = web_sys::Url::new(&request.url).unwrap().pathname();
    let html = content(&path);
    Box::pin(async move { Ok(FrameResponse { content: FrameContent::Html(html), url: None }) })
}

async fn start() {
    if APP.with(|a| a.borrow().is_none()) {
        let entries = vec![
            entry::<Inner>(),
            entry::<InHost>(),
            entry::<Direct>(),
            entry::<InFragment>(),
            entry::<InWrapper>(),
            entry::<DeferredEntry>(),
            entry::<HidesEntry>(),
            entry::<HidesPlain>(),
        ];
        let app = rmx::run(RunOptions { entries, resolve_frame: Some(Rc::new(resolve)) });
        APP.with(|a| *a.borrow_mut() = Some(app));
    }
    let app = APP.with(|a| a.borrow_mut().take()).expect("app");
    app.ready().await;
    APP.with(|a| *a.borrow_mut() = Some(app));
}

fn flush() {
    APP.with(|a| a.borrow().as_ref().unwrap().flush());
}

async fn tick() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

async fn microtask() {
    let _ = JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await;
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

/// Mount a frame loading `src` and wait until `selector` is in its content.
async fn mount(src: &'static str, selector: &str) -> Mounted {
    start().await;
    let document = web_sys::window().unwrap().document().unwrap();
    let el: web_sys::HtmlElement = document.create_element("div").unwrap().unchecked_into();
    document.body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    root.render(rsx! { <Frame src={src}/> });
    root.flush();
    for _ in 0..200 {
        if el.query_selector(selector).unwrap().is_some() {
            break;
        }
        tick().await;
    }
    assert!(el.query_selector(selector).unwrap().is_some(), "{src} rendered {selector}");
    Mounted { el, root }
}

async fn expect_single_click(src: &'static str) {
    let m = mount(src, "button").await;
    CLICKS.with(|c| c.set(0));
    let button: web_sys::HtmlElement = m.el.query_selector("button").unwrap().unwrap().unchecked_into();
    button.click();
    let initial = CLICKS.with(Cell::get);

    UPDATE_OUTER.with(|u| (u.borrow().as_ref().expect("the outer entry hydrated"))());
    flush();
    let after: web_sys::HtmlElement = m.el.query_selector("button").unwrap().unwrap().unchecked_into();
    assert_eq!(after, button, "the button is kept");
    button.click();
    assert_eq!([initial, CLICKS.with(Cell::get)], [1, 2], "{src}");
}

#[wasm_bindgen_test]
async fn attaches_one_listener_when_an_imported_entry_is_directly_inside_a_host_element() {
    expect_single_click("/in-host").await;
}

#[wasm_bindgen_test]
async fn attaches_one_listener_when_an_imported_entry_is_returned_directly() {
    expect_single_click("/direct").await;
}

#[wasm_bindgen_test]
async fn attaches_one_listener_when_an_imported_entry_is_inside_a_fragment() {
    expect_single_click("/in-fragment").await;
}

#[wasm_bindgen_test]
async fn attaches_one_listener_when_an_imported_entry_is_inside_a_wrapper_component() {
    expect_single_click("/in-wrapper").await;
}

async fn expect_deferred_removal(src: &'static str) {
    let m = mount(src, "#deferred-remove").await;
    BEFORE_REMOVE.with(|c| c.set(0));
    REMOVED.with(|c| c.set(0));
    let element = m.el.query_selector("#deferred-remove").unwrap().unwrap();

    HIDE_INNER.with(|f| (f.borrow().as_ref().expect("the outer entry hydrated"))());
    flush();
    microtask().await;
    let retained = m.el.query_selector("#deferred-remove").unwrap().as_ref() == Some(&element);
    let pending = (BEFORE_REMOVE.with(Cell::get), REMOVED.with(Cell::get), retained);

    if let Some(release) = RELEASE.with(|r| r.borrow_mut().take()) {
        release.call0(&JsValue::NULL).unwrap();
    }
    tick().await;
    assert_eq!(pending, (1, 0, true), "{src}: removal waits for the persisted work");
    assert_eq!(REMOVED.with(Cell::get), 1, "{src}");
    assert!(m.el.query_selector("#deferred-remove").unwrap().is_none(), "{src}");
}

#[wasm_bindgen_test]
async fn defers_removal_of_an_ordinary_child_component_until_persistence_finishes() {
    expect_deferred_removal("/hides-plain").await;
}

#[wasm_bindgen_test]
async fn defers_removal_of_an_imported_client_entry_until_persistence_finishes() {
    expect_deferred_removal("/hides-entry").await;
}
