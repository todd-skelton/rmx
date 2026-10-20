//! Server for the ui demo gallery: `cargo run -p ui-demo`.
//!
//! Env: `HOST` (127.0.0.1), `PORT` (44105), `PUBLIC_DIR` (examples/ui/public).

#[cfg(not(target_arch = "wasm32"))]
mod server;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    server::main();
}

#[cfg(target_arch = "wasm32")]
fn main() {}
