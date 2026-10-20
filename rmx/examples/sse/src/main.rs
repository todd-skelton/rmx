//! Server for the sse demo: `cargo run -p sse-demo`.
//!
//! Env: `HOST` (127.0.0.1), `PORT` (44102), `PUBLIC_DIR` (examples/sse/public).

#[cfg(not(target_arch = "wasm32"))]
mod server;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    server::main();
}

#[cfg(target_arch = "wasm32")]
fn main() {}
