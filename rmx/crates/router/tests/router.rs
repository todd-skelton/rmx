//! Ports of fetch-router's router.test.ts, plus route map listing (`remix routes`).
//!
//! Not ported: Remix's runtime rejections of malformed controllers (missing, unknown or
//! nested action keys), which are compile errors here (see the `compile_fail` examples on
//! `RouteMap`); thrown-error propagation, since Rust handlers return errors as values
//! (`Result` responses) and the server answers panics with 500; and custom matchers, which
//! rmx-router does not take (`Router::routes` lists the registered patterns instead).

use std::sync::{Arc, Mutex};

use rmx::fetch::{block_on, Body, Request, Response};
use rmx::router::routes::{form, get, post, prefix, Route, RouteMap, RouteTree};
use rmx::router::{action, Context, Controller, IntoResponse, Next, Router};

fn req(method: &str, url: &str) -> Request {
    http::Request::builder().method(method).uri(url).body(Body::empty()).unwrap()
}

fn fetch(router: &Router, method: &str, url: &str) -> (u16, String) {
    let res: Response = block_on(router.fetch(req(method, url)));
    let status = res.status().as_u16();
    (status, block_on(res.into_body().text()).unwrap())
}

type Log = Arc<Mutex<Vec<String>>>;

fn log() -> Log {
    Arc::new(Mutex::new(Vec::new()))
}

fn take(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.lock().unwrap())
}

/// Middleware that records `name` and continues.
fn record(log: &Log, name: &'static str) -> impl Fn(Context, Next) -> rmx::router::BoxFuture<Response> + Send + Sync + 'static {
    let log = log.clone();
    move |ctx: Context, next: Next| {
        log.lock().unwrap().push(name.to_string());
        next.run(ctx)
    }
}

/// Middleware that records `<name> before` and `<name> after` around the rest of the chain.
fn around(log: &Log, name: &'static str) -> impl Fn(Context, Next) -> rmx::router::BoxFuture<Response> + Send + Sync + 'static {
    let log = log.clone();
    move |ctx: Context, next: Next| {
        let log = log.clone();
        Box::pin(async move {
            log.lock().unwrap().push(format!("{name} before"));
            let res = next.run(ctx).await;
            log.lock().unwrap().push(format!("{name} after"));
            res
        })
    }
}

#[derive(RouteMap)]
pub struct Site {
    pub home: Route,
    pub blog: Blog,
}

#[derive(RouteMap)]
pub struct Blog {
    pub index: Route,
    pub create: Route,
    pub show: Route,
}

fn site() -> Site {
    Site { home: get("/"), blog: Blog { index: get("/blog"), create: post("/blog"), show: get("/blog/:id") } }
}

// --- router.map() with single routes ---------------------------------------------------

#[test]
fn maps_a_single_route_to_a_request_handler() {
    let mut router = Router::new();
    router.map(&get("/"), action(|_: Context| async { "Home" }));
    assert_eq!(fetch(&router, "GET", "https://remix.run"), (200, "Home".into()));
}

#[test]
fn maps_a_single_route_to_an_action_with_middleware() {
    let log = log();
    let mut router = Router::new();
    let l = log.clone();
    let mw = move |ctx: Context, next: Next| {
        l.lock().unwrap().push(format!("middleware {}", ctx.param("id").unwrap()));
        next.run(ctx)
    };
    let l = log.clone();
    router.map(
        &rmx::router::routes::any("/profile/:id"),
        action(move |_: Context| {
            l.lock().unwrap().push("action".into());
            async { "OK" }
        })
        .with(mw),
    );
    assert_eq!(fetch(&router, "GET", "https://remix.run/profile/1"), (200, "OK".into()));
    assert_eq!(take(&log), ["middleware 1", "action"]);
}

#[test]
fn a_route_without_a_method_matches_any_request_method() {
    let mut router = Router::new();
    router.map(&rmx::router::routes::any("/"), action(|ctx: Context| async move { ctx.method().to_string() }));
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        assert_eq!(fetch(&router, method, "https://remix.run"), (200, method.to_string()));
    }
    assert_eq!(fetch(&router, "HEAD", "https://remix.run").0, 200);
}

// --- router.map() ----------------------------------------------------------------------

#[test]
fn maps_route_maps_to_controllers() {
    let routes = site();
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
    #[derive(RouteMap)]
    struct Routes {
        home: Route,
    }
    let log = log();
    let mut router = Router::new();
    let l = log.clone();
    router.map(
        &Routes { home: rmx::router::routes::any("/") },
        Controller::new(RoutesActions {
            home: action(move |_: Context| {
                l.lock().unwrap().push("action".into());
                async { "OK" }
            }),
        })
        .with(record(&log, "middleware")),
    );
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
    let routes = Routes { home: get("/"), blog: BlogIndex { index: get("/blog") } };
    let log = log();
    let mut router = Router::new();
    let l = log.clone();
    router.map(
        &routes,
        Controller::new(RoutesActions {
            home: action(move |_: Context| {
                l.lock().unwrap().push("home".into());
                async { "Home" }
            }),
        })
        .with(record(&log, "outer middleware")),
    );
    let l = log.clone();
    router.map(
        &routes.blog,
        BlogIndexActions {
            index: action(move |_: Context| {
                l.lock().unwrap().push("blog-index".into());
                async { "Blog" }
            }),
        },
    );
    assert_eq!(fetch(&router, "GET", "https://remix.run/"), (200, "Home".into()));
    assert_eq!(take(&log), ["outer middleware", "home"]);
    assert_eq!(fetch(&router, "GET", "https://remix.run/blog"), (200, "Blog".into()));
    assert_eq!(take(&log), ["blog-index"]);
}

#[test]
fn allows_selective_middleware_by_mapping_nested_route_subtrees_separately() {
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
    let routes = Routes { public: get("/public"), admin: Admin { dashboard: get("/admin/dashboard"), users: get("/admin/users") } };
    let log = log();
    let mut router = Router::new();
    let l = log.clone();
    router.map(
        &routes.public,
        action(move |_: Context| {
            l.lock().unwrap().push("public".into());
            async { "Public" }
        }),
    );
    let (l1, l2) = (log.clone(), log.clone());
    router.map(
        &routes.admin,
        Controller::new(AdminActions {
            dashboard: action(move |_: Context| {
                l1.lock().unwrap().push("dashboard".into());
                async { "Dashboard" }
            }),
            users: action(move |_: Context| {
                l2.lock().unwrap().push("users".into());
                async { "Users" }
            }),
        })
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
    #[derive(RouteMap)]
    struct Routes {
        home: Route,
    }
    let log = log();
    let mut router = Router::new().with(record(&log, "router middleware"));
    let l = log.clone();
    router.map(
        &Routes { home: get("/") },
        Controller::new(RoutesActions {
            home: action(move |_: Context| {
                l.lock().unwrap().push("action".into());
                async { "OK" }
            }),
        })
        .with(record(&log, "controller middleware")),
    );
    assert_eq!(fetch(&router, "GET", "https://remix.run/"), (200, "OK".into()));
    assert_eq!(take(&log), ["router middleware", "controller middleware", "action"]);
}

// --- router.mount() --------------------------------------------------------------------

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
    let admin_routes = AdminRoutes { index: rmx::router::routes::any("/"), users: Users { show: rmx::router::routes::any("/users/:id") } };
    let mut router = Router::new();
    router.mount("/admin", |admin| {
        admin.map(&admin_routes, AdminRoutesActions { index: action(|_: Context| async { "Admin" }) });
        admin.map(
            &admin_routes.users,
            UsersActions { show: action(|ctx: Context| async move { format!("User {}", ctx.param("id").unwrap()) }) },
        );
    });
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin"), (200, "Admin".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/users/123"), (200, "User 123".into()));
}

#[test]
fn preserves_router_controller_and_action_middleware_order() {
    #[derive(RouteMap)]
    struct Routes {
        update: Route,
    }
    let log = log();
    let mut router = Router::new().with(around(&log, "router"));
    let l = log.clone();
    router.mount("/admin", |admin| {
        admin.map(
            &Routes { update: post("/update") },
            Controller::new(RoutesActions {
                update: action(move |_: Context| {
                    l.lock().unwrap().push("handler".into());
                    async { "Updated" }
                })
                .with(around(&log, "action")),
            })
            .with(around(&log, "controller")),
        );
    });
    assert_eq!(fetch(&router, "POST", "https://remix.run/admin/update"), (200, "Updated".into()));
    assert_eq!(
        take(&log),
        [
            "router before",
            "controller before",
            "action before",
            "handler",
            "action after",
            "controller after",
            "router after"
        ]
    );
}

#[test]
fn lets_action_middleware_short_circuit_mounted_handlers() {
    let handled = Arc::new(Mutex::new(false));
    let h = handled.clone();
    let mut router = Router::new();
    router.mount("/admin", |admin| {
        admin.map(
            &get("/dashboard"),
            action(move |_: Context| {
                *h.lock().unwrap() = true;
                async { "Dashboard" }
            })
            .with(|_: Context, _: Next| async { (http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response() }),
        );
    });
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/dashboard"), (401, "Unauthorized".into()));
    assert!(!*handled.lock().unwrap());
}

#[test]
fn runs_action_middleware_when_a_get_route_serves_a_head_request() {
    let log = log();
    let mut router = Router::new();
    router.map(&get("/"), action(|_: Context| async { "Home" }).with(record(&log, "action middleware")));
    assert_eq!(fetch(&router, "HEAD", "https://remix.run/"), (200, String::new()));
    assert_eq!(take(&log), ["action middleware"]);
}

// --- built-in route maps ---------------------------------------------------------------

#[test]
fn maps_forms_and_resources() {
    use rmx::router::routes::{resources, FormActions, ResourcesActions};
    let contact = form("contact");
    let books = resources("books");
    let mut router = Router::new();
    router.map(
        &contact,
        FormActions { index: action(|_: Context| async { "form" }), action: action(|_: Context| async { "sent" }) },
    );
    let named = |name: &'static str| action(move |_: Context| async move { name });
    router.map(
        &books,
        ResourcesActions {
            index: named("index"),
            new: named("new"),
            show: named("show"),
            create: named("create"),
            edit: named("edit"),
            update: named("update"),
            destroy: named("destroy"),
        },
    );
    assert_eq!(fetch(&router, "GET", "https://x/contact").1, "form");
    assert_eq!(fetch(&router, "POST", "https://x/contact").1, "sent");
    assert_eq!(fetch(&router, "GET", "https://x/books/new").1, "new");
    assert_eq!(fetch(&router, "GET", "https://x/books/1/edit").1, "edit");
    assert_eq!(fetch(&router, "PUT", "https://x/books/1").1, "update");
    assert_eq!(fetch(&router, "DELETE", "https://x/books/1").1, "destroy");
    assert_eq!(fetch(&router, "POST", "https://x/books").1, "create");
}

#[test]
fn route_maps_work_through_lazy_lock() {
    static ROUTES: std::sync::LazyLock<Site> = std::sync::LazyLock::new(site);
    let mut router = Router::new();
    router.map(&ROUTES, SiteActions { home: action(|_: Context| async { "Home" }) });
    assert_eq!(fetch(&router, "GET", "https://x/"), (200, "Home".into()));
    assert_eq!(ROUTES.route_tree(), site().route_tree());
}

// --- listing (remix routes) ------------------------------------------------------------

#[derive(RouteMap)]
pub struct App {
    pub home: Route,
    pub auth: Auth,
    pub books: rmx::router::routes::Resources,
}

#[derive(RouteMap)]
pub struct Auth {
    pub login: rmx::router::routes::Form,
    pub logout: Route,
}

fn app() -> App {
    let auth = prefix("auth");
    App {
        home: rmx::router::routes::any("/"),
        auth: Auth { login: form("login"), logout: auth.post("logout") },
        books: rmx::router::routes::resources_with_param("books", "bookId"),
    }
}

#[test]
fn lists_a_route_map_as_a_tree() {
    assert_eq!(
        app().route_tree().format_tree(),
        "\
home  ANY    /
auth
├─ login
│  ├─ index   GET    /login
│  └─ action  POST   /login
└─ logout  POST   /auth/logout
books
├─ index    GET    /books
├─ new      GET    /books/new
├─ show     GET    /books/:bookId
├─ create   POST   /books
├─ edit     GET    /books/:bookId/edit
├─ update   PUT    /books/:bookId
└─ destroy  DELETE /books/:bookId
"
    );
}

#[test]
fn lists_a_route_map_as_a_table() {
    let table = app().route_tree().format_table(true);
    let lines: Vec<&str> = table.lines().collect();
    assert_eq!(lines[0], "Route              Method  Path");
    assert_eq!(lines[1], "home               ANY     /");
    assert_eq!(lines[2], "auth.login.index   GET     /login");
    assert_eq!(lines[3], "auth.login.action  POST    /login");
    assert_eq!(lines[11], "books.destroy      DELETE  /books/:bookId");
    assert_eq!(lines.len(), 12);
    let no_headers = app().route_tree().format_table(false);
    assert!(no_headers.starts_with("home "));
    assert_eq!(no_headers.lines().count(), 11);
}

#[test]
fn lists_a_route_map_as_json() {
    let json = Site { home: get("/"), blog: Blog { index: get("/blog"), create: post("/blog"), show: get("/blog/:id") } }
        .route_tree()
        .to_json();
    assert_eq!(
        json,
        concat!(
            r#"{"tree":["#,
            r#"{"children":[],"key":"home","kind":"route","method":"GET","name":"home","pattern":"/"},"#,
            r#"{"children":["#,
            r#"{"children":[],"key":"index","kind":"route","method":"GET","name":"blog.index","pattern":"/blog"},"#,
            r#"{"children":[],"key":"create","kind":"route","method":"POST","name":"blog.create","pattern":"/blog"},"#,
            r#"{"children":[],"key":"show","kind":"route","method":"GET","name":"blog.show","pattern":"/blog/:id"}"#,
            r#"],"key":"blog","kind":"group","name":"blog"}"#,
            r#"]}"#
        )
    );
}

#[test]
fn lists_a_single_route_and_an_empty_map() {
    assert_eq!(get("/").route_tree().format_tree(), "route  GET    /\n");
    assert_eq!(RouteTree::Group(vec![]).format_tree(), "No routes.\n");
}

#[test]
fn routes_command_parses_flags_like_remix_routes() {
    use rmx::router::routes::routes_command_output;
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let routes = app();
    assert_eq!(routes_command_output(&routes, &args(&[])).unwrap(), routes.route_tree().format_tree());
    assert_eq!(routes_command_output(&routes, &args(&["--table"])).unwrap(), routes.route_tree().format_table(true));
    assert_eq!(routes_command_output(&routes, &args(&["--table", "--no-headers"])).unwrap(), routes.route_tree().format_table(false));
    assert_eq!(routes_command_output(&routes, &args(&["--json"])).unwrap(), format!("{}\n", routes.route_tree().to_json()));
    assert!(routes_command_output(&routes, &args(&["--help"])).unwrap().starts_with("Usage:\n  routes [--json | --table]"));
    assert_eq!(routes_command_output(&routes, &args(&["--json", "--table"])).unwrap_err(), "Cannot combine --json with --table.");
    assert_eq!(routes_command_output(&routes, &args(&["--no-headers"])).unwrap_err(), "Cannot use --no-headers without --table.");
    assert_eq!(routes_command_output(&routes, &args(&["--verbose"])).unwrap_err(), "Unknown option --verbose.");
}

// --- router.fetch(), action middleware, 404, 405 and HEAD (router.test.ts) --------------

fn fetch_res(router: &Router, method: &str, url: &str) -> Response {
    block_on(router.fetch(req(method, url)))
}

#[test]
fn fetches_a_route_with_specific_method_actions() {
    let home = get("/");
    let mut router = Router::new();
    router.get(&home, |_: Context| async { "GET" });
    router.head(&home, |_: Context| async { "HEAD" });
    router.post(&home, |_: Context| async { "POST" });
    router.put(&home, |_: Context| async { "PUT" });
    router.patch(&home, |_: Context| async { "PATCH" });
    router.delete(&home, |_: Context| async { "DELETE" });
    router.options(&home, |_: Context| async { "OPTIONS" });
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        assert_eq!(fetch(&router, method, "https://remix.run"), (200, method.to_string()));
    }
    // The explicit HEAD route runs (its body is dropped by the server, not the router).
    assert_eq!(fetch(&router, "HEAD", "https://remix.run"), (200, "HEAD".into()));
}

#[test]
fn verb_methods_take_actions_with_middleware() {
    let log = log();
    let mut router = Router::new().with(record(&log, "router middleware"));
    let l = log.clone();
    router.get(
        "/",
        action(move |_: Context| {
            l.lock().unwrap().push("action".into());
            async { "OK" }
        })
        .with(record(&log, "action middleware 1"))
        .with(record(&log, "action middleware 2")),
    );
    let l = log.clone();
    router.get(
        "/b",
        move |_: Context| {
            l.lock().unwrap().push("action-b".into());
            async { "B" }
        },
    );
    assert_eq!(fetch(&router, "GET", "https://remix.run/"), (200, "OK".into()));
    assert_eq!(take(&log), ["router middleware", "action middleware 1", "action middleware 2", "action"]);
    assert_eq!(fetch(&router, "GET", "https://remix.run/b"), (200, "B".into()));
    assert_eq!(take(&log), ["router middleware", "action-b"]);
}

#[test]
fn action_middleware_can_short_circuit() {
    let log = log();
    let l = log.clone();
    let mut router = Router::new();
    let block = {
        let log = log.clone();
        move |_: Context, _: Next| {
            log.lock().unwrap().push("m2-short-circuit".to_string());
            async { (http::StatusCode::FORBIDDEN, "Blocked").into_response() }
        }
    };
    router.get(
        "/",
        action(move |_: Context| {
            l.lock().unwrap().push("action".into());
            async { "OK" }
        })
        .with(record(&log, "m1"))
        .with(block)
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
    let log = log();
    let (l1, l2) = (log.clone(), log.clone());
    let mut router = Router::new();
    router.map(
        &Admin { dashboard: get("/admin/dashboard"), users: get("/admin/users") },
        Controller::new(AdminActions {
            dashboard: action(move |_: Context| {
                l1.lock().unwrap().push("dashboard-action".into());
                async { "Dashboard" }
            }),
            users: action(move |_: Context| {
                l2.lock().unwrap().push("users-action".into());
                async { "Users" }
            })
            .with(record(&log, "users-action-middleware")),
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
fn not_found_handling() {
    let log = log();
    let mut router = Router::new().with(record(&log, "middleware"));
    assert_eq!(fetch(&router, "GET", "https://remix.run/nonexistent"), (404, "Not Found: /nonexistent".into()));
    assert_eq!(take(&log), ["middleware"]);
    router.get("/home", |_: Context| async { "Home" });
    let calls = Arc::new(Mutex::new(0));
    let c = calls.clone();
    router.not_found(move |ctx: Context| {
        *c.lock().unwrap() += 1;
        let path = ctx.url().pathname().to_string();
        async move { (http::StatusCode::NOT_FOUND, format!("Custom 404: {path}")) }
    });
    assert_eq!(fetch(&router, "GET", "https://remix.run/home"), (200, "Home".into()));
    assert_eq!(*calls.lock().unwrap(), 0);
    assert_eq!(fetch(&router, "GET", "https://remix.run/missing"), (404, "Custom 404: /missing".into()));
    assert_eq!(*calls.lock().unwrap(), 1);
}

#[test]
fn method_not_allowed_handling() {
    let mut router = Router::new();
    router.get("/home", |_: Context| async { "Home" });
    let res = fetch_res(&router, "POST", "https://remix.run/home");
    assert_eq!(res.status(), 405);
    assert_eq!(res.headers()["allow"], "GET, HEAD");
    assert_eq!(block_on(res.into_body().text()).unwrap(), "Method Not Allowed: POST");

    let mut router = Router::new();
    router.get("/posts/:id", |_: Context| async { "Show" });
    router.put("/posts/:id", |_: Context| async { "Update" });
    router.delete("/posts/:id", |_: Context| async { "Destroy" });
    let res = fetch_res(&router, "POST", "https://remix.run/posts/1");
    assert_eq!(res.status(), 405);
    assert_eq!(res.headers()["allow"], "GET, HEAD, PUT, DELETE");

    // Falls through to a less specific route that allows the method.
    let mut router = Router::new();
    router.get("/posts/featured", |_: Context| async { "Featured" });
    router.any("/posts/:id", |_: Context| async { "Any post" });
    assert_eq!(fetch(&router, "POST", "https://remix.run/posts/featured"), (200, "Any post".into()));

    // No 405 when nothing matches the URL, and an ANY catch-all handles other methods.
    let mut router = Router::new();
    router.get("/home", |_: Context| async { "Home" });
    assert_eq!(fetch(&router, "POST", "https://remix.run/missing").0, 404);
    router.any("/*rest", |_: Context| async { "Catch-all" });
    assert_eq!(fetch(&router, "POST", "https://remix.run/home"), (200, "Catch-all".into()));
}

#[test]
fn head_request_handling() {
    // A GET route serves HEAD without a body and keeps its headers.
    let mut router = Router::new();
    router.get("/file", |_: Context| async {
        let mut res = rmx::fetch::response::text("content");
        res.headers_mut().insert("x-extra", "yes".parse().unwrap());
        res
    });
    let res = fetch_res(&router, "HEAD", "https://remix.run/file");
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["x-extra"], "yes");
    assert_eq!(block_on(res.into_body().text()).unwrap(), "");

    // An explicit HEAD route beats a GET route for the same pattern.
    let mut router = Router::new();
    router.get("/file", |_: Context| async { "content" });
    router.head("/file", |_: Context| async {
        let mut res = rmx::fetch::response::text("");
        res.headers_mut().insert("x-head", "explicit".parse().unwrap());
        res
    });
    assert_eq!(fetch_res(&router, "HEAD", "https://remix.run/file").headers()["x-head"], "explicit");

    // A more specific GET route beats a less specific HEAD route.
    let mut router = Router::new();
    router.head("/*rest", |_: Context| async {
        let mut res = rmx::fetch::response::text("");
        res.headers_mut().insert("x-head", "catch-all".parse().unwrap());
        res
    });
    router.get("/file", |_: Context| async {
        let mut res = rmx::fetch::response::text("content");
        res.headers_mut().insert("x-get", "specific".parse().unwrap());
        res
    });
    let res = fetch_res(&router, "HEAD", "https://remix.run/file");
    assert_eq!(res.headers()["x-get"], "specific");
    assert!(res.headers().get("x-head").is_none());
    assert_eq!(block_on(res.into_body().text()).unwrap(), "");

    // 405 for HEAD when only other methods match.
    let mut router = Router::new();
    router.post("/submit", |_: Context| async { "Submitted" });
    let res = fetch_res(&router, "HEAD", "https://remix.run/submit");
    assert_eq!(res.status(), 405);
    assert_eq!(res.headers()["allow"], "POST");
}

// --- trailing slash handling -------------------------------------------------------------

#[test]
fn trailing_slashes_must_match_exactly() {
    let mut router = Router::new();
    router.get("/about", |_: Context| async { "About" });
    router.get("/contact/", |_: Context| async { "Contact" });
    assert_eq!(fetch(&router, "GET", "https://remix.run/about"), (200, "About".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/about/").0, 404);
    assert_eq!(fetch(&router, "GET", "https://remix.run/contact/"), (200, "Contact".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/contact").0, 404);

    let api = prefix("api");
    let mut router = Router::new();
    router.get(api.get("/users"), |_: Context| async { "Users" });
    router.get(api.get("/posts/"), |_: Context| async { "Posts" });
    assert_eq!(fetch(&router, "GET", "https://remix.run/api/users"), (200, "Users".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/api/users/").0, 404);
    assert_eq!(fetch(&router, "GET", "https://remix.run/api/posts/"), (200, "Posts".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/api/posts").0, 404);
}

#[test]
fn root_path_matches_with_and_without_a_trailing_slash() {
    let mut router = Router::new();
    router.get("/", |_: Context| async { "Home" });
    assert_eq!(fetch(&router, "GET", "https://remix.run/"), (200, "Home".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run"), (200, "Home".into()));
}

#[test]
fn nested_route_maps_keep_trailing_slashes_exact() {
    let admin = prefix("admin");
    let users = admin.prefix("users");
    let mut router = Router::new();
    router.get(admin.get("/"), |_: Context| async { "Admin Dashboard" });
    router.get(users.get("/"), |_: Context| async { "Users List" });
    router.get(users.get(":id"), |ctx: Context| async move { format!("User {}", ctx.param("id").unwrap()) });
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin"), (200, "Admin Dashboard".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/").0, 404);
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/users"), (200, "Users List".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/users/").0, 404);
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/users/123"), (200, "User 123".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/users/123/").0, 404);
}

// --- router.mount() --------------------------------------------------------------------

#[test]
fn mounts_routes_below_static_and_param_prefixes() {
    let mut router = Router::new();
    router.mount("/admin", |admin| {
        admin.get("/", |_: Context| async { "Admin" });
        admin.get("/users/:id", |ctx: Context| async move { format!("User {}", ctx.param("id").unwrap()) });
    });
    router.mount("/orgs/:orgId", |org| {
        org.get("/", |ctx: Context| async move { format!("Org {}", ctx.param("orgId").unwrap()) });
        org.get("/users/:userId", |ctx: Context| async move {
            format!("Org {} User {}", ctx.param("orgId").unwrap(), ctx.param("userId").unwrap())
        });
    });
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin"), (200, "Admin".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/users/123"), (200, "User 123".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/users/123").0, 404);
    assert_eq!(fetch(&router, "GET", "https://remix.run/orgs/acme"), (200, "Org acme".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/orgs/acme/users/123"), (200, "Org acme User 123".into()));
}

#[test]
fn mounted_patterns_use_route_pattern_semantics() {
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
fn the_right_most_param_wins_when_mounted_and_child_routes_share_a_name() {
    let mut router = Router::new();
    router.mount("/orgs/:id", |org| {
        org.get("/users/:id", |ctx: Context| async move { format!("User {}", ctx.param("id").unwrap()) });
    });
    assert_eq!(fetch(&router, "GET", "https://remix.run/orgs/acme/users/123"), (200, "User 123".into()));
}

#[test]
fn composes_nested_mount_prefixes_and_shares_specificity_with_the_parent() {
    let mut router = Router::new();
    router.mount("/api", |api| {
        api.mount("/v1", |v1| {
            v1.get("/status", |_: Context| async { "ok" });
        });
        api.get("/:id", |ctx: Context| async move { format!("Mounted {}", ctx.param("id").unwrap()) });
    });
    router.get("/api/health", |_: Context| async { "Parent health" });
    assert_eq!(fetch(&router, "GET", "https://remix.run/api/v1/status"), (200, "ok".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/api/health"), (200, "Parent health".into()));
    assert_eq!(fetch(&router, "GET", "https://remix.run/api/users"), (200, "Mounted users".into()));
}

#[test]
fn mounted_paths_use_the_parent_not_found_handler() {
    let mut router = Router::new();
    router.not_found(|ctx: Context| {
        let path = ctx.url().pathname().to_string();
        async move { (http::StatusCode::NOT_FOUND, format!("Missing {path}")) }
    });
    router.mount("/admin", |admin| {
        admin.get("/", |_: Context| async { "Admin" });
    });
    assert_eq!(fetch(&router, "GET", "https://remix.run/admin/missing"), (404, "Missing /admin/missing".into()));
}

#[test]
fn registers_mounted_routes_with_their_prefixed_patterns() {
    let mut router = Router::new();
    router.mount("/admin/:adminId", |admin| {
        admin.get("/", |_: Context| async { "Admin" });
        admin.get("/users/:id", |_: Context| async { "User" });
        admin.mount("/settings", |settings| {
            settings.get("/profile", |_: Context| async { "Profile" });
        });
    });
    assert_eq!(router.routes(), ["GET /admin/:adminId", "GET /admin/:adminId/users/:id", "GET /admin/:adminId/settings/profile"]);
}
