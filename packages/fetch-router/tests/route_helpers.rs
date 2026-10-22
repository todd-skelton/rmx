//! Ports of fetch-router's route helper tests (method.test.ts, form.test.ts,
//! resource.test.ts, resources.test.ts), one test per case.
//!
//! Remix's `only`, `exclude` and `names` options change the keys of the returned object. A
//! Rust route map is a struct, so an app declares the struct it wants with
//! `#[derive(RouteMap)]` and fills it from the helper; those cases port as declared structs,
//! and `listing` checks the exact names, methods, paths and order, so a route that `only`
//! or `exclude` would drop is shown to be absent. Not ported: "throws an error if both only
//! and exclude are specified" (resource and resources), since a declared struct has no way
//! to say both.

use http::Method;
use rmx::router::routes::{
    any, delete, form, form_with, get, head, options, patch, post, prefix, put, resource, resources, resources_with_param, Form, Resources, Route, RouteMap,
};

/// Every route of a map as "name METHOD /path", in declaration order.
fn listing(map: &impl RouteMap) -> Vec<String> {
    map.route_tree().routes().iter().map(|n| format!("{} {}", n.name, n.route.as_ref().unwrap())).collect()
}

fn route(method: Method, pattern: &str) -> Route {
    Route::new(Some(method), pattern)
}

// ---------------------------------------------------------------------------------------
// method.test.ts: route helpers composition
// ---------------------------------------------------------------------------------------

#[derive(RouteMap)]
pub struct Posts {
    pub index: Route,
    pub create: Route,
    pub show: Route,
    pub update: Route,
    pub patch: Route,
    pub destroy: Route,
}

#[derive(RouteMap)]
pub struct Api {
    pub health: Route,
    pub options: Route,
}

#[derive(RouteMap)]
pub struct Composed {
    pub home: Route,
    pub posts: Posts,
    pub api: Api,
}

#[test]
fn composes_route_helpers_in_a_route_map() {
    let routes = Composed {
        home: any("/"),
        posts: Posts {
            index: get("/posts"),
            create: post("/posts"),
            show: get("/posts/:id"),
            update: put("/posts/:id"),
            patch: patch("/posts/:id"),
            destroy: delete("/posts/:id"),
        },
        api: Api { health: head("/api/health"), options: options("/api/settings") },
    };
    assert_eq!(routes.home, Route::new(None, "/"));
    assert_eq!(routes.posts.index, route(Method::GET, "/posts"));
    assert_eq!(routes.posts.create, route(Method::POST, "/posts"));
    assert_eq!(routes.posts.show, route(Method::GET, "/posts/:id"));
    assert_eq!(routes.posts.update, route(Method::PUT, "/posts/:id"));
    assert_eq!(routes.posts.patch, route(Method::PATCH, "/posts/:id"));
    assert_eq!(routes.posts.destroy, route(Method::DELETE, "/posts/:id"));
    assert_eq!(routes.api.health, route(Method::HEAD, "/api/health"));
    assert_eq!(routes.api.options, route(Method::OPTIONS, "/api/settings"));
}

#[derive(RouteMap)]
pub struct Users {
    pub index: Route,
    pub create: Route,
    pub show: Route,
    pub update: Route,
    pub destroy: Route,
}

#[derive(RouteMap)]
pub struct ApiV1 {
    pub users: Users,
}

#[derive(RouteMap)]
pub struct WithApi {
    pub home: Route,
    pub api: ApiV1,
}

#[test]
fn composes_route_helpers_with_base_paths() {
    let base = prefix("api/v1");
    let api_routes = ApiV1 {
        users: Users { index: base.get("/"), create: base.post("/"), show: base.get("/:id"), update: base.put("/:id"), destroy: base.delete("/:id") },
    };
    let routes = WithApi { home: any("/"), api: api_routes };
    assert_eq!(routes.api.users.index, route(Method::GET, "/api/v1"));
    assert_eq!(routes.api.users.create, route(Method::POST, "/api/v1"));
    assert_eq!(routes.api.users.show, route(Method::GET, "/api/v1/:id"));
    assert_eq!(routes.api.users.update, route(Method::PUT, "/api/v1/:id"));
    assert_eq!(routes.api.users.destroy, route(Method::DELETE, "/api/v1/:id"));
}

#[derive(RouteMap)]
pub struct Profile {
    pub show: Route,
    pub edit: Route,
    pub update: Route,
}

#[derive(RouteMap)]
pub struct Mixed {
    pub home: Route,
    pub about: Route,
    pub contact: Route,
    pub login: Route,
    pub logout: Route,
    pub profile: Profile,
}

#[test]
fn mixes_helper_methods_with_string_patterns() {
    // A bare string in a Remix route map is a route for any method: `any` here.
    let routes = Mixed {
        home: any("/"),
        about: any("/about"),
        contact: get("/contact"),
        login: post("/auth/login"),
        logout: delete("/auth/logout"),
        profile: Profile { show: any("/profile"), edit: get("/profile/edit"), update: patch("/profile") },
    };
    assert_eq!(routes.home, Route::new(None, "/"));
    assert_eq!(routes.about, Route::new(None, "/about"));
    assert_eq!(routes.contact, route(Method::GET, "/contact"));
    assert_eq!(routes.login, route(Method::POST, "/auth/login"));
    assert_eq!(routes.logout, route(Method::DELETE, "/auth/logout"));
    assert_eq!(routes.profile.show, Route::new(None, "/profile"));
    assert_eq!(routes.profile.edit, route(Method::GET, "/profile/edit"));
    assert_eq!(routes.profile.update, route(Method::PATCH, "/profile"));
}

#[derive(RouteMap)]
pub struct ComplexApi {
    pub posts: Route,
    pub create_post: Route,
    pub update_post: Route,
    pub delete_post: Route,
}

#[derive(RouteMap)]
pub struct Complex {
    pub api: ComplexApi,
    pub health_check: Route,
}

#[test]
fn uses_helper_methods_with_complex_patterns() {
    let routes = Complex {
        api: ComplexApi {
            posts: get("/api/posts(/:lang)"),
            create_post: post("/api/posts"),
            update_post: put("/api/posts/:id"),
            delete_post: delete("/api/posts/:id"),
        },
        health_check: head("/health"),
    };
    assert_eq!(routes.api.posts, route(Method::GET, "/api/posts(/:lang)"));
    assert_eq!(routes.api.create_post, route(Method::POST, "/api/posts"));
    assert_eq!(routes.api.update_post, route(Method::PUT, "/api/posts/:id"));
    assert_eq!(routes.api.delete_post, route(Method::DELETE, "/api/posts/:id"));
    assert_eq!(routes.health_check, route(Method::HEAD, "/health"));
}

/// method.test.ts's type assertions on `composedRoutes`, which spreads
/// `route('posts', {...})` into the top level.
#[test]
fn composed_routes_join_a_prefix_and_keep_each_method() {
    let posts = prefix("posts");
    let all = [
        posts.get("/"),
        posts.post("/"),
        posts.get(":id"),
        posts.put(":id"),
        posts.delete(":id"),
        head("/api/health"),
        options("/api/settings"),
        patch("/patch"),
        put("/misc/put"),
        prefix("api").head("health"),
        prefix("api").options("settings"),
    ]
    .map(|r| r.to_string());
    assert_eq!(
        all,
        [
            "GET /posts",
            "POST /posts",
            "GET /posts/:id",
            "PUT /posts/:id",
            "DELETE /posts/:id",
            "HEAD /api/health",
            "OPTIONS /api/settings",
            "PATCH /patch",
            "PUT /misc/put",
            "HEAD /api/health",
            "OPTIONS /api/settings",
        ]
    );
}

// ---------------------------------------------------------------------------------------
// form.test.ts: form routes helper
// ---------------------------------------------------------------------------------------

#[test]
fn creates_a_route_map_with_index_and_action_routes() {
    let login = form("login");
    assert_eq!(login.index, route(Method::GET, "/login"));
    assert_eq!(login.action, route(Method::POST, "/login"));
    assert_eq!(listing(&login), ["index GET /login", "action POST /login"]);
}

#[test]
fn supports_a_custom_form_method() {
    let settings = form_with("settings", Method::PUT);
    assert_eq!(settings.index, route(Method::GET, "/settings"));
    assert_eq!(settings.action, route(Method::PUT, "/settings"));
}

/// `form('profile', { names: { index: 'show' } })`
#[derive(RouteMap)]
pub struct ProfileForm {
    pub show: Route,
    pub action: Route,
}

#[test]
fn supports_a_custom_index_name() {
    let f = form("profile");
    let profile = ProfileForm { show: f.index, action: f.action };
    assert_eq!(profile.show, route(Method::GET, "/profile"));
    assert_eq!(profile.action, route(Method::POST, "/profile"));
    assert_eq!(listing(&profile), ["show GET /profile", "action POST /profile"]);
}

/// `form('signup', { names: { action: 'register' } })`
#[derive(RouteMap)]
pub struct SignupForm {
    pub index: Route,
    pub register: Route,
}

#[test]
fn supports_a_custom_action_name() {
    let f = form("signup");
    let signup = SignupForm { index: f.index, register: f.action };
    assert_eq!(signup.index, route(Method::GET, "/signup"));
    assert_eq!(signup.register, route(Method::POST, "/signup"));
    assert_eq!(listing(&signup), ["index GET /signup", "register POST /signup"]);
}

/// `form('contact', { names: { index: 'show', action: 'submit' } })`
#[derive(RouteMap)]
pub struct ContactForm {
    pub show: Route,
    pub submit: Route,
}

#[test]
fn supports_custom_names_for_both_index_and_action() {
    let f = form("contact");
    let contact = ContactForm { show: f.index, submit: f.action };
    assert_eq!(contact.show, route(Method::GET, "/contact"));
    assert_eq!(contact.submit, route(Method::POST, "/contact"));
    assert_eq!(listing(&contact), ["show GET /contact", "submit POST /contact"]);
}

/// `form('account', { formMethod: 'PATCH', names: { index: 'edit', action: 'update' } })`
#[derive(RouteMap)]
pub struct AccountForm {
    pub edit: Route,
    pub update: Route,
}

#[test]
fn supports_custom_names_with_custom_form_method() {
    let f = form_with("account", Method::PATCH);
    let account = AccountForm { edit: f.index, update: f.action };
    assert_eq!(account.edit, route(Method::GET, "/account"));
    assert_eq!(account.update, route(Method::PATCH, "/account"));
    assert_eq!(listing(&account), ["edit GET /account", "update PATCH /account"]);
}

#[derive(RouteMap)]
pub struct Account {
    pub index: Route,
    pub action: Route,
    pub settings: Form,
}

#[derive(RouteMap)]
pub struct WithAccount {
    pub account: Account,
}

#[test]
fn creates_nested_forms() {
    let f = form("account");
    let routes = WithAccount { account: Account { index: f.index, action: f.action, settings: form("account/settings") } };
    assert_eq!(routes.account.index, route(Method::GET, "/account"));
    assert_eq!(routes.account.action, route(Method::POST, "/account"));
    assert_eq!(routes.account.settings.index, route(Method::GET, "/account/settings"));
    assert_eq!(routes.account.settings.action, route(Method::POST, "/account/settings"));
    assert_eq!(
        listing(&routes),
        [
            "account.index GET /account",
            "account.action POST /account",
            "account.settings.index GET /account/settings",
            "account.settings.action POST /account/settings",
        ]
    );
}

// ---------------------------------------------------------------------------------------
// resource.test.ts: resource routes helper
// ---------------------------------------------------------------------------------------

#[test]
fn creates_a_resource() {
    let book = resource("book");
    // Key order matters: new comes before show.
    assert_eq!(
        listing(&book),
        ["new GET /book/new", "show GET /book", "create POST /book", "edit GET /book/edit", "update PUT /book", "destroy DELETE /book"]
    );
    assert_eq!(book.show, route(Method::GET, "/book"));
    assert_eq!(book.new, route(Method::GET, "/book/new"));
    assert_eq!(book.create, route(Method::POST, "/book"));
    assert_eq!(book.edit, route(Method::GET, "/book/edit"));
    assert_eq!(book.update, route(Method::PUT, "/book"));
    assert_eq!(book.destroy, route(Method::DELETE, "/book"));
}

/// `resource('book', { only: ['show', 'update'] })`, and the same map as
/// `{ exclude: ['new', 'create', 'edit', 'destroy'] }`.
#[derive(RouteMap)]
pub struct ShowUpdateBook {
    pub show: Route,
    pub update: Route,
}

#[test]
fn creates_a_resource_with_only_option() {
    let r = resource("book");
    let book = ShowUpdateBook { show: r.show, update: r.update };
    assert_eq!(book.show, route(Method::GET, "/book"));
    assert_eq!(book.update, route(Method::PUT, "/book"));
    assert_eq!(listing(&book), ["show GET /book", "update PUT /book"]);
}

#[test]
fn creates_a_resource_with_exclude_option() {
    let r = resource("book");
    let book = ShowUpdateBook { show: r.show, update: r.update };
    assert_eq!(book.show, route(Method::GET, "/book"));
    assert_eq!(book.update, route(Method::PUT, "/book"));
    let all = listing(&book).join("\n");
    for excluded in ["new", "create", "edit", "destroy"] {
        assert!(!all.contains(excluded), "{excluded} in {all}");
    }
}

/// `resource('book', { names: { show: 'view', new: 'newForm', ... } })`
#[derive(RouteMap)]
pub struct RenamedBook {
    pub view: Route,
    pub new_form: Route,
    pub store: Route,
    pub edit_form: Route,
    pub save: Route,
    pub delete: Route,
}

#[test]
fn creates_a_resource_with_custom_route_names() {
    let r = resource("book");
    let book = RenamedBook { view: r.show, new_form: r.new, store: r.create, edit_form: r.edit, save: r.update, delete: r.destroy };
    assert_eq!(
        listing(&book),
        ["view GET /book", "new_form GET /book/new", "store POST /book", "edit_form GET /book/edit", "save PUT /book", "delete DELETE /book"]
    );
}

/// `resource('book', { names: { show: 'view', create: 'store' } })`
#[derive(RouteMap)]
pub struct PartlyRenamedBook {
    pub view: Route,
    pub new: Route,
    pub store: Route,
    pub edit: Route,
    pub update: Route,
    pub destroy: Route,
}

#[test]
fn creates_a_resource_with_partial_custom_route_names() {
    let r = resource("book");
    let book = PartlyRenamedBook { view: r.show, new: r.new, store: r.create, edit: r.edit, update: r.update, destroy: r.destroy };
    assert_eq!(
        listing(&book),
        ["view GET /book", "new GET /book/new", "store POST /book", "edit GET /book/edit", "update PUT /book", "destroy DELETE /book"]
    );
}

/// `resource('book', { only: ['show', 'create'], names: { show: 'view', create: 'store', update: 'save' } })`
#[derive(RouteMap)]
pub struct ViewStoreBook {
    pub view: Route,
    pub store: Route,
}

#[test]
fn creates_a_resource_with_custom_route_names_and_only_option() {
    let r = resource("book");
    let book = ViewStoreBook { view: r.show, store: r.create };
    assert_eq!(listing(&book), ["view GET /book", "store POST /book"]);
}

/// `resource('book', { exclude: ['new', 'edit', 'destroy'], names: { show: 'view', create: 'store', update: 'save' } })`
#[derive(RouteMap)]
pub struct ViewStoreSaveBook {
    pub view: Route,
    pub store: Route,
    pub save: Route,
}

#[test]
fn creates_a_resource_with_custom_route_names_and_exclude_option() {
    let r = resource("book");
    let book = ViewStoreSaveBook { view: r.show, store: r.create, save: r.update };
    assert_eq!(listing(&book), ["view GET /book", "store POST /book", "save PUT /book"]);
}

// ---------------------------------------------------------------------------------------
// resources.test.ts: resources routes helper
// ---------------------------------------------------------------------------------------

#[test]
fn creates_resources_with_index_route() {
    let books = resources("books");
    // Key order matters: new comes before show.
    assert_eq!(
        listing(&books),
        [
            "index GET /books",
            "new GET /books/new",
            "show GET /books/:id",
            "create POST /books",
            "edit GET /books/:id/edit",
            "update PUT /books/:id",
            "destroy DELETE /books/:id",
        ]
    );
    assert_eq!(books.index, route(Method::GET, "/books"));
    assert_eq!(books.new, route(Method::GET, "/books/new"));
    assert_eq!(books.show, route(Method::GET, "/books/:id"));
    assert_eq!(books.create, route(Method::POST, "/books"));
    assert_eq!(books.edit, route(Method::GET, "/books/:id/edit"));
    assert_eq!(books.update, route(Method::PUT, "/books/:id"));
    assert_eq!(books.destroy, route(Method::DELETE, "/books/:id"));
}

#[test]
fn creates_resources_with_custom_param() {
    let posts = resources_with_param("posts", "slug");
    assert_eq!(
        listing(&posts),
        [
            "index GET /posts",
            "new GET /posts/new",
            "show GET /posts/:slug",
            "create POST /posts",
            "edit GET /posts/:slug/edit",
            "update PUT /posts/:slug",
            "destroy DELETE /posts/:slug",
        ]
    );
    assert_eq!(posts.update.href([("slug", "hello")]), "/posts/hello");
}

/// `resources('books', { only: ['index', 'show', 'create'] })`, and the same map as
/// `{ exclude: ['new', 'edit', 'update', 'destroy'] }`.
#[derive(RouteMap)]
pub struct IndexShowCreateBooks {
    pub index: Route,
    pub show: Route,
    pub create: Route,
}

#[test]
fn creates_resources_with_only_option() {
    let r = resources("books");
    let books = IndexShowCreateBooks { index: r.index, show: r.show, create: r.create };
    assert_eq!(listing(&books), ["index GET /books", "show GET /books/:id", "create POST /books"]);
}

#[test]
fn creates_resources_with_exclude_option() {
    let r = resources("books");
    let books = IndexShowCreateBooks { index: r.index, show: r.show, create: r.create };
    let all = listing(&books).join("\n");
    for excluded in ["new", "edit", "update", "destroy"] {
        assert!(!all.contains(excluded), "{excluded} in {all}");
    }
    assert_eq!(books.create, route(Method::POST, "/books"));
}

/// `resources('articles', { only: ['index', 'show'], param: 'slug' })`
#[derive(RouteMap)]
pub struct IndexShowArticles {
    pub index: Route,
    pub show: Route,
}

#[test]
fn creates_resources_with_custom_param_and_only_option() {
    let r = resources_with_param("articles", "slug");
    let articles = IndexShowArticles { index: r.index, show: r.show };
    assert_eq!(listing(&articles), ["index GET /articles", "show GET /articles/:slug"]);
}

/// `resources('books', { names: { index: 'list', new: 'newForm', ... } })`
#[derive(RouteMap)]
pub struct RenamedBooks {
    pub list: Route,
    pub new_form: Route,
    pub view: Route,
    pub store: Route,
    pub edit_form: Route,
    pub save: Route,
    pub delete: Route,
}

#[test]
fn creates_resources_with_custom_route_names() {
    let r = resources("books");
    let books = RenamedBooks { list: r.index, new_form: r.new, view: r.show, store: r.create, edit_form: r.edit, save: r.update, delete: r.destroy };
    assert_eq!(
        listing(&books),
        [
            "list GET /books",
            "new_form GET /books/new",
            "view GET /books/:id",
            "store POST /books",
            "edit_form GET /books/:id/edit",
            "save PUT /books/:id",
            "delete DELETE /books/:id",
        ]
    );
}

/// `resources('books', { names: { index: 'list', show: 'view', create: 'store' } })`
#[derive(RouteMap)]
pub struct PartlyRenamedBooks {
    pub list: Route,
    pub new: Route,
    pub view: Route,
    pub store: Route,
    pub edit: Route,
    pub update: Route,
    pub destroy: Route,
}

#[test]
fn creates_resources_with_partial_custom_route_names() {
    let r = resources("books");
    let books = PartlyRenamedBooks { list: r.index, new: r.new, view: r.show, store: r.create, edit: r.edit, update: r.update, destroy: r.destroy };
    assert_eq!(
        listing(&books),
        [
            "list GET /books",
            "new GET /books/new",
            "view GET /books/:id",
            "store POST /books",
            "edit GET /books/:id/edit",
            "update PUT /books/:id",
            "destroy DELETE /books/:id",
        ]
    );
}

/// `resources('books', { only: ['index', 'show'], names: { index: 'list', show: 'view', create: 'store' } })`
#[derive(RouteMap)]
pub struct ListViewBooks {
    pub list: Route,
    pub view: Route,
}

#[test]
fn creates_resources_with_custom_route_names_and_only_option() {
    let r = resources("books");
    let books = ListViewBooks { list: r.index, view: r.show };
    assert_eq!(listing(&books), ["list GET /books", "view GET /books/:id"]);
}

/// `resources('books', { exclude: ['new', 'edit', 'update', 'destroy'], names: { index: 'list', show: 'view', create: 'store' } })`
#[derive(RouteMap)]
pub struct ListViewStoreBooks {
    pub list: Route,
    pub view: Route,
    pub store: Route,
}

#[test]
fn creates_resources_with_custom_route_names_and_exclude_option() {
    let r = resources("books");
    let books = ListViewStoreBooks { list: r.index, view: r.show, store: r.create };
    assert_eq!(listing(&books), ["list GET /books", "view GET /books/:id", "store POST /books"]);
}

/// `resources('posts', { param: 'slug', names: { index: 'list', show: 'view', edit: 'editForm', update: 'save', destroy: 'delete' } })`
#[derive(RouteMap)]
pub struct RenamedPosts {
    pub list: Route,
    pub new: Route,
    pub view: Route,
    pub create: Route,
    pub edit_form: Route,
    pub save: Route,
    pub delete: Route,
}

#[test]
fn creates_resources_with_custom_route_names_and_custom_param() {
    let r = resources_with_param("posts", "slug");
    let posts = RenamedPosts { list: r.index, new: r.new, view: r.show, create: r.create, edit_form: r.edit, save: r.update, delete: r.destroy };
    assert_eq!(
        listing(&posts),
        [
            "list GET /posts",
            "new GET /posts/new",
            "view GET /posts/:slug",
            "create POST /posts",
            "edit_form GET /posts/:slug/edit",
            "save PUT /posts/:slug",
            "delete DELETE /posts/:slug",
        ]
    );
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

#[derive(RouteMap)]
pub struct WithBrands {
    pub brands: Brands,
}

#[test]
fn creates_nested_resources() {
    let b = resources("brands");
    let routes = WithBrands {
        brands: Brands {
            index: b.index,
            new: b.new,
            show: b.show,
            create: b.create,
            edit: b.edit,
            update: b.update,
            destroy: b.destroy,
            products: resources("brands/:brandId/products"),
        },
    };
    assert_eq!(
        listing(&routes),
        [
            "brands.index GET /brands",
            "brands.new GET /brands/new",
            "brands.show GET /brands/:id",
            "brands.create POST /brands",
            "brands.edit GET /brands/:id/edit",
            "brands.update PUT /brands/:id",
            "brands.destroy DELETE /brands/:id",
            "brands.products.index GET /brands/:brandId/products",
            "brands.products.new GET /brands/:brandId/products/new",
            "brands.products.show GET /brands/:brandId/products/:id",
            "brands.products.create POST /brands/:brandId/products",
            "brands.products.edit GET /brands/:brandId/products/:id/edit",
            "brands.products.update PUT /brands/:brandId/products/:id",
            "brands.products.destroy DELETE /brands/:brandId/products/:id",
        ]
    );
    assert_eq!(routes.brands.products.edit.href([("brandId", "1"), ("id", "2")]), "/brands/1/products/2/edit");
}
