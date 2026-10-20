//! `demos/ui/cases/draggable`: a stateful mixin that drags its host with the pointer.
//!
//! The mixin keeps the host element from its `insert` event, adds a `pointerdown` handler in
//! `render`, and while a drag is active listens on `window` for moves. It dispatches
//! `rmx:dragstart` and `rmx:dragend` events with the final position.

use rmx::{mixin, on, rsx, Component, Element, Handle, HostMixin, Mixin, MixinHandle, Node};

pub const DRAG_START_EVENT: &str = "rmx:dragstart";
pub const DRAG_END_EVENT: &str = "rmx:dragend";

/// `mix={draggable(true)}`
pub fn draggable(enabled: bool) -> Mixin {
    mixin::<Draggable>(enabled)
}

pub struct Draggable {
    enabled: bool,
    #[cfg(target_arch = "wasm32")]
    browser: browser::DragState,
}

impl HostMixin for Draggable {
    type Args = bool;

    fn setup(h: &MixinHandle<Self>, _tag: &'static str) -> Self {
        h.on_insert(|this, _, event| {
            #[cfg(target_arch = "wasm32")]
            {
                this.browser.node = Some(event.node.element().clone());
            }
            #[cfg(not(target_arch = "wasm32"))]
            let _ = (this, event);
        });
        h.on_remove(|this, _| this.stop_drag());
        Draggable {
            enabled: true,
            #[cfg(target_arch = "wasm32")]
            browser: browser::DragState::new(h),
        }
    }

    fn render(&mut self, enabled: &bool, host: &mut Element, h: &MixinHandle<Self>) {
        self.enabled = *enabled;
        if !self.enabled {
            self.stop_drag();
        }
        host.mix(on("pointerdown", h.on_event(|this, h, event| this.on_pointer_down(event, h))));
    }
}

impl Draggable {
    fn stop_drag(&mut self) {
        #[cfg(target_arch = "wasm32")]
        self.browser.stop();
    }

    fn on_pointer_down(&mut self, event: &rmx::Event, h: &MixinHandle<Self>) {
        if !self.enabled {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        self.browser.start(event, h);
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (event, h);
    }
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use rmx::MixinHandle;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::{JsCast, JsValue};

    use super::{Draggable, DRAG_END_EVENT, DRAG_START_EVENT};

    pub(super) struct DragState {
        pub(super) node: Option<web_sys::Element>,
        pointer_id: Option<i32>,
        start_left: f64,
        start_top: f64,
        start_x: i32,
        start_y: i32,
        on_move: Closure<dyn Fn(web_sys::PointerEvent)>,
        on_done: Closure<dyn Fn(web_sys::PointerEvent)>,
    }

    impl DragState {
        /// The window listeners live as long as the mixin; a drag only adds and removes them.
        pub(super) fn new(h: &MixinHandle<Draggable>) -> DragState {
            let move_handle = h.clone();
            let done_handle = h.clone();
            DragState {
                node: None,
                pointer_id: None,
                start_left: 0.0,
                start_top: 0.0,
                start_x: 0,
                start_y: 0,
                on_move: Closure::new(move |e: web_sys::PointerEvent| {
                    move_handle.with(|this, h| {
                        if this.browser.moved(&e) {
                            h.update();
                        }
                    });
                }),
                on_done: Closure::new(move |e: web_sys::PointerEvent| {
                    done_handle.with(|this, _| this.browser.done(&e));
                }),
            }
        }

        fn style(&self) -> Option<web_sys::CssStyleDeclaration> {
            Some(self.node.as_ref()?.dyn_ref::<web_sys::HtmlElement>()?.style())
        }

        pub(super) fn start(&mut self, event: &rmx::Event, _h: &MixinHandle<Draggable>) {
            let Some(e) = event.raw().dyn_ref::<web_sys::PointerEvent>() else { return };
            if e.button() != 0 {
                return;
            }
            let (Some(node), Some(style)) = (self.node.clone(), self.style()) else { return };
            let window = web_sys::window().expect("window");
            if let Ok(Some(computed)) = window.get_computed_style(&node) {
                if computed.get_property_value("position").ok().as_deref() == Some("static") {
                    let _ = style.set_property("position", "relative");
                }
            }
            let _ = style.set_property("cursor", "grabbing");
            self.start_left = read_px(&style.get_property_value("left").unwrap_or_default());
            self.start_top = read_px(&style.get_property_value("top").unwrap_or_default());
            self.start_x = e.client_x();
            self.start_y = e.client_y();
            self.pointer_id = Some(e.pointer_id());
            let _ = node.set_pointer_capture(e.pointer_id());
            for (name, f) in [("pointermove", &self.on_move), ("pointerup", &self.on_done), ("pointercancel", &self.on_done)] {
                let _ = window.add_event_listener_with_callback(name, f.as_ref().unchecked_ref());
            }
            dispatch_drag_event(&node, &style, DRAG_START_EVENT);
        }

        fn moved(&mut self, e: &web_sys::PointerEvent) -> bool {
            let Some(style) = self.style() else { return false };
            if self.pointer_id != Some(e.pointer_id()) {
                return false;
            }
            let dx = (e.client_x() - self.start_x) as f64;
            let dy = (e.client_y() - self.start_y) as f64;
            let _ = style.set_property("left", &format!("{}px", self.start_left + dx));
            let _ = style.set_property("top", &format!("{}px", self.start_top + dy));
            true
        }

        fn done(&mut self, e: &web_sys::PointerEvent) {
            if self.pointer_id != Some(e.pointer_id()) {
                return;
            }
            self.stop();
            if let (Some(node), Some(style)) = (self.node.clone(), self.style()) {
                dispatch_drag_event(&node, &style, DRAG_END_EVENT);
            }
        }

        pub(super) fn stop(&mut self) {
            if self.pointer_id.take().is_none() {
                return;
            }
            if let Some(style) = self.style() {
                let _ = style.set_property("cursor", "grab");
            }
            let window = web_sys::window().expect("window");
            for (name, f) in [("pointermove", &self.on_move), ("pointerup", &self.on_done), ("pointercancel", &self.on_done)] {
                let _ = window.remove_event_listener_with_callback(name, f.as_ref().unchecked_ref());
            }
        }
    }

    fn dispatch_drag_event(node: &web_sys::Element, style: &web_sys::CssStyleDeclaration, ty: &str) {
        let detail = js_sys::Object::new();
        let left = read_px(&style.get_property_value("left").unwrap_or_default());
        let top = read_px(&style.get_property_value("top").unwrap_or_default());
        let _ = js_sys::Reflect::set(&detail, &"left".into(), &JsValue::from_f64(left));
        let _ = js_sys::Reflect::set(&detail, &"top".into(), &JsValue::from_f64(top));
        let init = web_sys::CustomEventInit::new();
        init.set_detail(&detail);
        init.set_bubbles(true);
        if let Ok(event) = web_sys::CustomEvent::new_with_event_init_dict(ty, &init) {
            let _ = node.dispatch_event(&event);
        }
    }

    fn read_px(value: &str) -> f64 {
        value.trim().trim_end_matches("px").parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(0.0)
    }
}

/// `draggable.demo.tsx`
pub struct DraggableDemo;

impl Component for DraggableDemo {
    type Props = ();
    rmx::client_entry!("DraggableDemo");

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        DraggableDemo
    }

    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! {
            <main style="font-family: system-ui, sans-serif; padding: 24px;">
                <h1>"Draggable mixin demo"</h1>
                <p>"Drag the box with your mouse or trackpad."</p>
                <div style="position: relative; width: 100%; max-width: 720px; height: 420px; border: 1px dashed #c2c2c2; border-radius: 8px; overflow: hidden; background-color: #fafafa;">
                    <div
                        id="draggable-box"
                        mix={[draggable(true)]}
                        style="position: absolute; left: 24px; top: 24px; width: 180px; padding: 14px 16px; border-radius: 10px; background-color: #2563eb; color: white; box-shadow: 0 8px 20px rgba(37, 99, 235, 0.35); user-select: none; touch-action: none; cursor: grab;"
                    >
                        "drag me"
                    </div>
                </div>
            </main>
        }
    }
}
