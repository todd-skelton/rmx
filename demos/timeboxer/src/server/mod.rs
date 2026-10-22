//! The server: `router.ts`, `middleware/*` and the controllers under `actions/`.

mod auth;
pub mod data;
mod home;
pub mod ics;
pub mod passwords;
mod schedules;
#[cfg(test)]
mod tests;
pub mod ui;

use http::StatusCode;
use rmx::{rsx, Node};
use rmx::assets::{AssetOptions, AssetServer};
use rmx::cookie::{Cookie, SameSite};
use rmx::fetch::Response;
use rmx::response;
use rmx::middleware::auth::{auth, scheme, session_scheme, AuthExt};
use rmx::middleware::csrf::{csrf, csrf_token};
use rmx::middleware::form_data::form_data;
use rmx::middleware::logger::logger;
use rmx::middleware::session::session;
use rmx::middleware::render::{Render, ResponseInit};
use rmx::router::{action, Context, Middleware, Next, Router};
use rmx::session::SessionStorage;
use serde::{Deserialize, Serialize};

use crate::routes::ROUTES;
use data::{Db, User};
use ui::{PageScripts, ScriptsProvider};

/// The client entry module, relative to the asset root.
pub const ENTRY_FILE: &str = "app/actions/public/entry.js";

/// `createRouter(...)` plus what Remix reads from module scope and the environment.
pub struct TimeboxerOptions<S> {
    pub session_cookie: Cookie,
    pub session_storage: S,
    pub db: Db,
    /// Serves `/assets/*path`; `None` renders pages without the client script.
    pub assets: Option<AssetServer>,
}

/// `middleware/session.ts`: `__timebox_session`, signed with `secret`, for 30 days.
pub fn session_cookie(secret: &str, secure: bool) -> Cookie {
    Cookie::new("__timebox_session")
        .secrets(vec![secret.into()])
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .max_age(60 * 60 * 24 * 30)
        .path("/")
}

/// `actions/assets/controller.ts`: the asset server for the client entry and its wasm.
pub fn asset_server(root: &std::path::Path, fingerprint: bool) -> Result<AssetServer, rmx::assets::AssetError> {
    AssetServer::new(AssetOptions {
        root_dir: root.to_path_buf(),
        base_path: "/assets".into(),
        mounts: vec![("app".into(), "app".into())],
        allow_files: vec!["app/**/public/**".into()],
        deny_files: vec!["app/**/*.test.*".into()],
        fingerprint,
        ..Default::default()
    })
}

pub fn router<S: SessionStorage>(options: TimeboxerOptions<S>) -> Router {
    let TimeboxerOptions { session_cookie, session_storage, db, assets } = options;
    let mut router = Router::new();
    if std::env::var("RMX_LOG").is_ok() {
        router.use_middleware(logger());
    }
    router.use_middleware(session(session_cookie, session_storage));
    router.use_middleware(form_data());
    router.use_middleware(csrf());
    router.use_middleware(load_database(db.clone()));
    router.use_middleware(load_auth(db));
    router.use_middleware(load_scripts(assets.clone()));

    if let Some(assets) = assets {
        router.map(
            &ROUTES.assets,
            action(move |ctx: Context| {
                let assets = assets.clone();
                async move { assets.fetch(&ctx).unwrap_or_else(|| response::status(StatusCode::NOT_FOUND, "Not Found")) }
            }),
        );
    }
    home::routes(&mut router);
    auth::routes(&mut router);
    schedules::routes(&mut router);
    router
}

// ---------------------------------------------------------------------------------------
// middleware/database.ts and middleware/auth.ts
// ---------------------------------------------------------------------------------------

fn load_database(db: Db) -> impl Middleware {
    move |mut ctx: Context, next: Next| {
        ctx.set(db.clone());
        next.run(ctx)
    }
}

pub fn db(ctx: &Context) -> &Db {
    ctx.get::<Db>().expect("load_database() runs before handlers")
}

/// What sign-in saves in the session under `auth`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSession {
    pub user_id: i64,
}

fn load_auth(db: Db) -> impl Middleware {
    auth(vec![scheme(
        session_scheme(
            |session| session.get::<AuthSession>("auth"),
            move |value: AuthSession| {
                let user = db.find_user(value.user_id);
                async move { user }
            },
        )
        .name("session")
        .invalidate(|session| session.unset("auth")),
    )])
}

/// `context.get(Auth)` when `ok`.
pub fn current_user(ctx: &Context) -> Option<&User> {
    ctx.auth::<User>().identity()
}

// ---------------------------------------------------------------------------------------
// render({ assets })
// ---------------------------------------------------------------------------------------

/// Resolve the client entry's URL and module preloads for this request, so the document
/// can link them (Remix's `scriptEntry`).
fn load_scripts(assets: Option<AssetServer>) -> impl Middleware {
    move |mut ctx: Context, next: Next| {
        let scripts = match &assets {
            Some(assets) => match assets.script_entry(ENTRY_FILE) {
                Ok(entry) => PageScripts { href: Some(entry.href), preloads: entry.preloads },
                Err(e) => {
                    eprintln!("timeboxer: client entry unavailable ({e}); build it with ./scripts/build-client.ps1 -Crate timeboxer-demo -Out demos/timeboxer/app/actions/public/pkg");
                    PageScripts::default()
                }
            },
            None => PageScripts::default(),
        };
        ctx.set(scripts);
        next.run(ctx)
    }
}

pub fn render(ctx: &Context, node: Node) -> Response {
    render_with(ctx, node, ResponseInit::default())
}

pub fn render_with(ctx: &Context, node: Node, init: ResponseInit) -> Response {
    let scripts = ctx.get::<PageScripts>().cloned().unwrap_or_default();
    ctx.render_with(rsx! { <ScriptsProvider scripts={scripts}>{node}</ScriptsProvider> }, init)
}

/// `getCsrfToken(context)`
pub fn token(ctx: &Context) -> String {
    csrf_token(ctx)
}

/// `Response.json(value, { status })`
pub fn json_with<T: Serialize + ?Sized>(status: StatusCode, value: &T) -> Response {
    let mut res = response::json(value);
    *res.status_mut() = status;
    res
}

/// `Response.json(value)`
pub fn json<T: Serialize + ?Sized>(value: &T) -> Response {
    response::json(value)
}

/// `redirect(href, 303)`
pub fn see_other(href: &str) -> Response {
    response::redirect_with(href, StatusCode::SEE_OTHER)
}
