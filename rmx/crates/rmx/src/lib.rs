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
//! | `remix/middleware/*` | `rmx::middleware` | server |
//! | `remix/middleware/render`, `remix/component/server` | `rmx::render` | server |
//! | `remix/node-fetch-server` | `rmx::serve` | server |
//! | `remix/auth` | `rmx::auth` | server |
//! | `remix/cookie` | `rmx::cookie` | server |
//! | `remix/session` | `rmx::session` | server |
//! | `remix/assets` | `rmx::assets` | server |
//! | `spaResponse`, client-side routing | [`rmx::spa`](spa) | browser and server |
//! | `remix/fetch-proxy` | `rmx::fetch_proxy` | server |
//! | `remix/file-storage` | `rmx::file_storage` | server |
//! | `remix/form-data-parser`, `remix/multipart-parser` | `rmx::fetch`, `rmx::multipart_parser` | server |
//! | `remix/html-template` | `rmx::html_template` | server |
//! | `remix/tar-parser` | `rmx::tar_parser` | server |
//! | Web Crypto helpers | `rmx::crypto` | server |
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
    rmx_file_storage as file_storage, rmx_html_template as html_template, rmx_middleware as middleware, rmx_multipart_parser as multipart_parser,
    rmx_render as render, rmx_serve as serve, rmx_session as session, rmx_tar_parser as tar_parser, rmx_terminal as terminal,
};
