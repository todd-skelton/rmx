//! Middleware for `rmx-router`, one module per concern, each behind a cargo feature:
//!
//! | Module | Remix package |
//! | --- | --- |
//! | [`static_files`] | `remix/middleware/static` |
//! | [`logger`] | `remix/middleware/logger` |
//! | [`form_data`] | `remix/middleware/form-data` |
//! | [`method_override`] | `remix/middleware/method-override` |
//! | [`compression`] | `remix/middleware/compression` |
//! | [`session`] | `remix/middleware/session` |
//! | [`csrf`] | `remix/middleware/csrf` |
//! | [`cop`] | `remix/middleware/cop` |
//! | [`cors`] | `remix/middleware/cors` |
//! | [`auth`] | `remix/middleware/auth` (and `completeAuth` from `remix/auth`) |
//! | [`live_reload`] | the browser side of `npm run hmr` (page reloads under `rmx dev`) |

#[cfg(feature = "auth")]
pub mod auth;
#[cfg(feature = "compression")]
pub mod compression;
#[cfg(feature = "cop")]
pub mod cop;
#[cfg(feature = "cors")]
pub mod cors;
#[cfg(feature = "csrf")]
pub mod csrf;
#[cfg(feature = "form-data")]
pub mod form_data;
#[cfg(feature = "live-reload")]
pub mod live_reload;
#[cfg(feature = "logger")]
pub mod logger;
#[cfg(feature = "method-override")]
pub mod method_override;
#[cfg(feature = "session")]
pub mod session;
#[cfg(feature = "static")]
pub mod static_files;

#[cfg(feature = "auth")]
pub use auth::{auth, require_auth, Auth, AuthExt};
#[cfg(feature = "compression")]
pub use compression::compression;
#[cfg(feature = "cop")]
pub use cop::{cop, Cop};
#[cfg(feature = "cors")]
pub use cors::{cors, Cors};
#[cfg(feature = "csrf")]
pub use csrf::{csrf, csrf_token, Csrf};
#[cfg(feature = "form-data")]
pub use form_data::{form_data, FormDataExt};
#[cfg(feature = "logger")]
pub use logger::logger;
#[cfg(feature = "method-override")]
pub use method_override::method_override;
#[cfg(feature = "session")]
pub use session::{session, SessionExt};
#[cfg(feature = "static")]
pub use static_files::{file_response, file_response_for, static_files, AcceptRanges, ETag, StaticOptions};

