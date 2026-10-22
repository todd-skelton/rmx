//! `packages/ui/src/demos/tabs.demo.tsx`: headless tabs with app-owned markup.

use rmx::{css, rsx, Component, Handle, Mixin, Node};
use rmx::ui::tabs;

pub struct TabsDemo;

impl Component for TabsDemo {
    type Props = ();
    rmx::client_entry!("TabsDemo");

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        TabsDemo
    }

    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! {
            <tabs::Context default_active_tab="overview">
                <div mix={[root_css(), tabs::root()]}>
                    <div aria-label="Project sections" mix={[list_css(), tabs::list()]}>
                        <button mix={[tab_css(), tabs::tab("overview")]} type="button">"Overview"</button>
                        <button mix={[tab_css(), tabs::tab("activity")]} type="button">"Activity"</button>
                        <button mix={[tab_css(), tabs::tab(tabs::TabOptions::new("settings").disabled(true))]} type="button">"Settings"</button>
                    </div>
                    <div mix={[panel_css(), tabs::panel("overview")]}>"Project health, owner notes, and current milestones."</div>
                    <div mix={[panel_css(), tabs::panel("activity")]}>"Recent commits, deploys, and review handoffs."</div>
                    <div mix={[panel_css(), tabs::panel("settings")]}>"Visibility, notifications, and billing preferences."</div>
                </div>
            </tabs::Context>
        }
    }
}

fn root_css() -> Mixin {
    css("display: grid; gap: 12px; width: min(100%, 28rem); font-family: \"Inter Variable\", Inter, ui-sans-serif, system-ui, sans-serif;")
}

fn list_css() -> Mixin {
    css("display: flex; gap: 4px; border-block-end: 1px solid light-dark(#d1d1d1, #444444);")
}

fn tab_css() -> Mixin {
    css("appearance: none; border: 0; border-block-end: 2px solid transparent; background: transparent; color: light-dark(#6d6d6d, #b3b3b3); font: 600 13px/18px inherit; letter-spacing: 0; padding: 8px 10px; \
         &[data-state=\"active\"] { border-block-end-color: light-dark(#151515, #ececec); color: light-dark(#151515, #ececec); } \
         &:disabled { opacity: 0.4; } \
         &:focus-visible { outline: 2px solid light-dark(#3573f6, #6eaaff); outline-offset: -2px; }")
}

fn panel_css() -> Mixin {
    css("color: light-dark(#4f4f4f, #b3b3b3); font-size: 13px; line-height: 20px; padding: 4px 10px;")
}
