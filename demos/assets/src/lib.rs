//! The browser half of the assets demo (`app/actions/public/entry.ts`).
//!
//! `entry.js` is served by rsmix-assets with its imports rewritten to asset URLs. It loads
//! this crate's wasm-bindgen module and calls [`mount`] with the text from `content.js`,
//! which renders [`ClientCard`] into `#app-root`.

use rsmix::{rsx, Component, Handle, Node};

#[derive(Clone, Default)]
pub struct ClientCardProps {
    pub content: String,
}

pub struct ClientCard;

impl Component for ClientCard {
    type Props = ClientCardProps;

    fn setup(_: &ClientCardProps, _: &Handle<Self>) -> Self {
        ClientCard
    }

    fn render(&self, props: &ClientCardProps, _: &Handle<Self>) -> Node {
        rsx! {
            <section class="client-card">
                <h2>"Client code is live"</h2>
                <p>
                    "This UI comes from an rsmix component compiled to WebAssembly and served by "
                    <code>"rsmix-assets"</code>
                    "."
                </p>
                <p>{props.content.clone()}</p>
                <p class="client-note">
                    "Rebuild the client after editing demos/assets/src/lib.rs, then refresh to confirm the server keeps running."
                </p>
            </section>
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::cell::RefCell;

    use rsmix::{rsx, Root};
    use wasm_bindgen::prelude::wasm_bindgen;

    use super::ClientCard;

    thread_local! {
        static ROOT: RefCell<Option<Root>> = const { RefCell::new(None) };
    }

    /// Render the client card into `container`, replacing its loading text.
    #[wasm_bindgen]
    pub fn mount(container: web_sys::Element, content: String) {
        let root = rsmix::create_root(&container);
        root.render(rsx! { <ClientCard content={content}/> });
        root.flush();
        ROOT.with(|r| *r.borrow_mut() = Some(root));
    }
}
