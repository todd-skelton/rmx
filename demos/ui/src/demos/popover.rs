//! `packages/ui/src/demos/popover.demo.tsx`: anchored popover behavior with focus
//! restoration and outside-click dismissal.

use rsmix::{css, on, rsx, Component, Handle, Mixin, Node};
use rsmix::ui::anchor::{AnchorOptions, Placement};
use rsmix::ui::popover;

pub struct PopoverDemo {
    open: bool,
}

impl Component for PopoverDemo {
    type Props = ();
    rsmix::client_entry!("PopoverDemo");

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        PopoverDemo { open: false }
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let hide = h.clone();
        let surface = popover::SurfaceOptions::new(self.open, move |_| {
            hide.with(|this, h| {
                this.open = false;
                h.update();
            });
        });
        rsx! {
            <popover::Context>
                <div mix={demo_css()}>
                    <button type="button" mix={[
                        popover::anchor(AnchorOptions::new(Placement::BottomStart).offset(8.0)),
                        popover::focus_on_hide(),
                        on("click", h.on(|this, h| {
                            this.open = !this.open;
                            h.update();
                        })),
                    ]}>"View options"</button>
                    <div mix={[surface_css(), popover::surface(surface)]}>
                        <button type="button" mix={[
                            popover::focus_on_show(),
                            on("click", h.on(|this, h| {
                                this.open = false;
                                h.update();
                            })),
                        ]}>"Close"</button>
                        <div mix={surface_body_css()}>"Popover content stays app-owned."</div>
                    </div>
                </div>
            </popover::Context>
        }
    }
}

fn demo_css() -> Mixin {
    css("display: grid; place-items: center; min-height: 10rem; width: min(100%, 24rem);")
}

fn surface_css() -> Mixin {
    css("box-sizing: border-box; width: 14rem; margin: 0; border: 1px solid light-dark(#d1d1d1, #444444); border-radius: 8px; background: light-dark(#ffffff, #1a1a1a); box-shadow: 0 16px 40px rgba(0, 0, 0, 0.14); color: light-dark(#151515, #ececec); padding: 8px; &:popover-open { display: grid; gap: 8px; }")
}

fn surface_body_css() -> Mixin {
    css("font-family: \"Inter Variable\", Inter, ui-sans-serif, system-ui, sans-serif; font-size: 13px; line-height: 18px; font-weight: 500; letter-spacing: 0; color: light-dark(#4f4f4f, #b3b3b3); padding: 2px 4px 4px;")
}
