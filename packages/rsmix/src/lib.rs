//! Remix 3 for Rust, distributed as one crate (the `remix` package and its subpath exports).
//!
//! The component model (`remix/component`) is at the root: [`rsx!`], [Component],
//! [Handle], [`css`], [`on`], [`link`], [Frame], [`run`] and the rest. Everything else
//! is a module named after its Remix subpath:
//!
//! | Remix | rsmix | |
//! | --- | --- | --- |
//! | `remix/component` | `rsmix` | browser and server |
//! | `remix/ui` | [`rsmix::ui`](ui) | browser and server, feature `ui` (default) |
//! | `remix/route-pattern` | [`rsmix::route_pattern`](route_pattern) | browser and server |
//! | `remix/data-schema` | [`rsmix::data_schema`](data_schema) | browser and server |
//! | `remix/headers` | [`rsmix::headers`](headers) | browser and server |
//! | `remix/mime` | [`rsmix::mime`](mime) | browser and server |
//! | `URL`, `URLSearchParams` | [`rsmix::url`](url) | browser and server |
//! | `remix/router` (fetch-router) | [`rsmix::router`](router) | browser (route maps, hrefs) and server |
//! | `remix/response/*` (`html`, `redirect`, `file`, `compress`) | [`rsmix::response`](response) | browser and server |
//! | `Request`, `Response`, `FormData`, `AbortSignal` (the Fetch API) | [`rsmix::fetch`](fetch) | browser (types) and server |
//! | `remix/middleware/auth` | `rsmix::middleware::auth` | server |
//! | `remix/middleware/compression` | `rsmix::middleware::compression` | server |
//! | `remix/middleware/cop` | `rsmix::middleware::cop` | server |
//! | `remix/middleware/cors` | `rsmix::middleware::cors` | server |
//! | `remix/middleware/csrf` | `rsmix::middleware::csrf` | server |
//! | `remix/middleware/form-data` | `rsmix::middleware::form_data` | server |
//! | `remix/middleware/logger` | `rsmix::middleware::logger` | server |
//! | `remix/middleware/method-override` | `rsmix::middleware::method_override` | server |
//! | `remix/middleware/session` | `rsmix::middleware::session` | server |
//! | `remix/middleware/static` | `rsmix::middleware::static_files` | server |
//! | the browser side of `npm run hmr` | `rsmix::middleware::live_reload` (page reloads under `rsmix dev`) | server |
//! | `remix/middleware/render` | `rsmix::middleware::render` | server |
//! | `remix/middleware/async-context` | `rsmix::middleware::async_context` (lives in fetch-router: the router's `Context` keeps its scope in step) | server |
//! | `remix/node-fetch-server` | `rsmix::node_fetch_server` | server |
//! | `remix/auth` | `rsmix::auth` | server |
//! | `remix/cookie` | `rsmix::cookie` | server |
//! | `remix/session` | `rsmix::session` | server |
//! | `remix/assets` | `rsmix::assets` | server |
//! | `spaResponse`, client-side routing | [`rsmix::spa`](spa) | browser and server, feature `spa` (default) |
//! | `remix/fetch-proxy` | `rsmix::fetch_proxy` | server |
//! | `remix/file-storage` | `rsmix::file_storage` | server |
//! | `remix/lazy-file` | [`rsmix::lazy_file`](lazy_file) | browser and server |
//! | `remix/fs` | `rsmix::fs` | server |
//! | `remix/form-data-parser`, `remix/multipart-parser` | `rsmix::fetch`, `rsmix::multipart_parser` | server |
//! | `remix/html-template` | `rsmix::html_template` | server |
//! | `remix/tar-parser` | `rsmix::tar_parser` | server |
//! | Web Crypto (Rust has no platform one) | `rsmix::crypto` | server |
//! | `remix/terminal` | `rsmix::terminal` | server |
//! | `remix/cli` (`remix new`, `routes`, `assets`, `test`, `version`) | `rsmix::cli`, the `rsmix` binary | native |
//!
//! Server modules exist when compiling for a native target; a browser (wasm32) build of the
//! same crate sees only the browser column, so one dependency serves both halves of an app.
//!
//! A client crate that only hydrates client entries can leave out the browser runtime's
//! frame loading and soft navigation with `default-features = false` (features `frames`,
//! `navigation`, `spa` and `ui` are on by default). Its `<Frame>`s then render their
//! fallback in the browser, frame reloads load the document, and links and forms are plain
//! browser navigations. The server crate keeps the defaults.
//!
//! ```
//! use rsmix::router::{Context, Router};
//! use rsmix::{rsx, Node};
//!
//! fn page(name: &str) -> Node {
//!     rsx! { <h1>"Hello, "{name.to_string()}</h1> }
//! }
//!
//! let mut router = Router::new();
//! router.get("/hello/:name", |ctx: Context| async move {
//!     rsmix::response::text(rsmix::render_to_string(&page(ctx.param("name").unwrap_or("world"))))
//! });
//! let res = rsmix::fetch::block_on(router.fetch(http::Request::get("http://x/hello/rust").body(rsmix::fetch::Body::empty()).unwrap()));
//! assert!(rsmix::fetch::block_on(res.into_body().text()).unwrap().starts_with("<h1>Hello, rust</h1>"));
//! ```

pub use rsmix_component::*;

pub use rsmix_data_schema as data_schema;
pub use rsmix_fetch as fetch;
pub use rsmix_response as response;
pub use rsmix_headers as headers;
pub use rsmix_lazy_file as lazy_file;
pub use rsmix_mime as mime;
pub use rsmix_route_pattern as route_pattern;
pub use rsmix_fetch_router as router;
#[cfg(feature = "spa")]
pub use rsmix_spa as spa;
#[cfg(feature = "ui")]
pub use rsmix_ui as ui;
pub use rsmix_url as url;

#[cfg(not(target_arch = "wasm32"))]
pub use {
    rsmix_assets as assets, rsmix_auth as auth, rsmix_cli as cli, rsmix_cookie as cookie, rsmix_crypto as crypto, rsmix_fetch_proxy as fetch_proxy,
    rsmix_file_storage as file_storage, rsmix_fs as fs, rsmix_html_template as html_template, rsmix_multipart_parser as multipart_parser,
    rsmix_node_fetch_server as node_fetch_server, rsmix_session as session, rsmix_tar_parser as tar_parser, rsmix_terminal as terminal,
};

/// Remix's `remix/middleware/*` subpaths. Each is its own crate, `rsmix-<name>-middleware`
/// under `packages/`, like Remix's `<name>-middleware` packages. Every module is
/// re-exported under its name, with its middleware function beside it, so
/// `rsmix::middleware::session(..)` and `rsmix::middleware::session::SessionExt` both work.
///
/// | Module | Remix package |
/// | --- | --- |
/// | [`auth`](rsmix_auth_middleware) | `remix/middleware/auth` (and `completeAuth` from `remix/auth`) |
/// | [`compression`](rsmix_compression_middleware) | `remix/middleware/compression` |
/// | [`cop`](rsmix_cop_middleware) | `remix/middleware/cop` |
/// | [`cors`](rsmix_cors_middleware) | `remix/middleware/cors` |
/// | [`csrf`](rsmix_csrf_middleware) | `remix/middleware/csrf` |
/// | [`async_context`](rsmix_fetch_router::async_context) | `remix/middleware/async-context` |
/// | [`render`](rsmix_render_middleware) | `remix/middleware/render` |
/// | [`form_data`](rsmix_form_data_middleware) | `remix/middleware/form-data` |
/// | [`live_reload`](rsmix_live_reload_middleware) | the browser side of `npm run hmr` (page reloads under `rsmix dev`) |
/// | [`logger`](rsmix_logger_middleware) | `remix/middleware/logger` |
/// | [`method_override`](rsmix_method_override_middleware) | `remix/middleware/method-override` |
/// | [`session`](rsmix_session_middleware) | `remix/middleware/session` |
/// | [`static_files`](rsmix_static_middleware) | `remix/middleware/static` (`static` is a Rust keyword) |
#[cfg(not(target_arch = "wasm32"))]
pub mod middleware {
    pub use rsmix_fetch_router::async_context;
    pub use rsmix_fetch_router::async_context::async_context;
    pub use rsmix_render_middleware as render;
    pub use rsmix_render_middleware::{render, RenderOptions};
    pub use rsmix_auth_middleware as auth;
    pub use rsmix_auth_middleware::{auth, require_auth, Auth, AuthExt};
    pub use rsmix_compression_middleware as compression;
    pub use rsmix_compression_middleware::compression;
    pub use rsmix_cop_middleware as cop;
    pub use rsmix_cop_middleware::{cop, Cop};
    pub use rsmix_cors_middleware as cors;
    pub use rsmix_cors_middleware::{cors, Cors};
    pub use rsmix_csrf_middleware as csrf;
    pub use rsmix_csrf_middleware::{csrf, csrf_token, Csrf};
    pub use rsmix_form_data_middleware as form_data;
    pub use rsmix_form_data_middleware::{form_data, FormDataExt};
    pub use rsmix_live_reload_middleware as live_reload;
    pub use rsmix_live_reload_middleware::live_reload;
    pub use rsmix_logger_middleware as logger;
    pub use rsmix_logger_middleware::logger;
    pub use rsmix_method_override_middleware as method_override;
    pub use rsmix_method_override_middleware::method_override;
    pub use rsmix_session_middleware as session;
    pub use rsmix_session_middleware::{session, SessionExt};
    pub use rsmix_static_middleware as static_files;
    pub use rsmix_static_middleware::{static_files, AcceptRanges, AcceptRangesFn, Digest, DigestFn, ETag, StaticOptions};
}
