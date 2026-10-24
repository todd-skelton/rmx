//! `rsmix-ui`: UI helpers for rsmix, the Rust counterpart of Remix's `@remix-run/ui`.
//!
//! Like `@remix-run/ui`, this crate is versioned apart from the core component model and is
//! not re-exported by `rsmix`. It provides [`accordion`], [`anchor`], [`animation`], [`combobox`],
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

/// Remix's internal `waitForCssTransition`, public only for the browser tests.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub use event::wait_for_css_transition;

// browser-only-mixins.test.tsx: browser-only mixins render nothing on the server, and host
// props and attribute mixins next to them survive. Remix's internal onKeyDown and
// onOutsideClick helpers are plain `on` listeners here.
#[cfg(test)]
mod browser_only_mixins_tests {
    use rsmix::{attrs, on, render_to_string, rsx, AttrValue, EventHandler};

    use crate::animation::{animate_entrance, animate_exit, animate_layout};
    use crate::popover;
    use crate::typeahead::hidden_typeahead;

    #[test]
    fn preserves_host_props_alongside_animation_and_event_helpers() {
        let html = render_to_string(&rsx! {
            <div id="host" mix={[
                animate_entrance(true),
                animate_exit(true),
                animate_layout(true),
                on("keydown", EventHandler::new(|_| {})),
                on("pointerdown", EventHandler::new(|_| {})),
                hidden_typeahead(|_| {}),
                attrs(vec![("title", AttrValue::Str("server".into()))]),
            ]}>"Child content"</div>
        });
        assert_eq!(html, r#"<div id="host" title="server">Child content</div>"#);
    }

    #[test]
    fn keeps_popover_attributes_alongside_browser_only_focus_and_anchor_helpers() {
        let html = render_to_string(&rsx! {
            <popover::Context>
                <button mix={[popover::anchor(crate::anchor::AnchorOptions::default()), popover::focus_on_hide()]}>"Open"</button>
                <div mix={[popover::surface(popover::SurfaceOptions::new(false, |_| {})), popover::focus_on_show()]}>"Content"</div>
            </popover::Context>
        });
        assert_eq!(html, r#"<button>Open</button><div popover="manual">Content</div>"#);
    }
}
