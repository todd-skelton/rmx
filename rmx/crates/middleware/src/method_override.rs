//! Let HTML forms send PUT/PATCH/DELETE via a hidden `_method` field.

use http::Method;
use rmx_router::{Context, Middleware, Next};

use crate::form_data::FormDataExt;

/// Overrides the request method with the uppercased value of the `_method` form field.
///
/// Must run after [`crate::form_data()`], which only parses forms for requests other than GET
/// and HEAD. Like Remix `methodOverride()`, the first value for the field is used when it is
/// text, and only the router's request methods (GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS)
/// are accepted.
pub fn method_override() -> impl Middleware {
    method_override_field("_method")
}

/// [`method_override`] with a custom field name (Remix `methodOverride({ fieldName })`).
pub fn method_override_field(field: &'static str) -> impl Middleware {
    move |mut ctx: Context, next: Next| async move {
        let value = ctx.form_data().get_value(field).and_then(|v| v.as_text()).map(str::to_ascii_uppercase);
        if let Some(m) = value.as_deref().and_then(request_method) {
            ctx.set_method(m);
        }
        next.run(ctx).await
    }
}

/// Remix `isRequestMethod`: the methods the router supports.
fn request_method(m: &str) -> Option<Method> {
    Some(match m {
        "GET" => Method::GET,
        "HEAD" => Method::HEAD,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "PATCH" => Method::PATCH,
        "DELETE" => Method::DELETE,
        "OPTIONS" => Method::OPTIONS,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use rmx_fetch::{block_on, Body};
    use rmx_router::{Context, Router};

    fn router(mw: impl rmx_router::Middleware + 'static) -> Router {
        let mut router = Router::new().with(crate::form_data()).with(mw);
        router.post("/", |_: Context| async { "Created" });
        router.delete("/", |_: Context| async { "Deleted" });
        router.put("/", |_: Context| async { "Updated" });
        router
    }

    fn send(router: &Router, content_type: &str, body: &'static str) -> String {
        send_as(router, "POST", content_type, body)
    }

    fn send_as(router: &Router, method: &str, content_type: &str, body: &'static str) -> String {
        let req = http::Request::builder()
            .method(method)
            .uri("https://remix.run/")
            .header("content-type", content_type)
            .body(Body::from(body))
            .unwrap();
        block_on(block_on(router.fetch(req)).into_body().text()).unwrap()
    }

    const FORM: &str = "application/x-www-form-urlencoded";

    // method-override.test.ts: overrides the request method with the value of the method override field
    #[test]
    fn overrides_the_request_method_with_the_field_value() {
        assert_eq!(send(&router(super::method_override()), FORM, "_method=DELETE"), "Deleted");
    }

    #[test]
    fn overrides_any_request_that_carries_a_form() {
        assert_eq!(send_as(&router(super::method_override()), "PUT", FORM, "_method=delete"), "Deleted");
    }

    #[test]
    fn ignores_methods_the_router_does_not_support() {
        let r = router(super::method_override());
        assert_eq!(send(&r, FORM, "_method=PURGE"), "Created");
        assert_eq!(send(&r, FORM, "_method=%20delete"), "Created");
    }

    #[test]
    fn uses_the_first_value_only_when_it_is_text() {
        let r = router(super::method_override());
        assert_eq!(send(&r, FORM, "_method=put&_method=delete"), "Updated");
        let file = "--b\r\nContent-Disposition: form-data; name=\"_method\"; filename=\"m.txt\"\r\n\r\nDELETE\r\n\
                    --b\r\nContent-Disposition: form-data; name=\"_method\"\r\n\r\nPUT\r\n--b--\r\n";
        assert_eq!(send(&r, "multipart/form-data; boundary=b", file), "Created");
    }

    #[test]
    fn reads_a_custom_field_name() {
        let r = router(super::method_override_field("verb"));
        assert_eq!(send(&r, FORM, "verb=delete&_method=put"), "Deleted");
    }

    #[test]
    fn overrides_post_from_form_field() {
        let mut router = Router::new().with(crate::form_data()).with(super::method_override());
        router.delete("/users/:id", |ctx: Context| async move { format!("deleted {}", ctx.param("id").unwrap()) });
        let req = http::Request::builder()
            .method("POST")
            .uri("http://x/users/5")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("_method=delete"))
            .unwrap();
        let res = block_on(router.fetch(req));
        assert_eq!(block_on(res.into_body().text()).unwrap(), "deleted 5");
    }
}
