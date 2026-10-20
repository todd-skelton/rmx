//! Static server for the spa demo (like `vite preview`): serves `public/` and answers
//! every other GET with `index.html`, so deep links load the app. Routing happens in the
//! browser. `cargo run -p spa-demo`
//!
//! Env: `HOST` (127.0.0.1), `PORT` (44103), `PUBLIC_DIR` (examples/spa/public).

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use std::path::PathBuf;

    use rmx_middleware::static_files::{file_response, static_files, ETag, StaticOptions};
    use rmx_router::{Context, Router};

    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44103".into());
    let public = PathBuf::from(std::env::var("PUBLIC_DIR").unwrap_or_else(|_| "examples/spa/public".into()));
    let options = StaticOptions { cache_control: Some("no-store".into()), etag: ETag::Off, last_modified: false, ..Default::default() };
    let mut router = Router::new();
    router.use_middleware(static_files(public.clone(), options.clone()));
    let index = public.join("index.html");
    router.not_found(move |ctx: Context| {
        let (index, options) = (index.clone(), options.clone());
        async move { file_response(&index, ctx.headers(), false, &options).unwrap_or_else(|_| rmx_fetch::response::not_found()) }
    });
    let addr = format!("{host}:{port}");
    println!("spa demo on http://{addr}");
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(rmx_serve::listen(&addr, router)).expect("serve");
}

#[cfg(target_arch = "wasm32")]
fn main() {}
