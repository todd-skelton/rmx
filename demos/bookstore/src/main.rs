//! Server for the bookstore demo: `cargo run -p bookstore-demo`. List its routes with
//! `cargo run -p bookstore-demo -- routes` (`--table`, `--json`).
//!
//! Env: `HOST` (127.0.0.1), `PORT` (44108), `PUBLIC_DIR` (demos/bookstore/public),
//! `TMP_DIR` (demos/bookstore/tmp, for sessions and uploads), `CART_DELAY_MS` (1000).

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use std::time::Duration;

    use bookstore_demo::server::{data::Db, router, session_cookie, BookstoreOptions};
    use rmx::file_storage::FsFileStorage;
    use rmx::session::FsStorage;

    rmx::router::routes::routes_command(&*bookstore_demo::routes::ROUTES);

    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44108".into());
    let public_dir = std::env::var("PUBLIC_DIR").unwrap_or_else(|_| "demos/bookstore/public".into());
    let tmp = std::path::PathBuf::from(std::env::var("TMP_DIR").unwrap_or_else(|_| "demos/bookstore/tmp".into()));
    let delay = std::env::var("CART_DELAY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(1000);

    let options = BookstoreOptions {
        session_cookie: session_cookie(),
        session_storage: FsStorage::new(tmp.join("sessions")).expect("session directory"),
        uploads: FsFileStorage::new(tmp.join("uploads")).expect("uploads directory"),
        db: Db::seeded(),
        public_dir,
        cart_delay: Duration::from_millis(delay),
    };
    let addr = format!("{host}:{port}");
    println!("Bookstore is running on http://{addr}");
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(rmx::node_fetch_server::listen(&addr, router(options))).expect("serve");
}

#[cfg(target_arch = "wasm32")]
fn main() {}
