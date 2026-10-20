//! Request-scoped rendering for rmx-router (Remix's `render-middleware`).
//!
//! `ctx.render(node)` turns a component tree into an HTML [`Response`]. `<Frame>`s in the
//! tree resolve through the current router, as internal `GET` subrequests that carry the
//! original request's cookies and authorization:
//!
//! - A frame without a `fallback` is blocking: the response waits for it and inlines it.
//! - A frame with a `fallback` sends the fallback first; its content streams later as a
//!   `<template id>` chunk that the browser runtime swaps in.
//!
//! Requests from the browser runtime (`X-Remix-Frame: true`) get the same treatment, so a
//! frame reload or a soft navigation receives HTML the runtime can reconcile.
//!
//! ```ignore
//! use rmx_render::Render;
//!
//! router.get("/", |ctx: Context| async move {
//!     ctx.render(rsx! { <Layout><Frame src="/activity" fallback={rsx! { <p>"Loading"</p> }}/></Layout> })
//! });
//! ```

use std::future::Future;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};

use http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use rmx::ssr::{self, FrameSegment, Segment};
use rmx::Node;
use rmx_fetch::{Body, Response};
use rmx_router::{Context, Router};
use rmx_url::Url;

/// Sent by the browser runtime (and internal subrequests) when asking for frame content.
pub const FRAME_HEADER: &str = "x-remix-frame";
/// The frame name a browser frame request targets.
pub const TARGET_HEADER: &str = "x-remix-target";
/// The top frame URL, passed to internal frame subrequests.
pub const TOP_HEADER: &str = "x-remix-top-frame";

/// Status and headers for [`Render::render_with`].
#[derive(Clone, Debug)]
pub struct ResponseInit {
    pub status: StatusCode,
    pub headers: HeaderMap,
}

impl Default for ResponseInit {
    fn default() -> Self {
        ResponseInit { status: StatusCode::OK, headers: HeaderMap::new() }
    }
}

impl ResponseInit {
    pub fn status(status: StatusCode) -> ResponseInit {
        ResponseInit { status, ..ResponseInit::default() }
    }

    pub fn header(mut self, name: &'static str, value: &str) -> ResponseInit {
        if let Ok(v) = HeaderValue::from_str(value) {
            self.headers.append(HeaderName::from_static(name), v);
        }
        self
    }
}

/// `ctx.render(node)` for rmx-router handlers.
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
        let top = self.header(TOP_HEADER).map(str::to_string).unwrap_or_else(|| url.href());
        rmx::set_server_frame_src(&url.href(), &top);
        let rendered = ssr::render(&node);
        drop(node);
        let forward = forwarded_headers(self.headers(), &top);
        respond(self.router().clone(), url, forward, rendered, init)
    }

    fn is_frame_request(&self) -> bool {
        self.header(FRAME_HEADER) == Some("true")
    }

    fn frame_target(&self) -> Option<&str> {
        self.header(TARGET_HEADER)
    }
}

/// Credentials and session headers carried into frame subrequests.
fn forwarded_headers(headers: &HeaderMap, top: &str) -> HeaderMap {
    let mut out = HeaderMap::new();
    for name in [header::COOKIE, header::AUTHORIZATION, header::ACCEPT_LANGUAGE, header::HOST] {
        for v in headers.get_all(&name) {
            out.append(name.clone(), v.clone());
        }
    }
    out.insert(HeaderName::from_static(FRAME_HEADER), HeaderValue::from_static("true"));
    if let Ok(v) = HeaderValue::from_str(top) {
        out.insert(HeaderName::from_static(TOP_HEADER), v);
    }
    out
}

fn html_response(body: Body, init: ResponseInit) -> Response {
    let mut res = Response::new(body);
    *res.status_mut() = init.status;
    res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=UTF-8"));
    for (name, value) in &init.headers {
        res.headers_mut().append(name.clone(), value.clone());
    }
    res
}

/// A full document gets a doctype; fragments (frame content) are sent as-is.
fn with_doctype(mut html: String) -> String {
    let start = html.trim_start().get(..9).unwrap_or("").to_ascii_lowercase();
    if start.starts_with("<html") {
        html.insert_str(0, "<!DOCTYPE html>");
    }
    html
}

fn respond(router: Router, base: Url, forward: HeaderMap, rendered: ssr::Rendered, init: ResponseInit) -> Response {
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
        stream_with_frames(router, base, forward, rendered, |chunk| tx.send(chunk)).await;
    });
    html_response(body, init)
}

type FrameFuture = Pin<Box<dyn Future<Output = (usize, String)> + Send>>;

async fn stream_with_frames(
    router: Router,
    base: Url,
    forward: HeaderMap,
    rendered: ssr::Rendered,
    mut send: impl FnMut(String) -> bool,
) {
    let ssr::Rendered { segments, mut data, styles } = rendered;
    let frames: Vec<(usize, &FrameSegment)> = segments
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Segment::Frame(f) => Some((i, f)),
            Segment::Html(_) => None,
        })
        .collect();

    // Start every frame at once; blocking ones must finish before the first byte.
    let mut pending: Vec<FrameFuture> = frames
        .iter()
        .map(|(i, f)| {
            let i = *i;
            let fut = fetch_frame(router.clone(), base.clone(), f.src.clone(), forward.clone());
            Box::pin(async move { (i, fut.await) }) as FrameFuture
        })
        .collect();
    let blocking = |i: usize| matches!(&segments[i], Segment::Frame(f) if f.fallback.is_none());
    let mut contents: Vec<Option<String>> = vec![None; segments.len()];
    let mut blocking_left = frames.iter().filter(|(i, _)| blocking(*i)).count();
    while blocking_left > 0 {
        let Some((i, html)) = next_done(&mut pending).await else { break };
        if blocking(i) {
            blocking_left -= 1;
        }
        contents[i] = Some(html);
    }

    // The page: blocking frames inlined, non-blocking ones showing their fallback.
    let mut html = String::new();
    let mut streamed = Vec::new();
    for (i, seg) in segments.iter().enumerate() {
        match seg {
            Segment::Html(h) => html.push_str(h),
            Segment::Frame(f) => {
                html.push_str("<!--rmx:f:");
                html.push_str(&f.id);
                html.push_str("-->");
                match (&f.fallback, contents[i].take()) {
                    (None, Some(content)) => html.push_str(&content),
                    (Some(fallback), done) => {
                        html.push_str(fallback);
                        streamed.push((f.id.clone(), done));
                    }
                    (None, None) => {}
                }
                html.push_str("<!--/rmx:f-->");
            }
        }
    }
    for (id, _, _, status) in &mut data.frames {
        if !streamed.iter().any(|(sid, _)| sid == id) {
            *status = "resolved";
        }
    }
    let html = with_doctype(ssr::finalize_html(html, &styles, &data));
    let (head, tail) = split_body_end(html);
    if !send(head) {
        return;
    }

    // Templates for non-blocking frames, in completion order.
    let id_of = |i: usize| match &segments[i] {
        Segment::Frame(f) => f.id.clone(),
        Segment::Html(_) => String::new(),
    };
    for (id, done) in &streamed {
        if let Some(content) = done {
            if !send(template(id, content)) {
                return;
            }
        }
    }
    while let Some((i, content)) = next_done(&mut pending).await {
        if !send(template(&id_of(i), &content)) {
            return;
        }
    }
    send(tail);
}

fn template(id: &str, content: &str) -> String {
    format!("<template id=\"{id}\">{content}<!--{}--></template>", ssr::TEMPLATE_END)
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

/// GET a frame through the router, following redirects. Error responses are used as-is,
/// so an application's error UI shows up inside the frame.
async fn fetch_frame(router: Router, base: Url, src: String, forward: HeaderMap) -> String {
    let mut url = match base.join(&src) {
        Ok(u) => u,
        Err(_) => return String::new(),
    };
    for _ in 0..10 {
        let mut req = http::Request::builder().method(Method::GET).uri(url.href());
        if let Some(h) = req.headers_mut() {
            h.extend(forward.clone());
        }
        let Ok(req) = req.body(Body::empty()) else { return String::new() };
        let res = router.fetch(req).await;
        if res.status().is_redirection() {
            let next = res.headers().get(header::LOCATION).and_then(|v| v.to_str().ok()).and_then(|l| url.join(l).ok());
            if let Some(next) = next {
                url = next;
                continue;
            }
        }
        return res.into_body().text().await.unwrap_or_default();
    }
    String::new()
}

/// Wait for the next frame future to finish (in any order).
fn next_done(pending: &mut Vec<FrameFuture>) -> impl Future<Output = Option<(usize, String)>> + '_ {
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
mod tests;
