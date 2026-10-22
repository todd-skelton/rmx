//! Port of Remix's `fetch-router/src/lib/route-map.test.ts`, one test per case. A
//! `route({ ... })` map is a `#[derive(RouteMap)]` struct, a pattern string is an `any` route,
//! and `route(base, { ... })` is `prefix(base)`. The file's type assertions (each route's
//! method and pattern type) are the field types and values of the structs here.

use http::Method;
use rmx::route_pattern::HrefOptions;
use rmx::router::routes::{any, prefix, Route, RouteMap};
use rmx::url::{SearchParams, Url};

#[test]
fn stores_route_method_and_pattern() {
    let route = Route::new(Some(Method::GET), "/users/:id");

    assert_eq!(route.method(), Some(&Method::GET));
    assert_eq!(route.pattern().to_string(), "/users/:id");
}

#[derive(RouteMap)]
struct About {
    index: Route,
    company: Route,
}

#[test]
fn creates_a_route_map() {
    #[derive(RouteMap)]
    struct Routes {
        home: Route,
        about: About,
    }
    let routes = Routes { home: any("/"), about: About { index: any("about"), company: any("about/company") } };

    assert_eq!(routes.home, Route::new(None, "/"));
    assert_eq!(routes.about.index, Route::new(None, "/about"));
    assert_eq!(routes.about.company, Route::new(None, "/about/company"));
}

#[derive(RouteMap)]
struct Categories {
    index: Route,
    new: Route,
    show: Route,
    edit: Route,
}

fn categories() -> Categories {
    let base = prefix("categories");
    Categories { index: base.any("/"), new: base.any("/new"), show: base.any("/:slug"), edit: base.any("/:slug/edit") }
}

#[test]
fn creates_a_route_map_with_a_base() {
    let routes = categories();

    assert_eq!(routes.index, Route::new(None, "/categories"));
    assert_eq!(routes.new, Route::new(None, "/categories/new"));
    assert_eq!(routes.show, Route::new(None, "/categories/:slug"));
    assert_eq!(routes.edit, Route::new(None, "/categories/:slug/edit"));
}

#[test]
fn creates_a_route_map_with_a_nested_route_map() {
    #[derive(RouteMap)]
    struct Routes {
        home: Route,
        about: Route,
        categories: Categories,
    }
    let routes = Routes { home: any("/"), about: any("/about"), categories: categories() };

    assert_eq!(routes.home, Route::new(None, "/"));
    assert_eq!(routes.about, Route::new(None, "/about"));
    assert_eq!(routes.categories.index, Route::new(None, "/categories"));
    assert_eq!(routes.categories.new, Route::new(None, "/categories/new"));
    assert_eq!(routes.categories.show, Route::new(None, "/categories/:slug"));
    assert_eq!(routes.categories.edit, Route::new(None, "/categories/:slug/edit"));
}

#[test]
fn creates_nested_routes_using_object_spread_syntax() {
    // Spreading a based map into another is building its routes from the same prefix.
    #[derive(RouteMap)]
    struct Routes {
        home: Route,
        posts: Route,
        edit_post: Route,
    }
    let posts = prefix("posts");
    let routes = Routes { home: any("/"), posts: posts.any("/"), edit_post: posts.any("/:slug/edit") };

    assert_eq!(routes.home, Route::new(None, "/"));
    assert_eq!(routes.posts, Route::new(None, "/posts"));
    assert_eq!(routes.edit_post, Route::new(None, "/posts/:slug/edit"));
}

#[test]
fn builds_hrefs_with_search_params_and_a_base_url() {
    let users = any("/users/:id");
    let options = HrefOptions {
        base_url: Some(Url::parse("https://example.com/admin/settings").unwrap()),
        search_params: SearchParams::parse("tab=profile&tab=security"),
    };

    assert_eq!(users.href_with([("id", "123")], &options), "../users/123?tab=profile&tab=security");
}
