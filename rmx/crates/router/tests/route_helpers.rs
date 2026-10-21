//! Ports of fetch-router's route helper tests (form.test.ts, resource.test.ts,
//! resources.test.ts, method.test.ts) for the shapes the helpers build.
//!
//! Not ported: the `only`, `exclude` and `names` options. They change the shape of the
//! returned map, which in rmx is a struct, so an app declares the struct it wants with
//! `#[derive(RouteMap)]` (see `declared_subsets_replace_only_and_names` below and the
//! bookstore's `ReadOnly` and `Users`); "only and exclude together" cannot be expressed.

use http::Method;
use rmx::router::routes::{delete, form, form_with, get, head, patch, post, prefix, put, resource, resources, resources_with_param, Form, Resources, Route, RouteMap};

fn s(route: &Route) -> String {
    route.to_string()
}

#[test]
fn method_helpers_build_routes_for_each_method() {
    assert_eq!(s(&get("/")), "GET /");
    assert_eq!(s(&post("/login")), "POST /login");
    assert_eq!(s(&put("/a")), "PUT /a");
    assert_eq!(s(&patch("/a")), "PATCH /a");
    assert_eq!(s(&delete("/a")), "DELETE /a");
    assert_eq!(s(&head("/a")), "HEAD /a");
    assert_eq!(s(&prefix("api").patch("items/:id")), "PATCH /api/items/:id");
}

#[test]
fn form_creates_index_and_action_routes() {
    let login = form("login");
    assert_eq!((s(&login.index), s(&login.action)), ("GET /login".into(), "POST /login".into()));
}

#[test]
fn form_supports_a_custom_form_method() {
    let settings = form_with("settings", Method::PUT);
    assert_eq!((s(&settings.index), s(&settings.action)), ("GET /settings".into(), "PUT /settings".into()));
    let nested = prefix("account").form_with("password", Method::PATCH);
    assert_eq!(s(&nested.action), "PATCH /account/password");
}

#[derive(RouteMap)]
pub struct Account {
    pub index: Route,
    pub action: Route,
    pub settings: Form,
}

#[test]
fn creates_nested_forms() {
    let account = form("account");
    let routes = Account { index: account.index, action: account.action, settings: form("account/settings") };
    assert_eq!(s(&routes.index), "GET /account");
    assert_eq!(s(&routes.action), "POST /account");
    assert_eq!(s(&routes.settings.index), "GET /account/settings");
    assert_eq!(s(&routes.settings.action), "POST /account/settings");
    let table = routes.route_tree().format_table(false);
    assert!(table.contains("settings.index") && table.contains("/account/settings"), "{table}");
}

#[test]
fn resource_creates_the_six_singleton_routes() {
    let r = resource("profile");
    let all = [&r.new, &r.show, &r.create, &r.edit, &r.update, &r.destroy].map(s);
    assert_eq!(all, ["GET /profile/new", "GET /profile", "POST /profile", "GET /profile/edit", "PUT /profile", "DELETE /profile"]);
}

#[test]
fn resources_create_the_seven_collection_routes() {
    let r = resources("books");
    let all = [&r.index, &r.new, &r.show, &r.create, &r.edit, &r.update, &r.destroy].map(s);
    assert_eq!(all, ["GET /books", "GET /books/new", "GET /books/:id", "POST /books", "GET /books/:id/edit", "PUT /books/:id", "DELETE /books/:id"]);
}

#[test]
fn resources_take_a_custom_param() {
    let r = resources_with_param("books", "bookId");
    assert_eq!(s(&r.show), "GET /books/:bookId");
    assert_eq!(s(&r.edit), "GET /books/:bookId/edit");
    assert_eq!(r.update.href([("bookId", "7")]), "/books/7");
}

#[derive(RouteMap)]
pub struct Brands {
    pub index: Route,
    pub new: Route,
    pub show: Route,
    pub create: Route,
    pub edit: Route,
    pub update: Route,
    pub destroy: Route,
    pub products: Resources,
}

#[test]
fn creates_nested_resources() {
    let b = resources("brands");
    let routes = Brands {
        index: b.index,
        new: b.new,
        show: b.show,
        create: b.create,
        edit: b.edit,
        update: b.update,
        destroy: b.destroy,
        products: resources("brands/:brandId/products"),
    };
    assert_eq!(s(&routes.destroy), "DELETE /brands/:id");
    let p = &routes.products;
    let all = [&p.index, &p.new, &p.show, &p.create, &p.edit, &p.update, &p.destroy].map(s);
    assert_eq!(
        all,
        [
            "GET /brands/:brandId/products",
            "GET /brands/:brandId/products/new",
            "GET /brands/:brandId/products/:id",
            "POST /brands/:brandId/products",
            "GET /brands/:brandId/products/:id/edit",
            "PUT /brands/:brandId/products/:id",
            "DELETE /brands/:brandId/products/:id",
        ]
    );
    assert_eq!(p.edit.href([("brandId", "1"), ("id", "2")]), "/brands/1/products/2/edit");
}

/// `resources('books', { only: ['index', 'show'], names: { show: 'view' } })`
#[derive(RouteMap)]
pub struct ReadOnlyBooks {
    pub index: Route,
    pub view: Route,
}

#[test]
fn declared_subsets_replace_only_and_names() {
    let b = resources("books");
    let routes = ReadOnlyBooks { index: b.index, view: b.show };
    let tree = routes.route_tree().format_table(false);
    assert!(tree.contains("view") && tree.contains("/books/:id") && !tree.contains("edit"), "{tree}");
    // The generated controller has exactly these two actions.
    let _ = ReadOnlyBooksActions { index: rmx::router::action(|_: rmx::router::Context| async { "" }), view: rmx::router::action(|_: rmx::router::Context| async { "" }) };
}
