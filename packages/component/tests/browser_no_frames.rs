//! The browser runtime built without the `frames` and `navigation` features: `run`
//! hydrates client entries from the page's `rmx-data`, the document is the only frame, and
//! a client-rendered `<Frame>` shows its fallback. Run with
//! `pwsh ./scripts/test-browser.ps1 -Test browser_no_frames -NoDefaultFeatures`.
#![cfg(all(target_arch = "wasm32", not(feature = "frames")))]

extern crate rsmix_component as rsmix;

use rsmix::{create_root, entry, render_to_string, rsx, Component, Frame, Frames, Handle, Node, RunOptions};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn find(selector: &str) -> web_sys::HtmlElement {
    document().query_selector(selector).unwrap().unwrap_or_else(|| panic!("no {selector}")).unchecked_into()
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct CounterProps {
    initial: i32,
}

struct Counter {
    count: i32,
}

impl Component for Counter {
    type Props = CounterProps;
    rsmix::client_entry!("Counter");

    fn setup(props: &CounterProps, _: &Handle<Self>) -> Self {
        Counter { count: props.initial }
    }

    fn render(&self, _: &CounterProps, h: &Handle<Self>) -> Node {
        rsx! {
            <button class="count" on:click={h.on(|this, h| {
                this.count += 1;
                h.update();
            })}>{self.count}</button>
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct NoProps {}

/// An entry with `()`-like props, serialized as `{}`, next to one inside plain markup.
struct Label;

impl Component for Label {
    type Props = NoProps;
    rsmix::client_entry!("Label");

    fn setup(_: &NoProps, _: &Handle<Self>) -> Self {
        Label
    }

    fn render(&self, _: &NoProps, _: &Handle<Self>) -> Node {
        rsx! { <span class="label">"label"</span> }
    }
}

#[wasm_bindgen_test]
async fn hydrates_entries_and_treats_the_document_as_the_only_frame() {
    let page = rsx! {
        <div id="page">
            <section><Counter initial={3}/></section>
            <Label/>
        </div>
    };
    let container = document().create_element("div").unwrap();
    container.set_inner_html(&render_to_string(&page));
    document().body().unwrap().append_child(&container).unwrap();
    let server_button = find(".count");

    let app = rsmix::run(RunOptions { entries: vec![entry::<Counter>(), entry::<Label>()], ..Default::default() });
    app.ready().await;

    // The entry adopted the server button and handles clicks.
    let button = find(".count");
    assert!(button.is_same_node(Some(&server_button)));
    assert_eq!(button.text_content().unwrap(), "3");
    button.click();
    app.flush();
    assert_eq!(button.text_content().unwrap(), "4");
    assert_eq!(find(".label").text_content().unwrap(), "label");

    // The top frame is the document; there are no named frames.
    assert_eq!(Frames.top().src(), web_sys::window().unwrap().location().href().unwrap());
    assert!(Frames.top().name().is_none());
    assert!(Frames.get("anything").is_none());

    // A client-rendered <Frame> shows its fallback.
    let host = document().create_element("div").unwrap();
    document().body().unwrap().append_child(&host).unwrap();
    let root = create_root(&host);
    root.render(rsx! { <Frame src="/never-loaded" fallback={rsx! { <p class="fallback">"Loading"</p> }}/> });
    root.flush();
    assert_eq!(host.inner_html(), r#"<p class="fallback">Loading</p>"#);

    // dispose() unmounts the entries and leaves their DOM.
    app.dispose();
    button.click();
    app.flush();
    assert_eq!(button.text_content().unwrap(), "4");
    assert!(find(".count").is_same_node(Some(&button)));
}
