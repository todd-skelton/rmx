//! `renderWith(createRenderer)` from `remix/middleware/render`: middleware that puts a
//! per-request renderer for any input type into the request context.
//!
//! ```
//! use rmx_fetch::{block_on, response, Body};
//! use rmx_render::{render_with, RendererExt, ResponseInit};
//! use rmx_router::{Context, Router};
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
use rmx_router::{Context, Middleware, Next};

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
