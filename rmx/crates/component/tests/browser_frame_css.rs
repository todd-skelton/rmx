//! Ports of Remix's `frame.css.test.tsx`: `css()` rules that arrive with frame content
//! stay while any frame still uses them, and reloading a frame with unchanged styles does
//! not grow the stylesheet. Frames are client-created and load through a custom resolver.
//! Run with `pwsh ./test-browser.ps1 -Test browser_frame_css`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::cell::RefCell;
use std::rc::Rc;

use rmx::{create_root, css, on, render_to_string, rsx, Component, Frame, Handle, Node, FrameContent, FrameRequest, FrameResponse, Frames, ReloadOptions, Root, RunOptions};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

thread_local! {
    static APP: RefCell<Option<rmx::App>> = const { RefCell::new(None) };
}

const SHARED: &str = "color: rgb(11, 22, 33)";
const STABLE: &str = "color: rgb(44, 55, 66)";

fn content(path: &str) -> String {
    let node = match path {
        "/a" => rsx! { <div><div id="a-shared" mix={css(SHARED)}>"A"</div></div> },
        "/a2" => rsx! { <div id="a-plain">"A2"</div> },
        "/b" => rsx! { <div id="b-shared" mix={css(SHARED)}>"B"</div> },
        "/stable" => rsx! { <div id="stable" mix={css(STABLE)}>"Stable"</div> },
        "/style-frame" => rsx! { <section id="style-frame" class="frame-loaded">"Frame loaded"</section> },
        other => panic!("unexpected frame source {other}"),
    };
    render_to_string(&node)
}

fn resolve(request: FrameRequest) -> rmx::ResolveFuture {
    let html = content(&web_sys::Url::new(&request.url).unwrap().pathname());
    Box::pin(async move { Ok(FrameResponse { content: FrameContent::Html(html), url: None }) })
}

async fn start() {
    if APP.with(|a| a.borrow().is_none()) {
        let app = rmx::run(RunOptions { resolve_frame: Some(Rc::new(resolve)), ..Default::default() });
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

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn color(id: &str) -> String {
    let el = document().get_element_by_id(id).unwrap_or_else(|| panic!("#{id}"));
    web_sys::window().unwrap().get_computed_style(&el).unwrap().unwrap().get_property_value("color").unwrap()
}

/// Rules across every adopted stylesheet.
fn count_rules() -> u32 {
    let count = js_sys::Function::new_no_args("return Array.from(document.adoptedStyleSheets).reduce((n, s) => n + s.cssRules.length, 0)");
    count.call0(&JsValue::NULL).unwrap().as_f64().unwrap() as u32
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

async fn mount(node: rmx::Node, ready: &[&str]) -> Mounted {
    start().await;
    let el: web_sys::HtmlElement = document().create_element("div").unwrap().unchecked_into();
    document().body().unwrap().append_child(&el).unwrap();
    let root = create_root(&el);
    root.render(node);
    root.flush();
    for _ in 0..200 {
        if ready.iter().all(|id| document().get_element_by_id(id).is_some()) {
            break;
        }
        tick().await;
    }
    Mounted { el, root }
}

#[wasm_bindgen_test]
async fn keeps_a_sibling_frame_styled_after_the_first_frame_reloads_away_from_a_shared_style() {
    let _m = mount(rsx! { <main><Frame name="a" src="/a"/><Frame name="b" src="/b"/></main> }, &["a-shared", "b-shared"]).await;
    assert_eq!(color("a-shared"), "rgb(11, 22, 33)", "initial a");
    assert_eq!(color("b-shared"), "rgb(11, 22, 33)", "initial b");

    let _ = Frames.get("a").unwrap().reload_async(ReloadOptions { src: Some("/a2".into()), ..Default::default() }).await;

    assert!(document().get_element_by_id("a-plain").is_some());
    assert!(document().get_element_by_id("a-shared").is_none());
    assert_eq!(color("b-shared"), "rgb(11, 22, 33)", "b after a reloads");
}

#[wasm_bindgen_test]
async fn does_not_accumulate_rules_when_a_frame_reloads_with_unchanged_styles() {
    let _m = mount(rsx! { <main><Frame name="stable" src="/stable"/></main> }, &["stable"]).await;
    let frame = Frames.get("stable").unwrap();
    assert_eq!(frame.src(), "/stable");

    let _ = frame.reload_async(ReloadOptions::default()).await;
    let after_first = count_rules();
    let _ = frame.reload_async(ReloadOptions::default()).await;
    let _ = frame.reload_async(ReloadOptions::default()).await;

    assert_eq!(count_rules(), after_first);
    assert_eq!(color("stable"), "rgb(44, 55, 66)");
}

// frame.test.tsx: removing a client-created frame keeps the managed stylesheet

const SHELL: &str = "color: rgb(0, 187, 255)";

/// A shell with a `css()` rule and a frame (with a styled fallback) it can remove.
struct Shell {
    mounted: bool,
}

impl Component for Shell {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Shell { mounted: true }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let toggle = h.on(|this, h| {
            this.mounted = !this.mounted;
            h.update();
        });
        let frame = self.mounted.then(|| {
            let fallback = rsx! { <div mix={css("color: rgb(255, 0, 187)")}>"Loading style frame..."</div> };
            rsx! { <Frame src="/style-frame" fallback={fallback}/> }
        });
        rsx! {
            <main id="shell" mix={css(SHELL)}>
                <button id="toggle-frame" type="button" mix={on("click", toggle)}>"Toggle"</button>
                {frame.unwrap_or(Node::Empty)}
            </main>
        }
    }
}

#[wasm_bindgen_test]
async fn does_not_dispose_managed_stylesheets_when_removing_a_client_created_frame() {
    let m = mount(rsx! { <Shell/> }, &["style-frame"]).await;
    let sheets = || js_sys::Function::new_no_args("return document.adoptedStyleSheets.length").call0(&JsValue::NULL).unwrap().as_f64().unwrap();
    let before = sheets();
    assert_eq!(color("shell"), "rgb(0, 187, 255)");

    let button: web_sys::HtmlElement = document().get_element_by_id("toggle-frame").unwrap().unchecked_into();
    button.click();
    m.root.flush();
    tick().await;
    assert!(document().get_element_by_id("style-frame").is_none(), "the frame is removed");
    let after = sheets();
    assert!(after > 0.0 && after >= before, "{before} -> {after}");
    assert_eq!(color("shell"), "rgb(0, 187, 255)", "the shell's rule is still applied");
}
