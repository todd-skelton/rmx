//! Client-only rmx apps (Remix's `remix/spa`).
//!
//! The same router that would run on a server runs in the page. Route handlers return
//! `ctx.render(node)` responses; [`run`] sends the current URL and every same-origin link
//! and form navigation through the router and renders the resulting node into the document,
//! using the browser runtime's top frame. Responses keep the fetch contract (status,
//! headers, redirects), so middleware works as it does on a server.
//!
//! ```ignore
//! let mut router = Router::new();
//! router.get("/", |ctx: Context| async move { ctx.render(rsx! { <Home/> }) });
//! rmx_spa::run(router, rmx_spa::RunOptions { fallback: Some(rsx! { <Loading/> }) });
//! ```

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use http::{header, HeaderValue, StatusCode};
use rmx::Node;
use rmx_fetch::{Body, Response};
use rmx_router::Context;

/// Status and headers for [`Render::render_with`].
#[derive(Clone, Debug, Default)]
pub struct ResponseInit {
    pub status: Option<StatusCode>,
    pub headers: Vec<(header::HeaderName, HeaderValue)>,
}

impl ResponseInit {
    pub fn status(status: StatusCode) -> ResponseInit {
        ResponseInit { status: Some(status), ..ResponseInit::default() }
    }
}

/// `ctx.render(node)` for routes running in the browser.
pub trait Render {
    /// A `200` response carrying `node` for the SPA runtime to render.
    fn render(&self, node: Node) -> Response {
        self.render_with(node, ResponseInit::default())
    }

    /// A response carrying `node`, with a status and headers.
    fn render_with(&self, node: Node, init: ResponseInit) -> Response;
}

impl Render for Context {
    fn render_with(&self, node: Node, init: ResponseInit) -> Response {
        spa_response(node, init)
    }
}

/// The key a response carries to find its node (nodes are not `Send`, so they stay in a
/// thread-local table rather than in the response).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SpaNode(u64);

thread_local! {
    static NODES: RefCell<HashMap<u64, Node>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u64> = const { Cell::new(0) };
}

/// A bodyless response associated with a renderable node (Remix's `spaResponse.create`).
pub fn spa_response(node: Node, init: ResponseInit) -> Response {
    let id = NEXT.with(|n| {
        n.set(n.get() + 1);
        n.get()
    });
    NODES.with(|nodes| nodes.borrow_mut().insert(id, node));
    let mut res = Response::new(Body::empty());
    *res.status_mut() = init.status.unwrap_or(StatusCode::OK);
    for (name, value) in init.headers {
        res.headers_mut().append(name, value);
    }
    res.extensions_mut().insert(SpaNode(id));
    res
}

/// The node a response from [`spa_response`] carries. Each node can be taken once.
pub fn take_node(res: &Response) -> Option<Node> {
    let SpaNode(id) = *res.extensions().get::<SpaNode>()?;
    NODES.with(|nodes| nodes.borrow_mut().remove(&id))
}

/// Drop a response's node without rendering it.
fn discard(res: &Response) {
    let _ = take_node(res);
}

const MAX_REDIRECTS: usize = 10;

/// Send a frame request through the router, following redirects like `fetch` does: a
/// `303` (and a `301`/`302` after a POST) turns into a `GET` without a body.
pub async fn resolve(router: &rmx_router::Router, request: rmx::FrameRequest) -> Result<rmx::FrameResponse, String> {
    let mut url = rmx_url::Url::parse(&request.url).map_err(|e| format!("invalid frame URL {}: {e}", request.url))?;
    let origin = url.origin();
    let mut method = request.method.to_ascii_uppercase();
    let mut form = request.form;
    let mut redirected = None;
    for _ in 0..=MAX_REDIRECTS {
        let mut builder = http::Request::builder().method(method.as_str()).uri(url.href()).header(header::ACCEPT, "text/html");
        if let Some(target) = &request.target {
            builder = builder.header("x-remix-target", target.as_str());
        }
        let body = match (&form, method.as_str()) {
            (Some(fields), m) if m != "GET" && m != "HEAD" => {
                builder = builder.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
                let mut params = rmx_url::SearchParams::new();
                for (k, v) in fields {
                    params.append(k, v);
                }
                Body::from(params.to_string())
            }
            _ => Body::empty(),
        };
        let req = builder.body(body).map_err(|e| e.to_string())?;
        let res = router.fetch(req).await;
        let status = res.status().as_u16();
        if !matches!(status, 301 | 302 | 303 | 307 | 308) {
            let node = take_node(&res).ok_or_else(|| format!("expected an rmx SPA response from {} ({status})", url.href()))?;
            return Ok(rmx::FrameResponse { content: rmx::FrameContent::Node(node), url: redirected });
        }
        discard(&res);
        let location = res.headers().get(header::LOCATION).and_then(|v| v.to_str().ok()).ok_or("redirect without Location")?;
        let next = url.join(location).map_err(|e| e.to_string())?;
        if next.origin() != origin {
            return Err("SPA routes cannot redirect to another origin".into());
        }
        if (status == 303 && method != "GET" && method != "HEAD") || (matches!(status, 301 | 302) && method == "POST") {
            method = "GET".into();
            form = None;
        }
        url = next;
        redirected = Some(url.href());
    }
    Err(format!("SPA route exceeded {MAX_REDIRECTS} redirects"))
}

/// Options for [`run`].
#[derive(Default)]
pub struct RunOptions {
    /// Shown while the first route loads.
    pub fallback: Option<Node>,
}

/// Start a client-rendered app: render `fallback`, then the route for the current URL,
/// and route every same-origin navigation through `router`.
#[cfg(target_arch = "wasm32")]
pub fn run(router: rmx_router::Router, options: RunOptions) {
    use std::rc::Rc;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    let router = Rc::new(router);
    let resolve_frame: rmx::ResolveFrame = Rc::new(move |request| {
        let router = router.clone();
        Box::pin(async move { resolve(&router, request).await })
    });
    let fallback = options.fallback;
    let start = Closure::once_into_js(move || {
        let top = rmx::Frames.top();
        if let Some(fallback) = fallback {
            top.replace(fallback);
        }
        top.reload();
    });
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        let _ = document.add_event_listener_with_callback("rmx:ready", start.unchecked_ref());
    }
    rmx::run(rmx::RunOptions { resolve_frame: Some(resolve_frame), ..Default::default() });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmx::rsx;
    use rmx_router::Router;

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread().build().expect("runtime").block_on(f)
    }

    fn request(url: &str, method: &str, form: Option<Vec<(String, String)>>) -> rmx::FrameRequest {
        rmx::FrameRequest { url: url.into(), method: method.into(), form, target: None }
    }

    fn text(response: rmx::FrameResponse) -> String {
        match response.content {
            rmx::FrameContent::Node(node) => rmx::render_to_string(&node),
            rmx::FrameContent::Html(html) => html,
        }
    }

    #[test]
    fn routes_render_nodes_and_follow_redirects() {
        let mut router = Router::new();
        router.get("/", |ctx: Context| async move { ctx.render(rsx! { <h1>"Home"</h1> }) });
        router.get("/greet", |ctx: Context| async move {
            let name = ctx.url().search_params().get("name").unwrap_or("friend").to_string();
            ctx.render(rsx! { <h1>{format!("Hello, {name}!")}</h1> })
        });
        router.post("/greet", |mut ctx: Context| async move {
            let form = ctx.take_body().form().await.expect("form");
            let name = form.get("name").unwrap_or("friend").to_string();
            rmx_fetch::response::redirect_with(&format!("/greet?name={name}"), StatusCode::SEE_OTHER)
        });
        router.get("/plain", |_: Context| async { "not a node" });

        let home = block_on(resolve(&router, request("http://localhost/", "GET", None))).expect("home");
        assert!(home.url.is_none());
        assert_eq!(text(home), "<h1>Home</h1>");

        let form = Some(vec![("name".to_string(), "Ada".to_string())]);
        let greet = block_on(resolve(&router, request("http://localhost/greet", "POST", form))).expect("greet");
        assert_eq!(greet.url.as_deref(), Some("http://localhost/greet?name=Ada"));
        assert_eq!(text(greet), "<h1>Hello, Ada!</h1>");

        let plain = block_on(resolve(&router, request("http://localhost/plain", "GET", None)));
        assert!(plain.is_err(), "a response without a node cannot be rendered");
    }

    #[test]
    fn status_and_headers_carry_through() {
        let res = spa_response(
            rsx! { <p>"Missing"</p> },
            ResponseInit { status: Some(StatusCode::NOT_FOUND), headers: vec![(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))] },
        );
        assert_eq!(res.status(), 404);
        assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(take_node(&res).map(|n| rmx::render_to_string(&n)).as_deref(), Some("<p>Missing</p>"));
        assert!(take_node(&res).is_none(), "a node is taken once");
    }
}
