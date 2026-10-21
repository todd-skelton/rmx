//! Server for the social-auth demo: `cargo run -p social-auth-demo`.
//!
//! Env: `HOST` (127.0.0.1), `PORT` (44106), `SESSION_SECRET`, `SESSION_DIR`
//! (examples/social-auth/tmp/sessions), `PUBLIC_DIR` (examples/social-auth/public), and
//! `GOOGLE_CLIENT_ID`/`GOOGLE_CLIENT_SECRET`, `GITHUB_CLIENT_ID`/`GITHUB_CLIENT_SECRET`,
//! `X_CLIENT_ID`/`X_CLIENT_SECRET` to enable each provider. Register
//! `http://127.0.0.1:<PORT>/auth/<provider>/callback` as the provider's redirect URI.

use rmx::cookie::{Cookie, SameSite};
use rmx::fetch::{Body, BoxError, Request, Response};
use rmx::session::FsStorage;
use social_auth_demo::external::{demo_origin, provider_label, ExternalProviders, PROVIDER_NAMES};
use social_auth_demo::{data::Db, router};

/// Provider HTTP calls go over HTTPS through `ureq` on Tokio's blocking pool (Remix uses the
/// runtime's `fetch`).
fn https_fetch() -> impl Fn(Request) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response, BoxError>> + Send>> + Send + Sync + 'static {
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
    move |req: Request| {
        let agent = agent.clone();
        Box::pin(async move {
            let (parts, body) = req.into_parts();
            let body = body.bytes().await?.to_vec();
            tokio::task::spawn_blocking(move || -> Result<Response, BoxError> {
                let res = agent.run(http::Request::from_parts(parts, body))?;
                let (parts, mut body) = res.into_parts();
                let bytes = body.with_config().limit(10 * 1024 * 1024).read_to_vec()?;
                Ok(http::Response::from_parts(parts, Body::from(bytes)))
            })
            .await?
        })
    }
}

fn main() {
    rmx::router::routes::routes_command(&*social_auth_demo::routes::ROUTES);
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
    let providers = ExternalProviders::from_env(&demo_origin(&port), |name| std::env::var(name).ok(), https_fetch());

    let addr = format!("{host}:{port}");
    println!("social-auth demo is running on http://{addr}\n");
    println!("Demo accounts:\n  admin@example.com / password123\n  user@example.com / password123\n");
    println!("Social auth providers:");
    for (name, ..) in PROVIDER_NAMES {
        match providers.is_enabled(name) {
            true => println!("  {} enabled", provider_label(name)),
            false => println!("  {} disabled (missing {})", provider_label(name), providers.missing_env_vars(name).join(", ")),
        }
    }

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(rmx::serve::listen(&addr, router(cookie, storage, Db::seeded(), &public, providers))).expect("serve");
}
