//! Route map listing (Remix's `remix routes` command) and route maps in statics.

use rsmix::fetch::{block_on, Body, Request, Response};
use rsmix::router::routes::{form, get, post, prefix, Route, RouteMap, RouteTree};
use rsmix::router::{action, Context, Router};

fn req(method: &str, url: &str) -> Request {
    http::Request::builder().method(method).uri(url).body(Body::empty()).unwrap()
}

fn fetch(router: &Router, method: &str, url: &str) -> (u16, String) {
    let res: Response = block_on(router.fetch(req(method, url)));
    let status = res.status().as_u16();
    (status, block_on(res.into_body().text()).unwrap())
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

// --- built-in route maps ---------------------------------------------------------------

#[test]
fn maps_forms_and_resources() {
    use rsmix::router::routes::{resources, FormActions, ResourcesActions};
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
    pub books: rsmix::router::routes::Resources,
}

#[derive(RouteMap)]
pub struct Auth {
    pub login: rsmix::router::routes::Form,
    pub logout: Route,
}

fn app() -> App {
    let auth = prefix("auth");
    App {
        home: rsmix::router::routes::any("/"),
        auth: Auth { login: form("login"), logout: auth.post("logout") },
        books: rsmix::router::routes::resources_with_param("books", "bookId"),
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
    // Cells are padded to their column, the last one too, as Remix's reporter table does.
    // The widest path is /books/:bookId/edit (19 characters).
    assert_eq!(lines[0], format!("Route              Method  {:19}", "Path"));
    assert_eq!(lines[1], format!("home               ANY     {:19}", "/"));
    assert_eq!(lines[2], format!("auth.login.index   GET     {:19}", "/login"));
    assert_eq!(lines[3], format!("auth.login.action  POST    {:19}", "/login"));
    assert_eq!(lines[11], format!("books.destroy      DELETE  {:19}", "/books/:bookId"));
    assert_eq!(lines.len(), 12);
    let no_headers = app().route_tree().format_table(false);
    assert!(no_headers.starts_with("home "));
    assert_eq!(no_headers.lines().count(), 11);
}

#[test]
fn headers_set_the_minimum_column_widths() {
    let table = get("/").route_tree().format_table(true);
    assert_eq!(table, "Route  Method  Path\nroute  GET     /   \n");
}

#[test]
fn routes_command_styles_each_padded_header_cell() {
    use rsmix::router::routes::routes_command_output_with;
    let args = ["--table".to_string()];
    let bold = |header: &str| format!("\x1b[1m{header}\x1b[22m");
    let table = routes_command_output_with(&get("/"), &args, &bold).unwrap();
    assert_eq!(table, "\x1b[1mRoute\x1b[22m  \x1b[1mMethod\x1b[22m  \x1b[1mPath\x1b[22m\nroute  GET     /   \n");
    let rows = routes_command_output_with(&get("/"), &["--table".to_string(), "--no-headers".to_string()], &bold).unwrap();
    assert_eq!(rows, "route  GET     /   \n");
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
    use rsmix::router::routes::routes_command_output;
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

