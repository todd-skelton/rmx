//! `app/routes.ts`: the route map, shared by the server and the client entries.

use std::sync::LazyLock;

use rmx::router::routes::{prefix, Form, Route, RouteMap};

#[derive(RouteMap)]
pub struct Routes {
    pub assets: Route,
    pub home: Route,
    pub auth: Auth,
    pub schedules: Schedules,
}

#[derive(RouteMap)]
pub struct Auth {
    pub index: Route,
    pub login: Form,
    pub logout: Route,
    pub signup: Form,
}

#[derive(RouteMap)]
pub struct Schedules {
    pub index: Route,
    pub create: Route,
    pub destroy: Route,
    pub download_ics: Route,
    pub show: Route,
    pub update: Route,
}

pub static ROUTES: LazyLock<Routes> = LazyLock::new(|| {
    let auth = prefix("/auth");
    let schedules = prefix("/schedules");
    Routes {
        assets: prefix("/assets").get("/*path"),
        home: rmx::router::routes::any("/"),
        auth: Auth { index: auth.any("/"), login: auth.form("login"), logout: auth.post("logout"), signup: auth.form("signup") },
        schedules: Schedules {
            index: schedules.get("/"),
            create: schedules.post("/"),
            destroy: schedules.delete("/:scheduleId"),
            download_ics: schedules.get("/:scheduleId/download.ics"),
            show: schedules.get("/:scheduleId"),
            update: schedules.put("/:scheduleId"),
        },
    }
});

/// `routes.schedules.show.href({ scheduleId })`
pub fn schedule_href(id: i64) -> String {
    ROUTES.schedules.show.href([("scheduleId", id.to_string().as_str())])
}
