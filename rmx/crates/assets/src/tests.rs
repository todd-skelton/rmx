use std::path::PathBuf;

use rmx_router::Router;

use super::*;

/// A scratch app directory with the assets demo's layout.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("rmx-assets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let files = [
            ("app/public/entry.js", "import init from './pkg/app.js';\nawait init();\n"),
            ("app/public/pkg/app.js", "export default async function init() { return new URL('app_bg.wasm', import.meta.url) }\n"),
            ("app/public/pkg/app_bg.wasm", "\0asm"),
            ("app/public/styles/app.css", ".a { background: url('../images/image.svg') } .b { background: url('../images/image.svg?transform=recolor:ff0000') }"),
            ("app/public/images/image.svg", "<svg>\n  <rect fill=\"currentColor\"/>\n</svg>\n"),
            ("app/public/secret.test.js", "nope"),
            ("app/server.rs", "fn main() {}"),
        ];
        for (path, content) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        Fixture { root }
    }

    fn server(&self, fingerprint: bool) -> AssetServer {
        AssetServer::new(AssetOptions {
            root_dir: self.root.clone(),
            base_path: "/assets".into(),
            mounts: vec![("app".into(), "app".into())],
            allow_files: vec!["app/**/public/**".into()],
            deny_files: vec!["app/**/*.test.*".into()],
            fingerprint,
            global_transforms: vec![GlobalTransform::new(&[".svg"], |bytes| {
                let svg = String::from_utf8(bytes).map_err(|e| e.to_string())?;
                Ok(svg.lines().map(str::trim).collect::<String>().into())
            })],
            transforms: vec![(
                "recolor".into(),
                FileTransform::new(&[".svg"], Param::Required, |bytes, color| {
                    let color = color.unwrap_or_default();
                    if !(3..=8).contains(&color.len()) || !color.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return Err("Expected a hex color".into());
                    }
                    Ok(String::from_utf8_lossy(&bytes).replace("currentColor", &format!("#{color}")).into())
                }),
            )],
            ..Default::default()
        })
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn router(assets: AssetServer) -> Router {
    let mut router = Router::new();
    router.get("/assets/*path", move |ctx: Context| {
        let assets = assets.clone();
        async move { assets.fetch(&ctx).unwrap_or_else(rmx_fetch::response::not_found) }
    });
    router
}

fn get(router: &Router, url: &str, headers: &[(&str, &str)]) -> (StatusCode, http::HeaderMap, String) {
    let mut req = http::Request::builder().uri(format!("http://localhost{url}"));
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let res = rmx_fetch::block_on(router.fetch(req.body(Body::empty()).unwrap()));
    let (parts, body) = res.into_parts();
    (parts.status, parts.headers, rmx_fetch::block_on(body.text()).unwrap())
}

#[test]
fn css_references_become_asset_urls() {
    let fx = Fixture::new("css");
    let assets = fx.server(false);
    let href = assets.href("app/public/styles/app.css").unwrap();
    assert_eq!(href, "/assets/app/public/styles/app.css");
    let router = router(assets);
    let (status, headers, css) = get(&router, &href, &[]);
    assert_eq!(status, 200);
    assert_eq!(headers[header::CONTENT_TYPE], "text/css; charset=utf-8");
    assert_eq!(headers[header::CACHE_CONTROL], "no-cache");
    assert!(css.contains("url('/assets/app/public/images/image.svg')"), "{css}");
    assert!(css.contains("url('/assets/app/public/images/image.svg?transform=recolor%3Aff0000')"), "{css}");

    let etag = headers[header::ETAG].to_str().unwrap().to_string();
    let (status, _, _) = get(&router, &href, &[("if-none-match", &etag)]);
    assert_eq!(status, 304);
}

#[test]
fn fingerprints_are_immutable_and_follow_content() {
    let fx = Fixture::new("fingerprint");
    let assets = fx.server(true);
    let css_href = assets.href("app/public/styles/app.css").unwrap();
    assert!(css_href.starts_with("/assets/app/public/styles/app.@") && css_href.ends_with(".css"), "{css_href}");
    let router = router(assets.clone());
    let (status, headers, css) = get(&router, &css_href, &[]);
    assert_eq!(status, 200);
    assert_eq!(headers[header::CACHE_CONTROL], "public, max-age=31536000, immutable");
    let svg_href = assets.href("app/public/images/image.svg").unwrap();
    assert!(css.contains(&svg_href), "linked assets are fingerprinted too: {css}");

    // Editing a linked file changes the URLs of the files that link to it.
    std::fs::write(fx.root.join("app/public/images/image.svg"), "<svg><circle fill=\"currentColor\"/></svg>").unwrap();
    let new_svg = assets.href("app/public/images/image.svg").unwrap();
    let new_css = assets.href("app/public/styles/app.css").unwrap();
    assert_ne!(new_svg, svg_href);
    assert_ne!(new_css, css_href);
    let (status, _, _) = get(&router, &css_href, &[]);
    assert_eq!(status, 404, "a stale fingerprint is not served as immutable");
    let (status, _, _) = get(&router, "/assets/app/public/styles/app.css", &[]);
    assert_eq!(status, 200, "the plain URL still works");
}

#[test]
fn transforms_run_on_request() {
    let fx = Fixture::new("transforms");
    let assets = fx.server(false);
    let router = router(assets.clone());
    let (_, headers, svg) = get(&router, "/assets/app/public/images/image.svg", &[]);
    assert_eq!(headers[header::CONTENT_TYPE], "image/svg+xml");
    assert_eq!(svg, "<svg><rect fill=\"currentColor\"/></svg>", "the global transform minified it");
    let href = assets.href_with("app/public/images/image.svg", &["recolor:8B5CF6"]).unwrap();
    let (status, _, svg) = get(&router, &href, &[]);
    assert_eq!(status, 200);
    assert!(svg.contains("#8B5CF6"), "{svg}");
    for bad in ["recolor:nothex", "recolor", "unknown"] {
        let (status, _, _) = get(&router, &format!("/assets/app/public/images/image.svg?transform={bad}"), &[]);
        assert_eq!(status, 400, "{bad}");
    }
}

#[test]
fn access_rules_and_paths() {
    let fx = Fixture::new("access");
    let assets = fx.server(false);
    let router = router(assets.clone());
    for url in [
        "/assets/app/server.rs",
        "/assets/app/public/secret.test.js",
        "/assets/app/public/../server.rs",
        "/assets/app/public/%2e%2e/server.rs",
        "/assets/app/public/missing.css",
        "/assets/other/x.css",
    ] {
        let (status, _, _) = get(&router, url, &[]);
        assert_eq!(status, 404, "{url}");
    }
    assert!(matches!(assets.href("app/server.rs"), Err(AssetError::NotServable(_))));
    assert!(AssetServer::new(AssetOptions { root_dir: fx.root.clone(), ..Default::default() }).is_err(), "allow_files is required");
}

#[test]
fn script_entries_link_modules_and_wasm() {
    let fx = Fixture::new("scripts");
    let assets = fx.server(true);
    let entry = assets.script_entry("app/public/entry.js").unwrap();
    assert_eq!(entry.preloads.len(), 2, "{entry:?}");
    assert_eq!(entry.preloads[0], entry.href);
    let router = router(assets.clone());
    let (_, headers, js) = get(&router, &entry.href, &[]);
    assert_eq!(headers[header::CONTENT_TYPE], "text/javascript; charset=utf-8");
    assert!(js.contains(&format!("from '{}'", entry.preloads[1])), "{js}");
    let (_, _, pkg) = get(&router, &entry.preloads[1], &[]);
    let wasm = assets.href("app/public/pkg/app_bg.wasm").unwrap();
    assert!(wasm.contains(".@"));
    assert!(pkg.contains(&format!("new URL('{wasm}', import.meta.url)")), "{pkg}");
}
