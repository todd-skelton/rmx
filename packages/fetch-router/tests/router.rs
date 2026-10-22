//! Port of Remix's `fetch-router/src/lib/router.test.ts`, one test per case, in its order.
//!
//! `route({ ... })` route maps are `#[derive(RouteMap)]` structs (a pattern string is an `any`
//! route); `{ middleware, handler }` action objects are `action(handler).with(middleware)`;
//! `createRouter({ middleware, defaultHandler, matcher })` is `Router::with_matcher`,
//! `.with(middleware)` and `.not_found(handler)`. A thrown error is a panic, which propagates
//! out of `router.fetch` and which middleware catches with `catch_panic`.
//!
//! Seven cases check controller and action shapes that Remix verifies with TypeScript or at
//! `map()` time; in rmx the compiler checks them, so six are `compile_fail` doctests on
//! `RouteMap` (packages/fetch-router/src/routes.rs) and the positive one is a test here:
//! "requires controllers to define actions under `actions`", "rejects controllers without an
//! object actions property", "rejects action objects without function handlers", "rejects
//! nested route map keys in controller actions", "rejects unknown controller action keys",
//! "requires direct leaf actions in controllers".

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use http::StatusCode;
use rmx::fetch::{block_on, Body, Request, Response};
use rmx::response;
use rmx::route_pattern::{Match, Matcher, MatcherResourceError, MultiMatcher, RoutePattern};
use rmx::router::routes::{any, get, post, prefix, Route, RouteMap};
use rmx::router::{action, catch_panic, Action, BoxFuture, Context, Controller, IntoResponse, Next, RouteEntry, Router};
use rmx::url::Url;

fn req(method: &str, url: &str) -> Request {
    http::Request::builder().method(method).uri(url).body(Body::empty()).unwrap()
}

fn fetch_res(router: &Router, method: &str, url: &str) -> Response {
    block_on(router.fetch(req(method, url)))
}

fn fetch(router: &Router, method: &str, url: &str) -> (u16, String) {
    let res = fetch_res(router, method, url);
    let status = res.status().as_u16();
    (status, block_on(res.into_body().text()).unwrap())
}

/// What `router.fetch` rejected with: the panic message.
fn rejects(router: &Router, url: &str) -> String {
    block_on(catch_panic(router.fetch(req("GET", url)))).expect_err("router.fetch rejects").0
}

type Log = Arc<Mutex<Vec<String>>>;

fn log() -> Log {
    Arc::new(Mutex::new(Vec::new()))
}

fn take(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.lock().unwrap())
}

fn push(log: &Log, entry: &str) {
    log.lock().unwrap().push(entry.to_string());
}

/// Middleware that records `name` and continues.
fn record(log: &Log, name: &'static str) -> impl Fn(Context, Next) -> BoxFuture<Response> + Send + Sync + 'static {
    let log = log.clone();
    move |ctx: Context, next: Next| {
        push(&log, name);
        next.run(ctx)
    }
}

/// Middleware that records `<name> before` and `<name> after` around the rest of the chain.
fn around(log: &Log, name: &'static str) -> impl Fn(Context, Next) -> BoxFuture<Response> + Send + Sync + 'static {
    let log = log.clone();
    move |ctx: Context, next: Next| {
        let log = log.clone();
        Box::pin(async move {
            push(&log, &format!("{name} before"));
            let res = next.run(ctx).await;
            push(&log, &format!("{name} after"));
            res
        })
    }
}

/// A handler that records `name` and answers `body`.
fn logged(log: &Log, name: &'static str, body: &'static str) -> Action {
    let log = log.clone();
    action(move |_: Context| {
        push(&log, name);
        async move { body }
    })
}

/// A response with `body` and one extra header.
fn with_header(body: &'static str, name: &'static str, value: &'static str) -> Response {
    let mut res = response::text(body);
    res.headers_mut().insert(name, value.parse().unwrap());
    res
}

#[derive(RouteMap)]
struct Home {
    home: Route,
}

fn home() -> Home {
    Home { home: any("/") }
}

// --- router.fetch() ----------------------------------------------------------------------

#[test]
fn can_be_passed_as_a_direct_function_reference() {
    let mut router = Router::new();
    router.get("/", |_: Context| async { "Home" });

    // A clone is the same router, so a `fetch` function needs no binding to it.
    let fetch = {
        let router = router.clone();
        move |request: Request| router.fetch(request)
    };

    let res = block_on(fetch(req("GET", "https://remix.run")));
    assert_eq!(block_on(res.into_body().text()).unwrap(), "Home");
}

#[test]
fn fetches_a_route() {
    let mut router = Router::new();
    router.get("/", |_: Context| async { "Home" });

    assert_eq!(fetch(&router, "GET", "https://remix.run"), (200, "Home".into()));
}

#[test]
fn uses_a_request_input_directly() {
    #[derive(Clone, Debug, PartialEq)]
    struct Marker(u32);
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move {
        assert_eq!(ctx.request().extensions().get::<Marker>(), Some(&Marker(7)));
        "Home"
    });
    let mut request = req("GET", "https://remix.run");
    request.extensions_mut().insert(Marker(7));

    let res = block_on(router.fetch(request));

    assert_eq!(res.status(), 200);
    assert_eq!(block_on(res.into_body().text()).unwrap(), "Home");
}

#[test]
fn fetches_a_route_with_middleware() {
    let routes = home();
    let log = log();
    let mut router = Router::new();
    router.get(&routes.home, action(|_: Context| async { "Home" }).with(record(&log, "middleware")));

    assert_eq!(fetch(&router, "GET", "https://remix.run"), (200, "Home".into()));
    assert_eq!(take(&log), ["middleware"]);
}

#[test]
fn runs_router_middleware_before_fetching_a_route() {
    let routes = home();
    let log = log();
    let mut router = Router::new().with(record(&log, "router middleware"));
    router.get(&routes.home, action(|_: Context| async { "Home" }).with(record(&log, "action middleware")));

    assert_eq!(fetch(&router, "GET", "https://remix.run"), (200, "Home".into()));
    assert_eq!(take(&log), ["router middleware", "action middleware"]);
}

#[test]
fn fetches_a_route_with_specific_method_actions() {
    let routes = home();
    let mut router = Router::new();
    router.get(&routes.home, |_: Context| async { "GET" });
    router.head(&routes.home, |_: Context| async { "HEAD" });
    router.post(&routes.home, |_: Context| async { "POST" });
    router.put(&routes.home, |_: Context| async { "PUT" });
    router.patch(&routes.home, |_: Context| async { "PATCH" });
    router.delete(&routes.home, |_: Context| async { "DELETE" });
    router.options(&routes.home, |_: Context| async { "OPTIONS" });

    for method in ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        assert_eq!(fetch(&router, method, "https://remix.run"), (200, method.to_string()));
    }
}

#[test]
fn replaces_any_corresponding_options_set_in_the_original_request_when_options_are_provided() {
    // `fetch(input, init)` builds a new request from `init`; in Rust the caller builds the
    // request it wants, so a second fetch with a `From` header sees it and the first does not.
    let log: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let l = log.clone();
    let mut router = Router::new();
    router.get(
        "/",
        action(|_: Context| async { "Home" }).with(move |ctx: Context, next: Next| {
            l.lock().unwrap().push(ctx.header("from").map(str::to_string));
            next.run(ctx)
        }),
    );

    block_on(router.fetch(req("GET", "https://remix.run")));
    assert_eq!(std::mem::take(&mut *log.lock().unwrap()), [None]);

    let request = http::Request::builder().uri("https://remix.run").header("From", "admin@remix.run").body(Body::empty()).unwrap();
    block_on(router.fetch(request));
    assert_eq!(std::mem::take(&mut *log.lock().unwrap()), [Some("admin@remix.run".to_string())]);
}

#[test]
fn runs_router_middleware_even_when_there_are_no_routes() {
    let log = log();
    let router = Router::new().with(record(&log, "middleware"));

    assert_eq!(fetch(&router, "GET", "https://remix.run/nonexistent"), (404, "Not Found: /nonexistent".into()));
    assert_eq!(take(&log), ["middleware"]);
}

#[test]
fn runs_router_middleware_even_when_no_route_matches() {
    let log = log();
    let mut router = Router::new().with(record(&log, "middleware"));
    router.get("/", |_: Context| async { "Home" });

    assert_eq!(fetch(&router, "GET", "https://remix.run/nonexistent"), (404, "Not Found: /nonexistent".into()));
    assert_eq!(take(&log), ["middleware"]);
}

// --- router.map() with single routes -----------------------------------------------------

#[test]
fn maps_a_single_route_to_a_request_handler() {
    let routes = home();
    let mut router = Router::new();
    router.map(&routes.home, action(|_: Context| async { "Home" }));

    assert_eq!(fetch(&router, "GET", "https://remix.run"), (200, "Home".into()));
}

#[test]
fn maps_a_single_route_to_an_action_with_middleware() {
    #[derive(RouteMap)]
    struct Routes {
        profile: Route,
    }
    let routes = Routes { profile: any("/profile/:id") };
    let log = log();
    let l = log.clone();
    let mut router = Router::new();
    router.map(
        &routes.profile,
        logged(&log, "action", "OK").with(move |ctx: Context, next: Next| {
            push(&l, &format!("middleware {}", ctx.param("id").unwrap()));
            next.run(ctx)
        }),
    );

    assert_eq!(fetch(&router, "GET", "https://remix.run/profile/1"), (200, "OK".into()));
    assert_eq!(take(&log), ["middleware 1", "action"]);
}

#[test]
fn matches_any_request_method() {
    let mut router = Router::new();
    router.map(&any("/"), action(|ctx: Context| async move { ctx.method().to_string() }));

    for method in ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        assert_eq!(fetch(&router, method, "https://remix.run"), (200, method.to_string()));
    }
}

// --- router.map() ------------------------------------------------------------------------

#[derive(RouteMap)]
struct Site {
    home: Route,
    blog: Blog,
}

#[derive(RouteMap)]
struct Blog {
    index: Route,
    create: Route,
    show: Route,
}

#[test]
fn maps_route_maps_to_controllers() {
    let routes = Site { home: any("/"), blog: Blog { index: get("/blog"), create: post("/blog"), show: any("/blog/:id") } };
    let mut router = Router::new();
    router.map(&routes, SiteActions { home: action(|_: Context| async { "Home" }) });
    router.map(
        &routes.blog,
        BlogActions {
            index: action(|_: Context| async { "Blog" }),
            create: action(|_: Context| async { "Blog Post Created" }),
            show: action(|ctx: Context| async move { format!("Blog Post {}", ctx.param("id").unwrap()) }),
        },
    );

    assert_eq!(fetch(&router, "GET", "https://remix.run"), (200, "Home".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/blog"), (200, "Blog".into()));
    assert_eq!(fetch(&router, "POST", "https://remix.run/blog"), (200, "Blog Post Created".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/blog/1"), (200, "Blog Post 1".into()));
}

#[test]
fn maps_a_route_map_to_actions_with_middleware() {
    let log = log();
    let mut router = Router::new();
    router.map(&home(), Controller::new(HomeActions { home: logged(&log, "action", "OK") }).with(record(&log, "middleware")));

    assert_eq!(fetch(&router, "GET", "https://remix.run/"), (200, "OK".into()));
    assert_eq!(take(&log), ["middleware", "action"]);
}

#[test]
fn applies_controller_middleware_only_to_direct_route_actions() {
    #[derive(RouteMap)]
    struct Routes {
        home: Route,
        blog: BlogIndex,
    }
    #[derive(RouteMap)]
    struct BlogIndex {
        index: Route,
    }
    let routes = Routes { home: any("/"), blog: BlogIndex { index: any("/blog") } };
    let log = log();
    let mut router = Router::new();
    router.map(&routes, Controller::new(RoutesActions { home: logged(&log, "home", "Home") }).with(record(&log, "outer middleware")));
    router.map(&routes.blog, BlogIndexActions { index: logged(&log, "blog-index", "Blog") });

    assert_eq!(fetch(&router, "GET", "https://remix.run/"), (200, "Home".into()));
    assert_eq!(take(&log), ["outer middleware", "home"]);
    assert_eq!(fetch(&router, "GET", "https://remix.run/blog"), (200, "Blog".into()));
    assert_eq!(take(&log), ["blog-index"]);
}

#[test]
fn allows_selective_middleware_by_mapping_specific_nested_route_subtrees_separately() {
    #[derive(RouteMap)]
    struct Routes {
        public: Route,
        admin: Admin,
    }
    #[derive(RouteMap)]
    struct Admin {
        dashboard: Route,
        users: Route,
    }
    let routes = Routes { public: any("/public"), admin: Admin { dashboard: any("/admin/dashboard"), users: any("/admin/users") } };
    let log = log();
    let mut router = Router::new();
    router.map(&routes.public, logged(&log, "public", "Public"));
    router.map(
        &routes.admin,
        Controller::new(AdminActions { dashboard: logged(&log, "dashboard", "Dashboard"), users: logged(&log, "users", "Users") })
            .with(record(&log, "auth")),
    );

    assert_eq!(fetch(&router, "GET", "https://remix.run/public"), (200, "Public".into()));
    assert_eq!(take(&log), ["public"]);
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/dashboard"), (200, "Dashboard".into()));
    assert_eq!(take(&log), ["auth", "dashboard"]);
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/users"), (200, "Users".into()));
    assert_eq!(take(&log), ["auth", "users"]);
}

#[test]
fn runs_both_router_and_controller_middleware() {
    let log = log();
    let mut router = Router::new().with(record(&log, "router middleware"));
    router.map(&home(), Controller::new(HomeActions { home: logged(&log, "action", "OK") }).with(record(&log, "controller middleware")));

    assert_eq!(fetch(&router, "GET", "https://remix.run/"), (200, "OK".into()));
    assert_eq!(take(&log), ["router middleware", "controller middleware", "action"]);
}

// --- router.get() ------------------------------------------------------------------------

#[test]
fn get_maps_a_single_route_to_a_request_handler() {
    let mut router = Router::new();
    router.get("/", |_: Context| async { "Home" });

    assert_eq!(fetch(&router, "GET", "https://remix.run"), (200, "Home".into()));
}

#[test]
fn get_maps_a_single_route_to_an_action_with_middleware() {
    let log = log();
    let mut router = Router::new();
    router.get("/", action(|_: Context| async { "Home" }).with(record(&log, "middleware")));

    assert_eq!(fetch(&router, "GET", "https://remix.run"), (200, "Home".into()));
    assert_eq!(take(&log), ["middleware"]);
}

// --- action middleware -------------------------------------------------------------------

#[test]
fn action_middleware_runs_after_router_middleware() {
    let log = log();
    let mut router = Router::new().with(record(&log, "router middleware"));
    router.get(
        "/",
        logged(&log, "action", "OK").with(record(&log, "action middleware 1")).with(record(&log, "action middleware 2")),
    );

    assert_eq!(fetch(&router, "GET", "https://remix.run/").0, 200);
    assert_eq!(take(&log), ["router middleware", "action middleware 1", "action middleware 2", "action"]);
}

#[test]
fn action_middleware_runs_only_on_the_route_it_is_defined_on() {
    let log = log();
    let mut router = Router::new();
    router.get("/a", logged(&log, "action-a", "A").with(record(&log, "action middleware a")));
    router.get("/b", logged(&log, "action-b", "B"));

    assert_eq!(fetch(&router, "GET", "https://remix.run/a"), (200, "A".into()));
    assert_eq!(take(&log), ["action middleware a", "action-a"]);
    assert_eq!(fetch(&router, "GET", "https://remix.run/b"), (200, "B".into()));
    assert_eq!(take(&log), ["action-b"]);
}

#[test]
fn action_middleware_works_with_an_empty_middleware_array() {
    let log = log();
    let mut router = Router::new();
    // `action(handler)` with no `.with(...)` calls is the empty middleware array.
    router.get("/", logged(&log, "action", "OK"));

    assert_eq!(fetch(&router, "GET", "https://remix.run/").0, 200);
    assert_eq!(take(&log), ["action"]);
}

#[test]
fn action_middleware_works_with_action_objects_that_omit_middleware() {
    let log = log();
    let mut router = Router::new();
    let l = log.clone();
    // A plain handler converts into an action without middleware.
    router.get("/", move |_: Context| {
        push(&l, "action");
        async { "OK" }
    });

    assert_eq!(fetch(&router, "GET", "https://remix.run/").0, 200);
    assert_eq!(take(&log), ["action"]);
}

#[test]
fn handles_middleware_that_returns_a_response_short_circuits() {
    let log = log();
    let l = log.clone();
    let mut router = Router::new();
    router.get(
        "/",
        logged(&log, "action", "OK")
            .with(record(&log, "m1"))
            .with(move |_: Context, _: Next| {
                push(&l, "m2-short-circuit");
                async { (StatusCode::FORBIDDEN, "Blocked").into_response() }
            })
            .with(record(&log, "m3")),
    );

    assert_eq!(fetch(&router, "GET", "https://remix.run/"), (403, "Blocked".into()));
    assert_eq!(take(&log), ["m1", "m2-short-circuit"]);
}

#[test]
fn runs_both_controller_and_action_middleware() {
    #[derive(RouteMap)]
    struct Admin {
        dashboard: Route,
        users: Route,
    }
    let routes = Admin { dashboard: any("/admin/dashboard"), users: any("/admin/users") };
    let log = log();
    let mut router = Router::new();
    router.map(
        &routes,
        Controller::new(AdminActions {
            dashboard: logged(&log, "dashboard-action", "Dashboard"),
            users: logged(&log, "users-action", "Users").with(record(&log, "users-action-middleware")),
        })
        .with(record(&log, "auth"))
        .with(record(&log, "admin")),
    );

    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/dashboard"), (200, "Dashboard".into()));
    assert_eq!(take(&log), ["auth", "admin", "dashboard-action"]);
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/users"), (200, "Users".into()));
    assert_eq!(take(&log), ["auth", "admin", "users-action-middleware", "users-action"]);
}

#[test]
fn supports_route_map_action_objects_that_omit_middleware() {
    let mut router = Router::new();
    router.map(&home().home, action(|_: Context| async { "Home" }));

    assert_eq!(fetch(&router, "GET", "https://remix.run/"), (200, "Home".into()));
}

#[test]
fn allows_action_object_form_without_middleware() {
    // Remix checks the type only; here an `Action` without middleware is built and mapped.
    let action: Action = action(|_: Context| async { "Home" });
    let mut router = Router::new();
    router.map(&home().home, action);

    assert_eq!(fetch(&router, "GET", "https://remix.run/"), (200, "Home".into()));
}

// --- 404 handling ------------------------------------------------------------------------

#[test]
fn returns_a_404_response_when_no_route_matches() {
    let mut router = Router::new();
    router.get("/home", |_: Context| async { "Home" });

    assert_eq!(fetch(&router, "GET", "https://remix.run/nonexistent"), (404, "Not Found: /nonexistent".into()));
}

#[test]
fn supports_a_custom_default_handler() {
    let mut router = Router::new();
    router.not_found(|ctx: Context| {
        let mut res = response::status(StatusCode::NOT_FOUND, format!("Custom 404: {}", ctx.url().pathname()));
        res.headers_mut().insert("x-custom", "true".parse().unwrap());
        async move { res }
    });
    router.get("/home", |_: Context| async { "Home" });

    let res = fetch_res(&router, "GET", "https://remix.run/missing");

    assert_eq!(res.status(), 404);
    assert_eq!(res.headers()["x-custom"], "true");
    assert_eq!(block_on(res.into_body().text()).unwrap(), "Custom 404: /missing");
}

#[test]
fn calls_the_default_handler_only_when_no_routes_match() {
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let mut router = Router::new();
    router.not_found(move |_: Context| {
        c.fetch_add(1, Ordering::SeqCst);
        async { (StatusCode::NOT_FOUND, "Not Found") }
    });
    router.get("/", |_: Context| async { "Home" });
    router.get("/about", |_: Context| async { "About" });

    fetch(&router, "GET", "https://remix.run/");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    fetch(&router, "GET", "https://remix.run/about");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    fetch(&router, "GET", "https://remix.run/missing");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

// --- 405 handling ------------------------------------------------------------------------

#[test]
fn returns_a_405_response_when_routes_match_the_url_but_not_the_method() {
    let mut router = Router::new();
    router.get("/home", |_: Context| async { "Home" });

    let res = fetch_res(&router, "POST", "https://remix.run/home");

    assert_eq!(res.status(), 405);
    assert_eq!(res.headers()["allow"], "GET, HEAD");
    assert_eq!(block_on(res.into_body().text()).unwrap(), "Method Not Allowed: POST");
}

#[test]
fn includes_every_method_allowed_for_the_url_in_the_allow_header() {
    let mut router = Router::new();
    router.get("/posts/:id", |_: Context| async { "Show" });
    router.put("/posts/:id", |_: Context| async { "Update" });
    router.delete("/posts/:id", |_: Context| async { "Destroy" });

    let res = fetch_res(&router, "POST", "https://remix.run/posts/1");

    assert_eq!(res.status(), 405);
    assert_eq!(res.headers()["allow"], "GET, HEAD, PUT, DELETE");
}

#[test]
fn falls_through_to_a_less_specific_route_that_allows_the_method() {
    let mut router = Router::new();
    router.get("/posts/featured", |_: Context| async { "Featured" });
    router.any("/posts/:id", |_: Context| async { "Any post" });

    assert_eq!(fetch(&router, "POST", "https://remix.run/posts/featured"), (200, "Any post".into()));
}

#[test]
fn does_not_return_405_when_no_route_matches_the_url() {
    let mut router = Router::new();
    router.get("/home", |_: Context| async { "Home" });

    assert_eq!(fetch(&router, "POST", "https://remix.run/missing").0, 404);
}

#[test]
fn lets_an_any_catch_all_route_handle_mismatched_methods_without_a_405() {
    let mut router = Router::new();
    router.get("/home", |_: Context| async { "Home" });
    router.any("/*rest", |_: Context| async { "Catch-all" });

    assert_eq!(fetch(&router, "POST", "https://remix.run/home"), (200, "Catch-all".into()));
}

// --- HEAD request handling ---------------------------------------------------------------

#[test]
fn serves_head_requests_with_a_get_route_and_strips_the_response_body() {
    let mut router = Router::new();
    router.get("/file", |_: Context| async {
        let mut res = with_header("content", "x-extra", "yes");
        res.headers_mut().insert("content-type", "text/plain".parse().unwrap());
        res
    });

    let res = fetch_res(&router, "HEAD", "https://remix.run/file");

    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["x-extra"], "yes");
    assert!(res.body().is_empty_hint(), "the body is null");
}

#[test]
fn prefers_an_explicit_head_route_over_a_get_route_for_the_same_pattern() {
    let mut router = Router::new();
    router.get("/file", |_: Context| async { "content" });
    router.head("/file", |_: Context| async { with_header("", "x-head", "explicit") });

    let res = fetch_res(&router, "HEAD", "https://remix.run/file");

    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["x-head"], "explicit");
}

#[test]
fn prefers_a_more_specific_get_route_over_a_less_specific_head_route() {
    let mut router = Router::new();
    router.head("/*rest", |_: Context| async { with_header("", "x-head", "catch-all") });
    router.get("/file", |_: Context| async { with_header("content", "x-get", "specific") });

    let res = fetch_res(&router, "HEAD", "https://remix.run/file");

    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["x-get"], "specific");
    assert!(res.body().is_empty_hint(), "the body is null");
}

#[test]
fn runs_action_middleware_when_a_get_route_serves_a_head_request() {
    let log = log();
    let mut router = Router::new();
    router.get("/file", logged(&log, "handler", "content").with(record(&log, "middleware")));

    let res = fetch_res(&router, "HEAD", "https://remix.run/file");

    assert_eq!(res.status(), 200);
    assert!(res.body().is_empty_hint(), "the body is null");
    assert_eq!(take(&log), ["middleware", "handler"]);
}

#[test]
fn returns_a_405_response_for_head_requests_when_only_other_methods_match() {
    let mut router = Router::new();
    router.post("/submit", |_: Context| async { "Submitted" });

    let res = fetch_res(&router, "HEAD", "https://remix.run/submit");

    assert_eq!(res.status(), 405);
    assert_eq!(res.headers()["allow"], "POST");
}

// --- error handling ----------------------------------------------------------------------

#[test]
fn propagates_errors_thrown_in_request_handlers() {
    let mut router = Router::new();
    router.get("/", |_: Context| -> std::future::Ready<&'static str> { panic!("Action error") });

    assert_eq!(rejects(&router, "https://remix.run/"), "Action error");
}

#[test]
fn propagates_async_errors_thrown_in_route_actions() {
    let mut router = Router::new();
    router.get("/", |_: Context| async {
        std::future::ready(()).await;
        panic!("Async action error");
        #[allow(unreachable_code)]
        "OK"
    });

    assert_eq!(rejects(&router, "https://remix.run/"), "Async action error");
}

#[test]
fn propagates_errors_thrown_in_router_middleware() {
    let mut router = Router::new().with(|_: Context, _: Next| -> BoxFuture<Response> { panic!("Router middleware error") });
    router.get("/", |_: Context| async { "OK" });

    assert_eq!(rejects(&router, "https://remix.run/"), "Router middleware error");
}

#[test]
fn propagates_errors_thrown_in_action_middleware() {
    let mut router = Router::new();
    router.get(
        "/",
        action(|_: Context| async { "OK" }).with(|_: Context, _: Next| -> BoxFuture<Response> { panic!("Action middleware error") }),
    );

    assert_eq!(rejects(&router, "https://remix.run/"), "Action middleware error");
}

#[test]
fn propagates_errors_thrown_in_the_default_handler() {
    let mut router = Router::new();
    router.not_found(|_: Context| -> std::future::Ready<&'static str> { panic!("Default handler error") });
    router.get("/home", |_: Context| async { "Home" });

    assert_eq!(rejects(&router, "https://remix.run/missing"), "Default handler error");
}

#[test]
fn allows_middleware_to_catch_and_handle_errors_from_downstream() {
    let mut router = Router::new().with(|ctx: Context, next: Next| async move {
        match catch_panic(next.run(ctx)).await {
            Ok(res) => res,
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Caught: {error}")).into_response(),
        }
    });
    router.get("/", |_: Context| -> std::future::Ready<&'static str> { panic!("Action error") });

    assert_eq!(fetch(&router, "GET", "https://remix.run/"), (500, "Caught: Action error".into()));
}

// --- trailing slash handling -------------------------------------------------------------

#[test]
fn matches_routes_with_and_without_trailing_slashes_for_single_path_routes() {
    let mut router = Router::new();
    router.get("/about", |_: Context| async { "About" });
    router.get("/contact/", |_: Context| async { "Contact" });

    assert_eq!(fetch(&router, "GET", "https://remix.run/about"), (200, "About".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/about/").0, 404);
    assert_eq!(fetch(&router, "GET", "https://remix.run/contact/"), (200, "Contact".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/contact").0, 404);
}

#[test]
fn matches_routes_with_and_without_trailing_slashes_for_create_routes() {
    #[derive(RouteMap)]
    struct Api {
        users: Route,
        posts: Route,
    }
    let api = prefix("api");
    let routes = Api { users: api.any("/users"), posts: api.any("/posts/") };
    let mut router = Router::new();
    router.get(&routes.users, |_: Context| async { "Users" });
    router.get(&routes.posts, |_: Context| async { "Posts" });

    assert_eq!(fetch(&router, "GET", "https://remix.run/api/users"), (200, "Users".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/api/users/").0, 404);
    assert_eq!(fetch(&router, "GET", "https://remix.run/api/posts/"), (200, "Posts".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/api/posts").0, 404);
}

#[test]
fn handles_root_path_with_and_without_trailing_slash() {
    let mut router = Router::new();
    router.get("/", |_: Context| async { "Home" });

    assert_eq!(fetch(&router, "GET", "https://remix.run/"), (200, "Home".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run"), (200, "Home".into()));
}

#[test]
fn handles_nested_routes_with_trailing_slash_combinations() {
    #[derive(RouteMap)]
    struct Admin {
        dashboard: Route,
        users: Users,
    }
    #[derive(RouteMap)]
    struct Users {
        index: Route,
        show: Route,
    }
    let admin = prefix("admin");
    let routes = Admin { dashboard: admin.any("/"), users: Users { index: admin.any("/users"), show: admin.any("/users/:id") } };
    let mut router = Router::new();
    router.get(&routes.dashboard, |_: Context| async { "Admin Dashboard" });
    router.get(&routes.users.index, |_: Context| async { "Users List" });
    router.get(&routes.users.show, |ctx: Context| async move { format!("User {}", ctx.param("id").unwrap()) });

    assert_eq!(fetch(&router, "GET", "https://remix.run/admin"), (200, "Admin Dashboard".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/").0, 404);
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/users"), (200, "Users List".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/users/").0, 404);
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/users/123"), (200, "User 123".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/users/123/").0, 404);
}

// --- router.mount() ----------------------------------------------------------------------

#[test]
fn registers_routes_below_a_static_pathname_prefix() {
    let mut router = Router::new();
    router.mount("/admin", |admin| {
        admin.get("/", |_: Context| async { "Admin" });
        admin.get("/users/:id", |ctx: Context| async move { format!("User {}", ctx.param("id").unwrap()) });
    });

    assert_eq!(fetch(&router, "GET", "https://remix.run/admin"), (200, "Admin".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/users/123"), (200, "User 123".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/users/123").0, 404);
}

#[test]
fn registers_routes_below_a_route_pattern_prefix_with_params() {
    let mut router = Router::new();
    router.mount("/orgs/:orgId", |org| {
        org.get("/", |ctx: Context| async move { format!("Org {}", ctx.param("orgId").unwrap()) });
        org.get("/users/:userId", |ctx: Context| async move {
            format!("Org {} User {}", ctx.param("orgId").unwrap(), ctx.param("userId").unwrap())
        });
    });

    assert_eq!(fetch(&router, "GET", "https://remix.run/orgs/acme"), (200, "Org acme".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/orgs/acme/users/123"), (200, "Org acme User 123".into()));
}

#[test]
fn uses_route_pattern_semantics_for_mounted_route_patterns() {
    let mut router = Router::new();
    router.mount("/files/*path?download=true", |files| {
        files.get("/raw/:id", |ctx: Context| async move {
            format!("Path {} File {}", ctx.param("path").unwrap(), ctx.param("id").unwrap())
        });
    });

    assert_eq!(fetch(&router, "GET", "https://remix.run/files/a/b/raw/123?download=true"), (200, "Path a/b File 123".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/files/a/b/raw/123").0, 404);
}

#[test]
fn uses_the_right_most_param_when_mounted_and_child_routes_share_a_param_name() {
    let mut router = Router::new();
    router.mount("/orgs/:id", |org| {
        org.get("/users/:id", |ctx: Context| async move { format!("User {}", ctx.param("id").unwrap()) });
    });

    assert_eq!(fetch(&router, "GET", "https://remix.run/orgs/acme/users/123"), (200, "User 123".into()));
}

#[test]
fn maps_route_maps_and_controllers_below_the_mount_prefix() {
    #[derive(RouteMap)]
    struct AdminRoutes {
        index: Route,
        users: Users,
    }
    #[derive(RouteMap)]
    struct Users {
        show: Route,
    }
    let admin_routes = AdminRoutes { index: any("/"), users: Users { show: any("/users/:id") } };
    let mut router = Router::new();
    router.mount("/admin", |admin| {
        admin.map(&admin_routes, AdminRoutesActions { index: action(|_: Context| async { "Admin" }) });
        admin.map(&admin_routes.users, UsersActions { show: action(|ctx: Context| async move { format!("User {}", ctx.param("id").unwrap()) }) });
    });

    assert_eq!(fetch(&router, "GET", "https://remix.run/admin"), (200, "Admin".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/users/123"), (200, "User 123".into()));
}

#[test]
fn composes_nested_mount_prefixes() {
    let mut router = Router::new();
    router.mount("/api", |api| {
        api.mount("/v1", |v1| {
            v1.get("/status", |_: Context| async { "ok" });
        });
    });

    assert_eq!(fetch(&router, "GET", "https://remix.run/api/v1/status"), (200, "ok".into()));
}

#[test]
fn preserves_router_controller_and_action_middleware_order() {
    #[derive(RouteMap)]
    struct Routes {
        update: Route,
    }
    let routes = Routes { update: post("/update") };
    let log = log();
    let mut router = Router::new().with(around(&log, "router"));
    router.mount("/admin", |admin| {
        admin.map(
            &routes,
            Controller::new(RoutesActions { update: logged(&log, "handler", "Updated").with(around(&log, "action")) })
                .with(around(&log, "controller")),
        );
    });

    assert_eq!(fetch(&router, "POST", "https://remix.run/admin/update"), (200, "Updated".into()));
    assert_eq!(
        take(&log),
        ["router before", "controller before", "action before", "handler", "action after", "controller after", "router after"]
    );
}

#[test]
fn lets_existing_controller_middleware_short_circuit_mounted_handlers() {
    let log = log();
    let mut router = Router::new();
    router.mount("/admin", |admin| {
        admin.get(
            "/dashboard",
            logged(&log, "handled", "Dashboard").with(|_: Context, _: Next| async { (StatusCode::UNAUTHORIZED, "Unauthorized").into_response() }),
        );
    });

    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/dashboard"), (401, "Unauthorized".into()));
    assert!(take(&log).is_empty());
}

#[test]
fn shares_matcher_specificity_between_parent_and_mounted_routes() {
    let mut router = Router::new();
    router.mount("/api", |api| {
        api.get("/:id", |ctx: Context| async move { format!("Mounted {}", ctx.param("id").unwrap()) });
    });
    router.get("/api/health", |_: Context| async { "Parent health" });

    assert_eq!(fetch(&router, "GET", "https://remix.run/api/health"), (200, "Parent health".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/api/users"), (200, "Mounted users".into()));
}

#[test]
fn uses_the_parent_default_handler_for_unknown_mounted_paths() {
    let mut router = Router::new();
    router.not_found(|ctx: Context| {
        let path = ctx.url().pathname().to_string();
        async move { (StatusCode::NOT_FOUND, format!("Missing {path}")) }
    });
    router.mount("/admin", |admin| {
        admin.get("/", |_: Context| async { "Admin" });
    });

    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/missing"), (404, "Missing /admin/missing".into()));
}

#[test]
fn sets_context_router_to_the_parent_router_in_mounted_handlers() {
    let seen: Arc<Mutex<Option<Router>>> = Arc::new(Mutex::new(None));
    let s = seen.clone();
    let mut router = Router::new();
    router.mount("/admin", |admin| {
        admin.get("/", move |ctx: Context| {
            *s.lock().unwrap() = Some(ctx.router().clone());
            async { "Admin" }
        });
    });

    assert_eq!(fetch(&router, "GET", "https://remix.run/admin"), (200, "Admin".into()));
    assert_eq!(seen.lock().unwrap().take(), Some(router.clone()));
    assert_ne!(Some(Router::new()), Some(router));
}

/// A matcher that records the patterns added and counts `match_all` calls, delegating to the
/// built-in [`Matcher`] (the custom `MultiMatcher` objects Remix's tests build).
#[derive(Default)]
struct RecordingMatcher {
    inner: Matcher<Arc<RouteEntry>>,
    added: Arc<Mutex<Vec<String>>>,
    match_all_calls: Arc<AtomicUsize>,
}

impl MultiMatcher<Arc<RouteEntry>> for RecordingMatcher {
    fn ignore_case(&self) -> bool {
        self.inner.ignore_case()
    }

    fn add(&mut self, pattern: RoutePattern, data: Arc<RouteEntry>) -> Result<(), MatcherResourceError> {
        self.added.lock().unwrap().push(pattern.to_string());
        self.inner.add(pattern, data)
    }

    fn match_url(&self, url: &Url) -> Result<Option<Match<&Arc<RouteEntry>>>, MatcherResourceError> {
        self.inner.match_url(url)
    }

    fn match_all(&self, url: &Url) -> Result<Vec<Match<&Arc<RouteEntry>>>, MatcherResourceError> {
        self.match_all_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.match_all(url)
    }
}

#[test]
fn adds_mounted_routes_to_custom_matchers_with_their_prefixed_patterns() {
    let matcher = RecordingMatcher::default();
    let added = matcher.added.clone();
    let mut router = Router::with_matcher(matcher);
    router.mount("/admin/:adminId", |admin| {
        admin.get("/", |_: Context| async { "Admin" });
        admin.get("/users/:id", |_: Context| async { "User" });
        admin.mount("/settings", |settings| {
            settings.get("/profile", |_: Context| async { "Profile" });
        });
    });

    assert_eq!(*added.lock().unwrap(), ["/admin/:adminId", "/admin/:adminId/users/:id", "/admin/:adminId/settings/profile"]);
}

// --- custom matcher ----------------------------------------------------------------------

#[test]
fn uses_a_custom_matcher_when_provided() {
    let matcher = RecordingMatcher::default();
    let calls = matcher.match_all_calls.clone();
    let mut router = Router::with_matcher(matcher);
    router.get("/", |_: Context| async { "Home" });

    fetch(&router, "GET", "https://remix.run/");

    assert!(calls.load(Ordering::SeqCst) > 0, "the custom matcher is called");
}

#[test]
fn adds_routes_to_the_custom_matcher() {
    let matcher = RecordingMatcher::default();
    let added = matcher.added.clone();
    let mut router = Router::with_matcher(matcher);
    router.get("/home", |_: Context| async { "Home" });
    router.get("/about", |_: Context| async { "About" });

    assert_eq!(*added.lock().unwrap(), ["/home", "/about"]);
}
