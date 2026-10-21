//! Server for the frame-navigation demo: `cargo run -p frame-navigation-demo`.
//!
//! Env: `HOST` (127.0.0.1), `PORT` (44111), `PUBLIC_DIR` (examples/frame-navigation/public).

#[cfg(not(target_arch = "wasm32"))]
mod server;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    rmx::router::routes::routes_command(&*frame_navigation_demo::routes::ROUTES);
    server::main();
}

#[cfg(target_arch = "wasm32")]
fn main() {}
