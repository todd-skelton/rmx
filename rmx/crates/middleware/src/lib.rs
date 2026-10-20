//! Middleware for `rmx-router`, one module per concern, each behind a cargo feature:
//!
//! | Module | Remix package |
//! | --- | --- |
//! | [`static_files`] | `remix/middleware/static` |
//! | [`logger`] | `remix/middleware/logger` |
//! | [`form_data`] | `remix/middleware/form-data` |
//! | [`method_override`] | `remix/middleware/method-override` |
//! | [`compression`] | `remix/middleware/compression` |

#[cfg(feature = "compression")]
pub mod compression;
#[cfg(feature = "form-data")]
pub mod form_data;
#[cfg(feature = "logger")]
pub mod logger;
#[cfg(feature = "method-override")]
pub mod method_override;
#[cfg(feature = "static")]
pub mod static_files;

#[cfg(feature = "compression")]
pub use compression::compression;
#[cfg(feature = "form-data")]
pub use form_data::{form_data, FormDataExt};
#[cfg(feature = "logger")]
pub use logger::logger;
#[cfg(feature = "method-override")]
pub use method_override::method_override;
#[cfg(feature = "static")]
pub use static_files::{file_response, static_files, StaticOptions};

pub(crate) mod http_date;
