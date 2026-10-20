//! The formatting the client entries need, on whichever `Intl` the target has: ICU4X on
//! the server (see [`crate::intl`]), the browser's own `Intl` in wasm.

#[cfg(not(target_arch = "wasm32"))]
pub use crate::intl::{format_number, plural_category};

/// `new Intl.NumberFormat(locale).format(n)`
#[cfg(target_arch = "wasm32")]
pub fn format_number(locale: &str, n: i64) -> String {
    use js_sys::{Array, Intl, Object};
    use wasm_bindgen::JsValue;
    let format = Intl::NumberFormat::new(&Array::of1(&JsValue::from_str(locale)), &Object::new()).format();
    format.call1(&JsValue::NULL, &JsValue::from_f64(n as f64)).ok().and_then(|v| v.as_string()).unwrap_or_else(|| n.to_string())
}

/// `new Intl.PluralRules(locale).select(n)`
#[cfg(target_arch = "wasm32")]
pub fn plural_category(locale: &str, n: u64) -> &'static str {
    use js_sys::{Array, Intl, Object};
    use wasm_bindgen::JsValue;
    let rules = Intl::PluralRules::new(&Array::of1(&JsValue::from_str(locale)), &Object::new());
    match String::from(rules.select(n as f64)).as_str() {
        "zero" => "zero",
        "one" => "one",
        "two" => "two",
        "few" => "few",
        "many" => "many",
        _ => "other",
    }
}
