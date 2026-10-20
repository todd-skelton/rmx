//! `app/routes.ts`: the route map, shared by the server and the client entries.

use std::sync::LazyLock;

use rmx_router::routes::{form, get, post, prefix, resources_with_param, Form, Resources, Route};

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

pub struct Fragments {
    pub cart_button: Route,
    pub cart_items: Route,
}

pub struct Api {
    pub cart_toggle: Route,
}

pub struct Books {
    pub index: Route,
    pub genre: Route,
    pub show: Route,
}

pub struct Auth {
    pub login: Form,
    pub register: Form,
    pub logout: Route,
    pub forgot_password: Form,
    pub reset_password: Form,
}

pub struct Account {
    pub index: Route,
    /// `form('settings', { formMethod: 'PUT', names: { action: 'update' } })`
    pub settings: SettingsForm,
    /// `resources('orders', { only: ['index', 'show'], param: 'orderId' })`
    pub orders: Resources,
}

pub struct SettingsForm {
    pub index: Route,
    pub update: Route,
}

pub struct Cart {
    pub index: Route,
    pub api: CartApi,
}

pub struct CartApi {
    pub add: Route,
    pub update: Route,
    pub remove: Route,
}

pub struct Checkout {
    pub index: Route,
    pub action: Route,
    pub confirmation: Route,
}

pub struct Admin {
    pub index: Route,
    /// Full CRUD on books.
    pub books: Resources,
    /// `only: ['index', 'show', 'edit', 'update', 'destroy']`: users register themselves.
    pub users: Resources,
    /// `only: ['index', 'show']`
    pub orders: Resources,
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
            orders: resources_with_param("account/orders", "orderId"),
        },
        cart: Cart {
            index: cart.get("/"),
            api: CartApi { add: cart.post("api/add"), update: cart.put("api/update"), remove: cart.delete("api/remove") },
        },
        checkout: Checkout { index: checkout.get("/"), action: checkout.post("/"), confirmation: checkout.get(":orderId/confirmation") },
        admin: Admin {
            index: get("/admin"),
            books: resources_with_param("admin/books", "bookId"),
            users: resources_with_param("admin/users", "userId"),
            orders: resources_with_param("admin/orders", "orderId"),
        },
    }
});
