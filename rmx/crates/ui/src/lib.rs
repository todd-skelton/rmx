//! `rmx-ui`: UI helpers for rmx, the Rust counterpart of Remix's `@remix-run/ui`.
//!
//! Like `@remix-run/ui`, this crate is versioned apart from the core component model and is
//! not re-exported by `rmx`. It provides [`accordion`], [`animation`], [`tabs`] and [`toggle`].

pub mod accordion;
pub mod anchor;
pub mod animation;
mod event;
pub mod listbox;
pub mod popover;
pub mod select;
pub mod tabs;
pub mod toggle;
pub mod typeahead;
