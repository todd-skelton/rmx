//! Small browser helpers that compile on the server as no-ops, so shared component code can
//! call them from `queue_task` callbacks without `cfg` attributes.

/// `performance.now()` in the browser; `0.0` on the server.
pub fn now() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0.0
    }
}

/// Set `data-<name>` on the `<html>` element. No-op on the server.
pub fn set_document_data(name: &str, value: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let root = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
            .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
        if let Some(root) = root {
            let _ = root.dataset().set(name, value);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (name, value);
}
