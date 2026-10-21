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

mod app;
mod attrs;
mod component;
mod frame;
mod host_mixin;
mod mixin;
mod navigation;
mod node;
mod node_json;
mod signal;

pub mod browser;
pub mod ssr;

#[cfg(target_arch = "wasm32")]
pub(crate) mod dom;

pub use app::{report_error, AppError};
pub use component::{set_server_frame_src, Component, Fragment, FragmentProps, Handle};
pub use frame::{
    Frame, FrameContent, FrameEvent, FrameHandle, FrameProps, FrameRequest, FrameResponse, Frames, ReloadOptions, ResolveFrame,
    ResolveFuture,
};
pub use host_mixin::{mixin, mixin_with, BeforeRemoveEvent, HostMixin, InsertEvent, MixinHandle, MixinOptions};
pub use mixin::{attrs, css, node_ref, on, on_capture, IntoMixins, Mixin, NodeHandle, NodeRef};
pub use navigation::{link, link_with, navigate, navigate_async, History, NavigateOptions, Reset};
pub use node::{
    unsafe_html, AttrValue, Children, Element, Event, EventHandler, FrameNode, IntoAttr, IntoHtmlAttr, IntoNode, IntoProp, Key, Node,
    Str, StyleRule, UnsafeHtml,
};
pub use rmx_macros::rsx;
pub use signal::{ListenerId, Signal, TypedEventTarget};
pub use ssr::{render_to_string, Rendered};

#[cfg(target_arch = "wasm32")]
pub use dom::{create_range_root, create_root, entry, flush, run, App, Entry, Root, RunOptions};
/// The stylesheet behind `css()` (`createStyleManager` in `remix/component`).
#[cfg(target_arch = "wasm32")]
pub use dom::styles::{StyleManager, STYLE_LAYER};

/// Component testing helpers (`remix/component/test`): [`test::render`] mounts a tree for a
/// browser test.
#[cfg(target_arch = "wasm32")]
pub use dom::test;

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
