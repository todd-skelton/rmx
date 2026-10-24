//! Ports of Remix's `render.test.tsx` for the `rsmix::test` helpers.
//! Run with `pwsh ./scripts/test-browser.ps1 -Test browser_render`.
#![cfg(target_arch = "wasm32")]

extern crate rsmix_component as rsmix;

use rsmix::test::render;
use rsmix::{on, rsx, Component, Handle, Node};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[derive(Clone, Default)]
struct CounterProps {
    count: Option<i32>,
}

struct Counter {
    count: i32,
}

impl Component for Counter {
    type Props = CounterProps;

    fn setup(props: &CounterProps, _: &Handle<Self>) -> Self {
        Counter { count: props.count.unwrap_or(0) }
    }

    fn render(&self, _: &CounterProps, h: &Handle<Self>) -> Node {
        rsx! {
            <div>
                <h3>"Counter"</h3>
                <div>
                    <button data-action="decrement" mix={on("click", h.on(|this, h| { this.count -= 1; h.update(); }))}>"-"</button>
                    <span data-testid="count">{self.count}</span>
                    <button data-action="increment" mix={on("click", h.on(|this, h| { this.count += 1; h.update(); }))}>"+"</button>
                </div>
            </div>
        }
    }
}

fn count(t: &rsmix::test::RenderResult) -> i32 {
    t.text("[data-testid=count]").parse().unwrap()
}

#[wasm_bindgen_test]
fn renders_with_initial_count_of_0_when_not_specified() {
    let t = render(rsx! { <Counter/> });
    assert_eq!(count(&t), 0);
}

#[wasm_bindgen_test]
fn renders_with_a_provided_initial_count() {
    let t = render(rsx! { <Counter count={Some(5)}/> });
    assert_eq!(count(&t), 5);
}

#[wasm_bindgen_test]
fn flushes_updates_triggered_during_act() {
    let t = render(rsx! { <Counter/> });
    t.act(|| t.query("[data-action=increment]").unwrap().click());
    assert_eq!(count(&t), 1);
    t.act(|| t.query("[data-action=increment]").unwrap().click());
    assert_eq!(count(&t), 2);
    t.act(|| t.query("[data-action=decrement]").unwrap().click());
    assert_eq!(count(&t), 1);
    assert_eq!(t.query_all("button").len(), 2);
}

#[wasm_bindgen_test]
fn cleanup_disposes_the_root_and_removes_the_container_from_the_dom() {
    let t = render(rsx! { <div data-testid="manual">"hello"</div> });
    let container = t.container();
    let body = web_sys::window().unwrap().document().unwrap().body().unwrap();
    assert!(body.contains(Some(&container)));
    t.cleanup();
    assert!(!body.contains(Some(&container)));
}
