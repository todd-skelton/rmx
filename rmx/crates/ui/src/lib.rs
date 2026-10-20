//! `rmx-ui`: UI helpers for rmx, the Rust counterpart of Remix's `@remix-run/ui`.
//!
//! Like `@remix-run/ui`, this crate is versioned apart from the core component model and is
//! not re-exported by `rmx`. It provides [`accordion`], [`anchor`], [`animation`], [`combobox`],
//! [`listbox`], [`menu`], [`popover`], [`select`], [`tabs`], [`toggle`] and [`typeahead`].

pub mod accordion;
pub mod anchor;
pub mod animation;
pub mod combobox;
mod event;
mod hover_aim;
pub mod listbox;
pub mod menu;
pub mod popover;
pub mod select;
pub mod tabs;
pub mod toggle;
pub mod typeahead;
