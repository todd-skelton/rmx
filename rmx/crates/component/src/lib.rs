//! `rmx`: the Remix 3 component model in idiomatic Rust.
//!
//! A component is a struct. `setup` runs once and returns the initial state, `render` turns
//! that state into a [`Node`] tree, and nothing re-renders until a handler calls
//! [`Handle::update`]. There are no signals, hooks or dependency tracking.
//!
//! The same components render to HTML on the server ([`ssr::render`]) and, for components
//! marked with [`client_entry!`], hydrate in the browser as WebAssembly islands. `<Frame>`
//! streams server content into the page, and the browser runtime ([`run`]) turns links and
//! forms into soft navigations that reconcile server HTML into the live document.

// Lets `rsx!` (which names `::rmx`) work in this crate's own tests.
#[cfg(test)]
extern crate self as rmx;

mod component;
mod frame;
mod mixin;
mod navigation;
mod node;
mod node_json;

pub mod browser;
pub mod ssr;

#[cfg(target_arch = "wasm32")]
pub(crate) mod dom;

pub use component::{set_server_frame_src, Component, Handle};
pub use frame::{Frame, FrameEvent, FrameHandle, FrameProps, Frames, ReloadOptions};
pub use mixin::{attrs, css, node_ref, on, IntoMixins, Mixin, NodeHandle, NodeRef};
pub use navigation::{link, link_with, navigate, NavigateOptions};
pub use node::{AttrValue, Children, Element, Event, EventHandler, FrameNode, IntoAttr, IntoNode, IntoProp, Key, Node, Str, StyleRule};
pub use rmx_macros::rsx;
pub use ssr::{render_to_string, Rendered};

#[cfg(target_arch = "wasm32")]
pub use dom::{entry, flush, run, Entry, RunOptions};

#[doc(hidden)]
pub mod __private {
    pub use serde_json;
}

/// Marks a component as a client entry (an island). Use it inside `impl Component for ...`.
///
/// The server wraps the component's HTML in hydration markers and serializes its props;
/// the browser runtime finds the markers and hydrates the component by name.
///
/// ```ignore
/// impl Component for Counter {
///     type Props = CounterProps;
///     rmx::client_entry!("Counter");
///     // setup / render ...
/// }
/// ```
#[macro_export]
macro_rules! client_entry {
    ($name:literal) => {
        const CLIENT_ENTRY: ::core::option::Option<&'static str> = ::core::option::Option::Some($name);

        fn props_json(props: &Self::Props) -> ::core::option::Option<::std::string::String> {
            ::core::option::Option::Some(
                $crate::__private::serde_json::to_string(props)
                    .expect("client entry props must serialize to JSON"),
            )
        }
    };
}
