//! `packages/ui/src/demos/toggle.demo.tsx`: headless switch behavior for native inputs and
//! custom controls.

use rmx::{css, rsx, Component, Handle, Mixin, Node};
use rmx::ui::toggle::{toggle, ToggleOptions};

pub struct ToggleDemo;

impl Component for ToggleDemo {
    type Props = ();
    rmx::client_entry!("ToggleDemo");

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        ToggleDemo
    }

    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! {
            <div mix={root_css()}>
                <label mix={option_css()}>
                    <input mix={[native_control_css(), toggle(ToggleOptions::default().default_checked(true))]} name="notifications" type="checkbox"/>
                    "Email notifications"
                </label>
                <button mix={[custom_control_css(), toggle(ToggleOptions::default())]} type="button">"Desktop alerts"</button>
                <button disabled mix={[custom_control_css(), toggle(ToggleOptions::default())]} type="button">"Weekly digest"</button>
            </div>
        }
    }
}

fn root_css() -> Mixin {
    css("display: grid; gap: 12px; width: min(100%, 22rem); color: light-dark(#151515, #ececec); font-family: \"Inter Variable\", Inter, ui-sans-serif, system-ui, sans-serif; font-size: 13px; line-height: 18px; font-weight: 500; letter-spacing: 0;")
}

fn option_css() -> Mixin {
    css("display: flex; align-items: center; gap: 10px;")
}

fn native_control_css() -> Mixin {
    css("appearance: none; box-sizing: border-box; width: 32px; height: 18px; margin: 0; border: 1px solid light-dark(#b8b8b8, #666666); border-radius: 999px; background: light-dark(#e7e7e7, #333333); padding: 2px; \
         &::after { content: \"\"; display: block; width: 12px; height: 12px; border-radius: 50%; background: light-dark(#ffffff, #ececec); transition: translate 120ms ease; } \
         &:checked { border-color: light-dark(#3573f6, #6eaaff); background: light-dark(#3573f6, #6eaaff); } \
         &:checked::after { translate: 14px 0; } \
         &:focus-visible { outline: 2px solid light-dark(#3573f6, #6eaaff); outline-offset: 2px; }")
}

fn custom_control_css() -> Mixin {
    css("appearance: none; justify-self: start; min-height: 30px; border: 1px solid light-dark(#d1d1d1, #444444); border-radius: 6px; background: light-dark(#ffffff, #1a1a1a); color: inherit; font: inherit; padding: 5px 10px; \
         &[data-state=\"checked\"] { border-color: light-dark(#3573f6, #6eaaff); background: light-dark(#e8f0ff, #243a64); } \
         &:disabled { opacity: 0.4; } \
         &:focus-visible { outline: 2px solid light-dark(#3573f6, #6eaaff); outline-offset: 2px; }")
}
