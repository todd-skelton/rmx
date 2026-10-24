//! `app/actions/controller.test.ts`, plus the pieces the page depends on.

use std::path::Path;

use rsmix::fetch::{block_on, Body};

use super::home_page::PROMPTS;
use super::router;
use crate::routes::ROUTES;

fn get(path: &str) -> (u16, String, String) {
    let router = router(Path::new(env!("CARGO_MANIFEST_DIR")), false).expect("asset server");
    let req = http::Request::get(format!("http://localhost{path}")).body(Body::empty()).unwrap();
    let res = block_on(router.fetch(req));
    let status = res.status().as_u16();
    let content_type = res.headers().get("content-type").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
    (status, content_type, block_on(res.into_body().text()).unwrap())
}

#[test]
fn get_home_returns_the_home_page() {
    let (status, content_type, html) = get(&ROUTES.home.href(()));
    assert_eq!(status, 200);
    assert!(content_type.starts_with("text/html"), "{content_type}");
    assert!(html.contains("<html"), "{html}");
    assert!(html.contains("Welcome to") && html.contains("Coding with AI?"));
    for prompt in PROMPTS {
        assert!(html.contains(prompt), "{prompt}");
    }
}

#[test]
fn the_page_loads_the_client_entry_through_the_asset_server() {
    let (_, _, html) = get("/");
    assert!(html.contains(r#"<script type="module" src="/assets/app/actions/public/entry.js"></script>"#), "{html}");
    let (status, content_type, body) = get("/assets/app/actions/public/entry.js");
    assert_eq!(status, 200);
    assert!(content_type.contains("javascript"), "{content_type}");
    assert!(body.contains("template_demo.js"));
}

#[test]
fn serves_root_public_files_and_keeps_app_sources_private() {
    let (status, content_type, _) = get("/favicon.svg");
    assert_eq!((status, content_type.as_str()), (200, "image/svg+xml"));
    assert_eq!(get("/assets/app/routes.rs").0, 404);
    assert_eq!(get("/missing").0, 404, "home matches / only");
}
