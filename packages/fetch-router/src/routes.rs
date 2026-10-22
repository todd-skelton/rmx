//! Route definitions (Remix's `remix/routes`): a [`Route`] is a request method plus a URL
//! pattern, and knows how to build its own href.
//!
//! Group routes in plain structs and keep them in a `LazyLock` so handlers, links and form
//! actions all use the same definitions:
//!
//! ```
//! use std::sync::LazyLock;
//! use rmx_fetch_router::routes::{form, get, prefix, Form, Route};
//!
//! pub struct Routes { pub home: Route, pub contact: Form, pub blog: Blog }
//! pub struct Blog { pub index: Route, pub show: Route }
//!
//! pub static ROUTES: LazyLock<Routes> = LazyLock::new(|| {
//!     let blog = prefix("blog");
//!     Routes {
//!         home: get("/"),
//!         contact: form("contact"),
//!         blog: Blog { index: blog.get("/"), show: blog.get(":slug") },
//!     }
//! });
//!
//! assert_eq!(ROUTES.blog.show.href([("slug", "hello")]), "/blog/hello");
//! assert_eq!(ROUTES.contact.action.href(()), "/contact");
//! ```

use std::fmt;

use http::Method;
use rmx_route_pattern::{HrefParams, RoutePattern};
use rmx_url::SearchParams;

/// A request method (or any method) plus a pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    method: Option<Method>,
    pattern: RoutePattern,
}

impl Route {
    /// A route for `method`, or for any method when `None`. Panics on an invalid pattern,
    /// since patterns are written in source code.
    pub fn new(method: Option<Method>, pattern: &str) -> Route {
        let pattern = RoutePattern::parse(pattern).unwrap_or_else(|e| panic!("{e}"));
        Route { method, pattern }
    }

    pub fn method(&self) -> Option<&Method> {
        self.method.as_ref()
    }

    pub fn pattern(&self) -> &RoutePattern {
        &self.pattern
    }

    /// Build a link to this route. Panics when a required param is missing, which is a bug
    /// in the calling code; use [`Route::try_href`] when params come from user input.
    pub fn href(&self, params: impl HrefParams) -> String {
        self.try_href(params).unwrap_or_else(|e| panic!("href for {}: {e}", self.pattern))
    }

    pub fn try_href(&self, params: impl HrefParams) -> Result<String, rmx_route_pattern::HrefError> {
        self.pattern.href(params)
    }

    /// Build a link with options (Remix's `route.href(params, { baseURL, searchParams })`):
    /// search params first, and a path-relative href when the target shares `base_url`'s
    /// origin.
    pub fn href_with(&self, params: impl HrefParams, options: &rmx_route_pattern::HrefOptions) -> String {
        self.pattern.href_with(params, options).unwrap_or_else(|e| panic!("href for {}: {e}", self.pattern))
    }

    /// Build a link with extra search params.
    pub fn href_with_search(&self, params: impl HrefParams, search: &SearchParams) -> String {
        self.pattern
            .href_with_search(params, search)
            .unwrap_or_else(|e| panic!("href for {}: {e}", self.pattern))
    }

    /// This route mounted below `base`.
    pub fn under(&self, base: &RoutePattern) -> Route {
        Route { method: self.method.clone(), pattern: base.join(&self.pattern) }
    }
}

impl fmt::Display for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.method {
            Some(m) => write!(f, "{m} {}", self.pattern),
            None => write!(f, "ANY {}", self.pattern),
        }
    }
}

/// Anything that names a route: a [`Route`], or a pattern string (any method).
pub trait IntoRoute {
    fn into_route(self, method: Option<Method>) -> Route;
}

impl IntoRoute for &str {
    fn into_route(self, method: Option<Method>) -> Route {
        Route::new(method, self)
    }
}

impl IntoRoute for String {
    fn into_route(self, method: Option<Method>) -> Route {
        Route::new(method, &self)
    }
}

impl IntoRoute for &Route {
    fn into_route(self, method: Option<Method>) -> Route {
        match method {
            Some(m) => Route { method: Some(m), pattern: self.pattern.clone() },
            None => self.clone(),
        }
    }
}

impl IntoRoute for Route {
    fn into_route(self, method: Option<Method>) -> Route {
        (&self).into_route(method)
    }
}

/// A route that matches any method.
pub fn any(pattern: &str) -> Route {
    Route::new(None, pattern)
}

pub fn get(pattern: &str) -> Route {
    Route::new(Some(Method::GET), pattern)
}

pub fn post(pattern: &str) -> Route {
    Route::new(Some(Method::POST), pattern)
}

pub fn put(pattern: &str) -> Route {
    Route::new(Some(Method::PUT), pattern)
}

pub fn patch(pattern: &str) -> Route {
    Route::new(Some(Method::PATCH), pattern)
}

pub fn delete(pattern: &str) -> Route {
    Route::new(Some(Method::DELETE), pattern)
}

pub fn head(pattern: &str) -> Route {
    Route::new(Some(Method::HEAD), pattern)
}

pub fn options(pattern: &str) -> Route {
    Route::new(Some(Method::OPTIONS), pattern)
}

/// A base pattern that nested routes are joined onto, like `route('frames', {...})`.
#[derive(Clone, Debug)]
pub struct Prefix(RoutePattern);

pub fn prefix(pattern: &str) -> Prefix {
    Prefix(RoutePattern::parse(pattern).unwrap_or_else(|e| panic!("{e}")))
}

impl Prefix {
    pub fn route(&self, method: Option<Method>, pattern: &str) -> Route {
        Route::new(method, pattern).under(&self.0)
    }
    pub fn any(&self, pattern: &str) -> Route {
        self.route(None, pattern)
    }
    pub fn get(&self, pattern: &str) -> Route {
        self.route(Some(Method::GET), pattern)
    }
    pub fn post(&self, pattern: &str) -> Route {
        self.route(Some(Method::POST), pattern)
    }
    pub fn put(&self, pattern: &str) -> Route {
        self.route(Some(Method::PUT), pattern)
    }
    pub fn patch(&self, pattern: &str) -> Route {
        self.route(Some(Method::PATCH), pattern)
    }
    pub fn delete(&self, pattern: &str) -> Route {
        self.route(Some(Method::DELETE), pattern)
    }
    pub fn head(&self, pattern: &str) -> Route {
        self.route(Some(Method::HEAD), pattern)
    }
    pub fn options(&self, pattern: &str) -> Route {
        self.route(Some(Method::OPTIONS), pattern)
    }
    pub fn prefix(&self, pattern: &str) -> Prefix {
        Prefix(self.0.join(&RoutePattern::parse(pattern).unwrap_or_else(|e| panic!("{e}"))))
    }
    pub fn form(&self, pattern: &str) -> Form {
        self.form_with(pattern, Method::POST)
    }
    /// [`Prefix::form`] submitted with another method (Remix's `formMethod` option).
    pub fn form_with(&self, pattern: &str, method: Method) -> Form {
        let p = self.prefix(pattern);
        Form { index: p.get("/"), action: p.route(Some(method), "/") }
    }
    pub fn pattern(&self) -> &RoutePattern {
        &self.0
    }
}

/// `index` (GET) shows a form and `action` (POST) handles it, at the same URL.
#[derive(Clone, Debug)]
pub struct Form {
    pub index: Route,
    pub action: Route,
}

pub fn form(pattern: &str) -> Form {
    form_with(pattern, Method::POST)
}

/// A [`Form`] whose action uses `method` (Remix's `form(pattern, { formMethod })`), such as
/// `PUT` for a settings form sent through method override.
pub fn form_with(pattern: &str, method: Method) -> Form {
    Form { index: get(pattern), action: Route::new(Some(method), pattern) }
}

/// The seven RESTful routes for a collection (`resources('books')`).
#[derive(Clone, Debug)]
pub struct Resources {
    /// GET /books
    pub index: Route,
    /// GET /books/new
    pub new: Route,
    /// POST /books
    pub create: Route,
    /// GET /books/:id
    pub show: Route,
    /// GET /books/:id/edit
    pub edit: Route,
    /// PUT /books/:id
    pub update: Route,
    /// DELETE /books/:id
    pub destroy: Route,
}

/// Collection routes with `:id` as the member param.
pub fn resources(base: &str) -> Resources {
    resources_with_param(base, "id")
}

pub fn resources_with_param(base: &str, param: &str) -> Resources {
    let b = prefix(base);
    let member = format!(":{param}");
    Resources {
        index: b.get("/"),
        new: b.get("new"),
        create: b.post("/"),
        show: b.get(&member),
        edit: b.get(&format!("{member}/edit")),
        update: b.put(&member),
        destroy: b.delete(&member),
    }
}

/// The six routes for a singleton resource (`resource('profile')`).
#[derive(Clone, Debug)]
pub struct Resource {
    pub new: Route,
    pub create: Route,
    pub show: Route,
    pub edit: Route,
    pub update: Route,
    pub destroy: Route,
}

pub fn resource(base: &str) -> Resource {
    let b = prefix(base);
    Resource {
        new: b.get("new"),
        create: b.post("/"),
        show: b.get("/"),
        edit: b.get("edit"),
        update: b.put("/"),
        destroy: b.delete("/"),
    }
}

// ---------------------------------------------------------------------------------------
// Route maps
// ---------------------------------------------------------------------------------------

/// A route map: a [`Route`] or a struct of routes and nested route maps (Remix's
/// `RouteMap`). `#[derive(RouteMap)]` implements it for a struct and generates a
/// `<Name>Actions` struct with one [`Action`](crate::Action) per direct `Route` field,
/// which is what [`Router::map`](crate::Router::map) takes as a controller:
///
/// ```
/// use rmx::router::routes::{get, post, Route, RouteMap};
/// use rmx::router::{action, Context, Router};
///
/// #[derive(RouteMap)]
/// pub struct Routes { pub home: Route, pub blog: Blog }
/// #[derive(RouteMap)]
/// pub struct Blog { pub index: Route, pub create: Route }
///
/// let routes = Routes { home: get("/"), blog: Blog { index: get("/blog"), create: post("/blog") } };
/// let mut router = Router::new();
/// router.map(&routes, RoutesActions { home: action(|_: Context| async { "Home" }) });
/// router.map(&routes.blog, BlogActions {
///     index: action(|_: Context| async { "Blog" }),
///     create: action(|_: Context| async { "Created" }),
/// });
/// let res = rmx::fetch::block_on(router.fetch_url("https://remix.run/blog"));
/// assert_eq!(rmx::fetch::block_on(res.into_body().text()).unwrap(), "Blog");
/// ```
///
/// Remix checks controllers when they are mapped; here the compiler does. A controller must
/// name every direct route, and only those:
///
/// ```compile_fail,E0063
/// # use rmx::router::routes::{get, Route, RouteMap};
/// # use rmx::router::{action, Context};
/// #[derive(RouteMap)]
/// pub struct Routes { pub home: Route, pub about: Route }
/// // error: missing field `about` (Remix: "Missing action `about` in controller")
/// let _ = RoutesActions { home: action(|_: Context| async { "Home" }) };
/// ```
///
/// ```compile_fail,E0560
/// # use rmx::router::routes::{get, Route, RouteMap};
/// # use rmx::router::{action, Context};
/// #[derive(RouteMap)]
/// pub struct Routes { pub home: Route, pub blog: Blog }
/// #[derive(RouteMap)]
/// pub struct Blog { pub index: Route }
/// // error: no field `blog` (Remix: "Cannot map nested route map key `blog`")
/// let _ = RoutesActions {
///     home: action(|_: Context| async { "Home" }),
///     blog: action(|_: Context| async { "Blog" }),
/// };
/// ```
///
/// ```compile_fail,E0560
/// # use rmx::router::routes::{get, Route, RouteMap};
/// # use rmx::router::{action, Context};
/// #[derive(RouteMap)]
/// pub struct Routes { pub home: Route }
/// // error: no field `missing` (Remix: "Unknown action `missing` in controller")
/// let _ = RoutesActions {
///     home: action(|_: Context| async { "Home" }),
///     missing: action(|_: Context| async { "Missing" }),
/// };
/// ```
///
/// A route map takes a controller (its actions struct), not a handler (Remix: "controllers
/// must define actions under `actions`"), and a controller's actions must be that struct
/// (Remix: "Expected a controller with an object `actions` property"):
///
/// ```compile_fail,E0277
/// # use rmx::router::routes::{get, Route, RouteMap};
/// # use rmx::router::{Context, Router};
/// #[derive(RouteMap)]
/// pub struct Routes { pub home: Route }
/// let mut router = Router::new();
/// router.map(&Routes { home: get("/") }, |_: Context| async { "OK" });
/// ```
///
/// ```compile_fail,E0308
/// # use rmx::router::routes::{get, Route, RouteMap};
/// # use rmx::router::{Controller, Router};
/// #[derive(RouteMap)]
/// pub struct Routes { pub home: Route }
/// let mut router = Router::new();
/// router.map(&Routes { home: get("/") }, Controller { middleware: Vec::new(), actions: () });
/// ```
///
/// An action needs a handler function (Remix: "Expected a request handler function or action
/// object with a function `handler` property"):
///
/// ```compile_fail,E0277
/// # use rmx::router::action;
/// let _ = action(1);
/// ```
pub trait RouteMap {
    /// What a controller for this map supplies: an [`Action`](crate::Action) for a single
    /// route, or a struct with one action per direct route.
    type Actions;

    /// The routes as a tree, for listing them (`rmx routes`).
    fn route_tree(&self) -> RouteTree;

    /// Pair each direct route with its action.
    #[doc(hidden)]
    fn pair_actions(&self, actions: Self::Actions) -> Vec<(Route, crate::Action)>;
}

pub use rmx_macros::RouteMap;

impl RouteMap for Route {
    type Actions = crate::Action;

    fn route_tree(&self) -> RouteTree {
        RouteTree::Route(self.clone())
    }

    fn pair_actions(&self, action: crate::Action) -> Vec<(Route, crate::Action)> {
        vec![(self.clone(), action)]
    }
}

impl<T: RouteMap, F: FnOnce() -> T> RouteMap for std::sync::LazyLock<T, F> {
    type Actions = T::Actions;

    fn route_tree(&self) -> RouteTree {
        (**self).route_tree()
    }

    fn pair_actions(&self, actions: T::Actions) -> Vec<(Route, crate::Action)> {
        (**self).pair_actions(actions)
    }
}

macro_rules! builtin_route_map {
    ($map:ident, $actions:ident, $doc:literal, [$($field:ident),*]) => {
        #[doc = $doc]
        pub struct $actions {
            $(pub $field: crate::Action,)*
        }

        impl RouteMap for $map {
            type Actions = $actions;

            fn route_tree(&self) -> RouteTree {
                RouteTree::Group(vec![$((stringify!($field), RouteTree::Route(self.$field.clone()))),*])
            }

            fn pair_actions(&self, actions: $actions) -> Vec<(Route, crate::Action)> {
                vec![$((self.$field.clone(), actions.$field)),*]
            }
        }
    };
}

builtin_route_map!(Form, FormActions, "The actions for a [`Form`]: show it and handle its submission.", [index, action]);
builtin_route_map!(
    Resources,
    ResourcesActions,
    "The seven actions for [`Resources`].",
    [index, new, show, create, edit, update, destroy]
);
builtin_route_map!(Resource, ResourceActions, "The six actions for a [`Resource`].", [new, show, create, edit, update, destroy]);

/// A route map as data: a route, or named children in declaration order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteTree {
    Route(Route),
    Group(Vec<(&'static str, RouteTree)>),
}

/// One entry of a listed route map. `name` is the dotted path from the root
/// (`auth.login.action`), like the names `remix routes` prints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteNode {
    pub key: String,
    pub name: String,
    /// `None` for groups.
    pub route: Option<Route>,
    pub children: Vec<RouteNode>,
}

impl RouteNode {
    /// `GET`, `POST`, ... or `ANY`; `None` for groups.
    pub fn method(&self) -> Option<String> {
        self.route.as_ref().map(|r| r.method().map_or_else(|| "ANY".to_string(), |m| m.to_string()))
    }
}

impl RouteTree {
    /// The top-level entries, with dotted names. A single route lists as one entry named
    /// `route`.
    pub fn nodes(&self) -> Vec<RouteNode> {
        fn build(key: &str, parent: &str, tree: &RouteTree) -> RouteNode {
            let name = if parent.is_empty() { key.to_string() } else { format!("{parent}.{key}") };
            match tree {
                RouteTree::Route(r) => RouteNode { key: key.to_string(), name, route: Some(r.clone()), children: Vec::new() },
                RouteTree::Group(children) => {
                    let children = children.iter().map(|(k, t)| build(k, &name, t)).collect();
                    RouteNode { key: key.to_string(), name, route: None, children }
                }
            }
        }
        match self {
            RouteTree::Route(_) => vec![build("route", "", self)],
            RouteTree::Group(children) => children.iter().map(|(k, t)| build(k, "", t)).collect(),
        }
    }

    /// Every route in declaration order.
    pub fn routes(&self) -> Vec<RouteNode> {
        fn flatten(nodes: Vec<RouteNode>, out: &mut Vec<RouteNode>) {
            for n in nodes {
                if n.route.is_some() {
                    out.push(n);
                } else {
                    flatten(n.children, out);
                }
            }
        }
        let mut out = Vec::new();
        flatten(self.nodes(), &mut out);
        out
    }

    /// The tree view of `remix routes`:
    ///
    /// ```text
    /// home     GET    /
    /// books
    /// ├─ index  GET    /books
    /// └─ show   GET    /books/:slug
    /// ```
    pub fn format_tree(&self) -> String {
        fn render(lines: &mut Vec<String>, nodes: &[RouteNode], prefix: &str, root: bool) {
            let width = nodes.iter().filter(|n| n.route.is_some()).map(|n| n.key.len()).max().unwrap_or(0);
            for (i, n) in nodes.iter().enumerate() {
                let last = i + 1 == nodes.len();
                let branch = if root { "" } else if last { "└─ " } else { "├─ " };
                match &n.route {
                    Some(r) => {
                        let method = n.method().unwrap_or_default();
                        lines.push(format!("{prefix}{branch}{:width$}  {method:6} {}", n.key, r.pattern()));
                    }
                    None => {
                        lines.push(format!("{prefix}{branch}{}", n.key));
                        let child = if root { String::new() } else { format!("{prefix}{}", if last { "   " } else { "│  " }) };
                        render(lines, &n.children, &child, false);
                    }
                }
            }
        }
        let nodes = self.nodes();
        if nodes.is_empty() {
            return "No routes.\n".to_string();
        }
        let mut lines = Vec::new();
        render(&mut lines, &nodes, "", true);
        lines.iter().map(|l| format!("{}\n", l.trim_end())).collect()
    }

    /// The flat table of `remix routes --table`: route name, method and path. Like Remix's
    /// reporter table, every cell is padded to its column's width.
    pub fn format_table(&self, headers: bool) -> String {
        self.format_table_with(headers, &|header| header.to_string())
    }

    /// [`RouteTree::format_table`] with each padded header cell passed through `header`
    /// (`routes_command` makes them bold on a color terminal, as Remix does).
    pub fn format_table_with(&self, headers: bool, header: &dyn Fn(&str) -> String) -> String {
        const HEADERS: [&str; 3] = ["Route", "Method", "Path"];
        let rows: Vec<[String; 3]> = self
            .routes()
            .into_iter()
            .map(|n| {
                let path = n.route.as_ref().map(|r| r.pattern().to_string()).unwrap_or_default();
                [n.name.clone(), n.method().unwrap_or_default(), path]
            })
            .collect();
        let widths: Vec<usize> =
            (0..3).map(|c| rows.iter().map(|r| r[c].chars().count()).max().unwrap_or(0).max(HEADERS[c].len())).collect();
        let pad = |cell: &str, c: usize| format!("{cell:w$}", w = widths[c]);
        let mut out = String::new();
        if headers {
            let cells: Vec<String> = HEADERS.iter().enumerate().map(|(c, h)| header(&pad(h, c))).collect();
            out.push_str(&cells.join("  "));
            out.push('\n');
        }
        for row in &rows {
            let cells: Vec<String> = row.iter().enumerate().map(|(c, cell)| pad(cell, c)).collect();
            out.push_str(&cells.join("  "));
            out.push('\n');
        }
        out
    }

    /// The tree as JSON, shaped like `remix routes --json`'s `tree` (without controller
    /// owners, since rmx has no controller file convention).
    pub fn to_json(&self) -> String {
        fn string(s: &str) -> String {
            let mut out = String::from("\"");
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
        fn node(n: &RouteNode) -> String {
            let children: Vec<String> = n.children.iter().map(node).collect();
            let mut fields = vec![
                format!("\"children\":[{}]", children.join(",")),
                format!("\"key\":{}", string(&n.key)),
                format!("\"kind\":\"{}\"", if n.route.is_some() { "route" } else { "group" }),
            ];
            if let Some(m) = n.method() {
                fields.push(format!("\"method\":{}", string(&m)));
            }
            fields.push(format!("\"name\":{}", string(&n.name)));
            if let Some(r) = &n.route {
                fields.push(format!("\"pattern\":{}", string(&r.pattern().to_string())));
            }
            format!("{{{}}}", fields.join(","))
        }
        let nodes: Vec<String> = self.nodes().iter().map(node).collect();
        format!("{{\"tree\":[{}]}}", nodes.join(","))
    }
}

/// `remix routes` for an app binary: when the first argument is `routes`, print the route
/// map (tree, `--table` with optional `--no-headers`, or `--json`) and exit. Call it first
/// thing in `main`:
///
/// ```no_run
/// # use rmx::router::routes::{get, Route, RouteMap};
/// # #[derive(RouteMap)] pub struct Routes { pub home: Route }
/// # static ROUTES: std::sync::LazyLock<Routes> = std::sync::LazyLock::new(|| Routes { home: get("/") });
/// fn main() {
///     rmx::router::routes::routes_command(&*ROUTES); // cargo run -- routes --table
///     // start the server...
/// }
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub fn routes_command(map: &impl RouteMap) {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("routes") {
        return;
    }
    let args: Vec<String> = args.collect();
    let styles = rmx_terminal::Styles::detect();
    match routes_command_output_with(map, &args, &|header| styles.bold(header)) {
        Ok(out) => {
            print!("{out}");
            std::process::exit(0);
        }
        Err(e) => {
            eprint!("Error: {e}\n\n{ROUTES_HELP}");
            std::process::exit(1);
        }
    }
}

const ROUTES_HELP: &str = "\
Usage:
  routes [--json | --table] [--no-headers]

Show the route tree for this app.

Options:
  --json        Print the normalized route tree as JSON
  --table       Print routes as a flat table
  --no-headers  Omit the table header row when using --table
";

/// What [`routes_command`] prints for `args` (the arguments after `routes`), or the
/// usage error, without colors.
pub fn routes_command_output(map: &impl RouteMap, args: &[String]) -> Result<String, String> {
    routes_command_output_with(map, args, &|header| header.to_string())
}

/// [`routes_command_output`] with table header cells passed through `header`.
pub fn routes_command_output_with(map: &impl RouteMap, args: &[String], header: &dyn Fn(&str) -> String) -> Result<String, String> {
    let (mut json, mut table, mut no_headers) = (false, false, false);
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Ok(ROUTES_HELP.to_string()),
            "--json" => json = true,
            "--table" => table = true,
            "--no-headers" => no_headers = true,
            other if other.starts_with('-') => return Err(format!("Unknown option {other}.")),
            other => return Err(format!("Unexpected argument {other}.")),
        }
    }
    if json && table {
        return Err("Cannot combine --json with --table.".into());
    }
    if no_headers && !table {
        return Err("Cannot use --no-headers without --table.".into());
    }
    let tree = map.route_tree();
    Ok(if json {
        format!("{}\n", tree.to_json())
    } else if table {
        tree.format_table_with(!no_headers, header)
    } else {
        tree.format_tree()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_build_expected_routes() {
        let r = resources("brands/:brandId/products");
        assert_eq!(r.index.to_string(), "GET /brands/:brandId/products");
        assert_eq!(r.show.to_string(), "GET /brands/:brandId/products/:id");
        assert_eq!(r.edit.href([("brandId", "1"), ("id", "2")]), "/brands/1/products/2/edit");
        assert_eq!(r.update.to_string(), "PUT /brands/:brandId/products/:id");
        let p = resource("users/:userId/profile");
        assert_eq!(p.show.to_string(), "GET /users/:userId/profile");
        assert_eq!(p.edit.to_string(), "GET /users/:userId/profile/edit");
        let f = prefix("frames").form("account");
        assert_eq!(f.action.to_string(), "POST /frames/account");
        assert_eq!(prefix("frames").get("/sidebar").href(()), "/frames/sidebar");
    }
}
