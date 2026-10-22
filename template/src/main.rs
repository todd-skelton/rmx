//! Server for the starter app: `cargo run -p template-demo`, after building the client into
//! `app/actions/public/pkg` (see the README).
// rmx-template:remove-start
// In this repository the client builds with
// `./scripts/build-client.ps1 -Crate template-demo -Out template/app/actions/public/pkg`.
// rmx-template:remove-end
//! List its routes with `cargo run -p template-demo -- routes` and its browser assets with
//! `cargo run -p template-demo -- assets` (or `assets inspect <url-or-file>`).
//!
//! Env: `HOST` (127.0.0.1), `PORT` (44113), `APP_ROOT` (template),
//! `NODE_ENV=production` for fingerprinted assets.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    rmx::router::routes::routes_command(&*template_demo::routes::ROUTES);
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44113".into());
    let root = std::env::var("APP_ROOT").unwrap_or_else(|_| "template".into());
    let production = std::env::var("NODE_ENV").as_deref() == Ok("production");
    if let Ok(assets) = template_demo::server::asset_server(std::path::Path::new(&root), production) {
        rmx::assets::assets_command(&assets);
    }
    let router = template_demo::server::router(std::path::Path::new(&root), production).expect("asset server");
    let addr = format!("{host}:{port}");
    println!("Server listening on http://{addr}");
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(rmx::serve::listen(&addr, router)).expect("serve");
}

#[cfg(target_arch = "wasm32")]
fn main() {}
