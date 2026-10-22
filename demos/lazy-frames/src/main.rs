//! Server for the lazy-frames demo: `cargo run -p lazy-frames-demo`.
//!
//! Env: `HOST` (127.0.0.1), `PORT` (44100), `DEMO_DIR` (demos/lazy-frames),
//! `RMX_ENV=production` (cacheable frame responses), `DEMO_LATENCY_MS` (520).

#[cfg(not(target_arch = "wasm32"))]
mod server;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    server::main();
}

#[cfg(target_arch = "wasm32")]
fn main() {}
