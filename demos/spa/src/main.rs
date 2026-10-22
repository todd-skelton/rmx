//! Static server for the spa demo (like `vite preview`): serves `public/` and answers
//! every other GET with `index.html`, so deep links load the app. Routing happens in the
//! browser. `cargo run -p spa-demo`
//!
//! Env: `HOST` (127.0.0.1), `PORT` (44103), `PUBLIC_DIR` (demos/spa/public).

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use std::path::PathBuf;

    use rmx::response::{self, FileResponseOptions};
    use rmx::fs::{open_lazy_file, OpenLazyFileOptions};
    use rmx::middleware::static_files::{static_files, ETag, StaticOptions};
    use rmx::router::{Context, Router};

    // The browser router's routes (`routes`); this server only serves files.
    rmx::router::routes::routes_command(&*spa_demo::ROUTES);
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44103".into());
    let public = PathBuf::from(std::env::var("PUBLIC_DIR").unwrap_or_else(|_| "demos/spa/public".into()));
    let options = StaticOptions { cache_control: Some("no-store".into()), etag: ETag::Off, last_modified: false, ..Default::default() };
    let mut router = Router::new();
    router.use_middleware(static_files(public.clone(), options));
    let index = public.join("index.html");
    let index_options = FileResponseOptions { cache_control: Some("no-store".into()), etag: ETag::Off, last_modified: false, ..Default::default() };
    router.not_found(move |ctx: Context| {
        let (index, options) = (index.clone(), index_options.clone());
        async move {
            let Ok(file) = open_lazy_file(&index, OpenLazyFileOptions::default()) else { return response::not_found() };
            let response = response::file(&file, ctx.request(), &options);
            response.await.unwrap_or_else(|_| response::not_found())
        }
    });
    let addr = format!("{host}:{port}");
    println!("spa demo on http://{addr}");
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(rmx::node_fetch_server::listen(&addr, router)).expect("serve");
}

#[cfg(target_arch = "wasm32")]
fn main() {}
