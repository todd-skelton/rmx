//! `packages/ui/src/demos/combobox.demo.tsx`: headless combobox behavior with minimal local
//! styles.

use rsmix::{css, rsx, Component, Handle, Mixin, Node};
use rsmix::ui::combobox;
use rsmix::ui::listbox::ListboxOption;

pub struct ComboboxDemo {
    value: String,
}

fn frameworks() -> Vec<ListboxOption> {
    vec![
        ListboxOption::new("remix", "Remix").text_value(&["remix", "rmx"]),
        ListboxOption::new("react-router", "React Router").text_value(&["react router", "router"]),
        ListboxOption::new("react", "React"),
        ListboxOption::new("archived", "Archived").disabled(true),
    ]
}

impl Component for ComboboxDemo {
    type Props = ();
    rsmix::client_entry!("ComboboxDemo");

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        ComboboxDemo { value: "none".into() }
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let on_change = combobox::on_combobox_change(h.on_event(|this, h, event| {
            this.value = combobox::combobox_change(event).value.unwrap_or_else(|| "none".into());
            h.update();
        }));
        let field = format!("{}-field", h.dom_id());
        let options: Vec<Node> = frameworks()
            .into_iter()
            .map(|o| rsx! { <div key={o.value.clone()} mix={[option_css(), combobox::option(o.clone())]}>{o.label.clone()}</div> })
            .collect();
        rsx! {
            <div mix={[demo_css(), on_change]}>
                <label mix={label_css()} for={field.clone()}>"Framework"</label>
                <combobox::Context name="framework">
                    <input id={field} mix={[input_css(), combobox::input()]} placeholder="Search frameworks"/>
                    <div mix={[surface_css(), combobox::popover()]}>
                        <div mix={[list_css(), combobox::list()]}>{options}</div>
                    </div>
                    <input mix={[hidden_input_css(), combobox::hidden_input()]}/>
                </combobox::Context>
                <p mix={value_css()}>{format!("value={}", self.value)}</p>
            </div>
        }
    }
}

fn demo_css() -> Mixin {
    css("display: grid; gap: 8px; width: min(100%, 18rem);")
}

fn label_css() -> Mixin {
    css("color: light-dark(#151515, #ececec); font: 600 12px/16px \"Inter Variable\", Inter, ui-sans-serif, system-ui, sans-serif; letter-spacing: 0;")
}

fn input_css() -> Mixin {
    css("box-sizing: border-box; width: 100%; height: 32px; border: 1px solid light-dark(#d1d1d1, #444444); border-radius: 6px; color: light-dark(#151515, #ececec); font: 500 13px/18px \"Inter Variable\", Inter, ui-sans-serif, system-ui, sans-serif; letter-spacing: 0; padding: 0 10px; \
         &:focus-visible { border-color: light-dark(#3573f6, #6eaaff); outline: 2px solid light-dark(#1A72FF, #6eaaff); }")
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
