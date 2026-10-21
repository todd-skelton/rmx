//! `app/routes.ts`: the route map. Remix's `assets` route is left out: the static files
//! middleware serves the wasm client.

use std::sync::LazyLock;

use rmx::router::routes::{get, post, prefix, Route, RouteMap};

#[derive(RouteMap)]
pub struct Routes {
    pub home: Route,
    pub time: Route,
    pub reload_scope: Route,
    pub root_reload_client_entries: Route,
    pub scroll_anchoring: Route,
    pub scroll_anchoring_detail: Route,
    pub scroll_restoration: Route,
    pub scroll_restoration_detail: Route,
    pub newsletter_signup: Route,
    pub state_search: Route,
    pub client_mounted: Route,
    pub frames: Frames,
}

/// `route('frames', { ... })`
#[derive(RouteMap)]
pub struct Frames {
    pub client_frame_example: Route,
    pub client_frame_example_nested: Route,
    pub client_mounted_outer: Route,
    pub client_mounted_nested: Route,
    pub root_reload_entry_frame: Route,
    pub sidebar: Route,
    pub activity: Route,
    pub activity_detail: Route,
    pub time: Route,
    pub reload_scope: Route,
    pub reload_scope_blocking: Route,
    pub scroll_anchoring_items: Route,
    pub scroll_restoration_items: Route,
    pub state_search_results: Route,
}

pub static ROUTES: LazyLock<Routes> = LazyLock::new(|| {
    let frames = prefix("frames");
    Routes {
        home: get("/"),
        time: get("/time"),
        reload_scope: get("/reload-scope"),
        root_reload_client_entries: get("/root-reload-client-entries"),
        scroll_anchoring: get("/scroll-anchoring"),
        scroll_anchoring_detail: get("/scroll-anchoring/detail"),
        scroll_restoration: get("/scroll-restoration"),
        scroll_restoration_detail: get("/scroll-restoration/detail"),
        newsletter_signup: post("/scroll-restoration/newsletter"),
        state_search: get("/state-search"),
        client_mounted: get("/client-mounted"),
        frames: Frames {
            client_frame_example: frames.get("/client-frame-example"),
            client_frame_example_nested: frames.get("/client-frame-example/nested"),
            client_mounted_outer: frames.get("/client-mounted-outer"),
            client_mounted_nested: frames.get("/client-mounted-nested"),
            root_reload_entry_frame: frames.get("/root-reload-entry-frame"),
            sidebar: frames.get("/sidebar"),
            activity: frames.get("/activity"),
            activity_detail: frames.get("/activity/detail"),
            time: frames.get("/time"),
            reload_scope: frames.get("/reload-scope"),
            reload_scope_blocking: frames.get("/reload-scope/blocking"),
            scroll_anchoring_items: frames.get("/scroll-anchoring-items"),
            scroll_restoration_items: frames.get("/scroll-restoration-items"),
            state_search_results: frames.get("/state-search-results"),
        },
    }
});
