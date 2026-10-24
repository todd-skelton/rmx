//! `packages/ui/src/demos/accordion.demo.tsx`: headless accordion behavior with minimal
//! local styles.

use rsmix::{css, rsx, Component, Handle, Mixin, Node};
use rsmix::ui::accordion;

pub struct AccordionDemo;

impl Component for AccordionDemo {
    type Props = ();
    rsmix::client_entry!("AccordionDemo");

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        AccordionDemo
    }

    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let items: Vec<Node> = [
            ("shipping", "Shipping", "Default carrier, cutoff time, and delivery windows.", false),
            ("billing", "Billing", "Invoice cadence, billing contact, and tax settings.", false),
            ("archived", "Archived", "Unavailable settings.", true),
        ]
        .into_iter()
        .map(|(value, label, body, disabled)| {
            rsx! {
                <accordion::ItemContext key={value} value={value} disabled={disabled}>
                    <div mix={[item_css(), accordion::item()]}>
                        <h3 mix={heading_css()}>
                            <button mix={[trigger_css(), accordion::trigger()]} type="button">{label}</button>
                        </h3>
                        <div mix={[content_css(), accordion::content()]}>{body}</div>
                    </div>
                </accordion::ItemContext>
            }
        })
        .collect();
        rsx! {
            <accordion::Context default_value="shipping">
                <div mix={[root_css(), accordion::root()]}>{items}</div>
            </accordion::Context>
        }
    }
}

fn root_css() -> Mixin {
    css("display: grid; width: 24rem; max-width: 100%; border: 1px solid light-dark(#d1d1d1, #444444); border-radius: 8px; background: light-dark(#ffffff, #1a1a1a);")
}

fn item_css() -> Mixin {
    css("border-block-start: 1px solid light-dark(#e7e7e7, #333333); &:first-child { border-block-start: 0; } &[data-disabled] { opacity: 0.45; }")
}

fn heading_css() -> Mixin {
    css("margin: 0;")
}

fn trigger_css() -> Mixin {
    css("appearance: none; display: flex; justify-content: space-between; width: 100%; border: 0; background: transparent; color: light-dark(#151515, #ececec); font: 600 13px/18px \"Inter Variable\", Inter, ui-sans-serif, system-ui, sans-serif; letter-spacing: 0; padding: 10px 12px; text-align: left; \
         &::after { content: \"+\"; } \
         &[data-state=\"open\"]::after { content: \"-\"; } \
         &:focus-visible { outline: 2px solid light-dark(#3573f6, #6eaaff); outline-offset: -2px; }")
}

fn content_css() -> Mixin {
    css("color: light-dark(#4f4f4f, #b3b3b3); font: 500 13px/20px \"Inter Variable\", Inter, ui-sans-serif, system-ui, sans-serif; letter-spacing: 0; padding: 0 12px 12px; &[data-state=\"closed\"] { display: none; }")
}
