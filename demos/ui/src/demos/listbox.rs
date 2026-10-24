//! `packages/ui/src/demos/listbox.demo.tsx`: controlled selection and highlight state with
//! the listbox primitives.

use std::rc::Rc;

use rsmix::{css, rsx, Component, Handle, Mixin, Node};
use rsmix::ui::listbox::{self, ListboxOption};

pub struct ListboxDemo {
    value: Option<String>,
    active_value: Option<String>,
}

pub(crate) fn frameworks() -> Vec<ListboxOption> {
    vec![
        ListboxOption::new("remix", "Remix"),
        ListboxOption::new("react-router", "React Router"),
        ListboxOption::new("react", "React"),
        ListboxOption::new("archived", "Archived").disabled(true),
    ]
}

impl Component for ListboxDemo {
    type Props = ();
    rsmix::client_entry!("ListboxDemo");

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        ListboxDemo { value: Some("remix".into()), active_value: Some("remix".into()) }
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let (select, highlight) = (h.clone(), h.clone());
        let on_select: listbox::OnChange = Rc::new(move |value, _| {
            select.with(|this, h| {
                this.value = value.map(str::to_string);
                this.active_value = this.value.clone();
                h.update();
            });
        });
        let on_highlight: listbox::OnChange = Rc::new(move |value, _| {
            highlight.with(|this, h| {
                this.active_value = value.map(str::to_string);
                h.update();
            });
        });
        let options: Vec<Node> = frameworks()
            .into_iter()
            .map(|o| rsx! { <div key={o.value.clone()} mix={[option_css(), listbox::option(o.clone())]}>{o.label.clone()}</div> })
            .collect();
        rsx! {
            <div mix={demo_css()}>
                <p mix={label_css()}>"Framework"</p>
                <listbox::Context value={self.value.clone()} active_value={self.active_value.clone()} on_select={on_select} on_highlight={on_highlight}>
                    <div aria-label="Framework" tabindex="0" mix={[list_css(), listbox::list()]}>{options}</div>
                </listbox::Context>
                <p mix={value_css()}>{format!("value={}", self.value.as_deref().unwrap_or("null"))}</p>
            </div>
        }
    }
}

fn demo_css() -> Mixin {
    css("display: grid; gap: 8px; width: min(100%, 18rem);")
}

pub(crate) fn label_css() -> Mixin {
    css("margin: 0; font-family: \"Inter Variable\", Inter, ui-sans-serif, system-ui, sans-serif; font-size: 12px; line-height: 16px; font-weight: 650; letter-spacing: 0; color: light-dark(rgb(16 16 16 / 0.72), rgb(236 236 236 / 0.72));")
}

fn list_css() -> Mixin {
    css("display: grid; gap: 2px; margin: 0; border: 1px solid light-dark(#d1d1d1, #444444); border-radius: 8px; background: light-dark(#ffffff, #1a1a1a); box-shadow: 0 10px 28px rgba(0, 0, 0, 0.08); padding: 4px; outline: 0; \
         &:focus-visible { box-shadow: 0 10px 28px rgba(0, 0, 0, 0.08), 0 0 0 1px light-dark(#3573F6, #6eaaff), 0 0 0 4px rgba(53, 115, 246, 0.1); }")
}

pub(crate) fn option_css() -> Mixin {
    css("border-radius: 6px; color: light-dark(#151515, #ececec); font-family: \"Inter Variable\", Inter, ui-sans-serif, system-ui, sans-serif; font-size: 13px; line-height: 18px; font-weight: 500; letter-spacing: 0; padding: 6px 8px; \
         &[data-highlighted=\"true\"] { background: light-dark(rgb(16 16 16 / 0.05), rgb(236 236 236 / 0.12)); } \
         &[aria-selected=\"true\"] { background: light-dark(#101010, #ececec); color: light-dark(#ffffff, #151515); } \
         &[aria-disabled=\"true\"] { color: light-dark(rgb(16 16 16 / 0.36), rgb(236 236 236 / 0.36)); }")
}

pub(crate) fn value_css() -> Mixin {
    css("margin: 0; font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, \"Liberation Mono\", \"Courier New\", monospace; font-size: 12px; line-height: 16px; color: light-dark(#4f4f4f, #b3b3b3);")
}
