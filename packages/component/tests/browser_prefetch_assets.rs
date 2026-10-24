//! The default prefetch strategy, after the `default frame prefetch` cases of Remix's
//! `prefetch.test.ts`: the HTML request, inert asset discovery and the hints it adds.
//! `fetch` is stubbed and the hints are recorded as they are added to the head. Run with
//! `pwsh ./scripts/test-browser.ps1 -Test browser_prefetch_assets`.
#![cfg(target_arch = "wasm32")]

extern crate rsmix_component as rsmix;

use std::cell::RefCell;

use rsmix::{prefetch, PrefetchOptions, RunOptions};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

thread_local! {
    static APP: RefCell<Option<rsmix::App>> = const { RefCell::new(None) };
}

thread_local! {
    static BUSY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Async tests interleave in one page; prefetching is page-wide, so they take turns.
struct Serial;

async fn serial() -> Serial {
    while BUSY.with(|b| b.replace(true)) {
        sleep(10).await;
    }
    Serial
}

impl Drop for Serial {
    fn drop(&mut self) {
        BUSY.with(|b| b.set(false));
    }
}

async fn start() {
    if APP.with(|a| a.borrow().is_none()) {
        APP.with(|a| *a.borrow_mut() = Some(rsmix::run(RunOptions::default())));
    }
    let app = APP.with(|a| a.borrow_mut().take()).expect("app");
    app.ready().await;
    APP.with(|a| *a.borrow_mut() = Some(app));
}

fn get(target: &JsValue, key: &str) -> JsValue {
    js_sys::Reflect::get(target, &key.into()).unwrap_or(JsValue::UNDEFINED)
}

async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms).unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

fn absolute(path: &str) -> String {
    let document = web_sys::window().unwrap().document().unwrap();
    web_sys::Url::new_with_base(path, &document.base_uri().unwrap().unwrap()).unwrap().href()
}

/// A `fetch` stub answering by path (unknown paths get a 404), recording each call, and a
/// recorder of the prefetch hints added to the head. Both are undone on drop.
struct Harness(JsValue);

impl Harness {
    /// `routes`: `[path, body, status, content type, final URL or ""]`.
    fn new(routes: &[(&str, &str, u16, &str, &str)]) -> Harness {
        let table = js_sys::Array::new();
        for (path, body, status, ct, url) in routes {
            table.push(&js_sys::Array::of5(&(*path).into(), &(*body).into(), &(*status).into(), &(*ct).into(), &(*url).into()));
        }
        let f = js_sys::Function::new_with_args(
            "routes",
            "const original = window.fetch; const calls = []; const hints = [];
             window.fetch = async (input, init) => {
               const url = new URL(input, document.baseURI);
               calls.push({ url: url.href, init, headers: Object.fromEntries(new Headers(init && init.headers)) });
               const route = routes.find(r => r[0] === url.pathname + url.search);
               if (!route) return new Response('missing', { status: 404, headers: { 'Content-Type': 'text/html' } });
               const response = new Response(route[1], { status: route[2], headers: { 'Content-Type': route[3] } });
               if (route[4]) Object.defineProperty(response, 'url', { value: new URL(route[4], document.baseURI).href });
               return response;
             };
             const observer = new MutationObserver(records => {
               for (const r of records) for (const n of r.addedNodes) {
                 if (n.localName === 'link' && n.rel === 'prefetch') {
                   hints.push([n.getAttribute('href'), n.getAttribute('as'), n.getAttribute('crossorigin'), n.getAttribute('integrity'), n.getAttribute('referrerpolicy'), n.getAttribute('fetchpriority')].map(v => v ?? '').join(' ').trim());
                 }
               }
             });
             observer.observe(document.head, { childList: true });
             return { calls, hints, restore() { window.fetch = original; observer.disconnect() } };",
        );
        Harness(f.call1(&JsValue::NULL, &table).unwrap())
    }

    fn calls(&self) -> Vec<JsValue> {
        get(&self.0, "calls").unchecked_into::<js_sys::Array>().iter().collect()
    }

    fn urls(&self) -> Vec<String> {
        self.calls().iter().map(|c| get(c, "url").as_string().unwrap()).collect()
    }

    /// Hints added so far: `href as [crossorigin] [integrity] [referrerpolicy] low`.
    async fn hints(&self) -> Vec<String> {
        sleep(30).await;
        let normalize = |h: JsValue| h.as_string().unwrap().split_whitespace().collect::<Vec<_>>().join(" ");
        let mut hints: Vec<String> = get(&self.0, "hints").unchecked_into::<js_sys::Array>().iter().map(normalize).collect();
        hints.sort();
        // A finished hint can be added again by later discovery.
        hints.dedup();
        hints
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        get(&self.0, "restore").unchecked_into::<js_sys::Function>().call0(&self.0).unwrap();
    }
}

const HTML: &str = "text/html; charset=utf-8";

#[wasm_bindgen_test]
async fn fetches_html_with_low_priority_and_frame_headers() {
    let _serial = serial().await;
    start().await;
    let h = Harness::new(&[("/low", "<p>low</p>", 200, HTML, "")]);
    prefetch("/low", PrefetchOptions::default());
    prefetch("/low", PrefetchOptions { target: Some("side".into()) });
    sleep(30).await;
    let calls = h.calls();
    assert_eq!(h.urls(), [absolute("/low"), absolute("/low")], "the same URL for another target is separate work");
    let init = get(&calls[0], "init");
    assert_eq!((get(&init, "method").as_string(), get(&init, "mode").as_string(), get(&init, "priority").as_string()), (Some("GET".into()), Some("same-origin".into()), Some("low".into())));
    let headers = |i: usize| js_sys::JSON::stringify(&get(&calls[i], "headers")).unwrap().as_string().unwrap();
    assert_eq!(headers(0), r#"{"accept":"text/html","x-remix-frame":"true"}"#);
    assert_eq!(headers(1), r#"{"accept":"text/html","x-remix-frame":"true","x-remix-target":"side"}"#);
}

#[wasm_bindgen_test]
async fn discovers_explicit_assets_in_inert_html_and_nested_templates() {
    let _serial = serial().await;
    start().await;
    let page = r#"<!doctype html><html><head>
        <script type="importmap">{"imports":{"x":"/mapped.js"}}</script>
        <script type="module" src="/discover/entry.js"></script>
        <script>window.__rmxPrefetchRan = true</script>
        <script type="text/plain" src="/discover/not-js.txt"></script>
        <link rel="stylesheet" href="/discover/app.css" integrity="sha384-abc" referrerpolicy="no-referrer">
        <link rel="modulepreload" href="/discover/chunk.js">
        <link rel="preload" as="image" href="/discover/hero.png">
        <link rel="preload" as="image" href="/discover/responsive.png" imagesrcset="/a.png 1x, /b.png 2x">
        <link rel="preload" as="image" href="/discover/print.png" media="print">
        <link rel="preload" as="font" href="/discover/font.woff2" crossorigin>
        <link rel="icon" href="/discover/favicon.ico">
        <script src="data:text/javascript,1"></script>
        </head><body><template><div><script src="/discover/nested.js"></script></div><template><link rel="stylesheet" href="/discover/deep.css"></template></template></body></html>"#;
    let h = Harness::new(&[("/discover", page, 200, HTML, "")]);
    prefetch("/discover", PrefetchOptions::default());
    let hints = h.hints().await;
    let expected = [
        format!("{} script anonymous   low", absolute("/discover/chunk.js")),
        format!("{} script anonymous   low", absolute("/discover/entry.js")),
        format!("{} script    low", absolute("/discover/nested.js")),
        format!("{} style  sha384-abc no-referrer low", absolute("/discover/app.css")),
        format!("{} style    low", absolute("/discover/deep.css")),
        format!("{} font anonymous   low", absolute("/discover/font.woff2")),
        format!("{} image    low", absolute("/discover/hero.png")),
    ];
    let mut expected: Vec<String> = expected.into_iter().map(|s| s.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    expected.sort();
    let hints: Vec<String> = hints.into_iter().map(|s| s.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    assert_eq!(hints, expected);
    assert!(get(&web_sys::window().unwrap().into(), "__rmxPrefetchRan").is_undefined(), "parsed scripts do not run");
}

#[wasm_bindgen_test]
async fn honors_the_final_response_url_and_base_and_skips_active_assets() {
    let _serial = serial().await;
    start().await;
    let document = web_sys::window().unwrap().document().unwrap();
    let active = document.create_element("link").unwrap();
    active.set_attribute("rel", "stylesheet").unwrap();
    active.set_attribute("href", "/active/already.css").unwrap();
    document.head().unwrap().append_child(&active).unwrap();
    let page = r#"<link rel="stylesheet" href="/active/already.css"><link rel="stylesheet" href="relative.css">"#;
    let based = r#"<base href="/assets/"><script src="app.js"></script>"#;
    let h = Harness::new(&[("/redirect", page, 200, HTML, "/moved/page"), ("/based", based, 200, HTML, "")]);
    prefetch("/redirect", PrefetchOptions::default());
    prefetch("/based", PrefetchOptions::default());
    let hints = h.hints().await;
    assert_eq!(hints, [format!("{} script low", absolute("/assets/app.js")), format!("{} style low", absolute("/moved/relative.css"))]);
    active.remove();
}

#[wasm_bindgen_test]
async fn shares_only_pending_work_and_ignores_failed_and_non_html_responses() {
    let _serial = serial().await;
    start().await;
    let h = Harness::new(&[("/shared", r#"<script src="/shared/a.js"></script>"#, 200, HTML, ""), ("/json", r#"<script src="/json/a.js"></script>"#, 200, "application/json", ""), ("/fail", r#"<script src="/fail/a.js"></script>"#, 500, HTML, "")]);
    prefetch("/shared", PrefetchOptions::default());
    prefetch("/shared#again", PrefetchOptions::default());
    sleep(50).await;
    assert_eq!(h.urls(), [absolute("/shared")], "overlapping prefetches share discovery");
    prefetch("/shared", PrefetchOptions::default());
    sleep(50).await;
    assert_eq!(h.urls().len(), 2, "completed work is not cached");
    prefetch("/json", PrefetchOptions::default());
    prefetch("/fail", PrefetchOptions::default());
    let hints = h.hints().await;
    assert_eq!(hints, [format!("{} script low", absolute("/shared/a.js"))], "non-HTML and failed responses add no hints");
}
