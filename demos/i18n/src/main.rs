//! Server for the i18n demo: `cargo run -p i18n-demo`.
//!
//! Env: `HOST` (127.0.0.1), `PORT` (44107), `PUBLIC_DIR` (demos/i18n/public),
//! `RMX_ENV=production` for a `Secure` locale cookie.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    i18n_demo::server::main();
}

#[cfg(target_arch = "wasm32")]
fn main() {}
