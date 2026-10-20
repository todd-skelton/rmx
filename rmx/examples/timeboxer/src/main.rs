//! Server for the timeboxer demo: `cargo run -p timeboxer-demo`.
//!
//! Env: `SESSION_SECRET` (required, as in Remix), `HOST` (127.0.0.1), `PORT` (44112),
//! `ASSETS_ROOT` (examples/timeboxer), `TMP_DIR` (examples/timeboxer/tmp, for sessions),
//! `NODE_ENV=production` for secure cookies and fingerprinted assets.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use timeboxer_demo::server::{asset_server, data::Db, router, session_cookie, TimeboxerOptions};

    let Ok(secret) = std::env::var("SESSION_SECRET") else {
        eprintln!("SESSION_SECRET is required");
        std::process::exit(1);
    };
    let production = std::env::var("NODE_ENV").as_deref() == Ok("production");
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44112".into());
    let root = std::env::var("ASSETS_ROOT").unwrap_or_else(|_| "examples/timeboxer".into());
    let tmp = std::path::PathBuf::from(std::env::var("TMP_DIR").unwrap_or_else(|_| "examples/timeboxer/tmp".into()));

    let options = TimeboxerOptions {
        session_cookie: session_cookie(&secret, production),
        session_storage: rmx::session::FsStorage::new(tmp.join("sessions")).expect("session directory"),
        db: Db::new(),
        assets: Some(asset_server(std::path::Path::new(&root), production).expect("asset server")),
    };
    let addr = format!("{host}:{port}");
    println!("Timeboxer is running on http://{addr}");
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(rmx::serve::listen(&addr, router(options))).expect("serve");
}

#[cfg(target_arch = "wasm32")]
fn main() {}
