//! Let HTML forms send PUT/PATCH/DELETE via a hidden `_method` field.

use http::Method;
use rmx_router::{Context, Middleware, Next};

use crate::form_data::FormDataExt;

/// Must run after [`crate::form_data()`]. Only POST requests are overridden.
pub fn method_override() -> impl Middleware {
    method_override_field("_method")
}

pub fn method_override_field(field: &'static str) -> impl Middleware {
    move |mut ctx: Context, next: Next| async move {
        if ctx.method() == Method::POST {
            let value = ctx.form_data().get(field).map(|v| v.trim().to_ascii_uppercase());
            if let Some(m) = value.and_then(|v| Method::from_bytes(v.as_bytes()).ok()) {
                ctx.set_method(m);
            }
        }
        next.run(ctx).await
    }
}

#[cfg(test)]
mod tests {
    use rmx_fetch::{block_on, Body};
    use rmx_router::{Context, Router};

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
