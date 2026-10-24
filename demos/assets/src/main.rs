//! Server for the assets demo: `cargo run -p assets-demo`.
//!
//! Env: `HOST` (127.0.0.1), `PORT` (44104), `ASSETS_ROOT` (demos/assets),
//! `RSMIX_ENV=production` for fingerprinted, immutable asset URLs.

#[cfg(not(target_arch = "wasm32"))]
mod server;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    server::main();
}

#[cfg(target_arch = "wasm32")]
fn main() {}
