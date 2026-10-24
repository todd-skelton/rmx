//! rsmix-only static file tests: streaming, escaping, listing helpers and per-file options.
//! Remix's `static.test.ts` is ported one per case in `tests/static_files.rs`.

use std::path::PathBuf;

use rsmix_fetch::{block_on, Body, Response};
use rsmix_fetch_router::{Context, Router};

use super::*;

/// A fresh directory per test, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("rsmix-static-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn file(&self, rel: &str, content: &str) -> &Self {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
        self
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn router(root: &std::path::Path, options: StaticOptions) -> Router {
    let mut router = Router::new().with(static_files(root, options));
    router.get("/", |_: Context| async { "home" });
    router.get("/*path", |_: Context| async { rsmix_response::status(http::StatusCode::NOT_FOUND, "Fallback Handler") });
    router
}

fn get(router: &Router, path: &str, headers: &[(&str, &str)]) -> Response {
    request(router, "GET", path, headers)
}

fn request(router: &Router, method: &str, path: &str, headers: &[(&str, &str)]) -> Response {
    let mut b = http::Request::builder().method(method).uri(format!("http://x{path}"));
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    block_on(router.fetch(b.body(Body::empty()).unwrap()))
}

fn text(res: Response) -> String {
    block_on(res.into_body().text()).unwrap()
}

fn setup(name: &str) -> (TempDir, Router) {
    let dir = TempDir::new(name);
    dir.file("app.css", "body{}").file("logo.png", "0123456789").file("sub/index.html", "<p>i</p>");
    let router = router(&dir.0, StaticOptions::default());
    (dir, router)
}

#[test]
fn serves_files_conditionally_and_falls_through() {
    let (_dir, router) = setup("basic");
    let res = get(&router, "/app.css", &[]);
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "text/css; charset=utf-8");
    assert_eq!(res.headers()["content-length"], "6");
    let etag = res.headers()["etag"].to_str().unwrap().to_string();
    assert_eq!(text(res), "body{}");
    assert_eq!(get(&router, "/app.css", &[("if-none-match", &etag)]).status(), 304);
    let css = get(&router, "/app.css", &[("range", "bytes=1-3")]);
    assert_eq!((css.status().as_u16(), css.headers().get("accept-ranges")), (200, None), "compressible types are not ranged by default");
    assert_eq!(text(get(&router, "/sub/", &[])), "<p>i</p>");
    assert_eq!(text(get(&router, "/sub", &[])), "<p>i</p>", "no trailing slash");
    assert_eq!(text(get(&router, "/", &[])), "home");
    assert_eq!(get(&router, "/../Cargo.toml", &[]).status(), 404);
    assert_eq!(get(&router, "/%2e%2e/secret", &[]).status(), 404);
    let head = request(&router, "HEAD", "/app.css", &[]);
    assert_eq!((head.status().as_u16(), head.headers()["content-length"].to_str().unwrap()), (200, "6"));
    assert_eq!(text(head), "");
}

#[test]
fn conditional_requests_and_ranges() {
    let (_dir, router) = setup("conditional");
    let res = get(&router, "/logo.png", &[]);
    assert_eq!(res.headers()["accept-ranges"], "bytes");
    assert_eq!(res.headers()["x-content-type-options"], "nosniff");
    let etag = res.headers()["etag"].to_str().unwrap().to_string();
    let modified = res.headers()["last-modified"].to_str().unwrap().to_string();

    let part = get(&router, "/logo.png", &[("range", "bytes=2-4")]);
    assert_eq!(part.status(), 206);
    assert_eq!(part.headers()["content-range"], "bytes 2-4/10");
    assert_eq!(part.headers()["content-length"], "3");
    assert_eq!(text(part), "234");
    assert_eq!(text(get(&router, "/logo.png", &[("range", "bytes=-3")])), "789");
    assert_eq!(text(get(&router, "/logo.png", &[("range", "bytes=7-")])), "789");
    assert_eq!(text(get(&router, "/logo.png", &[("range", "bytes=8-100")])), "89", "end clamps to the size");
    assert_eq!(get(&router, "/logo.png", &[("range", "bytes=0-1,4-5")]).status(), 416, "no multipart ranges");
    let past_end = get(&router, "/logo.png", &[("range", "bytes=20-")]);
    assert_eq!((past_end.status().as_u16(), past_end.headers()["content-range"].to_str().unwrap()), (416, "bytes */10"));
    assert_eq!(get(&router, "/logo.png", &[("range", "lines=x")]).status(), 200, "unknown range units are ignored");
    assert_eq!(get(&router, "/logo.png", &[("range", "bytes=x")]).status(), 400);
    assert_eq!(get(&router, "/logo.png", &[("range", "bytes=5-2")]).status(), 400);
    assert_eq!(get(&router, "/logo.png", &[("range", "bytes=2-4"), ("if-range", "\"other\"")]).status(), 200, "stale If-Range sends everything");
    assert_eq!(get(&router, "/logo.png", &[("range", "bytes=2-4"), ("if-range", &modified)]).status(), 206);

    assert_eq!(get(&router, "/logo.png", &[("if-match", "\"nope\"")]).status(), 412);
    assert_eq!(get(&router, "/logo.png", &[("if-unmodified-since", "Thu, 01 Jan 1970 00:00:00 GMT")]).status(), 412);
    assert_eq!(get(&router, "/logo.png", &[("if-modified-since", &modified)]).status(), 304);
    assert_eq!(get(&router, "/logo.png", &[("if-none-match", &etag), ("if-modified-since", "Thu, 01 Jan 1970 00:00:00 GMT")]).status(), 304, "If-None-Match wins");
    assert_eq!(get(&router, "/logo.png", &[("if-none-match", "\"x\""), ("if-modified-since", &modified)]).status(), 200);
}

#[test]
fn streams_large_files_in_chunks() {
    let dir = TempDir::new("large");
    let data: String = (0..200_000).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
    dir.file("big.rmxunknown", &data);
    let r = router(&dir.0, StaticOptions::default());
    let mut body = get(&r, "/big.rmxunknown", &[]).into_body();
    let first = block_on(body.chunk()).unwrap().unwrap();
    assert!(first.len() < data.len(), "streamed, not buffered");
    let part = get(&r, "/big.rmxunknown", &[("range", "bytes=100000-100009")]);
    assert_eq!(part.headers().get("content-type"), None, "unknown types get no Content-Type");
    assert_eq!(text(part), &data[100_000..100_010]);
}

#[test]
fn index_files_and_options() {
    let dir = TempDir::new("index");
    dir.file("htm/index.htm", "<h1>HTM</h1>").file("both/index.html", "<h1>HTML</h1>").file("both/index.htm", "<h1>HTM</h1>");
    dir.file("custom/home.html", "<h1>Home</h1>").file("custom/default.html", "<h1>Default</h1>").file("none/a.txt", "a");
    let r = router(&dir.0, StaticOptions::default());
    assert_eq!(text(get(&r, "/htm/", &[])), "<h1>HTM</h1>");
    let both = get(&r, "/both/", &[]);
    assert_eq!(both.headers()["content-type"], "text/html; charset=utf-8");
    assert_eq!(text(both), "<h1>HTML</h1>");
    assert_eq!(text(get(&r, "/none/", &[])), "Fallback Handler");

    let custom = router(&dir.0, StaticOptions { index: vec!["missing.html".into(), "default.html".into(), "home.html".into()], ..Default::default() });
    assert_eq!(text(get(&custom, "/custom/", &[])), "<h1>Default</h1>", "tried in order");
    let off = router(&dir.0, StaticOptions { index: vec![], ..Default::default() });
    assert_eq!(text(get(&off, "/both/", &[])), "Fallback Handler");
}

#[test]
fn lists_directories() {
    let dir = TempDir::new("list");
    dir.file("downloads/folder10/nested/file.txt", "Nested file").file("downloads/index.html", "<h1>Index</h1>").file("downloads/empty.txt", "");
    std::fs::create_dir_all(dir.0.join("downloads/folder2")).unwrap();
    let r = router(&dir.0, StaticOptions { list_files: true, index: vec![], ..Default::default() });
    let res = get(&r, "/downloads/", &[]);
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "text/html; charset=UTF-8");
    let body = text(res);
    let rows: Vec<&str> = body.split(r#"<tr class="file-row">"#).skip(1).collect();
    assert_eq!(rows.len(), 5, "{body}");
    assert!(rows[0].contains(r#"href="/""#) && rows[0].contains(".."));
    assert!(rows[1].contains(r#"href="/downloads/folder2""#) && rows[1].contains(r#"<td class="size-cell"></td>"#) && rows[1].contains(">Folder<"));
    assert!(rows[2].contains(r#"href="/downloads/folder10""#), "natural order");
    assert!(rows[3].contains(r#"href="/downloads/empty.txt""#) && rows[3].contains(">0 B<") && rows[3].contains(">text/plain<"));
    assert!(rows[4].contains(r#"href="/downloads/index.html""#) && rows[4].contains(">14 B<") && rows[4].contains(">text/html<"));

    dir.file("root/index.html", "<h1>Index</h1>");
    let index_first = router(&dir.0.join("root"), StaticOptions { list_files: true, ..Default::default() });
    assert_eq!(text(get(&index_first, "/", &[])), "<h1>Index</h1>");

    dir.file("escape/a&b'.txt", "x");
    let escaped = text(get(&router(&dir.0, StaticOptions { list_files: true, ..Default::default() }), "/escape/", &[]));
    assert!(escaped.contains("a&amp;b&#39;.txt") && !escaped.contains("a&b"), "{escaped}");
}

#[test]
fn filters_and_ranges_per_file() {
    let dir = TempDir::new("filter");
    dir.file("test.txt", "Hello, World!").file("video.mp4", "fake video data").file(".secret", "s");
    let r = router(&dir.0, StaticOptions { filter: Some(Arc::new(|p: &str| !p.starts_with('.'))), ..Default::default() });
    assert_eq!(text(get(&r, "/.secret", &[])), "Fallback Handler");

    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    let ranges = AcceptRanges::With(Arc::new(move |file: &LazyFile| {
        log.lock().unwrap().push((file.name().to_string(), file.content_type().to_string(), file.size()));
        file.content_type().starts_with("video/")
    }));
    let r = router(&dir.0, StaticOptions { accept_ranges: ranges, ..Default::default() });
    let txt = get(&r, "/test.txt", &[("range", "bytes=0-4")]);
    assert_eq!((txt.status().as_u16(), txt.headers().get("accept-ranges")), (200, None));
    assert_eq!(text(txt), "Hello, World!");
    assert_eq!(text(get(&r, "/video.mp4", &[("range", "bytes=0-3")])), "fake");
    assert_eq!(seen.lock().unwrap()[0], ("test.txt".to_string(), "text/plain".to_string(), 13));

    let always = router(&dir.0, StaticOptions { accept_ranges: AcceptRanges::Always, ..Default::default() });
    assert_eq!(text(get(&always, "/test.txt", &[("range", "bytes=0-4")])), "Hello");
}

#[test]
fn strong_etags() {
    let dir = TempDir::new("strong");
    dir.file("test.txt", "Hello, World!");
    let r = router(&dir.0, StaticOptions { etag: ETag::Strong, ..Default::default() });
    let res = get(&r, "/test.txt", &[]);
    let etag = res.headers()["etag"].to_str().unwrap().to_string();
    assert_eq!(etag, "\"dffd6021bb2bd5b0af676290809ec3a53191dd81c7f70a4b28688a362182986f\"", "SHA-256 of the content");
    assert_eq!(get(&r, "/test.txt", &[("if-match", &etag)]).status(), 200, "strong tags satisfy If-Match");
    assert_eq!(get(&r, "/test.txt", &[("if-none-match", &etag)]).status(), 304);

    let custom = router(&dir.0, StaticOptions { etag: ETag::Strong, digest: Digest::with(|_: &LazyFile| async { "custom-hash-12345".to_string() }), ..Default::default() });
    assert_eq!(get(&custom, "/test.txt", &[]).headers()["etag"], "\"custom-hash-12345\"");
    let weak = get(&router(&dir.0, StaticOptions::default()), "/test.txt", &[]).headers()["etag"].to_str().unwrap().to_string();
    let weak_router = router(&dir.0, StaticOptions::default());
    assert_eq!(get(&weak_router, "/test.txt", &[("if-match", &weak)]).status(), 412, "weak tags never satisfy If-Match");
    let off = get(&router(&dir.0, StaticOptions { etag: ETag::Off, last_modified: false, ..Default::default() }), "/test.txt", &[]);
    assert_eq!((off.headers().get("etag"), off.headers().get("last-modified")), (None, None));
}

#[cfg(unix)]
fn symlink_file(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(windows)]
fn symlink_file(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(src, dst)
}

#[test]
fn symlinks_stay_inside_the_root() {
    let dir = TempDir::new("symlink");
    dir.file("public/asset.txt", "console.log(\"safe\")").file("secret.txt", "Secret content");
    let public = dir.0.join("public");
    // Creating symlinks can need extra privileges on Windows; skip when it is refused.
    if symlink_file(&public.join("asset.txt"), &public.join("asset.js")).is_err() {
        eprintln!("symlinks unavailable; skipping");
        return;
    }
    symlink_file(&dir.0.join("secret.txt"), &public.join("leak.txt")).unwrap();
    let r = router(&public, StaticOptions::default());
    let res = get(&r, "/asset.js", &[]);
    assert_eq!(res.headers()["content-type"], "text/javascript; charset=utf-8", "type from the requested path");
    assert_eq!(text(res), "console.log(\"safe\")");
    assert_eq!(text(get(&r, "/leak.txt", &[])), "Fallback Handler");
}

/// A directory junction (Windows) or directory symlink (Unix) at `link` pointing to `target`.
/// Junctions need no extra privileges.
#[cfg(windows)]
fn link_dir(target: &std::path::Path, link: &std::path::Path) -> bool {
    std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .is_ok_and(|out| out.status.success())
}

#[cfg(unix)]
fn link_dir(target: &std::path::Path, link: &std::path::Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[test]
fn linked_directories_stay_inside_the_root() {
    let dir = TempDir::new("linked-dir");
    dir.file("public/inside/a.txt", "inside").file("public/inside/index.html", "<p>linked index</p>").file("outside/secret.txt", "Secret content");
    let public = dir.0.join("public");
    if !link_dir(&public.join("inside"), &public.join("in")) {
        eprintln!("directory links unavailable; skipping");
        return;
    }
    assert!(link_dir(&dir.0.join("outside"), &public.join("out")));
    std::fs::create_dir_all(public.join("deep")).unwrap();
    assert!(link_dir(&dir.0.join("outside"), &public.join("deep").join("out")));
    let r = router(&public, StaticOptions::default());
    assert_eq!(text(get(&r, "/in/a.txt", &[])), "inside", "a link inside the root is followed");
    assert_eq!(text(get(&r, "/in/", &[])), "<p>linked index</p>", "and its index file");
    assert_eq!(text(get(&r, "/out/secret.txt", &[])), "Fallback Handler", "a link out of the root is not");
    assert_eq!(text(get(&r, "/deep/out/secret.txt", &[])), "Fallback Handler", "nor one further down");
    assert_eq!(text(get(&r, "/out/", &[])), "Fallback Handler");
    assert_eq!(text(get(&r, "/inside/a.txt", &[])), "inside");
}

#[test]
fn missing_paths_and_the_root_fall_through() {
    let (_dir, r) = setup("missing");
    assert_eq!(text(get(&r, "/nope/deeper/file.txt", &[])), "Fallback Handler");
    assert_eq!(text(get(&r, "/app.css/inside", &[])), "Fallback Handler", "a file is not a directory");
    assert_eq!(text(get(&r, "/", &[])), "home", "the root without an index file");
    assert_eq!(text(get(&r, "/./app.css", &[])), "body{}");
}

#[test]
fn listing_helpers() {
    assert_eq!(format_size(0), "0 B");
    assert_eq!(format_size(14), "14 B");
    assert_eq!(format_size(1536), "1.5 kB");
    assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    let mut names = vec!["folder10", "Folder2", "folder1", "a"];
    names.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(names, ["a", "folder1", "Folder2", "folder10"]);
}
