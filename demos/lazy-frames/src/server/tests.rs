//! A port of `app/router.test.ts`.

use std::time::Duration;

use super::*;

fn test_router() -> Router {
    router(Config {
        root: concat!(env!("CARGO_MANIFEST_DIR")).into(),
        production: false,
        latency: Duration::from_millis(20),
    })
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime").block_on(f)
}

async fn fetch(router: &Router, method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> (Response, String) {
    let mut req = http::Request::builder().method(method).uri(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let req = req.body(rsmix::fetch::Body::from(body.to_string())).expect("request");
    let res = router.fetch(req).await;
    let (parts, mut body) = res.into_parts();
    let mut text = String::new();
    while let Some(Ok(chunk)) = body.chunk().await {
        text.push_str(&String::from_utf8_lossy(&chunk));
    }
    (Response::from_parts(parts, rsmix::fetch::Body::empty()), text)
}

#[test]
fn server_renders_the_eager_artifact_once() {
    block_on(async {
        let router = test_router();
        let (res, doc) = fetch(&router, "GET", "http://localhost/", &[], "").await;
        assert_eq!(res.status(), 200);
        let marker = format!("data-motion-artifact=\"{MOTION_ARTIFACT_ID}\"");
        assert_eq!(doc.matches(&marker).count(), 1, "the regular Frame inlines the artifact; LazyFrame does not");
        assert!(doc.contains("Not requested"), "lazy placeholders render with the page");
    });
}

#[test]
fn server_renders_the_saved_theme() {
    block_on(async {
        let router = test_router();
        let cookie = format!("{THEME_COOKIE}=dark");
        let (_, doc) = fetch(&router, "GET", "http://localhost/", &[("cookie", &cookie)], "").await;
        assert!(doc.contains(r#"<html lang="en" data-theme="dark""#), "{}", &doc[..200]);
    });
}

#[test]
fn applies_the_latency_cookie_to_subsequent_requests() {
    block_on(async {
        let router = test_router();
        let (toggle, _) = fetch(
            &router,
            "POST",
            "http://localhost/demo-latency",
            &[("content-type", "application/x-www-form-urlencoded")],
            "enabled=true",
        )
        .await;
        assert_eq!(toggle.status(), 303);
        let set_cookie = toggle.headers()[header::SET_COOKIE].to_str().expect("ascii").to_string();
        let cookie = &set_cookie[..set_cookie.find(';').expect("attributes")];

        let (home, _) = fetch(&router, "GET", "http://localhost/", &[("cookie", cookie)], "").await;
        let (frame, _) = fetch(&router, "GET", "http://localhost/frames/html/field-notes", &[("cookie", cookie)], "").await;
        assert_eq!(home.headers()["server-timing"], "demo-latency;dur=20");
        assert_eq!(frame.headers()["server-timing"], "demo-latency;dur=20");

        let (bad, _) = fetch(
            &router,
            "POST",
            "http://localhost/demo-latency",
            &[("content-type", "application/x-www-form-urlencoded")],
            "enabled=maybe",
        )
        .await;
        assert_eq!(bad.status(), 400);
    });
}

#[test]
fn serves_each_frame_response_type_as_html() {
    block_on(async {
        let router = test_router();
        for url in [
            "http://localhost/frames/html/field-notes",
            "http://localhost/frames/ui/signal-board",
            "http://localhost/frames/interactive/retained-counter",
        ] {
            let (res, body) = fetch(&router, "GET", url, &[], "").await;
            assert_eq!(res.status(), 200, "{url}");
            let ct = res.headers()[header::CONTENT_TYPE].to_str().expect("ascii");
            assert!(ct.starts_with("text/html"), "{url}: {ct}");
            assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store", "{url}");
            assert!(!body.is_empty(), "{url}");
        }
        let (interactive, body) = fetch(&router, "GET", "http://localhost/frames/interactive/idea-counter", &[], "").await;
        assert_eq!(interactive.status(), 200);
        assert!(body.contains("rmx:h:"), "the playground is a client entry: {body}");

        let (missing, _) = fetch(&router, "GET", "http://localhost/frames/html/nope", &[], "").await;
        assert_eq!(missing.status(), 404);
        let (wrong_kind, _) = fetch(&router, "GET", "http://localhost/frames/ui/field-notes", &[], "").await;
        assert_eq!(wrong_kind.status(), 404);
    });
}

#[test]
fn lazy_frame_props_carry_their_placeholder_and_fallback() {
    block_on(async {
        let router = test_router();
        let (_, doc) = fetch(&router, "GET", "http://localhost/", &[], "").await;
        let start = doc.find(r#"id="rmx-data""#).expect("hydration data");
        let data = &doc[start..];
        assert!(data.contains(r#""n":"LazyFrame""#), "{data}");
        assert!(data.contains(r#""$rmx":true"#), "element props serialize as descriptors");
        assert!(data.contains("Request in progress"), "the fallback travels with the props");
    });
}
