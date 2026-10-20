//! `rmx`: the Remix 3 component model in idiomatic Rust.
//!
//! A component is a struct. `setup` runs once and returns the initial state, `render` turns
//! that state into a [`Node`] tree, and nothing re-renders until a handler calls
//! [`Handle::update`]. There are no signals, hooks or dependency tracking.
//!
//! The same components render to HTML on the server ([`render_to_string`]) and, for
//! components marked with [`client_entry!`], hydrate in the browser as WebAssembly islands.

mod component;
mod node;
mod ssr;

pub mod browser;
#[cfg(target_arch = "wasm32")]
mod dom;

pub use component::{Component, Handle};
pub use node::{AttrValue, Children, Element, Event, EventHandler, IntoAttr, IntoNode, Key, Node, Str};
pub use rmx_macros::rsx;
pub use ssr::render_to_string;

#[cfg(target_arch = "wasm32")]
pub use dom::{entry, flush, hydrate, Entry};

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
