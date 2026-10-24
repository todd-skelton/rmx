//! `app/routes.ts`: the shared route contract used by the server and the browser.

use std::sync::LazyLock;

use rsmix::router::routes::{any, get, Route, RouteMap};

#[derive(RouteMap)]
pub struct Routes {
    pub assets: Route,
    pub home: Route,
}

pub static ROUTES: LazyLock<Routes> = LazyLock::new(|| Routes { assets: get("/assets/*path"), home: any("/") });
