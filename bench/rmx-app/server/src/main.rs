//! The rmx benchmark server, on rmx's own stack like the Remix app it is compared with
//! (`bench/remix-app/app/router.tsx`): fetch-router, the static and form-data middleware,
//! `ctx.render` and node-fetch-server.
//!
//! Env: `HOST` (default 127.0.0.1), `PORT` (default 44101), `RMX_THREADS` (tokio worker threads; default = CPU count),
//! `RMX_PUBLIC` (static dir; default `bench/rmx-app/public`).

use std::sync::{Arc, Mutex};

use bench_app::Task;
use http::StatusCode;
use rmx::middleware::form_data::{form_data, FormDataExt};
use rmx::middleware::render::Render;
use rmx::middleware::static_files::{static_files, StaticOptions};
use rmx::response;
use rmx::router::{Context, Router};

fn main() {
    let threads = std::env::var("RMX_THREADS").ok().and_then(|v| v.parse::<usize>().ok());
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    if let Some(n) = threads {
        builder.worker_threads(n);
    }
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44101".into());
    let public = std::env::var("RMX_PUBLIC").unwrap_or_else(|_| "bench/rmx-app/public".into());
    let addr = format!("{host}:{port}");
    println!("rmx bench listening on http://{addr}");
    builder.enable_all().build().expect("tokio runtime").block_on(rmx::node_fetch_server::listen(&addr, router(&public))).expect("serve");
}

fn router(public: &str) -> Router {
    let tasks: Arc<Mutex<Vec<Task>>> = Arc::new(Mutex::new((1..=5).map(|id| Task { id, title: format!("Seed task {id}") }).collect()));
    let mut router = Router::new();
    router.use_middleware(static_files(public, StaticOptions { index: vec![], ..Default::default() }));
    router.use_middleware(form_data());

    let home = tasks.clone();
    router.get("/", move |ctx: Context| {
        let tasks = home.lock().expect("tasks lock").clone();
        async move { ctx.render(bench_app::home_page(&tasks)) }
    });
    router.post("/tasks", move |ctx: Context| {
        let title = ctx.form_data().get("title").unwrap_or_default().to_string();
        let mut tasks = tasks.lock().expect("tasks lock");
        let id = tasks.len() as u32 + 1;
        tasks.push(Task { id, title });
        async move { response::redirect_with("/", StatusCode::SEE_OTHER) }
    });
    router.get("/ssr/:n", |ctx: Context| async move {
        let n = ctx.param("n").and_then(|n| n.parse::<usize>().ok()).unwrap_or(0).min(10_000);
        ctx.render(bench_app::ssr_page(n))
    });
    router.get("/client", |ctx: Context| async move { ctx.render(bench_app::client_page()) });
    router
}
