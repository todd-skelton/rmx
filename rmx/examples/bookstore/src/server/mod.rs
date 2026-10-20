//! The server: `router.ts`, `middleware/*` and the controllers under `actions/`.

mod account;
mod admin;
mod auth;
mod books;
mod cart;
pub mod data;
mod root;
#[cfg(test)]
mod tests;
pub mod ui;

use std::time::Duration;

use http::StatusCode;
use rmx::{rsx, Node};
use rmx_cookie::{Cookie, SameSite};
use rmx_fetch::{response, FormValue, Response};
use rmx_file_storage::FileStorage;
use rmx_middleware::auth::{auth, require_auth as require_auth_identity, scheme, session_scheme, AuthExt};
use rmx_middleware::compression::compression;
use rmx_middleware::form_data::{form_data, FileUpload};
use rmx_middleware::logger::logger;
use rmx_middleware::method_override::method_override;
use rmx_middleware::session::{session, SessionExt};
use rmx_middleware::static_files::{static_files, ETag, StaticOptions};
use rmx_render::{Render, ResponseInit};
use rmx_router::{Context, Middleware, Next, Router};
use rmx_session::SessionStorage;
use serde::{Deserialize, Serialize};

use crate::cart::Cart;
use crate::routes::ROUTES;
use data::{now_ms, Db, Role, User};
use ui::{Viewer, ViewerProvider};

/// `createBookstoreRouter(options)` plus what Remix reads from module scope.
pub struct BookstoreOptions<S, U> {
    pub session_cookie: Cookie,
    pub session_storage: S,
    pub uploads: U,
    pub db: Db,
    pub public_dir: String,
    /// The pause the cart API takes to show pending UI (Remix skips it under test).
    pub cart_delay: Duration,
}

/// `middleware/session.ts`
pub fn session_cookie() -> Cookie {
    Cookie::new("session")
        .secrets(vec!["s3cr3t-k3y-for-d3mo".into()])
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(2_592_000)
        .path("/")
}

pub fn router<S: SessionStorage, U: FileStorage + Clone>(options: BookstoreOptions<S, U>) -> Router {
    let BookstoreOptions { session_cookie, session_storage, uploads, db, public_dir, cart_delay } = options;
    let mut router = Router::new();
    if std::env::var("RMX_LOG").is_ok() {
        router.use_middleware(logger());
    }
    router.use_middleware(compression());
    router.use_middleware(static_files(
        &public_dir,
        StaticOptions { cache_control: Some("no-store, must-revalidate".into()), etag: ETag::Off, last_modified: false, ..Default::default() },
    ));
    router.use_middleware(form_data().upload_handler(upload_handler(uploads.clone())));
    router.use_middleware(method_override());
    router.use_middleware(session(session_cookie, session_storage));
    router.use_middleware(load_database(db.clone()));
    router.use_middleware(load_auth(db));

    root::routes(&mut router, uploads);
    books::routes(&mut router);
    auth::routes(&mut router);
    cart::routes(&mut router, cart_delay);
    account::routes(&mut router);
    admin::routes(&mut router);
    router
}

// ---------------------------------------------------------------------------------------
// middleware/uploads.ts
// ---------------------------------------------------------------------------------------

/// Store each uploaded file and put its `/uploads/..` URL in the form instead.
fn upload_handler<U: FileStorage + Clone>(uploads: U) -> impl Fn(FileUpload) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<FormValue>> + Send>> + Send + Sync + 'static {
    move |upload: FileUpload| {
        let uploads = uploads.clone();
        Box::pin(async move {
            // An empty file input arrives as an empty, nameless file; keep the current cover.
            if upload.file.name.is_empty() && upload.file.size() == 0 {
                return None;
            }
            let ext = upload.file.name.rsplit('.').next().filter(|e| !e.is_empty()).unwrap_or("jpg").to_string();
            let key = format!("{}/{}-{}.{ext}", upload.field_name, now_ms(), random_base36(6));
            uploads.set(&key, upload.file).await.ok()?;
            Some(FormValue::Text(format!("/uploads/{key}")))
        })
    }
}

fn random_base36(len: usize) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut bytes = vec![0u8; len];
    rmx_crypto::fill_random(&mut bytes);
    bytes.iter().map(|b| DIGITS[*b as usize % 36] as char).collect()
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

/// What login saves in the session under `auth`.
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
        .invalidate(|session| session.unset("auth")),
    )])
}

/// `getCurrentUserSafely()`
pub fn current_user(ctx: &Context) -> Option<&User> {
    ctx.auth::<User>().identity()
}

/// `getCurrentUser()`: only behind [`require_auth`].
pub fn user(ctx: &Context) -> &User {
    current_user(ctx).expect("require_auth() runs before this handler")
}

/// `getCurrentCart()`
pub fn current_cart(ctx: &Context) -> Cart {
    ctx.session().get::<Cart>("cart").unwrap_or_default()
}

/// `requireAuth()`: send anonymous visitors to the login page, remembering where they were
/// going.
pub fn require_auth() -> impl Middleware {
    require_auth_identity::<User>().on_failure(|ctx, _| {
        let url = ctx.url();
        let return_to = safe_return_to(url.search_params().get("returnTo")).unwrap_or_else(|| format!("{}{}", url.pathname(), url.search()));
        response::redirect(&ROUTES.auth.login.index.href_with_search((), &[("returnTo", return_to)].into_iter().collect()))
    })
}

/// `requireAdmin()`: 403 for signed-in users who are not admins.
pub fn require_admin() -> impl Middleware {
    |ctx: Context, next: Next| async move {
        if current_user(&ctx).map(|u| u.role) != Some(Role::Admin) {
            return response::status(StatusCode::FORBIDDEN, "Forbidden");
        }
        next.run(ctx).await
    }
}

/// `returnTo` when it is a local path (not `//host`).
fn safe_return_to(value: Option<&str>) -> Option<String> {
    value.filter(|v| v.starts_with('/') && !v.starts_with("//")).map(str::to_string)
}

/// `getPostAuthRedirect(url)`
pub fn post_auth_redirect(ctx: &Context) -> String {
    safe_return_to(ctx.url().search_params().get("returnTo")).unwrap_or_else(|| ROUTES.account.index.href(()))
}

/// `getLoginRedirectURL(url, route)`: `route` carrying the request's safe `returnTo`.
pub fn login_redirect_url(ctx: &Context, route: &rmx_router::Route) -> String {
    match safe_return_to(ctx.url().search_params().get("returnTo")) {
        Some(r) => route.href_with_search((), &[("returnTo", r)].into_iter().collect()),
        None => route.href(()),
    }
}

/// `parseId()`
pub fn parse_id(value: Option<&str>) -> Option<i64> {
    value?.trim().parse().ok()
}

// ---------------------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------------------

/// `render(node)` with the viewer available to the layout.
pub fn render(ctx: &Context, node: Node) -> Response {
    render_with(ctx, node, ResponseInit::default())
}

pub fn render_with(ctx: &Context, node: Node, init: ResponseInit) -> Response {
    let viewer = Viewer { user: current_user(ctx).cloned() };
    ctx.render_with(rsx! { <ViewerProvider viewer={viewer}>{node}</ViewerProvider> }, init)
}

pub fn not_found_page(ctx: &Context, node: Node) -> Response {
    render_with(ctx, node, ResponseInit::status(StatusCode::NOT_FOUND))
}

/// `new Response(null, { status: 204 })`
pub fn no_content() -> Response {
    let mut res = Response::new(rmx_fetch::Body::empty());
    *res.status_mut() = StatusCode::NO_CONTENT;
    res
}

/// A form value that failed the demo's schema (Remix throws from `s.parse`).
pub fn bad_request(message: impl Into<String>) -> Response {
    response::status(StatusCode::BAD_REQUEST, message)
}
