//! Bubbling custom events that carry data (Remix's `ToggleChangeEvent`, `TabsChangeEvent`
//! and so on are `Event` subclasses with extra fields). rmx dispatches a plain `Event` with
//! the fields set as properties, and each widget module has a reader for them.

use rmx::Event;

/// A field to set on a dispatched event.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) enum Field<'a> {
    Bool(&'a str, bool),
    Str(&'a str, Option<&'a str>),
    List(&'a str, &'a [String]),
}

/// Dispatch a bubbling event named `name` from `target` with `fields` as properties.
#[cfg(target_arch = "wasm32")]
pub(crate) fn dispatch(target: &web_sys::EventTarget, name: &str, fields: &[Field<'_>]) {
    use wasm_bindgen::JsValue;
    let init = web_sys::EventInit::new();
    init.set_bubbles(true);
    let Ok(event) = web_sys::Event::new_with_event_init_dict(name, &init) else { return };
    for field in fields {
        let (key, value) = match field {
            Field::Bool(k, v) => (*k, JsValue::from_bool(*v)),
            Field::Str(k, v) => (*k, v.map_or(JsValue::NULL, JsValue::from_str)),
            Field::List(k, v) => (*k, v.iter().map(|s| JsValue::from_str(s)).collect::<js_sys::Array>().into()),
        };
        let _ = js_sys::Reflect::set(&event, &JsValue::from_str(key), &value);
    }
    let _ = target.dispatch_event(&event);
}

/// A numeric property of a raw event (`clientX`, `button`), 0 when missing.
#[cfg(target_arch = "wasm32")]
pub(crate) fn num_prop(event: &web_sys::Event, key: &str) -> f64 {
    js_sys::Reflect::get(event, &key.into()).ok().and_then(|v| v.as_f64()).unwrap_or(0.0)
}

/// A boolean property of a raw event (`shiftKey`), false when missing.
#[cfg(target_arch = "wasm32")]
pub(crate) fn bool_prop(event: &web_sys::Event, key: &str) -> bool {
    js_sys::Reflect::get(event, &key.into()).ok().and_then(|v| v.as_bool()).unwrap_or(false)
}

/// `event.relatedTarget` as a node.
#[cfg(target_arch = "wasm32")]
pub(crate) fn related_target(event: &web_sys::Event) -> Option<web_sys::Node> {
    wasm_bindgen::JsCast::dyn_into(js_sys::Reflect::get(event, &"relatedTarget".into()).ok()?).ok()
}

/// Focus a connected element that is not already focused and scroll it into view.
#[cfg(target_arch = "wasm32")]
pub(crate) fn focus_node(el: Option<&web_sys::HtmlElement>) {
    let Some(el) = el.filter(|el| el.is_connected()) else { return };
    let active = el.owner_document().and_then(|d| d.active_element());
    if active.as_ref() == Some(el.as_ref()) {
        return;
    }
    let _ = el.focus();
    let options = web_sys::ScrollIntoViewOptions::new();
    options.set_block(web_sys::ScrollLogicalPosition::Nearest);
    options.set_inline(web_sys::ScrollLogicalPosition::Nearest);
    el.scroll_into_view_with_scroll_into_view_options(&options);
}

/// Resolve after `ms` milliseconds.
#[cfg(target_arch = "wasm32")]
pub(crate) async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

/// Resolve when a CSS transition on `node` ends, or after two frames if none started
/// (Remix's `waitForCssTransition`). Resolves at once when `signal` aborts.
#[cfg(target_arch = "wasm32")]
pub async fn wait_for_css_transition(node: web_sys::HtmlElement, signal: web_sys::AbortSignal) {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    let resolve: Rc<RefCell<Option<js_sys::Function>>> = Rc::default();
    let slot = resolve.clone();
    let promise = js_sys::Promise::new(&mut |res, _| *slot.borrow_mut() = Some(res));
    let finish = move || {
        if let Some(f) = resolve.borrow_mut().take() {
            let _ = f.call0(&wasm_bindgen::JsValue::UNDEFINED);
        }
    };
    let finish = Rc::new(finish);
    let saw_run = Rc::new(Cell::new(false));
    let target: wasm_bindgen::JsValue = node.clone().into();

    let (t, saw) = (target.clone(), saw_run.clone());
    let on_run = Closure::<dyn Fn(web_sys::Event)>::new(move |e: web_sys::Event| {
        if e.target().is_some_and(|x| wasm_bindgen::JsValue::from(x) == t) {
            saw.set(true);
        }
    });
    let (t, f) = (target.clone(), finish.clone());
    let on_end = Closure::<dyn Fn(web_sys::Event)>::new(move |e: web_sys::Event| {
        if e.target().is_some_and(|x| wasm_bindgen::JsValue::from(x) == t) {
            f();
        }
    });
    let f = finish.clone();
    let on_abort = Closure::<dyn Fn()>::new(move || f());
    let _ = node.add_event_listener_with_callback("transitionrun", on_run.as_ref().unchecked_ref());
    let _ = node.add_event_listener_with_callback("transitionend", on_end.as_ref().unchecked_ref());
    let _ = node.add_event_listener_with_callback("transitioncancel", on_end.as_ref().unchecked_ref());
    let _ = signal.add_event_listener_with_callback("abort", on_abort.as_ref().unchecked_ref());
    if signal.aborted() {
        finish();
    } else if let Some(w) = web_sys::window() {
        let (f, saw) = (finish.clone(), saw_run.clone());
        let second = Closure::once_into_js(move || {
            if !saw.get() {
                f();
            }
        });
        let first = Closure::once_into_js(move || {
            if let Some(w) = web_sys::window() {
                let _ = w.request_animation_frame(second.unchecked_ref());
            }
        });
        let _ = w.request_animation_frame(first.unchecked_ref());
    }
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    let _ = node.remove_event_listener_with_callback("transitionrun", on_run.as_ref().unchecked_ref());
    let _ = node.remove_event_listener_with_callback("transitionend", on_end.as_ref().unchecked_ref());
    let _ = node.remove_event_listener_with_callback("transitioncancel", on_end.as_ref().unchecked_ref());
    let _ = signal.remove_event_listener_with_callback("abort", on_abort.as_ref().unchecked_ref());
}

/// Log a warning to the browser console.
#[cfg(target_arch = "wasm32")]
pub(crate) fn warn(message: &str) {
    web_sys::console::warn_1(&message.into());
}

/// Run `f` once when `signal` aborts.
#[cfg(target_arch = "wasm32")]
pub(crate) fn on_abort(signal: &web_sys::AbortSignal, f: impl FnOnce() + 'static) {
    use wasm_bindgen::JsCast;
    if signal.aborted() {
        return f();
    }
    let callback = wasm_bindgen::closure::Closure::once_into_js(f);
    let options = web_sys::AddEventListenerOptions::new();
    options.set_once(true);
    let _ = signal.add_event_listener_with_callback_and_add_event_listener_options("abort", callback.unchecked_ref(), &options);
}

/// A boolean field of the event's underlying browser event.
pub(crate) fn bool_field(event: &Event, key: &str) -> Option<bool> {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Reflect::get(event.raw(), &key.into()).ok()?.as_bool()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (event, key);
        None
    }
}

/// A string field of the event's underlying browser event.
pub(crate) fn str_field(event: &Event, key: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Reflect::get(event.raw(), &key.into()).ok()?.as_string()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (event, key);
        None
    }
}

/// A string array field of the event's underlying browser event.
pub(crate) fn list_field(event: &Event, key: &str) -> Option<Vec<String>> {
    #[cfg(target_arch = "wasm32")]
    {
        let value = js_sys::Reflect::get(event.raw(), &key.into()).ok()?;
        if !js_sys::Array::is_array(&value) {
            return None;
        }
        Some(js_sys::Array::from(&value).iter().filter_map(|v| v.as_string()).collect())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (event, key);
        None
    }
}

/// Set a property on the element a handler runs for (`currentTarget`).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn set_current_prop(event: &Event, key: &str, value: bool) {
    #[cfg(target_arch = "wasm32")]
    if let Some(el) = event.current_element() {
        let _ = js_sys::Reflect::set(&el, &key.into(), &value.into());
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (event, key, value);
}

/// Whether an attribute value turns a boolean state on (`true`, `"true"` or `""`).
pub(crate) fn attr_is_true(value: Option<&rmx::AttrValue>) -> bool {
    match value {
        Some(rmx::AttrValue::Bool(b)) => *b,
        Some(rmx::AttrValue::Str(s)) => s == "true" || s.is_empty(),
        _ => false,
    }
}

/// An attribute's boolean value, if it is set to one.
pub(crate) fn attr_bool(value: Option<&rmx::AttrValue>) -> Option<bool> {
    match value {
        Some(rmx::AttrValue::Bool(b)) => Some(*b),
        _ => None,
    }
}
