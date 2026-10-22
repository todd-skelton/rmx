//! The run-time half of Remix's `fetch-router/src/lib/router-types.test.ts`. Its cases check
//! TypeScript inference of the context that middleware provides (`context.get(CurrentUser)`
//! narrowing from `{ id } | null` to `{ id }`, `context.currentUser` properties, action and
//! controller middleware results, installer context contracts). Rust context keys are types
//! and `ctx.get::<T>()` is always an `Option`, so those checks have no counterpart; the
//! requests the cases send are ported here.

use rmx::fetch::{block_on, Body, FormData, Request};
use rmx::router::{Context, Next, Router};

#[derive(Clone)]
struct CurrentUser {
    id: &'static str,
}

#[derive(Clone)]
struct CurrentRole(&'static str);

fn request(method: &str, url: &str) -> Request {
    http::Request::builder().method(method).uri(url).body(Body::empty()).unwrap()
}

fn fetch(router: &Router, method: &str, url: &str) -> (u16, String) {
    let res = block_on(router.fetch(request(method, url)));
    (res.status().as_u16(), block_on(res.into_body().text()).unwrap())
}

/// Remix's `createRouter({ middleware: [requireUser(), setRole('viewer')] })`.
fn app_router() -> Router {
    Router::new()
        .with(|mut ctx: Context, next: Next| async move {
            ctx.set(CurrentUser { id: "user-1" });
            next.run(ctx).await
        })
        .with(|mut ctx: Context, next: Next| async move {
            ctx.set(CurrentRole("viewer"));
            next.run(ctx).await
        })
}

fn user(ctx: &Context) -> &'static str {
    ctx.get::<CurrentUser>().expect("requireUser ran").id
}

fn role(ctx: &Context) -> &'static str {
    ctx.get::<CurrentRole>().expect("setRole ran").0
}

// router-types.test.ts: keeps context values optional when middleware has not provided them
#[test]
fn keeps_context_values_optional_when_middleware_has_not_provided_them() {
    let mut router = Router::new();
    router.get("/public", |ctx: Context| async move {
        assert!(ctx.get::<CurrentUser>().is_none());
        assert!(ctx.get::<FormData>().is_none());
        "Public"
    });
    assert_eq!(fetch(&router, "GET", "https://remix.run/public"), (200, "Public".into()));
}

// router-types.test.ts: propagates router middleware context into route handlers
#[test]
fn propagates_router_middleware_context_into_route_handlers() {
    let mut router = app_router();
    router.get("/account/:accountId", |ctx: Context| async move {
        format!("{}:{}:{}", ctx.param("accountId").unwrap(), user(&ctx), role(&ctx))
    });
    assert_eq!(fetch(&router, "GET", "https://remix.run/account/123"), (200, "123:user-1:viewer".into()));
}

// router-types.test.ts: types constructor keys as available when middleware provides them
#[test]
fn reads_constructor_keys_when_middleware_provides_them() {
    let mut router = Router::new().with(|mut ctx: Context, next: Next| async move {
        ctx.set(FormData::new());
        next.run(ctx).await
    });
    router.post("/form", |ctx: Context| async move {
        let form = ctx.get::<FormData>().expect("setFormData ran");
        form.get("name").unwrap_or_default().to_string()
    });
    assert_eq!(fetch(&router, "POST", "https://remix.run/form"), (200, String::new()));
}

// router-types.test.ts: types mounted route installers with the parent router context
#[test]
fn mounted_route_installers_see_the_parent_router_context() {
    let mut router = app_router();
    router.mount("/accounts", |accounts| {
        accounts.get("/", |ctx: Context| async move { format!("{}:{}", user(&ctx), role(&ctx)) });
        accounts.get("/:accountId", |ctx: Context| async move {
            format!("{}:{}:{}", ctx.param("accountId").unwrap(), user(&ctx), role(&ctx))
        });
    });
    assert_eq!(fetch(&router, "GET", "https://remix.run/accounts"), (200, "user-1:viewer".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/accounts/123"), (200, "123:user-1:viewer".into()));
}

// router-types.test.ts: types mount pattern params in mounted route installers
#[test]
fn mount_pattern_params_reach_mounted_route_installers() {
    let mut router = app_router();
    router.mount("/orgs/:orgId", |org| {
        org.get("/", |ctx: Context| async move { format!("{}:{}", ctx.param("orgId").unwrap(), user(&ctx)) });
        org.get("/projects/:projectId", |ctx: Context| async move {
            format!("{}:{}", ctx.param("orgId").unwrap(), ctx.param("projectId").unwrap())
        });
        org.get("/duplicate/:orgId", |ctx: Context| async move { ctx.param("orgId").unwrap().to_string() });
        org.get("/optional(/:orgId)", |ctx: Context| async move { ctx.param("orgId").unwrap().to_string() });
    });
    assert_eq!(fetch(&router, "GET", "https://remix.run/orgs/acme"), (200, "acme:user-1".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/orgs/acme/projects/p123"), (200, "acme:p123".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/orgs/acme/duplicate/child"), (200, "child".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/orgs/acme/optional"), (200, "acme".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/orgs/acme/optional/child"), (200, "child".into()));
}
