//! `app/routes.ts`: the route map, shared by the server and the client entries.

use std::sync::LazyLock;

use rsmix::router::routes::{form, get, post, prefix, resources_with_param, Form, Resources, Route, RouteMap};

#[derive(RouteMap)]
pub struct Routes {
    pub uploads: Route,
    pub fragments: Fragments,
    pub api: Api,
    pub home: Route,
    pub about: Route,
    pub contact: Form,
    pub search: Route,
    pub books: Books,
    pub auth: Auth,
    pub account: Account,
    pub cart: Cart,
    pub checkout: Checkout,
    pub admin: Admin,
}

#[derive(RouteMap)]
pub struct Fragments {
    pub cart_button: Route,
    pub cart_items: Route,
}

#[derive(RouteMap)]
pub struct Api {
    pub cart_toggle: Route,
}

#[derive(RouteMap)]
pub struct Books {
    pub index: Route,
    pub genre: Route,
    pub show: Route,
}

#[derive(RouteMap)]
pub struct Auth {
    pub login: Form,
    pub register: Form,
    pub logout: Route,
    pub forgot_password: Form,
    pub reset_password: Form,
}

#[derive(RouteMap)]
pub struct Account {
    pub index: Route,
    /// `form('settings', { formMethod: 'PUT', names: { action: 'update' } })`
    pub settings: SettingsForm,
    /// `resources('orders', { only: ['index', 'show'], param: 'orderId' })`
    pub orders: ReadOnly,
}

/// A read-only collection: `resources(.., { only: ['index', 'show'] })`.
#[derive(RouteMap)]
pub struct ReadOnly {
    pub index: Route,
    pub show: Route,
}

impl ReadOnly {
    fn new(base: &str, param: &str) -> ReadOnly {
        let r = resources_with_param(base, param);
        ReadOnly { index: r.index, show: r.show }
    }
}

/// `resources('users', { only: ['index', 'show', 'edit', 'update', 'destroy'] })`: users
/// register themselves.
#[derive(RouteMap)]
pub struct Users {
    pub index: Route,
    pub show: Route,
    pub edit: Route,
    pub update: Route,
    pub destroy: Route,
}

#[derive(RouteMap)]
pub struct SettingsForm {
    pub index: Route,
    pub update: Route,
}

#[derive(RouteMap)]
pub struct Cart {
    pub index: Route,
    pub api: CartApi,
}

#[derive(RouteMap)]
pub struct CartApi {
    pub add: Route,
    pub update: Route,
    pub remove: Route,
}

#[derive(RouteMap)]
pub struct Checkout {
    pub index: Route,
    pub action: Route,
    pub confirmation: Route,
}

#[derive(RouteMap)]
pub struct Admin {
    pub index: Route,
    /// Full CRUD on books.
    pub books: Resources,
    pub users: Users,
    pub orders: ReadOnly,
}

pub static ROUTES: LazyLock<Routes> = LazyLock::new(|| {
    let account = prefix("account");
    let cart = prefix("cart");
    let checkout = prefix("checkout");
    Routes {
        uploads: get("/uploads/*key"),
        fragments: Fragments { cart_button: get("/fragments/cart-button/:bookId"), cart_items: get("/fragments/cart-items") },
        api: Api { cart_toggle: post("/api/cart/toggle") },
        home: get("/"),
        about: get("/about"),
        contact: form("contact"),
        search: get("/search"),
        books: Books { index: get("/books"), genre: get("/books/genre/:genre"), show: get("/books/:slug") },
        auth: Auth {
            login: form("login"),
            register: form("register"),
            logout: post("logout"),
            forgot_password: form("forgot-password"),
            reset_password: form("reset-password/:token"),
        },
        account: Account {
            index: account.get("/"),
            settings: SettingsForm { index: account.get("settings"), update: account.put("settings") },
            orders: ReadOnly::new("account/orders", "orderId"),
        },
        cart: Cart {
            index: cart.get("/"),
            api: CartApi { add: cart.post("api/add"), update: cart.put("api/update"), remove: cart.delete("api/remove") },
        },
        checkout: Checkout { index: checkout.get("/"), action: checkout.post("/"), confirmation: checkout.get(":orderId/confirmation") },
        admin: Admin {
            index: get("/admin"),
            books: resources_with_param("admin/books", "bookId"),
            users: {
                let r = resources_with_param("admin/users", "userId");
                Users { index: r.index, show: r.show, edit: r.edit, update: r.update, destroy: r.destroy }
            },
            orders: ReadOnly::new("admin/orders", "orderId"),
        },
    }
});
