//! Reload open pages when `rmx dev` restarts the app (the browser half of Remix's
//! `npm run hmr`, without hot module replacement: wasm modules cannot be swapped in place).
//!
//! When the app runs under `rmx dev` (`RMX_DEV_LIVE_RELOAD` is set), [`live_reload`]:
//!
//! - serves `GET /__rmx/live-reload`, an event stream that sends this server's build id and
//!   stays open, and
//! - appends a small script to full HTML documents (bodies that start with a doctype). The
//!   script remembers the first build id it sees; when the server restarts, the stream
//!   reconnects, gets a new id, and the page reloads.
//!
//! Otherwise it passes every request through untouched, so apps can install it always.
//! Install it inside `compression` (after it in `use_middleware` order) so it sees
//! uncompressed HTML; compressed documents are passed through without the script.

use std::sync::Arc;

use http::{header, HeaderValue, Method, StatusCode};
use rmx_fetch::{Body, Response};
use rmx_fetch_router::{BoxFuture, Context, Middleware, Next};

/// The event stream's path.
pub const LIVE_RELOAD_PATH: &str = "/__rmx/live-reload";

/// The environment variable `rmx dev` sets for the app it runs.
pub const LIVE_RELOAD_ENV: &str = "RMX_DEV_LIVE_RELOAD";

/// The script appended to HTML documents.
pub const LIVE_RELOAD_SCRIPT: &str = concat!(
    "<script type=\"module\">",
    "let id;new EventSource('/__rmx/live-reload').onmessage=(e)=>{",
    "if(id===undefined)id=e.data;else if(e.data!==id)location.reload()}",
    "</script>"
);

/// See the [module docs](self).
#[derive(Clone)]
pub struct LiveReload {
    /// `None` when disabled.
    build_id: Option<Arc<str>>,
}

/// Live reload, enabled when `RMX_DEV_LIVE_RELOAD` is set (by `rmx dev`).
pub fn live_reload() -> LiveReload {
    LiveReload::enabled(std::env::var_os(LIVE_RELOAD_ENV).is_some())
}

impl LiveReload {
    /// Live reload that is on or off regardless of the environment.
    pub fn enabled(on: bool) -> LiveReload {
        let build_id = on.then(|| {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
            Arc::from(format!("{}-{nanos}", std::process::id()))
        });
        LiveReload { build_id }
    }

    /// This server's build id, when enabled.
    pub fn build_id(&self) -> Option<&str> {
        self.build_id.as_deref()
    }
}

impl Middleware for LiveReload {
    fn call(&self, ctx: Context, next: Next) -> BoxFuture<Response> {
        let Some(build_id) = self.build_id.clone() else { return Box::pin(next.run(ctx)) };
        if ctx.url().pathname() == LIVE_RELOAD_PATH && *ctx.method() == Method::GET {
            return Box::pin(async move { event_stream(&build_id) });
        }
        let get = matches!(*ctx.method(), Method::GET | Method::HEAD);
        Box::pin(async move {
            let res = next.run(ctx).await;
            if !get || !is_html(&res) || res.headers().contains_key(header::CONTENT_ENCODING) {
                return res;
            }
            let (mut parts, body) = res.into_parts();
            parts.headers.remove(header::CONTENT_LENGTH);
            Response::from_parts(parts, append_to_documents(body))
        })
    }
}

fn event_stream(build_id: &str) -> Response {
    let message = format!("retry: 300\ndata: {build_id}\n\n");
    let body = Body::from_task(move |tx| async move {
        tx.send(message);
        // Stay open; the client reconnects when the server goes away.
        std::future::pending::<()>().await;
    });
    let mut res = Response::new(body);
    *res.status_mut() = StatusCode::OK;
    res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

fn is_html(res: &Response) -> bool {
    res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|ct| ct.trim_start().to_ascii_lowercase().starts_with("text/html"))
}

/// Forward `body` and add the script after the last chunk when the first chunk starts a full
/// document. Fragments (frame and navigation responses) are forwarded unchanged.
fn append_to_documents(mut body: Body) -> Body {
    Body::from_task(move |tx| async move {
        let mut document = None;
        while let Some(chunk) = body.chunk().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(e) => return tx.abort(e),
            };
            if document.is_none() {
                let text = String::from_utf8_lossy(&chunk[..chunk.len().min(64)]).trim_start().to_ascii_lowercase();
                document = Some(text.starts_with("<!doctype html"));
            }
            if !tx.send(chunk) {
                return;
            }
        }
        if document == Some(true) {
            tx.send(LIVE_RELOAD_SCRIPT);
        }
    })
}

#[cfg(test)]
mod tests {
    use rmx_fetch::block_on;
use rmx_response as response;
    use rmx_fetch_router::Router;

    use super::*;

    fn app(on: bool) -> Router {
        let mut router = Router::new().with(LiveReload::enabled(on));
        router.get("/", |_: Context| async { response::html("<html><body><h1>Home</h1></body></html>") });
        router.get("/fragment", |_: Context| async { response::html_stream(Body::from("<p>frame</p>")) });
        router.get("/frame", |_: Context| async {
            let mut res = Response::new(Body::from("<p>frame</p>"));
            res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html"));
            res
        });
        router.get("/text", |_: Context| async { response::text("<!DOCTYPE html>not html") });
        router
    }

    fn get(router: &Router, path: &str) -> (Response, String) {
        let res = block_on(router.fetch_url(&format!("http://localhost{path}")));
        let (parts, body) = res.into_parts();
        let text = block_on(body.text()).unwrap();
        (Response::from_parts(parts, Body::empty()), text)
    }

    #[test]
    fn appends_the_script_to_documents_only() {
        let router = app(true);
        let (res, html) = get(&router, "/");
        assert!(res.headers().get(header::CONTENT_LENGTH).is_none());
        assert_eq!(html, format!("<!DOCTYPE html><html><body><h1>Home</h1></body></html>{LIVE_RELOAD_SCRIPT}"));
        assert_eq!(get(&router, "/frame").1, "<p>frame</p>", "fragments are left alone");
        assert_eq!(get(&router, "/text").1, "<!DOCTYPE html>not html", "only text/html");
        assert!(get(&router, "/fragment").1.ends_with(LIVE_RELOAD_SCRIPT), "html_stream adds a doctype, so it is a document");
    }

    #[test]
    fn serves_the_build_id_as_an_event_stream() {
        let live = LiveReload::enabled(true);
        let id = live.build_id().unwrap().to_string();
        let mut router = Router::new().with(live);
        router.get("/", |_: Context| async { "home" });
        let res = block_on(router.fetch_url(&format!("http://localhost{LIVE_RELOAD_PATH}")));
        assert_eq!(res.headers()[header::CONTENT_TYPE], "text/event-stream");
        let mut body = res.into_body();
        let first = block_on(body.chunk()).unwrap().unwrap();
        assert_eq!(String::from_utf8_lossy(&first), format!("retry: 300\ndata: {id}\n\n"));
        assert_ne!(LiveReload::enabled(true).build_id(), Some(id.as_str()), "each start has its own id");
    }

    #[test]
    fn passes_everything_through_when_disabled() {
        let router = app(false);
        assert_eq!(get(&router, "/").1, "<!DOCTYPE html><html><body><h1>Home</h1></body></html>");
        assert_eq!(get(&router, LIVE_RELOAD_PATH).0.status(), StatusCode::NOT_FOUND);
        assert!(LiveReload::enabled(false).build_id().is_none());
    }
}
