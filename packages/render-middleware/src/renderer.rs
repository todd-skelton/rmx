//! `renderWith(createRenderer)` from `remix/middleware/render`: middleware that puts a
//! per-request renderer for any input type into the request context.
//!
//! ```
//! use rmx_fetch::{block_on, Body};
//! use rmx_response as response;
//! use rmx_render_middleware::{render_with, RendererExt, ResponseInit};
//! use rmx_fetch_router::{Context, Router};
//!
//! struct Page { title: String }
//!
//! let mut router = Router::new();
//! router.use_middleware(render_with(|_ctx: &Context| {
//!     |page: Page, init: ResponseInit| init.apply(response::html(format!("<title>{}</title>", page.title)))
//! }));
//! router.get("/", |ctx: Context| async move { ctx.renderer::<Page>().render(Page { title: "Home".into() }) });
//!
//! let res = block_on(router.fetch(http::Request::get("http://x/").body(Body::empty()).unwrap()));
//! assert_eq!(block_on(res.into_body().text()).unwrap(), "<!DOCTYPE html><title>Home</title>");
//! ```

use std::sync::Arc;

use rmx_fetch::Response;
use rmx_fetch_router::{Context, Middleware, Next};

use crate::ResponseInit;

/// A function that turns `I` into a response (Remix's `Renderer<input>`).
pub struct Renderer<I>(Arc<dyn Fn(I, ResponseInit) -> Response + Send + Sync>);

impl<I> Clone for Renderer<I> {
    fn clone(&self) -> Self {
        Renderer(self.0.clone())
    }
}

impl<I> Renderer<I> {
    pub fn new(render: impl Fn(I, ResponseInit) -> Response + Send + Sync + 'static) -> Renderer<I> {
        Renderer(Arc::new(render))
    }

    pub fn render(&self, input: I) -> Response {
        (self.0)(input, ResponseInit::default())
    }

    pub fn render_with(&self, input: I, init: ResponseInit) -> Response {
        (self.0)(input, init)
    }
}

/// Middleware that calls `create` for each request and stores the renderer it returns.
pub fn render_with<I, R>(create: impl Fn(&Context) -> R + Send + Sync + 'static) -> impl Middleware
where
    I: 'static,
    R: Fn(I, ResponseInit) -> Response + Send + Sync + 'static,
{
    move |mut ctx: Context, next: Next| {
        let renderer = Renderer::<I>::new(create(&ctx));
        ctx.set(renderer);
        next.run(ctx)
    }
}

/// `ctx.renderer::<I>()` reads the renderer [render_with] installed.
pub trait RendererExt {
    /// Panics when no `render_with` middleware for `I` ran, which is a setup bug.
    fn renderer<I: 'static>(&self) -> &Renderer<I>;
}

impl RendererExt for Context {
    fn renderer<I: 'static>(&self) -> &Renderer<I> {
        self.get::<Renderer<I>>()
            .unwrap_or_else(|| panic!("no render_with() middleware installed a renderer for {}", std::any::type_name::<I>()))
    }
}

/// Ports of Remix's `render.test.ts`. The type cases hold by construction: `renderer::<I>()`
/// only renders an `I`. Remix's custom init types become fields of the input.
#[cfg(test)]
mod tests {
    use super::*;
    use rmx_fetch::{block_on, Body};
    use rmx_fetch_router::Router;

    fn fetch(router: &Router, url: &str) -> Response {
        block_on(router.fetch(http::Request::get(url).body(Body::empty()).unwrap()))
    }

    #[test]
    fn adds_a_renderer_to_request_context() {
        let mut router = Router::new().with(render_with(|_: &Context| |value: String, init: ResponseInit| init.apply(Response::new(Body::from(value)))));
        router.get("/", |ctx: Context| async move { ctx.renderer::<String>().render_with("Hello".into(), ResponseInit::status(http::StatusCode::CREATED)) });
        let res = fetch(&router, "https://remix.run/");
        assert_eq!(res.status(), 201);
        assert_eq!(block_on(res.into_body().text()).unwrap(), "Hello");
    }

    #[test]
    fn creates_a_request_scoped_renderer_from_the_current_request_context() {
        let mut router = Router::new().with(render_with(|ctx: &Context| {
            let path = ctx.url().pathname().to_string();
            move |value: &'static str, init: ResponseInit| init.apply(Response::new(Body::from(format!("{path}:{value}"))))
        }));
        router.get("/rendered", |ctx: Context| async move { ctx.renderer::<&'static str>().render("OK") });
        let res = fetch(&router, "https://remix.run/rendered");
        assert_eq!(res.status(), 200);
        assert_eq!(block_on(res.into_body().text()).unwrap(), "/rendered:OK");
    }

    struct Json {
        ok: bool,
        pretty: bool,
    }

    #[test]
    fn preserves_custom_renderer_input_and_options_types() {
        let mut router = Router::new().with(render_with(|_: &Context| {
            |value: Json, init: ResponseInit| {
                let body = if value.pretty { format!("{{\n  \"ok\": {}\n}}", value.ok) } else { format!("{{\"ok\":{}}}", value.ok) };
                let mut res = Response::new(Body::from(body));
                res.headers_mut().insert(http::header::CONTENT_TYPE, http::HeaderValue::from_static("application/json"));
                init.apply(res)
            }
        }));
        router.get("/json", |ctx: Context| async move { ctx.renderer::<Json>().render(Json { ok: true, pretty: true }) });
        let res = fetch(&router, "https://remix.run/json");
        assert_eq!(res.status(), 200);
        assert_eq!(res.headers()["content-type"], "application/json");
        assert_eq!(block_on(res.into_body().text()).unwrap(), "{\n  \"ok\": true\n}");
    }
}
