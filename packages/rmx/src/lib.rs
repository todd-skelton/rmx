//! Remix 3 for Rust, distributed as one crate (the `remix` package and its subpath exports).
//!
//! The component model (`remix/component`) is at the root: [`rsx!`], [Component],
//! [Handle], [`css`], [`on`], [`link`], [Frame], [`run`] and the rest. Everything else
//! is a module named after its Remix subpath:
//!
//! | Remix | rmx | |
//! | --- | --- | --- |
//! | `remix/component` | `rmx` | browser and server |
//! | `remix/ui` | [`rmx::ui`](ui) | browser and server, feature `ui` (default) |
//! | `remix/route-pattern` | [`rmx::route_pattern`](route_pattern) | browser and server |
//! | `remix/data-schema` | [`rmx::data_schema`](data_schema) | browser and server |
//! | `remix/headers` | [`rmx::headers`](headers) | browser and server |
//! | `remix/mime` | [`rmx::mime`](mime) | browser and server |
//! | `URL`, `URLSearchParams` | [`rmx::url`](url) | browser and server |
//! | `remix/router` (fetch-router) | [`rmx::router`](router) | browser (route maps, hrefs) and server |
//! | `remix/response`, `Request`, `Response`, `FormData` | [`rmx::fetch`](fetch) | browser (types) and server |
//! | `remix/middleware/auth` | `rmx::middleware::auth` | server |
//! | `remix/middleware/compression` | `rmx::middleware::compression` | server |
//! | `remix/middleware/cop` | `rmx::middleware::cop` | server |
//! | `remix/middleware/cors` | `rmx::middleware::cors` | server |
//! | `remix/middleware/csrf` | `rmx::middleware::csrf` | server |
//! | `remix/middleware/form-data` | `rmx::middleware::form_data` | server |
//! | `remix/middleware/logger` | `rmx::middleware::logger` | server |
//! | `remix/middleware/method-override` | `rmx::middleware::method_override` | server |
//! | `remix/middleware/session` | `rmx::middleware::session` | server |
//! | `remix/middleware/static` | `rmx::middleware::static_files` | server |
//! | the browser side of `npm run hmr` | `rmx::middleware::live_reload` (page reloads under `rmx dev`) | server |
//! | `remix/middleware/render`, `remix/component/server` | `rmx::render` | server |
//! | `remix/node-fetch-server` | `rmx::serve` | server |
//! | `remix/auth` | `rmx::auth` | server |
//! | `remix/cookie` | `rmx::cookie` | server |
//! | `remix/session` | `rmx::session` | server |
//! | `remix/assets` | `rmx::assets` | server |
//! | `spaResponse`, client-side routing | [`rmx::spa`](spa) | browser and server |
//! | `remix/fetch-proxy` | `rmx::fetch_proxy` | server |
//! | `remix/file-storage` | `rmx::file_storage` | server |
//! | `remix/lazy-file` | [`rmx::lazy_file`](lazy_file) | browser and server |
//! | `remix/fs` | `rmx::fs` | server |
//! | `remix/form-data-parser`, `remix/multipart-parser` | `rmx::fetch`, `rmx::multipart_parser` | server |
//! | `remix/html-template` | `rmx::html_template` | server |
//! | `remix/tar-parser` | `rmx::tar_parser` | server |
//! | Web Crypto helpers | `rmx::crypto` | server |
//! | `remix/terminal` | `rmx::terminal` | server |
//! | `remix/cli` (`remix new`, `routes`, `assets`, `test`, `version`) | `rmx::cli`, the `rmx` binary | native |
//!
//! Server modules exist when compiling for a native target; a browser (wasm32) build of the
//! same crate sees only the browser column, so one dependency serves both halves of an app.
//!
//! ```
//! use rmx::router::{Context, Router};
//! use rmx::{rsx, Node};
//!
//! fn page(name: &str) -> Node {
//!     rsx! { <h1>"Hello, "{name.to_string()}</h1> }
//! }
//!
//! let mut router = Router::new();
//! router.get("/hello/:name", |ctx: Context| async move {
//!     rmx::fetch::response::text(rmx::render_to_string(&page(ctx.param("name").unwrap_or("world"))))
//! });
//! let res = rmx::fetch::block_on(router.fetch(http::Request::get("http://x/hello/rust").body(rmx::fetch::Body::empty()).unwrap()));
//! assert!(rmx::fetch::block_on(res.into_body().text()).unwrap().starts_with("<h1>Hello, rust</h1>"));
//! ```

pub use rmx_component::*;

pub use rmx_data_schema as data_schema;
pub use rmx_fetch as fetch;
pub use rmx_headers as headers;
pub use rmx_lazy_file as lazy_file;
pub use rmx_mime as mime;
pub use rmx_route_pattern as route_pattern;
pub use rmx_router as router;
pub use rmx_spa as spa;
#[cfg(feature = "ui")]
pub use rmx_ui as ui;
pub use rmx_url as url;

#[cfg(not(target_arch = "wasm32"))]
pub mod cli;

#[cfg(not(target_arch = "wasm32"))]
pub use {
    rmx_assets as assets, rmx_auth as auth, rmx_cookie as cookie, rmx_crypto as crypto, rmx_fetch_proxy as fetch_proxy,
    rmx_file_storage as file_storage, rmx_fs as fs, rmx_html_template as html_template, rmx_multipart_parser as multipart_parser,
    rmx_render as render, rmx_serve as serve, rmx_session as session, rmx_tar_parser as tar_parser, rmx_terminal as terminal,
};

/// Remix's `remix/middleware/*` subpaths. Each is its own crate, `rmx-<name>-middleware`
/// under `packages/`, like Remix's `<name>-middleware` packages. Every module is
/// re-exported under its name, with its middleware function beside it, so
/// `rmx::middleware::session(..)` and `rmx::middleware::session::SessionExt` both work.
///
/// | Module | Remix package |
/// | --- | --- |
/// | [`auth`](rmx_auth_middleware) | `remix/middleware/auth` (and `completeAuth` from `remix/auth`) |
/// | [`compression`](rmx_compression_middleware) | `remix/middleware/compression` |
/// | [`cop`](rmx_cop_middleware) | `remix/middleware/cop` |
/// | [`cors`](rmx_cors_middleware) | `remix/middleware/cors` |
/// | [`csrf`](rmx_csrf_middleware) | `remix/middleware/csrf` |
/// | [`form_data`](rmx_form_data_middleware) | `remix/middleware/form-data` |
/// | [`live_reload`](rmx_live_reload_middleware) | the browser side of `npm run hmr` (page reloads under `rmx dev`) |
/// | [`logger`](rmx_logger_middleware) | `remix/middleware/logger` |
/// | [`method_override`](rmx_method_override_middleware) | `remix/middleware/method-override` |
/// | [`session`](rmx_session_middleware) | `remix/middleware/session` |
/// | [`static_files`](rmx_static_middleware) | `remix/middleware/static` (`static` is a Rust keyword) |
#[cfg(not(target_arch = "wasm32"))]
pub mod middleware {
    pub use rmx_auth_middleware as auth;
    pub use rmx_auth_middleware::{auth, require_auth, Auth, AuthExt};
    pub use rmx_compression_middleware as compression;
    pub use rmx_compression_middleware::compression;
    pub use rmx_cop_middleware as cop;
    pub use rmx_cop_middleware::{cop, Cop};
    pub use rmx_cors_middleware as cors;
    pub use rmx_cors_middleware::{cors, Cors};
    pub use rmx_csrf_middleware as csrf;
    pub use rmx_csrf_middleware::{csrf, csrf_token, Csrf};
    pub use rmx_form_data_middleware as form_data;
    pub use rmx_form_data_middleware::{form_data, FormDataExt};
    pub use rmx_live_reload_middleware as live_reload;
    pub use rmx_live_reload_middleware::live_reload;
    pub use rmx_logger_middleware as logger;
    pub use rmx_logger_middleware::logger;
    pub use rmx_method_override_middleware as method_override;
    pub use rmx_method_override_middleware::method_override;
    pub use rmx_session_middleware as session;
    pub use rmx_session_middleware::{session, SessionExt};
    pub use rmx_static_middleware as static_files;
    pub use rmx_static_middleware::{static_files, AcceptRanges, AcceptRangesFn, Digest, DigestFn, ETag, StaticOptions};
}
