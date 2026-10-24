use super::*;
use rsmix_fetch::block_on;

fn req(method: &str, url: &str) -> Request {
    http::Request::builder().method(method).uri(url).body(Body::empty()).unwrap()
}

fn text(res: Response) -> String {
    block_on(res.into_body().text()).unwrap()
}

#[test]
fn routes_by_method_and_params() {
    let mut router = Router::new();
    router.get("/users/:id", |ctx: Context| async move { format!("User {}", ctx.param("id").unwrap()) });
    router.put("/users/:id", |ctx: Context| async move { format!("Updated {}", ctx.param("id").unwrap()) });

    let res = block_on(router.fetch(req("GET", "https://x/users/1")));
    assert_eq!(res.status(), 200);
    assert_eq!(text(res), "User 1");
    assert_eq!(text(block_on(router.fetch(req("PUT", "https://x/users/2")))), "Updated 2");

    let res = block_on(router.fetch(req("DELETE", "https://x/users/1")));
    assert_eq!(res.status(), 405);
    assert_eq!(res.headers()["allow"], "GET, HEAD, PUT");

    let res = block_on(router.fetch(req("HEAD", "https://x/users/1")));
    assert_eq!(res.status(), 200);
    assert_eq!(text(res), "");

    assert_eq!(block_on(router.fetch(req("GET", "https://x/nope"))).status(), 404);
}

#[test]
fn falls_through_to_less_specific_route_for_method() {
    let mut router = Router::new();
    router.get("/files/readme", |_: Context| async { "static" });
    router.any("/files/*path", |_: Context| async { "any" });
    assert_eq!(text(block_on(router.fetch(req("GET", "https://x/files/readme")))), "static");
    assert_eq!(text(block_on(router.fetch(req("POST", "https://x/files/readme")))), "any");
}

#[test]
fn middleware_order_and_context() {
    #[derive(Clone)]
    struct User(&'static str);

    let mut router = Router::new()
        .with(|ctx: Context, next: Next| async move {
            let mut res = next.run(ctx).await;
            res.headers_mut().insert("x-outer", "1".parse().unwrap());
            res
        })
        .with(|mut ctx: Context, next: Next| async move {
            ctx.set(User("ada"));
            next.run(ctx).await
        });
    router.group(vec![middleware(|ctx: Context, next: Next| async move {
        if ctx.header("authorization").is_none() {
            return (StatusCode::UNAUTHORIZED, "no").into_response();
        }
        next.run(ctx).await
    })], |admin| {
        admin.get("/admin", |ctx: Context| async move { format!("admin {}", ctx.get::<User>().unwrap().0) });
    });
    router.get("/me", |ctx: Context| async move { format!("me {}", ctx.get::<User>().unwrap().0) });

    let res = block_on(router.fetch(req("GET", "https://x/me")));
    assert_eq!(res.headers()["x-outer"], "1");
    assert_eq!(text(res), "me ada");
    let res = block_on(router.fetch(req("GET", "https://x/admin")));
    assert_eq!(res.status(), 401);
    assert_eq!(res.headers()["x-outer"], "1");
    let authed = http::Request::builder().uri("https://x/admin").header("authorization", "t").body(Body::empty()).unwrap();
    assert_eq!(text(block_on(router.fetch(authed))), "admin ada");
    // Router middleware also wraps 404s.
    assert_eq!(block_on(router.fetch(req("GET", "https://x/zzz"))).headers()["x-outer"], "1");
}

#[test]
fn sealed_values_can_be_read_but_not_replaced() {
    #[derive(Clone, Debug, PartialEq)]
    struct Owner(&'static str);
    #[derive(Clone)]
    struct Other;

    let sealing = |ctx: &mut Context| ctx.set_sealed(Owner("mw"), "Owner is sealed");
    let mut ctx = Context::new(req("GET", "https://x/"));
    sealing(&mut ctx);
    assert_eq!(ctx.get::<Owner>(), Some(&Owner("mw")));
    ctx.set(Other);
    assert!(ctx.get_mut::<Other>().is_some(), "other types stay writable");

    let replace = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ctx.set(Owner("handler"))));
    assert_eq!(*replace.unwrap_err().downcast::<String>().unwrap(), "Owner is sealed");
    let mutate = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ctx.get_mut::<Owner>();
    }));
    assert!(mutate.is_err());
    assert_eq!(ctx.get::<Owner>(), Some(&Owner("mw")));
}

#[test]
fn mount_and_route_maps() {
    use crate::routes::{form, get, prefix, resources};
    let books = resources("books");
    let contact = form("contact");
    let admin = prefix("admin");
    let mut router = Router::new();
    router.route(get("/"), |_: Context| async { "home" });
    router.route(&books.show, |ctx: Context| async move { format!("book {}", ctx.param("id").unwrap()) });
    router.route(&books.create, |_: Context| async { "created" });
    router.route(&contact.index, |_: Context| async { "form" });
    router.route(&contact.action, |mut ctx: Context| async move {
        let form = ctx.take_body().form().await.unwrap();
        format!("thanks {}", form.get("name").unwrap_or(""))
    });
    router.route(admin.get("users/:id"), |ctx: Context| async move { format!("admin user {}", ctx.param("id").unwrap()) });
    router.mount("/orgs/:orgId", |org| {
        org.get("/users/:userId", |ctx: Context| async move {
            format!("{}:{}", ctx.param("orgId").unwrap(), ctx.param("userId").unwrap())
        });
    });

    assert_eq!(text(block_on(router.fetch_url("https://x/"))), "home");
    assert_eq!(text(block_on(router.fetch_url("https://x/books/7"))), "book 7");
    assert_eq!(text(block_on(router.fetch(req("POST", "https://x/books")))), "created");
    assert_eq!(text(block_on(router.fetch_url("https://x/admin/users/3"))), "admin user 3");
    assert_eq!(text(block_on(router.fetch_url("https://x/orgs/acme/users/9"))), "acme:9");
    let post = http::Request::builder()
        .method("POST")
        .uri("https://x/contact")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from("name=Ada+L"))
        .unwrap();
    assert_eq!(text(block_on(router.fetch(post))), "thanks Ada L");
}

#[test]
fn subrequests_through_context_router() {
    let mut router = Router::new();
    router.get("/inner", |_: Context| async { "inner" });
    router.get("/outer", |ctx: Context| async move {
        let res = ctx.router().fetch_url("https://x/inner").await;
        format!("outer+{}", res.into_body().text().await.unwrap())
    });
    assert_eq!(text(block_on(router.fetch_url("https://x/outer"))), "outer+inner");
}

#[test]
fn handler_errors_become_responses() {
    let mut router = Router::new();
    router.get("/bad", |_: Context| async { Err::<Response, Error>(Error::bad_request("nope")) });
    router.get("/io", |_: Context| async {
        let v: Result<Response> = Err(std::io::Error::other("disk").into());
        v
    });
    let res = block_on(router.fetch_url("https://x/bad"));
    assert_eq!(res.status(), 400);
    assert_eq!(text(res), "nope");
    assert_eq!(block_on(router.fetch_url("https://x/io")).status(), 500);
}

#[test]
fn request_url_uses_host_header_for_origin_form() {
    let mut router = Router::new();
    router.get("/where", |ctx: Context| async move { ctx.url().href() });
    let r = http::Request::builder().uri("/where?x=1").header("host", "example.com:8080").body(Body::empty()).unwrap();
    assert_eq!(text(block_on(router.fetch(r))), "http://example.com:8080/where?x=1");
}

/// Resolves after being polled twice, so the future suspends once.
struct YieldOnce(bool);

impl std::future::Future for YieldOnce {
    type Output = ();

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<()> {
        if self.0 {
            return std::task::Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        std::task::Poll::Pending
    }
}

#[derive(Clone)]
struct User(&'static str);

fn describe_request() -> String {
    let Some(ctx) = async_context::get_context() else { return "no context".into() };
    format!("{} {} id={} user={}", ctx.method(), ctx.url().pathname(), ctx.param("id").unwrap_or_default(), ctx.get::<User>().map_or("-", |u| u.0))
}

// async-context.test.ts: stores the request context in AsyncLocalStorage (the handler sees the
// request's context through get_context())
#[test]
fn async_context_follows_the_request_into_helpers() {
    let mut router = Router::new().with(async_context::async_context()).with(|mut ctx: Context, next: Next| async move {
        ctx.set(User("ada"));
        next.run(ctx).await
    });
    router.get("/users/:id", |_: Context| async {
        let before = describe_request();
        YieldOnce(false).await;
        format!("{before} | {}", describe_request())
    });
    assert_eq!(text(block_on(router.fetch_url("https://x/users/7"))), "GET /users/7 id=7 user=ada | GET /users/7 id=7 user=ada");
    assert_eq!(describe_request(), "no context", "the scope ends with the request");
}

#[test]
fn async_context_is_absent_without_the_middleware() {
    let mut router = Router::new();
    router.get("/", |_: Context| async { describe_request() });
    assert_eq!(text(block_on(router.fetch_url("https://x/"))), "no context");
}
