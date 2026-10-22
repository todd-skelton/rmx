//! Port of Remix's `complete-auth.test.ts`. `complete_auth` lives in `rmx_auth_middleware::auth`.

mod common;

use common::*;
use rmx_fetch::Body;
use rmx_auth_middleware::complete_auth;
use rmx_router::Context;
use rmx_session::Session;
use serde_json::{json, Value};

#[test]
fn rotates_the_session_id_and_returns_the_session_for_auth_writes() {
    let mut router = session_router();
    router.get("/prepare", |ctx: Context| async move {
        let session = ctx.get::<Session>().unwrap();
        session.set("prepared", true);
        json_response(json!({ "sessionId": session.id() }))
    });
    router.post("/login", |ctx: Context| async move {
        let before_id = ctx.get::<Session>().unwrap().id();
        let session = complete_auth(&ctx);
        session.set("auth", json!({ "userId": "u1" }));
        json_response(json!({
            "beforeId": before_id,
            "afterId": session.id(),
            "prepared": session.get::<Value>("prepared"),
            "auth": session.get::<Value>("auth"),
        }))
    });

    let prepare = get(&router, "/prepare", None);
    let response = send(&router, http::Request::post("https://app.example.com/login"), Body::empty(), Some(&prepare));
    let body = response.json();

    assert_ne!(body["beforeId"], body["afterId"]);
    assert_eq!(body["prepared"], true);
    assert_eq!(body["auth"], json!({ "userId": "u1" }));
}
