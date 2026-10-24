//! `packages/ui/src/demos/select.demo.tsx`: headless select behavior with minimal local
//! styles.

use std::rc::Rc;

use rsmix::{css, rsx, Component, Handle, Mixin, Node};
use rsmix::ui::{popover, select};

use super::listbox::frameworks;

pub struct SelectDemo {
    value: String,
}

impl Component for SelectDemo {
    type Props = ();
    rsmix::client_entry!("SelectDemo");

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        SelectDemo { value: "remix".into() }
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let on_change = select::on_select_change(h.on_event(|this, h, event| {
            this.value = select::select_change(event).and_then(|c| c.value).unwrap_or_else(|| "none".into());
            h.update();
        }));
        let options: Vec<Node> = frameworks()
            .into_iter()
            .map(|o| rsx! { <div key={o.value.clone()} mix={[option_css(), select::option(o.clone())]}>{o.label.clone()}</div> })
            .collect();
        rsx! {
            <div mix={[demo_css(), on_change]}>
                <select::Context default_label="Choose framework" default_value="remix" label_swap_delay_ms={100} name="framework">
                    <button mix={[trigger_css(), select::trigger()]} type="button"><SelectLabel/></button>
                    <popover::Context>
                        <div mix={[surface_css(), select::popover()]}>
                            <div mix={[list_css(), select::list()]}>{options}</div>
                        </div>
                    </popover::Context>
                    <input mix={[hidden_input_css(), select::hidden_input()]}/>
                </select::Context>
                <p mix={value_css()}>{format!("value={}", self.value)}</p>
            </div>
        }
    }
}

/// Shows the select's current label (Remix's `SelectLabel` reads it from context).
pub struct SelectLabel;

impl Component for SelectLabel {
    type Props = ();

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        SelectLabel
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let label = h.context::<Rc<select::SelectState>>().map(|s| s.displayed_label()).unwrap_or_default();
        rsx! { <span>{label}</span> }
    }
}

fn demo_css() -> Mixin {
    css("display: grid; justify-items: start; gap: 8px; width: min(100%, 18rem);")
}

fn trigger_css() -> Mixin {
    css("appearance: none; display: inline-flex; align-items: center; min-width: 12rem; height: 32px; border: 1px solid light-dark(#d1d1d1, #444444); border-radius: 6px; background: light-dark(#ffffff, #1a1a1a); color: light-dark(#151515, #ececec); font: 600 13px/18px \"Inter Variable\", Inter, ui-sans-serif, system-ui, sans-serif; letter-spacing: 0; padding: 0 10px; \
         &:focus-visible { outline: 2px solid light-dark(#3573f6, #6eaaff); outline-offset: 2px; }")
}

fn surface_css() -> Mixin {
    css("box-sizing: border-box; margin: 0; border: 1px solid light-dark(#d1d1d1, #444444); border-radius: 6px; background: light-dark(#ffffff, #1a1a1a); box-shadow: 0 8px 24px rgba(0, 0, 0, 0.12); padding: 4px; &:popover-open { display: block; }")
}

fn list_css() -> Mixin {
    css("display: grid; gap: 2px; max-height: 12rem; overflow: auto; outline: 0;")
}

fn option_css() -> Mixin {
    css("border-radius: 4px; color: light-dark(#151515, #ececec); font: 500 13px/18px \"Inter Variable\", Inter, ui-sans-serif, system-ui, sans-serif; letter-spacing: 0; padding: 6px 8px; \
         &[data-highlighted=\"true\"] { background: light-dark(#eeeeee, #2c2c2c); } \
         &[aria-selected=\"true\"] { background: light-dark(#101010, #ececec); color: light-dark(#ffffff, #151515); } \
         &[aria-disabled=\"true\"] { color: light-dark(#9a9a9a, #666666); }")
}

fn hidden_input_css() -> Mixin {
    css("display: none;")
}

fn value_css() -> Mixin {
    css("margin: 0; color: light-dark(#6d6d6d, #b3b3b3); font: 500 12px/16px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;")
}
