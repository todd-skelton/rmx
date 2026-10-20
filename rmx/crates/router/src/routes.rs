//! Route definitions (Remix's `remix/routes`): a [`Route`] is a request method plus a URL
//! pattern, and knows how to build its own href.
//!
//! Group routes in plain structs and keep them in a `LazyLock` so handlers, links and form
//! actions all use the same definitions:
//!
//! ```
//! use std::sync::LazyLock;
//! use rmx_router::routes::{form, get, prefix, Form, Route};
//!
//! pub struct Routes { pub home: Route, pub contact: Form, pub blog: Blog }
//! pub struct Blog { pub index: Route, pub show: Route }
//!
//! pub static ROUTES: LazyLock<Routes> = LazyLock::new(|| {
//!     let blog = prefix("blog");
//!     Routes {
//!         home: get("/"),
//!         contact: form("contact"),
//!         blog: Blog { index: blog.get("/"), show: blog.get(":slug") },
//!     }
//! });
//!
//! assert_eq!(ROUTES.blog.show.href([("slug", "hello")]), "/blog/hello");
//! assert_eq!(ROUTES.contact.action.href(()), "/contact");
//! ```

use std::fmt;

use http::Method;
use rmx_route_pattern::{HrefParams, RoutePattern};
use rmx_url::SearchParams;

/// A request method (or any method) plus a pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    method: Option<Method>,
    pattern: RoutePattern,
}

impl Route {
    /// A route for `method`, or for any method when `None`. Panics on an invalid pattern,
    /// since patterns are written in source code.
    pub fn new(method: Option<Method>, pattern: &str) -> Route {
        let pattern = RoutePattern::parse(pattern).unwrap_or_else(|e| panic!("{e}"));
        Route { method, pattern }
    }

    pub fn method(&self) -> Option<&Method> {
        self.method.as_ref()
    }

    pub fn pattern(&self) -> &RoutePattern {
        &self.pattern
    }

    /// Build a link to this route. Panics when a required param is missing, which is a bug
    /// in the calling code; use [`Route::try_href`] when params come from user input.
    pub fn href(&self, params: impl HrefParams) -> String {
        self.try_href(params).unwrap_or_else(|e| panic!("href for {}: {e}", self.pattern))
    }

    pub fn try_href(&self, params: impl HrefParams) -> Result<String, rmx_route_pattern::HrefError> {
        self.pattern.href(params)
    }

    /// Build a link with extra search params.
    pub fn href_with_search(&self, params: impl HrefParams, search: &SearchParams) -> String {
        self.pattern
            .href_with_search(params, search)
            .unwrap_or_else(|e| panic!("href for {}: {e}", self.pattern))
    }

    /// This route mounted below `base`.
    pub fn under(&self, base: &RoutePattern) -> Route {
        Route { method: self.method.clone(), pattern: base.join(&self.pattern) }
    }
}

impl fmt::Display for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.method {
            Some(m) => write!(f, "{m} {}", self.pattern),
            None => write!(f, "ANY {}", self.pattern),
        }
    }
}

/// Anything that names a route: a [`Route`], or a pattern string (any method).
pub trait IntoRoute {
    fn into_route(self, method: Option<Method>) -> Route;
}

impl IntoRoute for &str {
    fn into_route(self, method: Option<Method>) -> Route {
        Route::new(method, self)
    }
}

impl IntoRoute for String {
    fn into_route(self, method: Option<Method>) -> Route {
        Route::new(method, &self)
    }
}

impl IntoRoute for &Route {
    fn into_route(self, method: Option<Method>) -> Route {
        match method {
            Some(m) => Route { method: Some(m), pattern: self.pattern.clone() },
            None => self.clone(),
        }
    }
}

impl IntoRoute for Route {
    fn into_route(self, method: Option<Method>) -> Route {
        (&self).into_route(method)
    }
}

/// A route that matches any method.
pub fn any(pattern: &str) -> Route {
    Route::new(None, pattern)
}

pub fn get(pattern: &str) -> Route {
    Route::new(Some(Method::GET), pattern)
}

pub fn post(pattern: &str) -> Route {
    Route::new(Some(Method::POST), pattern)
}

pub fn put(pattern: &str) -> Route {
    Route::new(Some(Method::PUT), pattern)
}

pub fn patch(pattern: &str) -> Route {
    Route::new(Some(Method::PATCH), pattern)
}

pub fn delete(pattern: &str) -> Route {
    Route::new(Some(Method::DELETE), pattern)
}

pub fn head(pattern: &str) -> Route {
    Route::new(Some(Method::HEAD), pattern)
}

/// A base pattern that nested routes are joined onto, like `route('frames', {...})`.
#[derive(Clone, Debug)]
pub struct Prefix(RoutePattern);

pub fn prefix(pattern: &str) -> Prefix {
    Prefix(RoutePattern::parse(pattern).unwrap_or_else(|e| panic!("{e}")))
}

impl Prefix {
    pub fn route(&self, method: Option<Method>, pattern: &str) -> Route {
        Route::new(method, pattern).under(&self.0)
    }
    pub fn any(&self, pattern: &str) -> Route {
        self.route(None, pattern)
    }
    pub fn get(&self, pattern: &str) -> Route {
        self.route(Some(Method::GET), pattern)
    }
    pub fn post(&self, pattern: &str) -> Route {
        self.route(Some(Method::POST), pattern)
    }
    pub fn put(&self, pattern: &str) -> Route {
        self.route(Some(Method::PUT), pattern)
    }
    pub fn delete(&self, pattern: &str) -> Route {
        self.route(Some(Method::DELETE), pattern)
    }
    pub fn prefix(&self, pattern: &str) -> Prefix {
        Prefix(self.0.join(&RoutePattern::parse(pattern).unwrap_or_else(|e| panic!("{e}"))))
    }
    pub fn form(&self, pattern: &str) -> Form {
        let p = self.prefix(pattern);
        Form { index: p.get("/"), action: p.post("/") }
    }
    pub fn pattern(&self) -> &RoutePattern {
        &self.0
    }
}

/// `index` (GET) shows a form and `action` (POST) handles it, at the same URL.
#[derive(Clone, Debug)]
pub struct Form {
    pub index: Route,
    pub action: Route,
}

pub fn form(pattern: &str) -> Form {
    Form { index: get(pattern), action: post(pattern) }
}

/// The seven RESTful routes for a collection (`resources('books')`).
#[derive(Clone, Debug)]
pub struct Resources {
    /// GET /books
    pub index: Route,
    /// GET /books/new
    pub new: Route,
    /// POST /books
    pub create: Route,
    /// GET /books/:id
    pub show: Route,
    /// GET /books/:id/edit
    pub edit: Route,
    /// PUT /books/:id
    pub update: Route,
    /// DELETE /books/:id
    pub destroy: Route,
}

/// Collection routes with `:id` as the member param.
pub fn resources(base: &str) -> Resources {
    resources_with_param(base, "id")
}

pub fn resources_with_param(base: &str, param: &str) -> Resources {
    let b = prefix(base);
    let member = format!(":{param}");
    Resources {
        index: b.get("/"),
        new: b.get("new"),
        create: b.post("/"),
        show: b.get(&member),
        edit: b.get(&format!("{member}/edit")),
        update: b.put(&member),
        destroy: b.delete(&member),
    }
}

/// The six routes for a singleton resource (`resource('profile')`).
#[derive(Clone, Debug)]
pub struct Resource {
    pub new: Route,
    pub create: Route,
    pub show: Route,
    pub edit: Route,
    pub update: Route,
    pub destroy: Route,
}

pub fn resource(base: &str) -> Resource {
    let b = prefix(base);
    Resource {
        new: b.get("new"),
        create: b.post("/"),
        show: b.get("/"),
        edit: b.get("edit"),
        update: b.put("/"),
        destroy: b.delete("/"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_build_expected_routes() {
        let r = resources("brands/:brandId/products");
        assert_eq!(r.index.to_string(), "GET /brands/:brandId/products");
        assert_eq!(r.show.to_string(), "GET /brands/:brandId/products/:id");
        assert_eq!(r.edit.href([("brandId", "1"), ("id", "2")]), "/brands/1/products/2/edit");
        assert_eq!(r.update.to_string(), "PUT /brands/:brandId/products/:id");
        let p = resource("users/:userId/profile");
        assert_eq!(p.show.to_string(), "GET /users/:userId/profile");
        assert_eq!(p.edit.to_string(), "GET /users/:userId/profile/edit");
        let f = prefix("frames").form("account");
        assert_eq!(f.action.to_string(), "POST /frames/account");
        assert_eq!(prefix("frames").get("/sidebar").href(()), "/frames/sidebar");
    }
}
