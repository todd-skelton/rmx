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
