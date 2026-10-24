//! Router tests for the assets demo. They run against a copy of `app/` in a temp directory
//! so edits can be simulated, with a stub client module in place of the wasm-bindgen build.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use http::header;
use rsmix::fetch::Body;

use super::*;

fn fixture() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("rsmix-assets-demo-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let public = dir.join("app/actions/public");
    std::fs::create_dir_all(public.join("pkg")).unwrap();
    std::fs::create_dir_all(public.join("styles")).unwrap();
    std::fs::create_dir_all(public.join("images")).unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("app/actions/public");
    for f in ["entry.js", "content.js", "styles/app.css", "images/image.svg"] {
        std::fs::copy(src.join(f), public.join(f)).unwrap();
    }
    std::fs::write(
        public.join("pkg/assets_demo.js"),
        "export function mount() {}\nexport default async function init() { await fetch(new URL('assets_demo_bg.wasm', import.meta.url)) }\n",
    )
    .unwrap();
    std::fs::write(public.join("pkg/assets_demo_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
    std::fs::write(public.join("entry.test.js"), "secret").unwrap();
    dir
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime").block_on(f)
}

fn get(router: &Router, url: &str, headers: &[(&str, &str)]) -> (http::response::Parts, String) {
    block_on(async {
        let mut req = http::Request::builder().uri(format!("http://localhost{url}"));
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let res = router.fetch(req.body(Body::empty()).unwrap()).await;
        let (parts, mut body) = res.into_parts();
        let mut text = String::new();
        while let Some(Ok(chunk)) = body.chunk().await {
            text.push_str(&String::from_utf8_lossy(&chunk));
        }
        (parts, text)
    })
}

/// The `href`/`src` values of every tag starting with `tag` in the page.
fn attrs(html: &str, tag: &str, attr: &str) -> Vec<String> {
    html.split(tag)
        .skip(1)
        .filter_map(|t| {
            let t = &t[..t.find('>')?];
            let start = t.find(&format!("{attr}=\""))? + attr.len() + 2;
            Some(t[start..start + t[start..].find('"')?].replace("&amp;", "&"))
        })
        .collect()
}

#[test]
fn home_links_assets_through_the_asset_server() {
    let router = router(asset_server(&fixture(), false).unwrap());
    let (parts, html) = get(&router, "/", &[]);
    assert_eq!(parts.status, 200, "{html}");
    assert!(html.contains("Loading client app..."));
    assert_eq!(attrs(&html, "<link rel=\"stylesheet\"", "href"), ["/assets/app/actions/public/styles/app.css"]);
    assert_eq!(
        attrs(&html, "<link rel=\"modulepreload\"", "href"),
        ["/assets/app/actions/public/entry.js", "/assets/app/actions/public/content.js", "/assets/app/actions/public/pkg/assets_demo.js"],
    );
    assert_eq!(
        attrs(&html, "<img", "src"),
        ["/assets/app/actions/public/images/image.svg", "/assets/app/actions/public/images/image.svg?transform=recolor%3A%238B5CF6"],
    );
    assert_eq!(attrs(&html, "<script type=\"module\"", "src"), ["/assets/app/actions/public/entry.js"]);
}

#[test]
fn modules_css_and_svgs_are_rewritten_and_transformed() {
    let dir = fixture();
    let router = router(asset_server(&dir, false).unwrap());
    let (parts, js) = get(&router, "/assets/app/actions/public/entry.js", &[]);
    assert_eq!(parts.headers[header::CONTENT_TYPE], "text/javascript; charset=utf-8");
    assert!(js.contains("from '/assets/app/actions/public/content.js'"), "{js}");
    assert!(js.contains("from '/assets/app/actions/public/pkg/assets_demo.js'"), "{js}");
    let (_, glue) = get(&router, "/assets/app/actions/public/pkg/assets_demo.js", &[]);
    assert!(glue.contains("new URL('/assets/app/actions/public/pkg/assets_demo_bg.wasm', import.meta.url)"), "{glue}");
    let (parts, _) = get(&router, "/assets/app/actions/public/pkg/assets_demo_bg.wasm", &[]);
    assert_eq!(parts.headers[header::CONTENT_TYPE], "application/wasm");

    let (_, css) = get(&router, "/assets/app/actions/public/styles/app.css", &[]);
    assert!(css.contains("url('/assets/app/actions/public/images/image.svg')"), "{css}");
    assert!(css.contains("url('/assets/app/actions/public/images/image.svg?transform=recolor%3Aff0000')"), "{css}");

    let (parts, svg) = get(&router, "/assets/app/actions/public/images/image.svg", &[]);
    assert_eq!(parts.headers[header::CONTENT_TYPE], "image/svg+xml");
    assert!(svg.starts_with("<svg xmlns=") && !svg.contains('\n'), "minified: {svg}");
    assert!(svg.contains("fill=\"currentColor\""));
    let (_, purple) = get(&router, "/assets/app/actions/public/images/image.svg?transform=recolor%3A%238B5CF6", &[]);
    assert!(purple.contains("fill=\"#8B5CF6\"") && !purple.contains("currentColor"), "{purple}");
    // The recolored SVG is kept in the disk cache under the root.
    let files = dir.join(rsmix::assets::DEFAULT_CACHE_DIRECTORY).join("files");
    let cached = std::fs::read_dir(&files).unwrap().flat_map(|d| std::fs::read_dir(d.unwrap().path()).unwrap()).any(|f| {
        let path = f.unwrap().path();
        path.extension().is_some_and(|e| e == "dat") && String::from_utf8_lossy(&std::fs::read(&path).unwrap()).ends_with(&purple)
    });
    assert!(cached, "a cache record holds the recolored SVG");
    // A transform that fails is a server error, reported through `on_error` (Remix's
    // `onError`); a malformed transform request is a 400.
    let (parts, _) = get(&router, "/assets/app/actions/public/images/image.svg?transform=recolor%3Ablue", &[]);
    assert_eq!(parts.status, 500);
    let (parts, body) = get(&router, "/assets/app/actions/public/images/image.svg?transform=recolor", &[]);
    assert_eq!(parts.status, 400);
    assert!(body.contains("File transform \"recolor\" requires a param"), "{body}");
}

#[test]
fn only_public_files_are_served() {
    let router = router(asset_server(&fixture(), false).unwrap());
    for url in [
        "/assets/app/actions/public/entry.test.js",
        "/assets/app/actions/public/missing.js",
        "/assets/app/actions/public/%2E%2E/%2E%2E/Cargo.toml",
        "/assets/src/server.rs",
    ] {
        assert_eq!(get(&router, url, &[]).0.status, 404, "{url}");
    }
}

#[test]
fn edits_show_up_on_the_next_request() {
    let dir = fixture();
    let router = router(asset_server(&dir, false).unwrap());
    let url = "/assets/app/actions/public/content.js";
    let (parts, before) = get(&router, url, &[]);
    assert_eq!(parts.headers[header::CACHE_CONTROL], "no-cache");
    let etag = parts.headers[header::ETAG].to_str().unwrap().to_string();
    assert_eq!(get(&router, url, &[("if-none-match", &etag)]).0.status, 304);

    std::fs::write(dir.join("app/actions/public/content.js"), "export function getContent() { return 'edited' }\n").unwrap();
    let (parts, after) = get(&router, url, &[("if-none-match", &etag)]);
    assert_eq!(parts.status, 200);
    assert_ne!(before, after);
    assert!(after.contains("'edited'"));
}

#[test]
fn production_urls_are_fingerprinted_and_immutable() {
    let dir = fixture();
    let router = router(asset_server(&dir, true).unwrap());
    let (_, html) = get(&router, "/", &[]);
    let css_href = attrs(&html, "<link rel=\"stylesheet\"", "href").remove(0);
    assert!(css_href.starts_with("/assets/app/actions/public/styles/app.@") && css_href.ends_with(".css"), "{css_href}");
    let (parts, css) = get(&router, &css_href, &[]);
    assert_eq!(parts.headers[header::CACHE_CONTROL], "public, max-age=31536000, immutable");
    assert!(css.contains("/assets/app/actions/public/images/image.@"), "CSS links fingerprinted images: {css}");

    // An edit to the image changes the stylesheet's URL too, because the CSS embeds it.
    std::fs::write(dir.join("app/actions/public/images/image.svg"), "<svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();
    let (_, html) = get(&router, "/", &[]);
    assert_ne!(attrs(&html, "<link rel=\"stylesheet\"", "href")[0], css_href);
    assert_eq!(get(&router, &css_href, &[]).0.status, 404, "stale fingerprints are not served");
}

#[test]
fn minify_svg_keeps_text_and_attributes() {
    let svg = "<svg\n  a=\"1\"\n  b=\"2\"\n>\n  <!-- note -->\n  <title id=\"t\">Demo  image</title>\n  <rect x=\"1\" />\n</svg>\n";
    assert_eq!(minify_svg(svg), "<svg a=\"1\" b=\"2\"><title id=\"t\">Demo image</title><rect x=\"1\"/></svg>");
}
