//! Server for the social-auth demo: `cargo run -p social-auth-demo`.
//!
//! Env: `HOST` (127.0.0.1), `PORT` (44106), `SESSION_SECRET`, `SESSION_DIR`
//! (examples/social-auth/tmp/sessions), `PUBLIC_DIR` (examples/social-auth/public).

use rmx_cookie::{Cookie, SameSite};
use rmx_session::FsStorage;
use social_auth_demo::{data::Db, external_provider_links, router};

fn main() {
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44106".into());
    let secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| "social-auth-demo-secret".into());
    let sessions = std::env::var("SESSION_DIR").unwrap_or_else(|_| "examples/social-auth/tmp/sessions".into());
    let public = std::env::var("PUBLIC_DIR").unwrap_or_else(|_| "examples/social-auth/public".into());

    // middleware/session.ts
    let cookie = Cookie::new("social-auth-session")
        .secrets(vec![secret])
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(60 * 60 * 24 * 30)
        .path("/");
    let storage = FsStorage::new(&sessions).expect("session directory");

    let addr = format!("{host}:{port}");
    println!("social-auth demo is running on http://{addr}\n");
    println!("Demo accounts:\n  admin@example.com / password123\n  user@example.com / password123\n");
    println!("Social auth providers:");
    for link in external_provider_links() {
        println!("  {} disabled ({})", link.label, link.disabled_reason);
    }

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(rmx_serve::listen(&addr, router(cookie, storage, Db::seeded(), &public))).expect("serve");
}
