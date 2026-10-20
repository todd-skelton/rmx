//! The few DOM reads the client entries need beyond `rmx::Event`: pointer coordinates,
//! focus targets and element geometry. On the server they return neutral values.

use rmx::{Event, NodeHandle};

/// `event.target === event.currentTarget`
pub fn target_is_current(event: &Event) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let target = event.raw().target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
        target.is_some() && target == event.current_element()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = event;
        false
    }
}

/// `event.target instanceof HTMLInputElement`
pub fn target_is_input(event: &Event) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        event.raw().target().is_some_and(|t| t.dyn_into::<web_sys::HtmlInputElement>().is_ok())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = event;
        false
    }
}

/// For `focusout`: whether focus moved to something outside the handler's element.
pub fn focus_left_current(event: &Event) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let next = event.raw().dyn_ref::<web_sys::FocusEvent>().and_then(|e| e.related_target()).and_then(|t| t.dyn_into::<web_sys::Node>().ok());
        match (next, event.current_element()) {
            (Some(next), Some(current)) => !current.contains(Some(&next)),
            _ => true,
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = event;
        true
    }
}

/// `Date.now()`
pub fn date_now() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
    }
}

/// What a pointer event carries that the grid gestures need.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pointer {
    pub x: f64,
    pub y: f64,
    pub button: i64,
    pub id: i64,
    pub shift: bool,
}

impl Pointer {
    pub fn from_event(event: &Event) -> Pointer {
        Pointer {
            x: event.prop_f64("clientX").unwrap_or_default(),
            y: event.prop_f64("clientY").unwrap_or_default(),
            button: event.prop_f64("button").unwrap_or_default() as i64,
            id: event.prop_f64("pointerId").unwrap_or_default() as i64,
            shift: event.prop_bool("shiftKey"),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn from_raw(raw: &web_sys::Event) -> Pointer {
        let num = |name: &str| js_sys::Reflect::get(raw, &name.into()).ok().and_then(|v| v.as_f64()).unwrap_or_default();
        Pointer { x: num("clientX"), y: num("clientY"), button: num("button") as i64, id: num("pointerId") as i64, shift: false }
    }
}

#[cfg(target_arch = "wasm32")]
type Listener = (&'static str, wasm_bindgen::closure::Closure<dyn Fn(web_sys::Event)>);

/// `pointermove`, `pointerup` and `pointercancel` listeners on `window` for one gesture.
/// The callbacks return whether to `preventDefault()`. Dropping removes the listeners.
pub struct WindowListeners {
    #[cfg(target_arch = "wasm32")]
    listeners: Vec<Listener>,
}

impl WindowListeners {
    pub fn bind(on_move: impl Fn(&Pointer) -> bool + 'static, on_end: impl Fn(&Pointer) -> bool + 'static) -> WindowListeners {
        #[cfg(target_arch = "wasm32")]
        {
            use std::rc::Rc;
            use wasm_bindgen::{closure::Closure, JsCast};
            let window = web_sys::window().expect("window");
            let on_end = Rc::new(on_end);
            let make = |f: Rc<dyn Fn(&Pointer) -> bool>| {
                Closure::<dyn Fn(web_sys::Event)>::new(move |raw: web_sys::Event| {
                    if f(&Pointer::from_raw(&raw)) {
                        raw.prevent_default();
                    }
                })
            };
            let listeners = vec![("pointermove", make(Rc::new(on_move))), ("pointerup", make(on_end.clone())), ("pointercancel", make(on_end))];
            for (name, f) in &listeners {
                let _ = window.add_event_listener_with_callback(name, f.as_ref().unchecked_ref());
            }
            WindowListeners { listeners }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            drop((on_move, on_end));
            WindowListeners {}
        }
    }
}

impl Drop for WindowListeners {
    fn drop(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            let listeners = std::mem::take(&mut self.listeners);
            if let Some(window) = web_sys::window() {
                for (name, f) in &listeners {
                    let _ = window.remove_event_listener_with_callback(name, f.as_ref().unchecked_ref());
                }
            }
            // A gesture usually ends inside one of these closures: free them afterwards.
            wasm_bindgen_futures::spawn_local(async move { drop(listeners) });
        }
    }
}

/// An element's `getBoundingClientRect()`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

pub fn rect(node: &NodeHandle) -> Rect {
    #[cfg(target_arch = "wasm32")]
    {
        let r = node.element().get_bounding_client_rect();
        Rect { left: r.left(), top: r.top(), width: r.width(), height: r.height() }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = node;
        Rect::default()
    }
}

pub fn set_scroll_top(node: &NodeHandle, top: f64) {
    #[cfg(target_arch = "wasm32")]
    node.element().set_scroll_top(top as i32);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (node, top);
}

/// `node.focus({ preventScroll: true })`
pub fn focus_without_scroll(node: &NodeHandle) {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        if let Some(el) = node.element().dyn_ref::<web_sys::HtmlElement>() {
            let options = web_sys::FocusOptions::new();
            options.set_prevent_scroll(true);
            let _ = el.focus_with_options(&options);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = node;
}

/// `event.currentTarget.focus({ preventScroll: true })`
pub fn focus_current(event: &Event) {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        if let Some(el) = event.current_element().and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
            let options = web_sys::FocusOptions::new();
            options.set_prevent_scroll(true);
            let _ = el.focus_with_options(&options);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = event;
}

/// The id of the schedule block that has focus (or contains it), if any.
pub fn focused_block_id() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let active = web_sys::window()?.document()?.active_element()?;
        active.closest("[data-schedule-block=\"true\"]").ok()??.get_attribute("data-schedule-block-id")
    }
    #[cfg(not(target_arch = "wasm32"))]
    None
}

/// `document.activeElement.blur()`
pub fn blur_active() {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.active_element()).and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
            let _ = el.blur();
        }
    }
}

/// Set the CSS `translate` of the blocks with these ids under `root`: the per-frame drag
/// offset, written straight to the DOM instead of re-rendering the grid.
pub fn set_block_translate(root: &NodeHandle, ids: &[String], translate: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let Ok(nodes) = root.element().query_selector_all("[data-schedule-block=\"true\"]") else { return };
        for i in 0..nodes.length() {
            let Some(el) = nodes.item(i).and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok()) else { continue };
            if el.get_attribute("data-schedule-block-id").is_some_and(|id| ids.contains(&id)) {
                let _ = el.style().set_property("translate", translate);
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (root, ids, translate);
}
