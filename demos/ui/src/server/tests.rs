use super::*;
use rsmix::fetch::Body;

fn get(url: &str) -> (http::response::Parts, String) {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let router = router(concat!(env!("CARGO_MANIFEST_DIR"), "/public"));
        let res = router.fetch(http::Request::get(format!("http://localhost{url}")).body(Body::empty()).unwrap()).await;
        let (parts, mut body) = res.into_parts();
        let mut text = String::new();
        while let Some(Ok(chunk)) = body.chunk().await {
            text.push_str(&String::from_utf8_lossy(&chunk));
        }
        (parts, text)
    })
}

#[test]
fn index_lists_every_demo() {
    let (parts, html) = get("/");
    assert_eq!(parts.status, 200);
    assert_eq!(parts.headers[http::header::CACHE_CONTROL], "no-store");
    for demo in DEMOS {
        assert!(html.contains(&format!("href=\"/demo/{}\"", demo.slug)), "{}", demo.slug);
    }
}

#[test]
fn demos_render_as_client_entries() {
    for demo in DEMOS {
        let (parts, html) = get(&format!("/demo/{}", demo.slug));
        assert_eq!(parts.status, 200, "{}", demo.slug);
        assert!(html.contains("<!--rmx:h:"), "{} is hydrated: {html}", demo.slug);
    }
    assert_eq!(get("/demo/nope").0.status, 404);
}

#[test]
fn server_html_includes_mixin_output() {
    let (_, html) = get("/demo/component/runtime/mixin-lifecycle");
    assert!(html.contains(r#"aria-label="Save: 0 clicks""#), "{html}");
    assert!(html.contains("data-mixin-id=\"rmx-m"), "{html}");
    let (_, html) = get("/demo/component/runtime/controlled-uncontrolled-values");
    assert!(html.contains(r#"<input id="controlled-text" value="hello""#), "{html}");
    assert!(html.contains(r#"<option value="alpha" selected>Alpha</option>"#), "{html}");
    assert!(html.contains(r#"<input id="uncontrolled-text" value="type to update this""#), "{html}");
    let (_, html) = get("/demo/component/runtime/keyed-list");
    assert!(html.contains(r#"placeholder="Item A" value="Item A""#), "{html}");
}
