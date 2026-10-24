//! Client-only rsmix apps (Remix's `remix/spa`).
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
//! rsmix_spa::run(router, rsmix_spa::RunOptions { fallback: Some(rsx! { <Loading/> }) });
//! ```

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use std::sync::Arc;

use http::header;
use rsmix::Node;
use rsmix_fetch::{Body, Response};
use rsmix_fetch_router::{Context, Middleware, Next};

/// Status and headers for [`Render::render_with`] (the one `ResponseInit` in `rsmix-fetch`).
pub use rsmix_response::ResponseInit;

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
        let node = match self.get::<RenderTransform>() {
            Some(transform) => (transform.0)(node, self),
            None => node,
        };
        spa_response(node, init)
    }
}

/// A route node transform.
type Transform = Arc<dyn Fn(Node, &Context) -> Node + Send + Sync>;

/// The transform [`render`] installs.
#[derive(Clone)]
struct RenderTransform(Transform);

/// Remix's SPA `render(transform)` middleware: `ctx.render(node)` renders
/// `transform(node, ctx)` instead, so a layout can wrap every route with the request in hand.
/// `ctx.render` works without it.
pub fn render(transform: impl Fn(Node, &Context) -> Node + Send + Sync + 'static) -> impl Middleware {
    let transform = RenderTransform(Arc::new(transform));
    move |mut ctx: Context, next: Next| {
        ctx.set(transform.clone());
        next.run(ctx)
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
    let mut res = init.apply(Response::new(Body::empty()));
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

/// The boundary of SPA multipart bodies.
const MULTIPART_BOUNDARY: &str = "----rmx-spa-form-boundary";

/// Remix's `normalizeLineBreaks`: form values travel with `\r\n` line breaks.
fn normalize_line_breaks(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n")
}

/// A form's body and content type for `enc_type` (Remix's `getRequestBody`):
/// `multipart/form-data`, `text/plain` (`name=value\r\n` lines), or URL encoding (the default).
fn form_body(fields: &[(String, String)], enc_type: Option<&str>) -> (String, Body) {
    let fields = fields.iter().map(|(k, v)| (normalize_line_breaks(k), normalize_line_breaks(v)));
    match enc_type.map(str::to_ascii_lowercase).as_deref() {
        Some("multipart/form-data") => {
            let mut body = String::new();
            for (name, value) in fields {
                let name = name.replace('"', "%22");
                body.push_str(&format!("--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
            }
            body.push_str(&format!("--{MULTIPART_BOUNDARY}--\r\n"));
            (format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"), Body::from(body))
        }
        Some("text/plain") => ("text/plain".into(), Body::from(fields.map(|(k, v)| format!("{k}={v}\r\n")).collect::<String>())),
        _ => {
            let mut params = rsmix_url::SearchParams::new();
            for (k, v) in fields {
                params.append(&k, &v);
            }
            ("application/x-www-form-urlencoded".into(), Body::from(params.to_string()))
        }
    }
}

/// Send a frame request through the router, following redirects like `fetch` does: a
/// `303` (and a `301`/`302` after a POST) turns into a `GET` without a body or content
/// type, and `307`/`308` repeat the request. A redirect without a `Location` is the
/// response.
pub async fn resolve(router: &rsmix_fetch_router::Router, request: rsmix::FrameRequest) -> Result<rsmix::FrameResponse, String> {
    let mut url = rsmix_url::Url::parse(&request.url).map_err(|e| format!("invalid frame URL {}: {e}", request.url))?;
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
                let (content_type, body) = form_body(fields, request.enc_type.as_deref());
                builder = builder.header(header::CONTENT_TYPE, content_type);
                body
            }
            _ => Body::empty(),
        };
        let req = builder.body(body).map_err(|e| e.to_string())?;
        let res = router.fetch(req).await;
        let status = res.status().as_u16();
        let location = res.headers().get(header::LOCATION).and_then(|v| v.to_str().ok()).map(str::to_string);
        let Some(location) = location.filter(|_| matches!(status, 301 | 302 | 303 | 307 | 308)) else {
            let node = take_node(&res).ok_or_else(|| format!("expected an rsmix SPA response from {} ({status})", url.href()))?;
            return Ok(rsmix::FrameResponse { content: rsmix::FrameContent::Node(node), url: redirected });
        };
        discard(&res);
        let next = url.join(&location).map_err(|e| e.to_string())?;
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
pub fn run(router: rsmix_fetch_router::Router, options: RunOptions) {
    use std::rc::Rc;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    let router = Rc::new(router);
    let resolve_frame: rsmix::ResolveFrame = Rc::new(move |request| {
        let router = router.clone();
        Box::pin(async move { resolve(&router, request).await })
    });
    let fallback = options.fallback;
    let start = Closure::once_into_js(move || {
        let top = rsmix::Frames.top();
        if let Some(fallback) = fallback {
            top.replace(fallback);
        }
        top.reload();
    });
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        let _ = document.add_event_listener_with_callback("rmx:ready", start.unchecked_ref());
    }
    rsmix::run(rsmix::RunOptions { resolve_frame: Some(resolve_frame), ..Default::default() });
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::StatusCode;
    use rsmix::rsx;
    use rsmix_fetch_router::Router;

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread().build().expect("runtime").block_on(f)
    }

    fn request(url: &str, method: &str, form: Option<Vec<(String, String)>>) -> rsmix::FrameRequest {
        rsmix::FrameRequest { url: url.into(), method: method.into(), form, enc_type: None, target: None, signal: rsmix::Signal::never() }
    }

    fn text(response: rsmix::FrameResponse) -> String {
        match response.content {
            rsmix::FrameContent::Node(node) => rsmix::render_to_string(&node),
            rsmix::FrameContent::Html(html) => html,
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
            rsmix_response::redirect_with(&format!("/greet?name={name}"), StatusCode::SEE_OTHER)
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

    fn post(url: &str, fields: &[(&str, &str)], enc_type: Option<&str>) -> rsmix::FrameRequest {
        let form = fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        rsmix::FrameRequest { enc_type: enc_type.map(str::to_string), ..request(url, "POST", Some(form)) }
    }

    /// A router that records each request's method, content type and body, and answers
    /// `/save` with a 303 to `/saved`.
    /// Requests seen: method, content type, body.
    type Seen = std::sync::Arc<std::sync::Mutex<Vec<(String, Option<String>, String)>>>;

    fn recording() -> (Router, Seen) {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut router = Router::new();
        for path in ["/save", "/saved", "/echo"] {
            let log = seen.clone();
            router.any(path, move |mut ctx: Context| {
                let log = log.clone();
                async move {
                    let content_type = ctx.header("content-type").map(str::to_string);
                    let method = ctx.method().to_string();
                    let body = ctx.take_body().text().await.unwrap_or_default();
                    log.lock().unwrap().push((method, content_type, body));
                    if ctx.url().pathname() == "/save" {
                        return rsmix_response::redirect_with("/saved", StatusCode::SEE_OTHER);
                    }
                    ctx.render(rsx! { <p>"Saved"</p> })
                }
            });
        }
        (router, seen)
    }

    // spa.test.browser.tsx, against the resolver the runtime calls.

    #[test]
    fn defaults_post_form_data_to_url_encoding() {
        let (router, seen) = recording();
        block_on(resolve(&router, post("http://localhost/echo", &[("name", "Ada Lovelace")], None))).unwrap();
        assert_eq!(seen.lock().unwrap()[0], ("POST".into(), Some("application/x-www-form-urlencoded".into()), "name=Ada+Lovelace".into()));
    }

    #[test]
    fn encodes_text_plain_form_submissions_with_normalized_line_breaks() {
        let (router, seen) = recording();
        let fields = [("note", "first\nsecond"), ("city", "Paris")];
        block_on(resolve(&router, post("http://localhost/echo", &fields, Some("text/plain")))).unwrap();
        assert_eq!(seen.lock().unwrap()[0], ("POST".into(), Some("text/plain".into()), "note=first\r\nsecond\r\ncity=Paris\r\n".into()));
    }

    #[test]
    fn encodes_multipart_form_submissions() {
        let (router, seen) = recording();
        block_on(resolve(&router, post("http://localhost/echo", &[("name", "Ada"), ("note", "a\rb")], Some("multipart/form-data")))).unwrap();
        let (method, content_type, body) = seen.lock().unwrap()[0].clone();
        assert_eq!(method, "POST");
        assert_eq!(content_type.as_deref(), Some(format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}").as_str()));
        let part = |name: &str, value: &str| format!("--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n");
        assert_eq!(body, format!("{}{}--{MULTIPART_BOUNDARY}--\r\n", part("name", "Ada"), part("note", "a\r\nb")));
    }

    #[test]
    fn drops_the_body_and_its_content_type_when_a_post_redirects_to_get() {
        let (router, seen) = recording();
        let res = block_on(resolve(&router, post("http://localhost/save", &[("name", "Ada")], None))).unwrap();
        assert_eq!(res.url.as_deref(), Some("http://localhost/saved"));
        assert_eq!(text(res), "<p>Saved</p>");
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0], ("POST".into(), Some("application/x-www-form-urlencoded".into()), "name=Ada".into()));
        assert_eq!(seen[1], ("GET".into(), None, String::new()));
    }

    #[test]
    fn follows_same_origin_redirects_with_fetch_method_semantics() {
        let mut router = Router::new();
        let methods = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = methods.clone();
        router.any("/r/:status", move |ctx: Context| {
            log.lock().unwrap().push(ctx.method().to_string());
            let status: u16 = ctx.param("status").unwrap().parse().unwrap();
            async move { rsmix_response::redirect_with("/done", StatusCode::from_u16(status).unwrap()) }
        });
        let log = methods.clone();
        router.any("/done", move |ctx: Context| {
            log.lock().unwrap().push(ctx.method().to_string());
            async move { ctx.render(rsx! { <h1>"Redirected"</h1> }) }
        });
        router.get("/away", |_: Context| async move { rsmix_response::redirect("https://elsewhere.example/") });
        router.get("/bare", |_: Context| async move {
            let mut res = Response::new(Body::empty());
            *res.status_mut() = StatusCode::FOUND;
            res
        });
        for (status, method, then) in [(302, "GET", "GET"), (302, "POST", "GET"), (301, "PUT", "PUT"), (303, "PUT", "GET"), (307, "POST", "POST"), (308, "POST", "POST")] {
            methods.lock().unwrap().clear();
            let req = rsmix::FrameRequest { form: Some(vec![("a".into(), "1".into())]), ..request(&format!("http://localhost/r/{status}"), method, None) };
            let res = block_on(resolve(&router, req)).unwrap();
            assert_eq!(text(res), "<h1>Redirected</h1>");
            assert_eq!(*methods.lock().unwrap(), [method, then], "{status} after {method}");
        }
        let away = block_on(resolve(&router, request("http://localhost/away", "GET", None)));
        assert_eq!(away.err().as_deref(), Some("SPA routes cannot redirect to another origin"));
        let bare = block_on(resolve(&router, request("http://localhost/bare", "GET", None)));
        assert!(bare.err().unwrap().starts_with("expected an rsmix SPA response"), "a redirect without Location is the response");
    }

    #[test]
    fn stops_after_ten_redirects() {
        let mut router = Router::new();
        router.get("/loop", |_: Context| async move { rsmix_response::redirect("/loop") });
        let err = block_on(resolve(&router, request("http://localhost/loop", "GET", None))).err().unwrap();
        assert_eq!(err, "SPA route exceeded 10 redirects");
    }

    #[test]
    fn adds_a_request_aware_node_renderer_to_the_router_context() {
        let mut router = Router::new().with(render(|node, ctx: &Context| {
            let path = ctx.url().pathname().to_string();
            rsx! { <h1>{path} ": " {node}</h1> }
        }));
        router.get("/hello", |ctx: Context| async move { ctx.render_with(rsx! { "Hello" }, ResponseInit::status(StatusCode::CREATED)) });
        router.not_found(|ctx: Context| async move { ctx.render_with(rsx! { "Not Found" }, ResponseInit::status(StatusCode::NOT_FOUND)) });
        let res = block_on(router.fetch(http::Request::get("http://localhost/hello").body(Body::empty()).unwrap()));
        assert_eq!(res.status(), 201);
        assert_eq!(take_node(&res).map(|n| rsmix::render_to_string(&n)).as_deref(), Some("<h1>/hello: Hello</h1>"));
        // The default handler gets the renderer too.
        let missing = block_on(resolve(&router, request("http://localhost/missing", "GET", None))).unwrap();
        assert_eq!(text(missing), "<h1>/missing: Not Found</h1>");
    }

    // component spa.test.tsx

    #[test]
    fn rejects_responses_not_created_by_spa_response() {
        assert!(take_node(&Response::new(Body::from("<p>html</p>"))).is_none());
    }

    #[test]
    fn status_and_headers_carry_through() {
        let res = spa_response(
            rsx! { <p>"Missing"</p> },
            ResponseInit::status(StatusCode::NOT_FOUND).header("cache-control", "no-store"),
        );
        assert_eq!(res.status(), 404);
        assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");
        assert!(res.body().is_empty_hint(), "an SPA response has no body");
        assert_eq!(take_node(&res).map(|n| rsmix::render_to_string(&n)).as_deref(), Some("<p>Missing</p>"));
        assert!(take_node(&res).is_none(), "a node is taken once");
    }
}
