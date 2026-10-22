//! A port of `app/router.test.ts`.

use std::time::Duration;

use super::*;

fn test_router() -> Router {
    router(concat!(env!("CARGO_MANIFEST_DIR"), "/public"), Duration::from_millis(20))
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime").block_on(f)
}

async fn fetch(router: &Router, method: &str, url: &str, headers: &[(&str, &str)]) -> (http::response::Parts, String) {
    let mut req = http::Request::builder().method(method).uri(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let res = router.fetch(req.body(Body::empty()).expect("request")).await;
    let (parts, mut body) = res.into_parts();
    let mut text = String::new();
    while let Some(Ok(chunk)) = body.chunk().await {
        text.push_str(&String::from_utf8_lossy(&chunk));
    }
    (parts, text)
}

#[test]
fn home_page_renders_html_with_the_limit() {
    block_on(async {
        let router = test_router();
        let (parts, text) = fetch(&router, "GET", "http://localhost/", &[]).await;
        assert_eq!(parts.status, 200);
        assert!(parts.headers[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/html"));
        assert!(text.contains("<html"));
        assert!(text.contains("Server-Sent Events"));
        assert!(text.contains("Compression"));
        assert!(text.contains("No limit set."));

        let (_, text) = fetch(&router, "GET", "http://localhost/?limit=5", &[]).await;
        assert!(text.contains("Stream will close after <strong>5</strong> messages."), "{text}");
        let (_, text) = fetch(&router, "GET", "http://localhost/?limit=abc", &[]).await;
        assert!(text.contains("No limit set."), "an invalid limit is ignored");
    });
}

#[test]
fn messages_stream_as_server_sent_events() {
    block_on(async {
        let router = test_router();
        let (parts, text) = fetch(&router, "GET", "http://localhost/messages?limit=2", &[]).await;
        assert_eq!(parts.headers[header::CONTENT_TYPE], "text/event-stream");
        assert_eq!(parts.headers[header::CACHE_CONTROL], "no-cache");
        let events: Vec<&str> = text.split("\n\n").filter(|e| !e.trim().is_empty()).collect();
        assert_eq!(events.len(), 2, "{text}");
        assert!(text.contains("event: message"));
        assert!(text.contains(r#""count":1"#));
        assert!(text.contains(r#""count":2"#));
    });
}

#[test]
fn messages_are_compressed_when_accepted() {
    block_on(async {
        let router = test_router();
        let (parts, _) = fetch(&router, "GET", "http://localhost/messages?limit=1", &[("accept-encoding", "gzip")]).await;
        assert_eq!(parts.headers[header::CONTENT_ENCODING], "gzip");
    });
}

#[test]
fn non_get_requests_are_405() {
    block_on(async {
        let router = test_router();
        for url in ["http://localhost/", "http://localhost/messages"] {
            let (parts, _) = fetch(&router, "POST", url, &[]).await;
            assert_eq!(parts.status, 405, "{url}");
            assert_eq!(parts.headers[header::ALLOW], "GET, HEAD", "{url}");
        }
    });
}
