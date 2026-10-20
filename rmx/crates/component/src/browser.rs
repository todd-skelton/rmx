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

/// Resolve on the next animation frame with its timestamp (`requestAnimationFrame` as a
/// future). Resolves at once with `0.0` on the server.
pub async fn next_frame() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            if let Some(w) = web_sys::window() {
                let _ = w.request_animation_frame(&resolve);
            }
        });
        wasm_bindgen_futures::JsFuture::from(promise).await.ok().and_then(|t| t.as_f64()).unwrap_or(0.0)
    }
    #[cfg(not(target_arch = "wasm32"))]
    0.0
}

/// `fetch(url, { method, body: formData, signal })` with the given fields as a
/// `multipart/form-data` body, resolving to the response status. Fails on a network error
/// or when `signal` aborts. On the server it fails at once.
///
/// ```ignore
/// let status = rmx::browser::fetch_form("POST", "/api/cart/toggle", &[("bookId".into(), "1".into())], Some(&signal)).await?;
/// ```
pub async fn fetch_form(method: &str, url: &str, fields: &[(String, String)], signal: Option<&crate::Signal>) -> Result<u16, String> {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let err = |e: wasm_bindgen::JsValue| e.as_string().or_else(|| js_sys::JSON::stringify(&e).ok().map(String::from)).unwrap_or_else(|| "fetch failed".into());
        let body = web_sys::FormData::new().map_err(err)?;
        for (name, value) in fields {
            body.append_with_str(name, value).map_err(err)?;
        }
        let init = web_sys::RequestInit::new();
        init.set_method(method);
        init.set_body(&body);
        if let Some(signal) = signal {
            init.set_signal(Some(&signal.inner));
        }
        let window = web_sys::window().ok_or("no window")?;
        let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str_and_init(url, &init)).await.map_err(err)?;
        let response: web_sys::Response = response.dyn_into().map_err(err)?;
        Ok(response.status())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (method, url, fields, signal);
        Err("fetch_form runs in the browser".into())
    }
}

/// Options for [`fetch`] (a subset of `RequestInit`).
#[derive(Clone, Default)]
pub struct FetchInit {
    /// `GET` when empty.
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub signal: Option<crate::Signal>,
}

/// A fetched response with its body read as text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FetchResponse {
    pub status: u16,
    /// The `Content-Type` header, or empty.
    pub content_type: String,
    pub text: String,
}

impl FetchResponse {
    /// `response.ok`: a 2xx status.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// `response.json()`
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, String> {
        serde_json::from_str(&self.text).map_err(|e| e.to_string())
    }
}

/// `fetch(url, init)` with the body read as text. Fails on a network error or when the
/// signal aborts. On the server it fails at once.
///
/// ```ignore
/// let init = FetchInit { method: "POST".into(), headers: vec![("Content-Type".into(), "application/json".into())], body: Some(json), ..Default::default() };
/// let res = rmx::browser::fetch("/schedules", init).await?;
/// ```
pub async fn fetch(url: &str, init: FetchInit) -> Result<FetchResponse, String> {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let err = |e: wasm_bindgen::JsValue| e.as_string().or_else(|| js_sys::JSON::stringify(&e).ok().map(String::from)).unwrap_or_else(|| "fetch failed".into());
        let request = web_sys::RequestInit::new();
        if !init.method.is_empty() {
            request.set_method(&init.method);
        }
        let headers = web_sys::Headers::new().map_err(err)?;
        for (name, value) in &init.headers {
            headers.append(name, value).map_err(err)?;
        }
        request.set_headers(&headers);
        if let Some(body) = &init.body {
            request.set_body(&wasm_bindgen::JsValue::from_str(body));
        }
        if let Some(signal) = &init.signal {
            request.set_signal(Some(&signal.inner));
        }
        let window = web_sys::window().ok_or("no window")?;
        let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str_and_init(url, &request)).await.map_err(err)?;
        let response: web_sys::Response = response.dyn_into().map_err(err)?;
        let content_type = response.headers().get("Content-Type").ok().flatten().unwrap_or_default();
        let text = wasm_bindgen_futures::JsFuture::from(response.text().map_err(err)?).await.map_err(err)?;
        Ok(FetchResponse { status: response.status(), content_type, text: text.as_string().unwrap_or_default() })
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (url, init);
        Err("fetch runs in the browser".into())
    }
}

/// `new Date().toLocaleTimeString()` in the browser; the UTC time on the server.
pub fn local_time_string() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        String::from(js_sys::Date::new_0().to_locale_time_string("en-US"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let (h, m, s) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
        let h12 = if h % 12 == 0 { 12 } else { h % 12 };
        format!("{h12}:{m:02}:{s:02} {}", if h < 12 { "AM" } else { "PM" })
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
