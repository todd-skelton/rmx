//! The Remix unpkg demo, in rmx: browse any npm package's files. Shows tarball parsing
//! (rmx-tar-parser), a file-storage tarball cache, `html!` templates and the `render_with`
//! escape hatch for a custom renderer, all server-side with no client code.

pub mod fetch;
pub mod format_bytes;
pub mod mime_type;
pub mod npm;
pub mod package_browser;
mod server;
pub mod ui;

pub use server::{router, ROUTES};
