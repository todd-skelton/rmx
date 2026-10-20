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

/// `Math.random()` in the browser; a fixed sequence on the server.
pub fn random() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Math::random()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::cell::Cell;
        thread_local!(static SEED: Cell<u64> = const { Cell::new(0x2545_f491_4f6c_dd1d) });
        SEED.with(|s| {
            let mut x = s.get();
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            s.set(x);
            (x >> 11) as f64 / (1u64 << 53) as f64
        })
    }
}

/// A `setInterval` timer that is cleared when dropped. Never fires on the server.
pub struct Interval {
    #[cfg(target_arch = "wasm32")]
    id: i32,
    #[cfg(target_arch = "wasm32")]
    _callback: wasm_bindgen::closure::Closure<dyn FnMut()>,
}

impl Interval {
    pub fn new(ms: u32, f: impl FnMut() + 'static) -> Interval {
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            let callback = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(f);
            let id = web_sys::window()
                .expect("window")
                .set_interval_with_callback_and_timeout_and_arguments_0(callback.as_ref().unchecked_ref(), ms as i32)
                .expect("setInterval");
            Interval { id, _callback: callback }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (ms, f);
            Interval {}
        }
    }
}

impl Drop for Interval {
    fn drop(&mut self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(w) = web_sys::window() {
            w.clear_interval_with_handle(self.id);
        }
    }
}

/// Resolve after `ms` milliseconds (`setTimeout` as a future). Resolves at once on the
/// server.
pub async fn sleep(ms: u32) {
    #[cfg(target_arch = "wasm32")]
    {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            if let Some(w) = web_sys::window() {
                let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
            }
        });
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = ms;
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
