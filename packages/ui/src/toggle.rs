//! Headless switch behavior for native checkboxes and custom controls (Remix's
//! `@remix-run/ui/toggle`).
//!
//! ```ignore
//! use rsmix_ui::toggle::{toggle, ToggleOptions};
//!
//! rsx! {
//!     <label><input name="notifications" mix={toggle(ToggleOptions::default().default_checked(true))}/>"Email notifications"</label>
//!     <button type="button" mix={toggle(ToggleOptions::default())}>"Desktop alerts"</button>
//! }
//! ```
//!
//! On an `<input>` the mixin makes a `role="switch"` checkbox and drives its `checked`
//! property. On any other element it renders `aria-checked`, makes the element focusable
//! and toggles on click and Space. Every host gets `data-state="checked" | "unchecked"`.
//! A change calls `on_checked_change` and dispatches a bubbling `rmx:toggle-change`
//! event (read it with [`on_toggle_change`] and [`toggle_checked`]).

use std::rc::Rc;

use rsmix::{mixin, node_ref, on, AttrValue, Element, Event, EventHandler, HostMixin, Mixin, MixinHandle, NodeHandle, Signal};

use crate::event::{attr_bool, attr_is_true};

/// The event dispatched when a toggle changes.
pub const TOGGLE_CHANGE_EVENT: &str = "rmx:toggle-change";

type InputRef = Rc<dyn Fn(&NodeHandle, &Signal)>;

/// Options for [`toggle`] (Remix's `ToggleOptions`).
#[derive(Clone, Default)]
pub struct ToggleOptions {
    /// Controlled state. Leave `None` (and no `checked` attribute) for uncontrolled.
    pub checked: Option<bool>,
    pub default_checked: Option<bool>,
    pub disabled: bool,
    pub form: Option<String>,
    /// Called with the input element when the host is an `<input>`.
    pub input_ref: Option<InputRef>,
    pub name: Option<String>,
    pub on_checked_change: Option<Rc<dyn Fn(bool)>>,
    pub read_only: bool,
    pub required: Option<bool>,
    pub tab_index: Option<i32>,
    pub value: Option<String>,
}

impl ToggleOptions {
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    pub fn default_checked(mut self, checked: bool) -> Self {
        self.default_checked = Some(checked);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn on_checked_change(mut self, f: impl Fn(bool) + 'static) -> Self {
        self.on_checked_change = Some(Rc::new(f));
        self
    }

    pub fn input_ref(mut self, f: impl Fn(&NodeHandle, &Signal) + 'static) -> Self {
        self.input_ref = Some(Rc::new(f));
        self
    }
}

/// Switch behavior for the host element.
pub fn toggle(options: ToggleOptions) -> Mixin {
    mixin::<Toggle>(options)
}

/// Listen for toggle changes on this element or its descendants.
pub fn on_toggle_change(handler: EventHandler) -> Mixin {
    on(TOGGLE_CHANGE_EVENT, handler)
}

/// The new state carried by an `rmx:toggle-change` event.
pub fn toggle_checked(event: &Event) -> Option<bool> {
    crate::event::bool_field(event, "checked")
}

/// The state behind one `toggle(..)` slot.
pub struct Toggle {
    tag: &'static str,
    uncontrolled: Option<bool>,
    /// The last render's options and derived state, for the event handlers.
    options: ToggleOptions,
    controlled: bool,
    checked: bool,
    blocked: bool,
}

impl Toggle {
    fn current_checked(&mut self, options: &ToggleOptions, host: &Element) -> bool {
        if let Some(checked) = options.checked.or_else(|| attr_bool(host.attr("checked"))) {
            return checked;
        }
        *self.uncontrolled.get_or_insert(options.default_checked.unwrap_or(false))
    }

    /// Apply a requested change (Remix's `setChecked`).
    fn set_checked(&mut self, h: &MixinHandle<Self>, event: &Event, next: bool) {
        if self.blocked {
            if self.tag == "input" {
                crate::event::set_current_prop(event, "checked", self.checked);
            }
            return;
        }
        if !self.controlled {
            self.uncontrolled = Some(next);
            h.update();
        }
        if let Some(f) = &self.options.on_checked_change {
            f(next);
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(el) = event.current_element() {
            crate::event::dispatch(&el, TOGGLE_CHANGE_EVENT, &[crate::event::Field::Bool("checked", next)]);
        }
    }
}

impl HostMixin for Toggle {
    type Args = ToggleOptions;

    fn setup(_: &MixinHandle<Self>, tag: &'static str) -> Self {
        Toggle { tag, uncontrolled: None, options: ToggleOptions::default(), controlled: false, checked: false, blocked: false }
    }

    fn render(&mut self, options: &ToggleOptions, host: &mut Element, h: &MixinHandle<Self>) {
        let input = self.tag == "input";
        let checked = self.current_checked(options, host);
        let disabled = options.disabled || attr_is_true(host.attr("disabled")) || attr_is_true(host.attr("aria-disabled"));
        let read_only = options.read_only || attr_is_true(host.attr("readonly")) || attr_is_true(host.attr("aria-readonly"));
        let required = options.required.unwrap_or_else(|| attr_is_true(host.attr("required")));
        self.options = options.clone();
        self.controlled = options.checked.is_some() || attr_bool(host.attr("checked")).is_some();
        self.checked = checked;
        self.blocked = disabled || read_only;

        host.set_attr("role", "switch");
        host.set_attr("data-state", if checked { "checked" } else { "unchecked" });
        if input {
            host.set_attr("checked", checked);
            host.set_attr("disabled", disabled);
            host.set_attr("readonly", read_only);
            host.set_attr("required", required);
            if host.attr("type").is_none() {
                host.set_attr("type", "checkbox");
            }
            if let Some(form) = &options.form {
                host.set_attr("form", form.clone());
            }
            if let Some(name) = &options.name {
                host.set_attr("name", name.clone());
            }
            if let Some(value) = &options.value {
                host.set_attr("value", value.clone());
            }
            if let Some(index) = options.tab_index {
                host.set_attr("tabindex", index);
            }
            if let Some(f) = options.input_ref.clone() {
                host.mix(node_ref(move |node, signal| f(node, signal)));
            }
        } else {
            host.set_attr("aria-checked", if checked { "true" } else { "false" });
            host.set_attr("aria-disabled", disabled.then_some("true"));
            host.set_attr("aria-readonly", read_only.then_some("true"));
            host.set_attr("aria-required", required.then_some("true"));
            if disabled {
                host.set_attr("tabindex", AttrValue::Absent);
            } else if let Some(index) = options.tab_index {
                host.set_attr("tabindex", index);
            } else if host.attr("tabindex").is_none() {
                host.set_attr("tabindex", "0");
            }
            if self.tag == "button" && host.attr("type").is_none() {
                host.set_attr("type", "button");
            }
        }

        host.mix(on(
            "click",
            h.on_event(|this, h, event| {
                let input = this.tag == "input";
                if !input {
                    event.prevent_default();
                }
                let next = if input { event.target_checked() } else { !this.checked };
                this.set_checked(h, event, next);
            }),
        ));
        host.mix(on(
            "keydown",
            h.on_event(|this, h, event| {
                if this.tag == "input" || event.key() != " " {
                    return;
                }
                event.prevent_default();
                let next = !this.checked;
                this.set_checked(h, event, next);
            }),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsmix::{render_to_string, rsx};

    #[test]
    fn native_input_renders_a_switch_checkbox() {
        let html = render_to_string(&rsx! {
            <input aria-label="Notifications" mix={toggle(ToggleOptions::default().default_checked(true).name("notifications"))}/>
        });
        assert!(html.starts_with("<input "), "{html}");
        for attr in [r#"role="switch""#, r#"data-state="checked""#, " checked", r#"type="checkbox""#, r#"name="notifications""#] {
            assert!(html.contains(attr), "{attr} in {html}");
        }
        assert!(!html.contains("aria-checked"), "{html}");
    }

    #[test]
    fn custom_host_gets_aria_state_and_focus() {
        let html = render_to_string(&rsx! { <button mix={toggle(ToggleOptions::default())}>"Desktop alerts"</button> });
        for attr in [r#"aria-checked="false""#, r#"data-state="unchecked""#, r#"tabindex="0""#, r#"type="button""#, r#"role="switch""#] {
            assert!(html.contains(attr), "{attr} in {html}");
        }
        let disabled = render_to_string(&rsx! { <button disabled mix={toggle(ToggleOptions::default())}>"Weekly digest"</button> });
        assert!(disabled.contains(r#"aria-disabled="true""#) && !disabled.contains("tabindex"), "{disabled}");
    }

    #[test]
    fn controlled_state_wins() {
        let html = render_to_string(&rsx! { <span mix={toggle(ToggleOptions::default().checked(true).default_checked(false))}/> });
        assert!(html.contains(r#"aria-checked="true""#), "{html}");
    }
}
