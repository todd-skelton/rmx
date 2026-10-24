//! Ported from app/router.test.ts, with test/mock-fetch.ts as [registry_mock].

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use rsmix::fetch::{block_on, Body};
use rsmix::file_storage::MemoryFileStorage;

use super::*;
use crate::fetch::Fetch;

fn fixture(name: &str) -> Bytes {
    std::fs::read(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap().into()
}

/// `createNpmRegistryMock({ 'is-number': ... })`: fixture metadata and tarball, 404 otherwise.
fn registry_mock(tarball_fetches: Arc<AtomicUsize>) -> Fetch {
    Fetch::new(move |req: http::Request<()>| {
        let url = req.uri().to_string();
        let counter = tarball_fetches.clone();
        async move {
            let (status, body) = match url.as_str() {
                "https://registry.npmjs.org/is-number" => (200, fixture("is-number-metadata.json")),
                "https://registry.npmjs.org/is-number/-/is-number-7.0.0.tgz" => {
                    counter.fetch_add(1, Ordering::SeqCst);
                    (200, fixture("is-number-7.0.0.tgz"))
                }
                _ => (404, Bytes::from_static(br#"{"error":"Not found"}"#)),
            };
            Ok(http::Response::builder().status(status).body(body).unwrap())
        }
    })
}

fn app() -> (Router, Arc<AtomicUsize>) {
    let fetches = Arc::new(AtomicUsize::new(0));
    (router(Npm::new(registry_mock(fetches.clone()), MemoryFileStorage::new())), fetches)
}

fn send(router: &Router, method: &str, path: &str) -> Response {
    let req = http::Request::builder().method(method).uri(format!("http://localhost{path}")).body(Body::empty()).unwrap();
    block_on(router.fetch(req))
}

fn get(router: &Router, path: &str) -> Response {
    send(router, "GET", path)
}

fn text(res: Response) -> String {
    block_on(res.into_body().text()).unwrap()
}

fn location(res: &Response) -> &str {
    res.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default()
}

#[test]
fn home_page_lists_examples() {
    let (router, _) = app();
    let res = get(&router, "/");
    assert_eq!(res.status(), 200);
    assert!(res.headers()["content-type"].to_str().unwrap().starts_with("text/html"));
    let html = text(res);
    assert!(html.starts_with("<!DOCTYPE html><html"));
    for needle in ["UNPKG", "@remix-run/cookie", "/react", "font-family: ui-monospace, SFMono-Regular, 'SF Mono'"] {
        assert!(html.contains(needle), "{needle}");
    }
}

#[test]
fn invalid_paths_and_unknown_packages_are_404s() {
    let (router, _) = app();
    let res = get(&router, "/@@invalid");
    assert_eq!(res.status(), 404);
    assert!(text(res).contains("Invalid path"));
    let res = get(&router, "/non-existent-package");
    assert_eq!(res.status(), 404);
    assert!(text(res).contains("not found"));
    let res = get(&router, "/is-number@99.0.0");
    assert_eq!(res.status(), 404);
    assert!(text(res).contains("Version &quot;99.0.0&quot; of package &quot;is-number&quot; was not found."));
}

#[test]
fn lists_package_directories_and_shows_files() {
    let (router, fetches) = app();
    let res = get(&router, "/is-number@7.0.0");
    assert_eq!(res.status(), 200);
    let html = text(res);
    for needle in ["is-number", "package.json", "index.js", "Version: 7.0.0", r#"<a href="/is-number@7.0.0/index.js">text/javascript</a>"#] {
        assert!(html.contains(needle), "{needle}");
    }

    let res = get(&router, "/is-number@7.0.0/package.json");
    assert_eq!(res.status(), 200);
    let html = text(res);
    assert!(html.contains("&quot;name&quot;"), "file content is escaped");
    assert!(html.contains("is-number"));
    assert!(html.contains(r#"<nav class="breadcrumb"><a href="/">Home</a> / <a href="/is-number@7.0.0">is-number@7.0.0</a> / <span>package.json</span></nav>"#));
    assert_eq!(fetches.load(Ordering::SeqCst), 1, "the tarball is fetched once, then cached");

    let res = get(&router, "/is-number@7.0.0/missing.js");
    assert_eq!(res.status(), 200, "unknown paths list as empty directories, as in Remix");
}

#[test]
fn non_get_requests_are_405s() {
    let (router, _) = app();
    let res = send(&router, "POST", "/");
    assert_eq!(res.status(), 405);
    assert_eq!(res.headers()["allow"], "GET, HEAD");
}

#[test]
fn redirects_unresolved_versions() {
    let (router, _) = app();
    let cases = [
        ("/is-number", "/is-number@7.0.0"),
        ("/is-number@latest", "/is-number@7.0.0"),
        ("/is-number@1", "/is-number@1.1.2"),
        ("/is-number@2.0", "/is-number@2.0.2"),
        ("/is-number@^1.0.0", "/is-number@1.1.2"),
        ("/is-number@~1.1.0", "/is-number@1.1.2"),
        ("/is-number@%5E2.0.0", "/is-number@2.1.0"),
        ("/is-number@%7E2.0.0", "/is-number@2.0.2"),
        ("/is-number@%3E=1.0.0%20%3C2.0.0", "/is-number@1.1.2"),
        ("/is-number@^7/package.json", "/is-number@7.0.0/package.json"),
    ];
    for (path, to) in cases {
        let res = get(&router, path);
        assert_eq!(res.status(), 302, "{path}");
        assert_eq!(location(&res), to, "{path}");
    }
    assert_eq!(get(&router, "/is-number@7.0.0").status(), 200, "fully resolved versions do not redirect");
}
