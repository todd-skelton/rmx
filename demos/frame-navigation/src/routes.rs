//! `app/routes.ts`: the route map, shared by the server and the browser. Remix's
//! `assets` route is left out: the static files middleware serves the wasm client.

use std::sync::LazyLock;

use rmx::router::routes::{get, prefix, Form, Route, RouteMap};

#[derive(RouteMap)]
pub struct Routes {
    pub main: Main,
    pub auth: Auth,
    pub settings: Settings,
}

/// `route('/', { ... })`
#[derive(RouteMap)]
pub struct Main {
    pub index: Route,
    pub courses: Route,
    pub calendar: Route,
    pub account: Account,
}

#[derive(RouteMap)]
pub struct Account {
    pub index: Route,
    pub edit: Form,
}

#[derive(RouteMap)]
pub struct Auth {
    pub login: Form,
    pub logout: Route,
}

#[derive(RouteMap)]
pub struct Settings {
    pub index: Route,
    pub overview: Route,
    pub profile: Route,
    pub notifications: Route,
    pub privacy: Route,
    pub grading: Route,
    pub integrations: Route,
}

pub static ROUTES: LazyLock<Routes> = LazyLock::new(|| {
    let account = prefix("account");
    let auth = prefix("auth");
    let settings = prefix("settings");
    Routes {
        main: Main {
            index: get("/"),
            courses: get("courses"),
            calendar: get("calendar"),
            account: Account { index: account.get("/"), edit: account.form("edit") },
        },
        auth: Auth { login: auth.form("login"), logout: auth.post("logout") },
        settings: Settings {
            index: settings.get("/"),
            overview: settings.get("overview"),
            profile: settings.get("profile"),
            notifications: settings.get("notifications"),
            privacy: settings.get("privacy"),
            grading: settings.get("grading"),
            integrations: settings.get("integrations"),
        },
    }
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_remix_route_map() {
        assert_eq!(ROUTES.main.index.href(()), "/");
        assert_eq!(ROUTES.main.account.edit.action.to_string(), "POST /account/edit");
        assert_eq!(ROUTES.auth.logout.to_string(), "POST /auth/logout");
        assert_eq!(ROUTES.settings.index.href(()), "/settings");
        assert_eq!(ROUTES.settings.integrations.href(()), "/settings/integrations");
    }
}
