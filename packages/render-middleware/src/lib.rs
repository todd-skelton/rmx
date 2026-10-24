//! Request-scoped rendering for rsmix-fetch-router (Remix's `render-middleware`).
//!
//! `ctx.render(node)` turns a component tree into an HTML [`Response`]. `<Frame>`s in the
//! tree resolve through the current router, as internal `GET` subrequests like Remix's
//! `render-ui.ts` makes them: they carry the original request's headers (cookies,
//! authorization, app headers) minus body, conditional and hop-by-hop headers, ask for
//! `text/html` with `identity` encoding, follow 301, 302, 303, 307 and 308 redirects (at
//! most 20), and share the original request's abort signal. A frame on another origin gets
//! none of the original headers. A frame response must be HTML, and a failed one must have
//! a body (its error UI); otherwise the page's body fails and the error goes to the
//! [`render`] middleware's `on_error` (by default printed to stderr), once, by the render
//! that started the request:
//!
//! - A frame without a `fallback` is blocking: the response waits for it and inlines it.
//! - A frame with a `fallback` sends the fallback first; its content streams later as a
//!   `<template id>` chunk that the browser runtime swaps in.
//!
//! Requests from the browser runtime (`X-Remix-Frame: true`) get the same treatment, so a
//! frame reload or a soft navigation receives HTML the runtime can reconcile.
//!
//! ```ignore
//! use rsmix_render_middleware::Render;
//!
//! router.get("/", |ctx: Context| async move {
//!     ctx.render(rsx! { <Layout><Frame src="/activity" fallback={rsx! { <p>"Loading"</p> }}/></Layout> })
//! });
//! ```

use std::future::Future;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};

use http::{header, HeaderMap, HeaderName, HeaderValue, Method};
use rsmix::ssr::{self, Segment};
use rsmix::Node;
use rsmix_fetch::{AbortError, AbortSignal, Body, BoxError, Response};
use rsmix_fetch_router::{Context, Middleware, Next, Router};
use rsmix_url::Url;
use std::sync::Arc;

mod renderer;
pub use renderer::{render_with, Renderer, RendererExt};

/// Sent by the browser runtime (and internal subrequests) when asking for frame content.
pub const FRAME_HEADER: &str = "x-remix-frame";
/// The frame name a browser frame request targets.
pub const TARGET_HEADER: &str = "x-remix-target";
/// The top frame URL, passed to internal frame subrequests (Remix's `X-Remix-Top-Frame-Src`).
pub const TOP_HEADER: &str = "x-remix-top-frame-src";

/// Redirects a frame subrequest follows before failing (Remix's `MAX_FRAME_REDIRECTS`).
const MAX_FRAME_REDIRECTS: usize = 20;

/// Headers a frame subrequest does not take from the original request (Remix's
/// `FRAME_REQUEST_HEADERS_TO_REMOVE`), besides every `sec-fetch-*`.
const FRAME_REQUEST_HEADERS_TO_REMOVE: [&str; 19] = [
    "connection",
    "content-encoding",
    "content-language",
    "content-length",
    "content-location",
    "content-type",
    "expect",
    "host",
    "if-match",
    "if-modified-since",
    "if-none-match",
    "if-range",
    "if-unmodified-since",
    "keep-alive",
    "range",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// The only headers a frame on another origin receives (Remix's `CROSS_ORIGIN_FRAME_HEADERS`;
/// never the top frame URL, which may hold private paths or queries).
const CROSS_ORIGIN_FRAME_HEADERS: [&str; 4] = ["accept", "accept-encoding", FRAME_HEADER, TARGET_HEADER];

/// Marks the frame subrequests a render makes: their errors are reported by that render.
#[derive(Clone, Copy)]
struct InternalFrameRequest;

/// Where render errors go.
type ErrorReporter = Arc<dyn Fn(&BoxError) + Send + Sync>;

/// Options for the [`render`] middleware (Remix's `RenderOptions`).
#[derive(Clone, Default)]
pub struct RenderOptions {
    on_error: Option<ErrorReporter>,
}

impl RenderOptions {
    /// Called when rendering fails (a frame that cannot be resolved). Without it, errors are
    /// printed to stderr (Remix's `console.error`).
    pub fn on_error(mut self, on_error: impl Fn(&BoxError) + Send + Sync + 'static) -> RenderOptions {
        self.on_error = Some(Arc::new(on_error));
        self
    }
}

/// Remix's `render(options)` middleware: the options `ctx.render` uses for this request.
/// `ctx.render` works without it, reporting errors to stderr.
pub fn render(options: RenderOptions) -> impl Middleware {
    move |mut ctx: Context, next: Next| {
        ctx.set(options.clone());
        next.run(ctx)
    }
}

/// Where a render reports errors: nowhere for internal frame subrequests (the render that
/// made them reports what reaches it), else `on_error` or stderr.
fn reporter(ctx: &Context) -> ErrorReporter {
    if ctx.request().extensions().get::<InternalFrameRequest>().is_some() {
        return Arc::new(|_: &BoxError| {});
    }
    match ctx.get::<RenderOptions>().and_then(|o| o.on_error.clone()) {
        Some(on_error) => on_error,
        None => Arc::new(|error: &BoxError| eprintln!("{error}")),
    }
}

/// Status and headers for [`Render::render_with`] (the one `ResponseInit` in `rsmix-fetch`).
pub use rsmix_response::ResponseInit;

/// `ctx.render(node)` for rsmix-fetch-router handlers.
pub trait Render {
    /// Render a component tree to a `200 text/html` response.
    fn render(&self, node: Node) -> Response {
        self.render_with(node, ResponseInit::default())
    }

    /// Render with a status and extra headers.
    fn render_with(&self, node: Node, init: ResponseInit) -> Response;

    /// Whether this request asks for frame content (a browser frame reload or soft
    /// navigation, or an internal frame subrequest).
    fn is_frame_request(&self) -> bool;

    /// The frame name a browser frame request targets.
    fn frame_target(&self) -> Option<&str>;
}

impl Render for Context {
    fn render_with(&self, node: Node, init: ResponseInit) -> Response {
        let url = self.url().clone();
        // Only frame requests name a top frame (Remix's `getTopFrameSrc`).
        let top = self.header(TOP_HEADER).filter(|_| self.is_frame_request()).map_or_else(|| url.href(), str::to_string);
        rsmix::set_server_frame_src(&url.href(), &top);
        let rendered = ssr::render(&node);
        drop(node);
        let frames = Frames { router: self.router().clone(), base: url, headers: frame_request_headers(self.headers(), &top), signal: self.signal().clone() };
        respond(frames, reporter(self), rendered, init)
    }

    fn is_frame_request(&self) -> bool {
        self.header(FRAME_HEADER) == Some("true")
    }

    fn frame_target(&self) -> Option<&str> {
        self.header(TARGET_HEADER)
    }
}

/// What a render needs to resolve its frames.
#[derive(Clone)]
struct Frames {
    router: Router,
    /// The URL being rendered: frame srcs resolve against it, and its origin decides which
    /// frames are cross-origin.
    base: Url,
    headers: HeaderMap,
    signal: AbortSignal,
}

/// The headers of a frame subrequest (Remix's `createFrameRequestHeaders`): the original
/// request's, without body, conditional, hop-by-hop and `sec-fetch-*` headers, asking for
/// HTML as a frame of `top`.
fn frame_request_headers(headers: &HeaderMap, top: &str) -> HeaderMap {
    let mut out = headers.clone();
    for name in FRAME_REQUEST_HEADERS_TO_REMOVE {
        out.remove(name);
    }
    let fetch_metadata: Vec<HeaderName> = out.keys().filter(|name| name.as_str().starts_with("sec-fetch-")).cloned().collect();
    for name in fetch_metadata {
        out.remove(name);
    }
    out.insert(header::ACCEPT, HeaderValue::from_static("text/html"));
    out.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    out.insert(HeaderName::from_static(FRAME_HEADER), HeaderValue::from_static("true"));
    match HeaderValue::from_str(top) {
        Ok(v) => out.insert(HeaderName::from_static(TOP_HEADER), v),
        Err(_) => out.remove(TOP_HEADER),
    };
    out
}

/// Remix's `createCrossOriginFrameHeaders`.
fn cross_origin_headers(headers: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for name in CROSS_ORIGIN_FRAME_HEADERS {
        if let Some(v) = headers.get(name) {
            out.insert(HeaderName::from_static(name), v.clone());
        }
    }
    out
}

fn html_response(body: Body, init: ResponseInit) -> Response {
    let mut res = Response::new(body);
    res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=UTF-8"));
    init.apply(res)
}

/// A full document gets a doctype; fragments (frame content) are sent as-is.
fn with_doctype(mut html: String) -> String {
    let start = html.trim_start().get(..9).unwrap_or("").to_ascii_lowercase();
    if start.starts_with("<html") {
        html.insert_str(0, "<!DOCTYPE html>");
    }
    html
}

fn respond(frames: Frames, report: ErrorReporter, rendered: ssr::Rendered, init: ResponseInit) -> Response {
    let has_frames = rendered.segments.iter().any(|s| matches!(s, Segment::Frame(_)));
    if !has_frames {
        let html: String = rendered
            .segments
            .into_iter()
            .map(|s| match s {
                Segment::Html(h) => h,
                Segment::Frame(_) => String::new(),
            })
            .collect();
        let html = with_doctype(ssr::finalize_html(html, &rendered.styles, &rendered.data));
        return html_response(Body::from(html), init);
    }
    let body = Body::from_task(move |tx| async move {
        // An aborted request drops the frame work (and with it the frame subrequests and
        // their bodies) and fails the body, like Remix's streams piped with the request
        // signal: the signal is checked first, so work finishing as the abort lands loses.
        let outcome = {
            let mut aborted = frames.signal.until_aborted();
            let mut run = std::pin::pin!(stream_with_frames(frames, rendered, |chunk| tx.send(chunk)));
            std::future::poll_fn(|cx| {
                if Pin::new(&mut aborted).poll(cx).is_ready() {
                    return Poll::Ready(Err(AbortError));
                }
                run.as_mut().poll(cx).map(Ok)
            })
            .await
        };
        let error: BoxError = match outcome {
            Ok(Ok(())) => return,
            Ok(Err(error)) => {
                report(&error);
                error
            }
            Err(aborted) => aborted.into(),
        };
        tx.abort(error);
    });
    html_response(body, init)
}

/// What finished: a frame's first part (inline content and the rest of its stream), or
/// the next chunk of a frame's streamed templates.
enum Done {
    Frame { index: usize, frame: Result<(String, Option<Body>), BoxError> },
    Chunk { rest: Body, chunk: Result<Option<String>, BoxError> },
}

type Work = Pin<Box<dyn Future<Output = Done> + Send>>;

fn next_chunk(mut rest: Body) -> Work {
    Box::pin(async move {
        let chunk = match rest.chunk().await {
            Some(Ok(bytes)) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Some(Err(error)) => Err(error),
            None => Ok(None),
        };
        Done::Chunk { rest, chunk }
    })
}

/// Every frame in the page, including frames inside other frames' fallbacks, in document
/// order.
fn collect_frames<'a>(segments: &'a [Segment], out: &mut Vec<&'a ssr::FrameSegment>) {
    for seg in segments {
        if let Segment::Frame(f) = seg {
            out.push(f);
            if let Some(fallback) = &f.fallback {
                collect_frames(fallback, out);
            }
        }
    }
}

/// Write the page: blocking frames inlined, non-blocking ones showing their fallback
/// (whose own frames are written the same way). Non-blocking frame ids go to `streamed`.
fn write_page(segments: &[Segment], contents: &mut std::collections::HashMap<String, String>, html: &mut String, streamed: &mut Vec<String>) {
    for seg in segments {
        match seg {
            Segment::Html(h) => html.push_str(h),
            Segment::Frame(f) => {
                html.push_str("<!--rmx:f:");
                html.push_str(&f.id);
                html.push_str("-->");
                match &f.fallback {
                    None => html.push_str(contents.get(&f.id).map_or("", String::as_str)),
                    Some(fallback) => {
                        write_page(fallback, contents, html, streamed);
                        streamed.push(f.id.clone());
                    }
                }
                html.push_str("<!--/rmx:f-->");
            }
        }
    }
}

async fn stream_with_frames(frames_in: Frames, rendered: ssr::Rendered, mut send: impl FnMut(String) -> bool) -> Result<(), BoxError> {
    let ssr::Rendered { segments, mut data, styles } = rendered;
    let mut frames = Vec::new();
    collect_frames(&segments, &mut frames);
    let blocking = |i: usize| frames[i].fallback.is_none();

    // Start every frame at once; blocking ones must finish before the first byte.
    let mut work: Vec<Work> = Vec::new();
    for (index, f) in frames.iter().enumerate() {
        let fut = fetch_frame(frames_in.clone(), f.src.clone(), f.name.clone());
        work.push(Box::pin(async move { Done::Frame { index, frame: fut.await } }));
    }
    let mut contents = std::collections::HashMap::new();
    // Template streams of frames whose content went out already.
    let mut early_rests: Vec<Body> = Vec::new();
    let mut blocking_left = (0..frames.len()).filter(|&i| blocking(i)).count();
    while blocking_left > 0 {
        let Some(done) = next_done(&mut work).await else { break };
        if let Done::Frame { index, frame } = done {
            let (head, rest) = frame?;
            if blocking(index) {
                blocking_left -= 1;
            }
            contents.insert(frames[index].id.clone(), head);
            early_rests.extend(rest);
        }
    }
    // Inlined frame content brings its own styles and hydration data; they belong to the
    // page (head and the one rmx-data script), not inside the frame region.
    let mut styles = styles;
    for html in contents.values_mut() {
        let (body, frame_styles, frame_data) = split_frame_html(std::mem::take(html));
        *html = body;
        for (class, css) in frame_styles {
            if !styles.iter().any(|(c, _)| *c == class) {
                styles.push((class, css));
            }
        }
        if let Some(d) = frame_data {
            data.merge(d);
        }
    }

    let mut html = String::new();
    let mut streamed = Vec::new();
    // Statuses stand as rendered: blocking frames are resolved, frames with a fallback
    // (here or inside inlined frame content) are pending until their template arrives.
    write_page(&segments, &mut contents, &mut html, &mut streamed);
    let html = with_doctype(ssr::finalize_html(html, &styles, &data));
    let document = html.starts_with("<!DOCTYPE");
    let (mut head, tail) = split_body_end(html);
    let more = !streamed.is_empty() || !early_rests.is_empty() || !work.is_empty();
    if !more {
        // Nothing streams: the page is one chunk.
        head.push_str(&tail);
        send(head);
        return Ok(());
    }
    head.push_str(ssr::flush_marker(document));
    if !send(head) {
        return Ok(());
    }

    // Templates for non-blocking frames that finished early (and nested frames'
    // templates), then the rest as they finish.
    for id in &streamed {
        if let Some(content) = contents.get(id) {
            if !send(template(id, content)) {
                return Ok(());
            }
        }
    }
    work.extend(early_rests.into_iter().map(next_chunk));
    while let Some(done) = next_done(&mut work).await {
        match done {
            Done::Frame { index, frame } => {
                let (head, rest) = frame?;
                if !send(template(&frames[index].id, &head)) {
                    return Ok(());
                }
                if let Some(rest) = rest {
                    work.push(next_chunk(rest));
                }
            }
            Done::Chunk { rest, chunk } => {
                if let Some(chunk) = chunk? {
                    if !send(chunk) {
                        return Ok(());
                    }
                    work.push(next_chunk(rest));
                }
            }
        }
    }
    if !tail.is_empty() {
        send(tail);
    }
    Ok(())
}

/// Take the leading `<style data-rmx-style>` tags (in the `<head>` a rendered fragment
/// starts with) and the `rmx-data` script out of a rendered frame fragment.
fn split_frame_html(html: String) -> (String, Vec<(String, String)>, Option<ssr::HydrationData>) {
    const STYLE_OPEN: &str = "<style data-rmx-style=\"";
    const DATA_OPEN: &str = "<script type=\"application/json\" id=\"rmx-data\">";
    let take_styles = |mut rest: &str, styles: &mut Vec<(String, String)>| -> usize {
        let start = rest.len();
        while let Some(after) = rest.strip_prefix(STYLE_OPEN) {
            let Some(q) = after.find("\">") else { break };
            let Some(end) = after.find("</style>") else { break };
            styles.push((after[..q].to_string(), after[q + 2..end].to_string()));
            rest = &after[end + 8..];
        }
        start - rest.len()
    };
    let mut styles = Vec::new();
    let mut body = match html.strip_prefix("<head>").and_then(|h| h.find("</head>").map(|end| (&h[..end], &h[end + 7..]))) {
        Some((head, after)) => {
            let used = take_styles(head, &mut styles);
            match &head[used..] {
                "" => after.to_string(),
                kept => format!("<head>{kept}</head>{after}"),
            }
        }
        None => {
            let used = take_styles(&html, &mut styles);
            html[used..].to_string()
        }
    };
    let mut data = None;
    if let Some(start) = body.find(DATA_OPEN) {
        if let Some(len) = body[start..].find("</script>") {
            let json = body[start + DATA_OPEN.len()..start + len].replace("\\u003c", "<");
            data = ssr::HydrationData::from_json(&json);
            body.replace_range(start..start + len + 9, "");
        }
    }
    (body, styles, data)
}

fn template(id: &str, content: &str) -> String {
    format!("<template id=\"{id}\">{}<!--{}--></template>", ssr::escape_template_content(content), ssr::TEMPLATE_END)
}

/// Split before the last `</body>` so streamed templates land inside the body.
fn split_body_end(mut html: String) -> (String, String) {
    match html.to_ascii_lowercase().rfind("</body>") {
        Some(pos) => {
            let tail = html.split_off(pos);
            (html, tail)
        }
        None => (html, String::new()),
    }
}

/// GET a frame through the router (Remix's `resolveFrame`), following redirects. Error
/// responses with a body are used as-is, so an application's error UI shows up inside the
/// frame.
///
/// Returns the frame's content up to its flush marker (without doctype markup, since it
/// goes inside a page), plus the rest of its body (the templates of its own non-blocking
/// frames) when it streams.
async fn fetch_frame(frames: Frames, src: String, name: Option<String>) -> Result<(String, Option<Body>), BoxError> {
    let mut headers = frames.headers.clone();
    // Named frames tell the handler which frame they are, like browser frame requests.
    match name.as_deref().and_then(|n| HeaderValue::from_str(n).ok()) {
        Some(v) => headers.insert(HeaderName::from_static(TARGET_HEADER), v),
        None => headers.remove(TARGET_HEADER),
    };
    let start = frames.base.join(&src).map_err(|e| format!("Invalid frame src {src:?}: {e}"))?;
    let origin = frames.base.origin();
    let mut url = start.clone();
    let mut redirects = 0;
    let res = loop {
        if url.origin() != origin {
            headers = cross_origin_headers(&headers);
        }
        let mut req = http::Request::builder().method(Method::GET).uri(url.href()).body(Body::empty())?;
        *req.headers_mut() = headers.clone();
        req.extensions_mut().insert(frames.signal.clone());
        req.extensions_mut().insert(InternalFrameRequest);
        let res = frames.router.fetch(req).await;
        let location = res.headers().get(header::LOCATION).and_then(|v| v.to_str().ok());
        let next = match location {
            Some(location) if matches!(res.status().as_u16(), 301 | 302 | 303 | 307 | 308) => url.join(location).ok(),
            _ => None,
        };
        let Some(next) = next else { break res };
        if redirects == MAX_FRAME_REDIRECTS {
            return Err(format!("Too many frame redirects while resolving {}", start.href()).into());
        }
        redirects += 1;
        url = next;
    };
    if !is_html(&res) {
        return Err("Frame response must be HTML".into());
    }
    if !res.status().is_success() && res.body().is_empty_hint() {
        let reason = res.status().canonical_reason().unwrap_or("");
        return Err(format!("Failed to resolve frame: {} {reason}", res.status().as_u16()).trim_end().to_string().into());
    }
    let mut body = res.into_body();
    let mut head = String::new();
    while let Some(bytes) = body.chunk().await {
        head.push_str(&String::from_utf8_lossy(&bytes?));
        if let Some((start, _)) = ssr::find_flush_marker(&head) {
            head.truncate(start);
            return Ok((ssr::strip_doctype(&head).into_owned(), Some(body)));
        }
    }
    Ok((ssr::strip_doctype(&head).into_owned(), None))
}

/// Remix's `isHtmlResponse`: the content type is `text/html`, with any parameters.
fn is_html(res: &Response) -> bool {
    let content_type = res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
    content_type.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("text/html")
}

/// Wait for the next piece of work to finish (in any order).
fn next_done(pending: &mut Vec<Work>) -> impl Future<Output = Option<Done>> + '_ {
    std::future::poll_fn(move |cx: &mut TaskContext<'_>| {
        if pending.is_empty() {
            return Poll::Ready(None);
        }
        for i in 0..pending.len() {
            if let Poll::Ready(out) = pending[i].as_mut().poll(cx) {
                drop(pending.swap_remove(i));
                return Poll::Ready(Some(out));
            }
        }
        Poll::Pending
    })
}
#[cfg(test)]
mod render_ui_tests;
#[cfg(test)]
mod stream_tests;
#[cfg(test)]
mod tests;
