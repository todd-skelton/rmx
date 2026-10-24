//! `packages/component/src/runtime/demos/basic.demo.tsx`

use rsmix::{on, rsx, Component, Handle, Node};

pub struct BasicCounter {
    count: u32,
}

impl Component for BasicCounter {
    type Props = ();
    rsmix::client_entry!("BasicCounter");

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        BasicCounter { count: 0 }
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        rsx! {
            <button mix={[on("click", h.on(|this, h| {
                this.count += 1;
                h.update();
            }))]}>
                "Ye ol' counter: " {self.count}
            </button>
        }
    }
}
