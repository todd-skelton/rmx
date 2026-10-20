//! The Remix 3 ui demo gallery in rmx (`demos/ui`).
//!
//! Each demo is a client entry the server renders and the browser hydrates. The component
//! runtime demos come from `packages/component/src/runtime/demos`, the mixin case from
//! `demos/ui/cases`. `mixin-lifecycle` is rmx's own page for checking stateful mixins in a
//! browser.

pub mod demos;

/// A gallery entry.
pub struct Demo {
    /// URL path under `/demo/`.
    pub slug: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// `Component runtime` or `Cases` section.
    pub section: &'static str,
    pub module: &'static str,
    pub center: bool,
}

pub const DEMOS: &[Demo] = &[
    Demo {
        slug: "component/runtime/basic",
        title: "Basic Counter",
        description: "The smallest interactive component counter from the standalone demos.",
        section: "Component runtime",
        module: "Runtime",
        center: false,
    },
    Demo {
        slug: "component/runtime/keyed-list",
        title: "Keyed List",
        description: "Exercises keyed DOM reconciliation with shuffling, reversing, and per-item controls.",
        section: "Component runtime",
        module: "Runtime",
        center: false,
    },
    Demo {
        slug: "component/runtime/controlled-uncontrolled-values",
        title: "Controlled and Uncontrolled Values",
        description: "Compares controlled props with uncontrolled DOM values across rerenders and remounts.",
        section: "Component runtime",
        module: "Runtime",
        center: false,
    },
    Demo {
        slug: "component/runtime/mixin-lifecycle",
        title: "Mixin Lifecycle",
        description: "Stateful mixins: setup, update without the owner, insert, remove and persisted removal.",
        section: "Component runtime",
        module: "Mixins",
        center: false,
    },
    Demo {
        slug: "cases/draggable",
        title: "Draggable Mixin",
        description: "A pointer-driven box using the draggable mixin from the standalone demos.",
        section: "Cases",
        module: "Draggable",
        center: false,
    },
];

pub fn find_demo(slug: &str) -> Option<&'static Demo> {
    DEMOS.iter().find(|d| d.slug == slug)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    use demos::*;
    rmx::run(rmx::RunOptions {
        entries: vec![
            rmx::entry::<basic::BasicCounter>(),
            rmx::entry::<keyed_list::KeyedList>(),
            rmx::entry::<controlled::ControlledValues>(),
            rmx::entry::<lifecycle::MixinLifecycle>(),
            rmx::entry::<draggable::DraggableDemo>(),
        ],
        ..Default::default()
    });
}
