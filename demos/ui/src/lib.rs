//! The Remix 3 ui demo gallery in rsmix (`demos/ui`).
//!
//! Each demo is a client entry the server renders and the browser hydrates. The component
//! runtime demos come from `packages/component/src/runtime/demos`, the mixin case from
//! `demos/ui/cases`. `mixin-lifecycle` is rsmix's own page for checking stateful mixins in a
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
        slug: "component/runtime/readme",
        title: "README Examples",
        description: "A gallery of small examples that mirror the package README snippets.",
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
        slug: "ui/animation",
        title: "Animation Gallery",
        description: "Entrance, exit, layout and spring animations from the @remix-run/ui animation demos.",
        section: "UI",
        module: "Animation",
        center: false,
    },
    Demo {
        slug: "ui/accordion",
        title: "Accordion Primitives",
        description: "Headless accordion behavior with minimal local styles.",
        section: "UI",
        module: "Accordion",
        center: true,
    },
    Demo {
        slug: "ui/anchor",
        title: "Anchor",
        description: "Position a floating element against an anchor with the low-level anchor primitive.",
        section: "UI",
        module: "Anchor",
        center: true,
    },
    Demo {
        slug: "ui/combobox",
        title: "Combobox Primitives",
        description: "Headless combobox behavior with minimal local styles.",
        section: "UI",
        module: "Combobox",
        center: true,
    },
    Demo {
        slug: "ui/listbox",
        title: "Listbox",
        description: "Controlled selection and highlight state with the listbox primitives.",
        section: "UI",
        module: "Listbox",
        center: true,
    },
    Demo {
        slug: "ui/menu",
        title: "Menu Primitives",
        description: "Headless menu behavior with minimal local styles.",
        section: "UI",
        module: "Menu",
        center: true,
    },
    Demo {
        slug: "ui/popover",
        title: "Popover",
        description: "Anchored popover behavior with focus restoration and outside-click dismissal.",
        section: "UI",
        module: "Popover",
        center: true,
    },
    Demo {
        slug: "ui/select",
        title: "Select Primitives",
        description: "Headless select behavior with minimal local styles.",
        section: "UI",
        module: "Select",
        center: true,
    },
    Demo {
        slug: "ui/tabs",
        title: "Tabs Primitives",
        description: "Headless tabs behavior with app-owned markup and minimal local styles.",
        section: "UI",
        module: "Tabs",
        center: true,
    },
    Demo {
        slug: "ui/toggle",
        title: "Toggle Primitives",
        description: "Headless switch behavior for native inputs and custom controls.",
        section: "UI",
        module: "Toggle",
        center: true,
    },
    Demo {
        slug: "cases/drummer",
        title: "Drummer",
        description: "A browser audio drum machine demo with tempo controls and keyboard input.",
        section: "Cases",
        module: "Drummer",
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
    rsmix::run(rsmix::RunOptions {
        entries: vec![
            rsmix::entry::<basic::BasicCounter>(),
            rsmix::entry::<keyed_list::KeyedList>(),
            rsmix::entry::<controlled::ControlledValues>(),
            rsmix::entry::<lifecycle::MixinLifecycle>(),
            rsmix::entry::<readme::ReadmeExamples>(),
            rsmix::entry::<draggable::DraggableDemo>(),
            rsmix::entry::<drummer::DrummerApp>(),
            rsmix::entry::<animation::AnimationGallery>(),
            rsmix::entry::<accordion::AccordionDemo>(),
            rsmix::entry::<anchor::AnchorDemo>(),
            rsmix::entry::<combobox::ComboboxDemo>(),
            rsmix::entry::<popover::PopoverDemo>(),
            rsmix::entry::<listbox::ListboxDemo>(),
            rsmix::entry::<menu::MenuDemo>(),
            rsmix::entry::<select::SelectDemo>(),
            rsmix::entry::<tabs::TabsDemo>(),
            rsmix::entry::<toggle::ToggleDemo>(),
        ],
        ..Default::default()
    });
}
