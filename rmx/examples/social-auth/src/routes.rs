//! `app/routes.ts`: the route map.

use std::sync::LazyLock;

use rmx::router::routes::{form, get, post, prefix, Form, Route, RouteMap};

#[derive(RouteMap)]
pub struct Routes {
    pub home: Route,
    pub account: Route,
    pub auth: Auth,
}

#[derive(RouteMap)]
pub struct Auth {
    pub login: Route,
    pub logout: Route,
    pub signup: Form,
    pub forgot_password: Form,
    pub reset_password: Form,
    pub google: External,
    pub github: External,
    pub x: External,
}

/// `route('/auth/<provider>', { login, callback })`
#[derive(RouteMap)]
pub struct External {
    pub login: Route,
    pub callback: Route,
}

fn external(name: &str) -> External {
    let p = prefix(&format!("/auth/{name}"));
    External { login: p.get("/login"), callback: p.get("/callback") }
}

pub static ROUTES: LazyLock<Routes> = LazyLock::new(|| Routes {
    home: get("/"),
    account: get("/account"),
    auth: Auth {
        login: post("/auth/login"),
        logout: post("/auth/logout"),
        signup: form("/auth/signup"),
        forgot_password: form("/auth/forgot-password"),
        reset_password: form("/auth/reset-password/:token"),
        google: external("google"),
        github: external("github"),
        x: external("x"),
    },
});
