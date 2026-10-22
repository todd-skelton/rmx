//! Port of Remix's `verify-credentials.test.ts`. Credentials login lives in
//! `rmx_auth_middleware::auth`. Remix's last case calls `verifyCredentials` with an empty context;
//! here it runs in a route, and the thrown error is a `verify` that returns `Err`.

mod common;

use common::*;
use rmx_fetch::Body;
use rmx_auth_middleware::{credentials_provider, verify_credentials};
use rmx_form_data_middleware::FormDataExt;
use rmx_fetch_router::{middleware, Context, Router};
use serde_json::json;

/// `router.post('/login', { middleware: [formData()], handler })`.
fn login_route(handler: impl Fn(Context) -> rmx_fetch::Response + Send + Sync + 'static) -> Router {
    let handler = std::sync::Arc::new(handler);
    let mut router = Router::new();
    router.group(vec![middleware(rmx_form_data_middleware::form_data())], |r| {
        r.post("/login", move |ctx: Context| {
            let handler = handler.clone();
            async move { handler(ctx) }
        });
    });
    router
}

fn post_login(router: &Router, email: &str, password: &str) -> Reply {
    let body = format!("email={email}&password={password}");
    send(router, http::Request::post("https://app.example.com/login").header("content-type", "application/x-www-form-urlencoded"), Body::from(body), None)
}

fn credentials(ctx: &Context) -> (String, String) {
    let form = ctx.form_data();
    (form.get("email").unwrap_or_default().to_string(), form.get("password").unwrap_or_default().to_string())
}

#[test]
fn returns_the_authenticated_result_when_credentials_are_valid() {
    let router = login_route(|ctx| {
        let provider = credentials_provider(credentials, |(email, password): (String, String), _: &Context| {
            std::future::ready((email == "mj@example.com" && password == "secret").then(|| json!({ "id": "u1" })))
        });
        json_response(json!(rmx_fetch::block_on(verify_credentials(&provider, &ctx))))
    });

    assert_eq!(post_login(&router, "mj@example.com", "secret").json(), json!({ "id": "u1" }));
}

#[test]
fn returns_null_when_credentials_are_rejected() {
    let router = login_route(|ctx| {
        let provider = credentials_provider(credentials, |_: (String, String), _: &Context| std::future::ready(None::<serde_json::Value>));
        json_response(json!({ "result": rmx_fetch::block_on(verify_credentials(&provider, &ctx)) }))
    });

    assert_eq!(post_login(&router, "mj@example.com", "wrong").json(), json!({ "result": null }));
}

#[test]
fn rethrows_unexpected_parse_or_verify_errors() {
    let router = login_route(|ctx| {
        let provider = credentials_provider(
            |_: &Context| json!({ "email": "mj@example.com" }),
            |_: serde_json::Value, _: &Context| std::future::ready(Err::<Option<String>, String>("verify failed".into())),
        );
        let result = rmx_fetch::block_on(verify_credentials(&provider, &ctx));
        json_response(json!({ "error": result.unwrap_err() }))
    });

    assert_eq!(post_login(&router, "", "").json(), json!({ "error": "verify failed" }));
}
