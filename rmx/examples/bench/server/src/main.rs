//! axum server for the rmx benchmark app.
//!
//! Env: `HOST` (default 127.0.0.1), `PORT` (default 44101), `RMX_THREADS` (tokio worker threads; default = CPU count),
//! `RMX_PUBLIC` (static dir; default `examples/bench/public`).

use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::response::{Html, Redirect};
use axum::routing::get;
use axum::{Form, Router};
use bench_app::Task;
use serde::Deserialize;
use tower_http::services::ServeDir;

struct AppState {
    tasks: Mutex<Vec<Task>>,
}

#[derive(Deserialize)]
struct NewTask {
    title: String,
}

fn main() {
    let threads = std::env::var("RMX_THREADS").ok().and_then(|v| v.parse::<usize>().ok());
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    if let Some(n) = threads {
        builder.worker_threads(n);
    }
    builder.enable_all().build().expect("tokio runtime").block_on(serve());
}

async fn serve() {
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44101".into());
    let public = std::env::var("RMX_PUBLIC").unwrap_or_else(|_| "examples/bench/public".into());
    let tasks = (1..=5).map(|id| Task { id, title: format!("Seed task {id}") }).collect();
    let state = Arc::new(AppState { tasks: Mutex::new(tasks) });

    let app = Router::new()
        .route("/", get(home))
        .route("/tasks", axum::routing::post(add_task))
        .route("/ssr/{n}", get(ssr_rows))
        .route("/client", get(client))
        .fallback_service(ServeDir::new(public))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}")).await.expect("bind");
    println!("rmx bench listening on http://{host}:{port}");
    axum::serve(listener, app).await.expect("serve");
}

// Node trees use Rc, so each handler renders to a String before any await point.

async fn home(State(state): State<Arc<AppState>>) -> Html<String> {
    let tasks = state.tasks.lock().expect("tasks lock").clone();
    Html(rmx::render_to_string(&bench_app::home_page(&tasks)))
}

async fn add_task(State(state): State<Arc<AppState>>, Form(form): Form<NewTask>) -> Redirect {
    let mut tasks = state.tasks.lock().expect("tasks lock");
    let id = tasks.len() as u32 + 1;
    tasks.push(Task { id, title: form.title });
    Redirect::to("/")
}

async fn ssr_rows(Path(n): Path<usize>) -> Html<String> {
    Html(rmx::render_to_string(&bench_app::ssr_page(n.min(10_000))))
}

async fn client() -> Html<String> {
    Html(rmx::render_to_string(&bench_app::client_page()))
}
