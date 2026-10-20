//! `packages/ui/src/demos/anchor.demo.tsx`: position a floating element against an anchor
//! with the low-level anchor primitive.

use rmx::{css, node_ref, rsx, Component, Handle, Mixin, Node};

pub struct AnchorDemo {
    #[cfg(target_arch = "wasm32")]
    anchor: Option<web_sys::HtmlElement>,
    #[cfg(target_arch = "wasm32")]
    floating: Option<web_sys::HtmlElement>,
    #[cfg(target_arch = "wasm32")]
    tracking: Option<rmx::ui::anchor::Anchored>,
}

impl AnchorDemo {
    #[cfg(target_arch = "wasm32")]
    fn position(&mut self) {
        use rmx::ui::anchor::{anchor, AnchorOptions, AnchorTarget, Placement};
        self.tracking = None;
        if let (Some(a), Some(f)) = (&self.anchor, &self.floating) {
            self.tracking = Some(anchor(f, AnchorTarget::Element(a.clone()), AnchorOptions::new(Placement::BottomStart).offset(8.0)));
        }
    }

    /// A ref that records the element in `slot` and positions the floating element.
    fn track(h: &Handle<Self>, floating: bool) -> Mixin {
        let h = h.clone();
        node_ref(move |node, signal| {
            #[cfg(target_arch = "wasm32")]
            {
                let Ok(el) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(node.element().clone()) else { return };
                h.with(|this, _| {
                    let slot = if floating { &mut this.floating } else { &mut this.anchor };
                    *slot = Some(el.clone());
                    this.position();
                });
                let h = h.clone();
                signal.on_abort(move || {
                    h.with(|this, _| {
                        let slot = if floating { &mut this.floating } else { &mut this.anchor };
                        if slot.as_ref() == Some(&el) {
                            *slot = None;
                        }
                        this.tracking = None;
                    });
                });
            }
            #[cfg(not(target_arch = "wasm32"))]
            let _ = (&h, node, signal, floating);
        })
    }
}

impl Component for AnchorDemo {
    type Props = ();
    rmx::client_entry!("AnchorDemo");

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        AnchorDemo {
            #[cfg(target_arch = "wasm32")]
            anchor: None,
            #[cfg(target_arch = "wasm32")]
            floating: None,
            #[cfg(target_arch = "wasm32")]
            tracking: None,
        }
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        rsx! {
            <div mix={demo_css()}>
                <button type="button" mix={Self::track(h, false)}>"Anchor target"</button>
                <div mix={[floating_css(), Self::track(h, true)]}>"Positioned surface"</div>
            </div>
        }
    }
}

fn demo_css() -> Mixin {
    css("display: grid; place-items: center; min-height: 9rem; width: min(100%, 24rem);")
}

fn floating_css() -> Mixin {
    css("box-sizing: border-box; width: 12rem; border: 1px solid light-dark(#d1d1d1, #444444); border-radius: 8px; background: light-dark(#ffffff, #1a1a1a); box-shadow: 0 12px 32px rgba(0, 0, 0, 0.12); color: light-dark(#151515, #ececec); font-family: \"Inter Variable\", Inter, ui-sans-serif, system-ui, sans-serif; font-size: 13px; line-height: 18px; font-weight: 500; letter-spacing: 0; padding: 10px 12px;")
}
