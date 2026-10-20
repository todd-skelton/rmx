//! Server for the frames demo: `cargo run -p frames-demo` (after `build-client.ps1`).
//!
//! Env: `HOST` (127.0.0.1), `PORT` (44110), `PUBLIC_DIR` (examples/frames/public).

#[cfg(not(target_arch = "wasm32"))]
mod server;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    server::main();
}

#[cfg(target_arch = "wasm32")]
fn main() {}
