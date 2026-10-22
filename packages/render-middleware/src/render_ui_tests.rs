//! Ports of Remix's `render-middleware/src/lib/render-ui.test.ts`, one test per case.
//!
//! Not ported: the client entry cases (rmx ships one wasm bundle, no import maps or
//! `clientEntry` module ids), the styles case (covered in `stream_tests.rs`), and the
//! component render error cases, since a failed rmx component is omitted on the server
//! instead of failing the stream; the error reporting cases run with frame resolution
//! errors instead. The default reporter prints to stderr, which is not captured here.

use std::sync::{Arc, Mutex};

use http::{HeaderMap, Method};
use rmx::{rsx, Component, Frame, Handle, Node};
use rmx_fetch::{block_on, response, AbortController, Body, BoxError, Response};
use rmx_router::{Context, Router};

use super::{render, Render, RenderOptions, ResponseInit};

/// Errors an `on_error` hook received.
#[derive(Clone, Default)]
struct Errors(Arc<Mutex<Vec<String>>>);

impl Errors {
    fn options(&self) -> RenderOptions {
        let errors = self.0.clone();
        RenderOptions::default().on_error(move |error: &BoxError| errors.lock().unwrap().push(error.to_string()))
    }

    fn list(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

fn fetch(router: &Router, request: http::request::Builder) -> Response {
    block_on(router.fetch(request.body(Body::empty()).unwrap()))
}

fn text(res: Response) -> Result<String, BoxError> {
    block_on(res.into_body().text())
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name).map(|v| v.to_str().unwrap().to_string())
}

/// Shows the frame it renders in and the top frame (Remix's `FrameLocation`).
struct FrameLocation;

impl Component for FrameLocation {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        FrameLocation
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let text = format!("{} (top: {})", h.frame().src(), h.frames().top().src());
        rsx! { <p>{text}</p> }
    }
}

#[test]
fn adds_a_remix_component_renderer_to_request_context() {
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move {
        ctx.render_with(rsx! { <h1>"Hello"</h1> }, ResponseInit::status(http::StatusCode::from_u16(201).unwrap()).header("X-Test", "render"))
    });
    let res = fetch(&router, http::Request::get("https://remix.run/"));
    assert_eq!(res.status(), 201);
    assert_eq!(res.headers()["content-type"], "text/html; charset=UTF-8");
    assert_eq!(res.headers()["x-test"], "render");
    assert_eq!(text(res).unwrap(), "<h1>Hello</h1>");
}

#[test]
fn resolves_nested_and_targeted_frames_through_the_current_router() {
    let seen: Arc<Mutex<Vec<HeaderMap>>> = Arc::default();
    let mut router = Router::new();
    router.post("/start", |ctx: Context| async move { ctx.render(rsx! { <Frame name="outer" src="/frames/first"/> }) });
    let log = seen.clone();
    router.get("/frames/first", move |ctx: Context| {
        log.lock().unwrap().push(ctx.headers().clone());
        async move { ctx.render(rsx! { <section><FrameLocation/><Frame name="inner" src="./second"/></section> }) }
    });
    let log = seen.clone();
    router.get("/frames/second", move |ctx: Context| {
        log.lock().unwrap().push(ctx.headers().clone());
        async move { ctx.render(rsx! { <FrameLocation/> }) }
    });

    let mut request = http::Request::builder().method(Method::POST).uri("https://remix.run/start");
    for (name, value) in [
        ("Accept", "application/json"),
        ("Accept-Encoding", "gzip"),
        ("Authorization", "Bearer secret"),
        ("Content-Length", "2"),
        ("Content-Type", "application/json"),
        ("Cookie", "session=abc"),
        ("Sec-Fetch-Dest", "document"),
        ("Sec-Fetch-Mode", "navigate"),
        ("Sec-Fetch-Site", "same-origin"),
        ("Sec-Fetch-User", "?1"),
        ("X-Session-Token", "token"),
    ] {
        request = request.header(name, value);
    }
    let html = text(block_on(router.fetch(request.body(Body::from("{}")).unwrap()))).unwrap();

    assert!(html.contains("https://remix.run/frames/first (top: https://remix.run/start)"), "{html}");
    assert!(html.contains("https://remix.run/frames/second (top: https://remix.run/start)"), "{html}");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    let first = &seen[0];
    assert_eq!(header(first, "accept").as_deref(), Some("text/html"));
    assert_eq!(header(first, "accept-encoding").as_deref(), Some("identity"));
    assert_eq!(header(first, "authorization").as_deref(), Some("Bearer secret"));
    assert_eq!(header(first, "cookie").as_deref(), Some("session=abc"));
    assert_eq!(header(first, "x-session-token").as_deref(), Some("token"));
    assert_eq!(header(first, "x-remix-frame").as_deref(), Some("true"));
    assert_eq!(header(first, "x-remix-target").as_deref(), Some("outer"));
    for gone in ["content-length", "content-type", "sec-fetch-dest", "sec-fetch-mode", "sec-fetch-site", "sec-fetch-user"] {
        assert_eq!(header(first, gone), None, "{gone}");
    }
    assert_eq!(header(&seen[1], "x-remix-target").as_deref(), Some("inner"));
}

#[test]
fn follows_frame_redirects_and_preserves_non_successful_html_response_content() {
    let cross_origin: Arc<Mutex<Option<HeaderMap>>> = Arc::default();
    let mut router = Router::new();
    router.get("/redirect-frame", |ctx: Context| async move {
        ctx.render(rsx! {
            <main>
                <Frame src="/redirect"/>
                <Frame src="/invalid"/>
                <Frame src="/not-a-redirect"/>
                <Frame src="https://other.example/cross-origin"/>
            </main>
        })
    });
    router.get("/redirect", |_: Context| async move { response::redirect("/ok") });
    router.get("/ok", |_: Context| async move { response::html("<strong>Redirected</strong>") });
    router.get("/invalid", |_: Context| async move { ResponseInit::status(http::StatusCode::from_u16(422).unwrap()).apply(response::html("<p>Validation failed</p>")) });
    router.get("/not-a-redirect", |_: Context| async move {
        ResponseInit::status(http::StatusCode::from_u16(300).unwrap()).header("Location", "/ok").apply(response::html("<p>Multiple choices</p>"))
    });
    let log = cross_origin.clone();
    router.get("/cross-origin", move |ctx: Context| {
        *log.lock().unwrap() = Some(ctx.headers().clone());
        async move { response::html("<span>Cross origin</span>") }
    });

    let request = http::Request::get("https://remix.run/redirect-frame")
        .header("Authorization", "Bearer secret")
        .header("Cookie", "session=abc")
        .header("X-Api-Key", "api-secret")
        .header("X-Session-Token", "session-secret");
    let html = text(fetch(&router, request)).unwrap();

    for content in ["<strong>Redirected</strong>", "<p>Validation failed</p>", "<p>Multiple choices</p>", "<span>Cross origin</span>"] {
        assert!(html.contains(content), "{content}: {html}");
    }
    let headers = cross_origin.lock().unwrap().clone().expect("the cross-origin frame was requested");
    for gone in ["authorization", "cookie", "x-api-key", "x-session-token", "x-remix-top-frame-src"] {
        assert_eq!(header(&headers, gone), None, "{gone}");
    }
    assert_eq!(header(&headers, "x-remix-frame").as_deref(), Some("true"));
}

#[test]
fn rejects_non_html_frame_responses() {
    let errors = Errors::default();
    let mut router = Router::new().with(render(errors.options()));
    router.get("/", |ctx: Context| async move { ctx.render(rsx! { <Frame src="/text"/> }) });
    router.get("/text", |_: Context| async move { response::text("Not UI") });
    let error = text(fetch(&router, http::Request::get("https://remix.run/"))).unwrap_err();
    assert_eq!(error.to_string(), "Frame response must be HTML");
    assert_eq!(errors.list(), ["Frame response must be HTML"]);
}

#[test]
fn rejects_failed_frame_responses_without_ui() {
    let errors = Errors::default();
    let mut router = Router::new().with(render(errors.options()));
    router.get("/", |ctx: Context| async move { ctx.render(rsx! { <Frame src="/missing"/> }) });
    router.get("/missing", |_: Context| async move {
        ResponseInit::status(http::StatusCode::from_u16(404).unwrap()).header("Content-Type", "text/html").apply(Response::new(Body::empty()))
    });
    let error = text(fetch(&router, http::Request::get("https://remix.run/"))).unwrap_err();
    assert_eq!(error.to_string(), "Failed to resolve frame: 404 Not Found");
    assert_eq!(errors.list().len(), 1);
}

#[test]
fn stops_after_too_many_frame_redirects() {
    let errors = Errors::default();
    let mut router = Router::new().with(render(errors.options()));
    router.get("/", |ctx: Context| async move { ctx.render(rsx! { <Frame src="/loop"/> }) });
    let hops = Arc::new(Mutex::new(0));
    let count = hops.clone();
    router.get("/loop", move |_: Context| {
        *count.lock().unwrap() += 1;
        async move { response::redirect("/loop") }
    });
    let error = text(fetch(&router, http::Request::get("https://remix.run/"))).unwrap_err();
    assert_eq!(error.to_string(), "Too many frame redirects while resolving https://remix.run/loop");
    assert_eq!(*hops.lock().unwrap(), 21, "the first request and 20 redirects");
    assert_eq!(errors.list().len(), 1);
}

#[test]
fn renders_frame_bodies_in_place_without_a_leaked_doctype() {
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move {
        ctx.render(rsx! {
            <main>
                <Frame src="/blocking"/>
                <p>"after"</p>
                <Frame src="/async" fallback={rsx! { <p>"loading"</p> }}/>
            </main>
        })
    });
    router.get("/blocking", |_: Context| async move { response::html("<!DOCTYPE html><h2>Blocking content</h2>") });
    router.get("/async", |_: Context| async move { response::html("<!DOCTYPE html><h2>Async content</h2>") });
    let html = text(fetch(&router, http::Request::get("https://remix.run/"))).unwrap();

    let slot_start = html.find("<!--rmx:f:").expect("frame slot");
    let slot = &html[slot_start..html[slot_start..].find("<!--/rmx:f-->").map(|e| slot_start + e).unwrap()];
    assert!(slot.contains("<h2>Blocking content</h2>"), "{html}");
    assert!(html.find("Blocking content").unwrap() < html.find("<p>after</p>").unwrap());
    let template = html.find("<template id=\"f").expect("a streamed template");
    assert!(html[template..].contains("<h2>Async content</h2><!--rmx:te--></template>"), "{html}");
    assert!(!html.to_ascii_lowercase().contains("<!doctype"), "frame doctypes are stripped: {html}");
}

/// Read `res`'s body on another thread; the handle yields its result.
fn read_in_background(res: Response) -> std::thread::JoinHandle<Result<String, String>> {
    std::thread::spawn(move || block_on(res.into_body().text()).map_err(|e| e.to_string()))
}

fn wait_for(what: &str, check: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !check() {
        assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn cancels_internal_frame_requests_with_the_original_request() {
    #[derive(Default)]
    struct Seen {
        started: bool,
        has_signal: bool,
        cancelled: bool,
    }
    struct Cancelled(Arc<Mutex<Seen>>);
    impl Drop for Cancelled {
        fn drop(&mut self) {
            self.0.lock().unwrap().cancelled = true;
        }
    }
    let seen: Arc<Mutex<Seen>> = Arc::default();
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move { ctx.render(rsx! { <Frame src="/slow"/> }) });
    let log = seen.clone();
    router.get("/slow", move |ctx: Context| {
        let mut seen = log.lock().unwrap();
        (seen.started, seen.has_signal) = (true, !ctx.signal().is_never());
        drop(seen);
        // Cancelled when the signal aborts or when the request is dropped.
        let guard = Cancelled(log.clone());
        async move {
            ctx.signal().until_aborted().await;
            drop(guard);
            response::html("<p>late</p>")
        }
    });
    let controller = AbortController::new();
    let mut request = http::Request::get("https://remix.run/").body(Body::empty()).unwrap();
    request.extensions_mut().insert(controller.signal());
    let reading = read_in_background(block_on(router.fetch(request)));
    wait_for("the frame request", || seen.lock().unwrap().started);
    assert!(!seen.lock().unwrap().cancelled);
    assert!(seen.lock().unwrap().has_signal, "the frame request carries the original request's signal");
    controller.abort();
    assert!(reading.join().unwrap().is_err(), "the page body fails");
    assert!(seen.lock().unwrap().cancelled, "the frame request was cancelled");
}

#[test]
fn cancels_returned_frame_streams_with_the_original_request() {
    let sender: Arc<Mutex<Option<rmx_fetch::BodySender>>> = Arc::default();
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move { ctx.render(rsx! { <Frame src="/stream"/> }) });
    let keep = sender.clone();
    router.get("/stream", move |_: Context| {
        let (tx, body) = Body::channel();
        *keep.lock().unwrap() = Some(tx);
        async move { ResponseInit::default().header("Content-Type", "text/html").apply(Response::new(body)) }
    });
    let controller = AbortController::new();
    let mut request = http::Request::get("https://remix.run/").body(Body::empty()).unwrap();
    request.extensions_mut().insert(controller.signal());
    let reading = read_in_background(block_on(router.fetch(request)));
    wait_for("the frame stream", || sender.lock().unwrap().is_some());
    let tx = sender.lock().unwrap().take().unwrap();
    assert!(!tx.is_closed());
    controller.abort();
    assert!(reading.join().unwrap().is_err());
    assert!(tx.is_closed(), "the frame stream was cancelled");
}

#[test]
fn reports_rendering_errors_for_browser_initiated_frame_requests() {
    let errors = Errors::default();
    let mut router = Router::new().with(render(errors.options()));
    router.get("/frame", |ctx: Context| async move { ctx.render(rsx! { <Frame src="/text"/> }) });
    router.get("/text", |_: Context| async move { response::text("Not UI") });
    let request = http::Request::get("https://remix.run/frame").header("X-Remix-Frame", "true").header("X-Remix-Target", "details");
    assert!(text(fetch(&router, request)).is_err());
    assert_eq!(errors.list(), ["Frame response must be HTML"]);
}

#[test]
fn reports_redirected_nested_errors_once_for_browser_initiated_frame_requests() {
    let errors = Errors::default();
    let child_errors = Errors::default();
    let mut router = Router::new().with(render(errors.options()));
    router.get("/", |ctx: Context| async move { ctx.render(rsx! { <Frame src="/redirect"/> }) });
    router.get("/redirect", |_: Context| async move { response::redirect("https://remix.run/child") });
    let child_options = child_errors.options();
    router.get("/child", move |mut ctx: Context| {
        ctx.set(child_options.clone());
        async move { ctx.render(rsx! { <Frame src="/text"/> }) }
    });
    router.get("/text", |_: Context| async move { response::text("Not UI") });
    let request = http::Request::get("https://remix.run/").header("X-Remix-Frame", "true");
    assert_eq!(text(fetch(&router, request)).unwrap_err().to_string(), "Frame response must be HTML");
    assert_eq!(errors.list().len(), 1);
    assert_eq!(child_errors.list().len(), 0, "the internal render does not report");
}

#[test]
fn reports_nested_frame_render_errors_once() {
    let errors = Errors::default();
    let mut router = Router::new().with(render(errors.options()));
    router.get("/", |ctx: Context| async move { ctx.render(rsx! { <Frame src="/child"/> }) });
    router.get("/child", |ctx: Context| async move { ctx.render(rsx! { <Frame src="/text"/> }) });
    router.get("/text", |_: Context| async move { response::text("Not UI") });
    assert!(text(fetch(&router, http::Request::get("https://remix.run/"))).is_err());
    assert_eq!(errors.list(), ["Frame response must be HTML"]);
}

#[test]
fn honors_the_top_frame_header_only_on_frame_requests() {
    let mut router = Router::new();
    router.get("/page", |ctx: Context| async move { ctx.render(rsx! { <FrameLocation/> }) });
    let plain = http::Request::get("https://remix.run/page").header("X-Remix-Top-Frame-Src", "https://remix.run/private?token=1");
    assert!(text(fetch(&router, plain)).unwrap().contains("(top: https://remix.run/page)"));
    let frame = http::Request::get("https://remix.run/page").header("X-Remix-Frame", "true").header("X-Remix-Top-Frame-Src", "https://remix.run/top");
    assert!(text(fetch(&router, frame)).unwrap().contains("(top: https://remix.run/top)"));
}
