//! Port of Remix's `static-middleware/src/lib/static.test.ts`, one test per case (the
//! `ignores ${method} requests` loop is one test per method). Remix's `{ middleware, handler }`
//! route object is `action(handler).with(middleware)`; `index: true` is the default index list
//! and `index: false` an empty one.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use http::{Method, StatusCode};
use rmx_fetch::{block_on, response, Body, Response};
use rmx_lazy_file::LazyFile;
use rmx_form_data_middleware::form_data;
use rmx_method_override_middleware::method_override;
use rmx_static_middleware::{static_files, AcceptRanges, ETag, StaticOptions};
use rmx_router::{action, Action, Context, Router};

/// `new Date('2025-01-01')`.
const JAN_2025: u64 = 1_735_689_600_000;

/// A fresh directory per test, removed on drop (`beforeEach`/`afterEach`).
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("static-files-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir.canonicalize().unwrap())
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    /// `createTestFile(filename, content)`.
    fn create(&self, rel: &str, content: &str) -> PathBuf {
        let path = self.path(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    /// `createTestFile(filename, content, date)`.
    fn create_at(&self, rel: &str, content: &str, ms: u64) -> PathBuf {
        let path = self.create(rel, content);
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_millis(ms)).unwrap();
        path
    }

    fn mkdir(&self, rel: &str) -> PathBuf {
        let path = self.path(rel);
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path, _dir: bool) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink(target: &Path, link: &Path, dir: bool) -> std::io::Result<()> {
    if dir {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

/// Windows refuses symlinks without Developer Mode or elevation; those tests then skip.
fn try_symlink(target: &Path, link: &Path, dir: bool) -> bool {
    match symlink(target, link, dir) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("symlinks unavailable ({error}); skipping");
            false
        }
    }
}

/// The route action: `staticFiles(root, options)` in front of a 404 fallback handler.
fn with_static(root: &Path, options: StaticOptions, fallback: &'static str) -> Action {
    action(move |_: Context| async move { response::status(StatusCode::NOT_FOUND, fallback) }).with(static_files(root, options))
}

/// `router.get('/*', { middleware: [staticFiles(root, options)], handler: 404 'Fallback Handler' })`.
fn router(root: &Path, options: StaticOptions) -> Router {
    let mut router = Router::new();
    router.get("/*", with_static(root, options, "Fallback Handler"));
    router
}

fn fetch(router: &Router, method: Method, path: &str, headers: &[(&str, &str)]) -> Response {
    let mut builder = http::Request::builder().method(method).uri(format!("https://remix.run{path}"));
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    block_on(router.fetch(builder.body(Body::empty()).unwrap()))
}

fn get(router: &Router, path: &str) -> Response {
    fetch(router, Method::GET, path, &[])
}

fn header(response: &Response, name: &str) -> Option<String> {
    response.headers().get(name).map(|v| v.to_str().unwrap().to_string())
}

fn text(response: Response) -> String {
    block_on(response.into_body().text()).unwrap()
}

fn not_found_with(response: Response, body: &str) {
    assert_eq!(response.status(), 404);
    assert_eq!(text(response), body);
}

// basic functionality

#[test]
fn serves_a_file() {
    let dir = TempDir::new();
    dir.create("test.txt", "Hello, World!");

    let response = get(&router(&dir.0, StaticOptions::default()), "/test.txt");

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Type").as_deref(), Some("text/plain; charset=utf-8"));
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn serves_a_file_with_head_request() {
    let dir = TempDir::new();
    dir.create("test.txt", "Hello, World!");
    let mut router = Router::new();
    router.head("/*", with_static(&dir.0, StaticOptions::default(), "Fallback Handler"));

    let response = fetch(&router, Method::HEAD, "/test.txt", &[]);

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Type").as_deref(), Some("text/plain; charset=utf-8"));
    assert_eq!(text(response), "");
}

#[test]
fn serves_files_from_nested_directories() {
    let dir = TempDir::new();
    dir.create("dir/subdir/file.txt", "Nested file");

    let response = get(&router(&dir.0, StaticOptions::default()), "/dir/subdir/file.txt");

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "Nested file");
}

#[test]
fn falls_through_to_handler_for_non_existent_file() {
    let dir = TempDir::new();
    let mut router = Router::new();
    router.get("/*", with_static(&dir.0, StaticOptions::default(), "Custom Fallback Handler"));

    not_found_with(get(&router, "/nonexistent.txt"), "Custom Fallback Handler");
}

#[test]
fn falls_through_to_handler_when_requesting_a_directory() {
    let dir = TempDir::new();
    dir.mkdir("subdir");

    not_found_with(get(&router(&dir.0, StaticOptions::default()), "/subdir"), "Fallback Handler");
}

#[test]
fn supports_etag_by_default() {
    let dir = TempDir::new();
    dir.create_at("test.txt", "Hello, World!", JAN_2025);
    let router = router(&dir.0, StaticOptions::default());

    let response = get(&router, "/test.txt");
    assert_eq!(response.status(), 200);
    let etag = header(&response, "ETag").expect("an ETag");
    assert_eq!(etag, "W/\"13-1735689600000\"");
    assert_eq!(text(response), "Hello, World!");

    let response2 = fetch(&router, Method::GET, "/test.txt", &[("If-None-Match", &etag)]);
    assert_eq!(response2.status(), 304);
    assert_eq!(text(response2), "");
}

#[test]
fn does_not_send_etag_if_etag_is_disabled() {
    let dir = TempDir::new();
    dir.create_at("test.txt", "Hello, World!", JAN_2025);

    let response = get(&router(&dir.0, StaticOptions { etag: ETag::Off, ..StaticOptions::default() }), "/test.txt");

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "ETag"), None);
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn supports_last_modified_by_default() {
    let dir = TempDir::new();
    dir.create_at("test.txt", "Hello, World!", JAN_2025);

    let response = get(&router(&dir.0, StaticOptions::default()), "/test.txt");

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Last-Modified").as_deref(), Some("Wed, 01 Jan 2025 00:00:00 GMT"));
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn does_not_send_last_modified_if_last_modified_is_disabled() {
    let dir = TempDir::new();
    dir.create_at("test.txt", "Hello, World!", JAN_2025);

    let response = get(&router(&dir.0, StaticOptions { last_modified: false, ..StaticOptions::default() }), "/test.txt");

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Last-Modified"), None);
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn does_not_support_accept_ranges_by_default_for_compressible_files() {
    let dir = TempDir::new();
    dir.create_at("test.txt", "Hello, World!", JAN_2025);
    let router = router(&dir.0, StaticOptions::default());

    let response = get(&router, "/test.txt");
    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Accept-Ranges"), None);
    assert_eq!(text(response), "Hello, World!");

    let response2 = fetch(&router, Method::GET, "/test.txt", &[("Range", "bytes=0-4")]);
    assert_eq!(response2.status(), 200);
    assert_eq!(text(response2), "Hello, World!");
}

#[test]
fn supports_range_requests_when_accept_ranges_is_explicitly_enabled() {
    let dir = TempDir::new();
    dir.create_at("test.txt", "Hello, World!", JAN_2025);
    let router = router(&dir.0, StaticOptions { accept_ranges: AcceptRanges::Always, ..StaticOptions::default() });

    let response = get(&router, "/test.txt");
    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Accept-Ranges").as_deref(), Some("bytes"));
    assert_eq!(text(response), "Hello, World!");

    let response2 = fetch(&router, Method::GET, "/test.txt", &[("Range", "bytes=0-4")]);
    assert_eq!(response2.status(), 206);
    assert_eq!(header(&response2, "Content-Range").as_deref(), Some("bytes 0-4/13"));
    assert_eq!(header(&response2, "Content-Length").as_deref(), Some("5"));
    assert_eq!(header(&response2, "Accept-Ranges").as_deref(), Some("bytes"));
    assert_eq!(text(response2), "Hello");
}

#[test]
fn supports_range_requests_with_accept_ranges_function() {
    let dir = TempDir::new();
    dir.create("test.txt", "Hello, World!");
    dir.create("video.dat", "fake video data");
    let ranges = AcceptRanges::With(Arc::new(|file: &LazyFile| file.content_type().starts_with("video/")));
    let router = router(&dir.0, StaticOptions { accept_ranges: ranges, ..StaticOptions::default() });

    // test.txt should not have ranges (text/plain doesn't match the video/* filter)
    let response = get(&router, "/test.txt");
    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Accept-Ranges"), None);
    assert_eq!(text(response), "Hello, World!");

    let response2 = fetch(&router, Method::GET, "/test.txt", &[("Range", "bytes=0-4")]);
    assert_eq!(response2.status(), 200);
    assert_eq!(text(response2), "Hello, World!");
}

#[test]
fn supports_cache_control() {
    let dir = TempDir::new();
    dir.create("test.txt", "Hello, World!");
    let options = StaticOptions { cache_control: Some("public, max-age=3600".into()), ..StaticOptions::default() };

    let response = get(&router(&dir.0, options), "/test.txt");

    assert_eq!(header(&response, "Cache-Control").as_deref(), Some("public, max-age=3600"));
}

#[test]
fn works_with_multiple_static_middleware_instances() {
    let dir = TempDir::new();
    dir.create("assets/style.css", "body {}");
    dir.create("images/logo.png", "PNG data");
    let router = router(&dir.0, StaticOptions::default());

    let response1 = get(&router, "/assets/style.css");
    assert_eq!(response1.status(), 200);
    assert_eq!(text(response1), "body {}");

    let response2 = get(&router, "/images/logo.png");
    assert_eq!(response2.status(), 200);
    assert_eq!(text(response2), "PNG data");
}

#[test]
fn works_as_fallback_middleware() {
    let dir = TempDir::new();
    dir.create("index.html", "<h1>Fallback Handler</h1>");
    let mut router = Router::new();
    router.get("/api/users", |_: Context| async { "Users API" });
    router.get("*path", with_static(&dir.0, StaticOptions::default(), "Fallback Handler"));

    assert_eq!(text(get(&router, "/api/users")), "Users API");
    assert_eq!(text(get(&router, "/index.html")), "<h1>Fallback Handler</h1>");
}

/// `it(`ignores ${method} requests`)`.
fn ignores_requests_with(method: Method) {
    let dir = TempDir::new();
    dir.create("test.txt", "Hello, World!");
    let mut router = Router::new();
    let action = with_static(&dir.0, StaticOptions::default(), "Fallback Handler");
    match method.as_str() {
        "POST" => router.post("/*path", action),
        "PUT" => router.put("/*path", action),
        "DELETE" => router.delete("/*path", action),
        "PATCH" => router.patch("/*path", action),
        "OPTIONS" => router.options("/*path", action),
        other => panic!("unexpected method {other}"),
    };

    not_found_with(fetch(&router, method, "/test.txt", &[]), "Fallback Handler");
}

#[test]
fn ignores_post_requests() {
    ignores_requests_with(Method::POST);
}

#[test]
fn ignores_put_requests() {
    ignores_requests_with(Method::PUT);
}

#[test]
fn ignores_delete_requests() {
    ignores_requests_with(Method::DELETE);
}

#[test]
fn ignores_patch_requests() {
    ignores_requests_with(Method::PATCH);
}

#[test]
fn ignores_options_requests() {
    ignores_requests_with(Method::OPTIONS);
}

#[test]
fn prevents_path_traversal_with_dot_dot_in_pathname() {
    let dir = TempDir::new();
    dir.create("secret.txt", "Secret content");
    dir.create("public/allowed.txt", "Allowed content");
    let router = router(&dir.path("public"), StaticOptions::default());

    let allowed = get(&router, "/allowed.txt");
    assert_eq!(allowed.status(), 200);
    assert_eq!(text(allowed), "Allowed content");

    assert_eq!(get(&router, "/../secret.txt").status(), 404);
}

#[test]
fn does_not_support_absolute_paths_in_the_url() {
    let dir = TempDir::new();
    let parent = dir.0.parent().unwrap();
    let secret = parent.join(format!("{}-secret-outside-root.txt", dir.0.file_name().unwrap().to_string_lossy()));
    std::fs::write(&secret, "Secret content").unwrap();
    let mut router = Router::new();
    router.get("*path", with_static(&dir.0, StaticOptions::default(), "Fallback Handler"));

    let path = secret.to_string_lossy().replace('\\', "/");
    let response = get(&router, &format!("/{}", path.trim_start_matches('/')));
    let _ = std::fs::remove_file(&secret);

    assert_eq!(response.status(), 404);
}

#[test]
fn serves_symlinked_files_inside_the_root_using_the_requested_path_metadata() {
    let dir = TempDir::new();
    let public = dir.mkdir("public");
    std::fs::write(public.join("asset.txt"), "console.log(\"safe\")").unwrap();
    if !try_symlink(&public.join("asset.txt"), &public.join("asset.js"), false) {
        return;
    }
    let files = Arc::new(Mutex::new(Vec::new()));
    let seen = files.clone();
    let ranges = AcceptRanges::With(Arc::new(move |file: &LazyFile| {
        seen.lock().unwrap().push((file.name().to_string(), file.content_type().to_string()));
        false
    }));

    let response = get(&router(&public, StaticOptions { accept_ranges: ranges, ..StaticOptions::default() }), "/asset.js");

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Type").as_deref(), Some("text/javascript; charset=utf-8"));
    assert_eq!(text(response), "console.log(\"safe\")");
    assert_eq!(*files.lock().unwrap(), [("asset.js".to_string(), "text/javascript".to_string())]);
}

#[test]
fn does_not_serve_symlinked_files_outside_the_root() {
    let dir = TempDir::new();
    let public = dir.mkdir("public");
    let secret = dir.create("secret.txt", "Secret content");
    if !try_symlink(&secret, &public.join("leak.txt"), false) {
        return;
    }

    not_found_with(get(&router(&public, StaticOptions::default()), "/leak.txt"), "Fallback Handler");
}

#[test]
fn does_not_serve_files_from_symlinked_directories_outside_the_root() {
    let dir = TempDir::new();
    let public = dir.mkdir("public");
    dir.create("private/secret.txt", "Secret content");
    if !try_symlink(&dir.path("private"), &public.join("private"), true) {
        return;
    }

    not_found_with(get(&router(&public, StaticOptions::default()), "/private/secret.txt"), "Fallback Handler");
}

#[test]
fn does_not_serve_index_files_from_symlinked_directories_outside_the_root() {
    let dir = TempDir::new();
    let public = dir.mkdir("public");
    dir.create("private/index.html", "Secret index");
    if !try_symlink(&dir.path("private"), &public.join("private"), true) {
        return;
    }

    not_found_with(get(&router(&public, StaticOptions::default()), "/private"), "Fallback Handler");
}

// filter option

#[test]
fn filters_files_based_on_custom_filter_function() {
    let dir = TempDir::new();
    dir.create("index.html", "<h1>Home</h1>");
    dir.create("secret.txt", "Secret");
    dir.create("public.txt", "Public");
    let options = StaticOptions { filter: Some(Arc::new(|path: &str| !path.contains("secret"))), ..StaticOptions::default() };
    let router = router(&dir.0, options);

    not_found_with(get(&router, "/secret.txt"), "Fallback Handler");

    let public = get(&router, "/public.txt");
    assert_eq!(public.status(), 200);
    assert_eq!(text(public), "Public");
}

// index option

fn index_router(dir: &TempDir, index: &[&str]) -> Router {
    router(&dir.0, StaticOptions { index: index.iter().map(|s| s.to_string()).collect(), ..StaticOptions::default() })
}

#[test]
fn serves_default_index_html_when_requesting_a_directory() {
    let dir = TempDir::new();
    dir.create("subdir/index.html", "<h1>Index Page</h1>");

    let response = get(&router(&dir.0, StaticOptions::default()), "/subdir/");

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Type").as_deref(), Some("text/html; charset=utf-8"));
    assert_eq!(text(response), "<h1>Index Page</h1>");
}

#[test]
fn serves_default_index_html_when_requesting_a_directory_without_trailing_slash() {
    let dir = TempDir::new();
    dir.create("subdir/index.html", "<h1>Index Page</h1>");

    let response = get(&router(&dir.0, StaticOptions::default()), "/subdir");

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Type").as_deref(), Some("text/html; charset=utf-8"));
    assert_eq!(text(response), "<h1>Index Page</h1>");
}

#[test]
fn serves_default_index_htm_when_index_html_does_not_exist() {
    let dir = TempDir::new();
    dir.create("subdir/index.htm", "<h1>HTM Index Page</h1>");

    let response = get(&router(&dir.0, StaticOptions::default()), "/subdir/");

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Type").as_deref(), Some("text/html; charset=utf-8"));
    assert_eq!(text(response), "<h1>HTM Index Page</h1>");
}

#[test]
fn prefers_index_html_over_index_htm_when_both_exist() {
    let dir = TempDir::new();
    dir.create("subdir/index.html", "<h1>HTML Index</h1>");
    dir.create("subdir/index.htm", "<h1>HTM Index</h1>");

    let response = get(&router(&dir.0, StaticOptions::default()), "/subdir/");

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "<h1>HTML Index</h1>");
}

#[test]
fn falls_through_when_directory_has_no_index_file() {
    let dir = TempDir::new();
    dir.mkdir("subdir");
    dir.create("subdir/other.txt", "Not an index file");

    not_found_with(get(&router(&dir.0, StaticOptions::default()), "/subdir/"), "Fallback Handler");
}

#[test]
fn serves_custom_index_file_when_specified() {
    let dir = TempDir::new();
    dir.create("subdir/default.html", "<h1>Custom Default Page</h1>");

    let response = get(&index_router(&dir, &["default.html"]), "/subdir/");

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "<h1>Custom Default Page</h1>");
}

#[test]
fn tries_custom_index_files_in_order() {
    let dir = TempDir::new();
    dir.create("subdir/home.html", "<h1>Home Page</h1>");
    dir.create("subdir/default.html", "<h1>Default Page</h1>");

    let response = get(&index_router(&dir, &["index.html", "home.html", "default.html"]), "/subdir/");

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "<h1>Home Page</h1>");
}

#[test]
fn serves_root_directory_index_file() {
    let dir = TempDir::new();
    dir.create("index.html", "<h1>Root Index</h1>");

    let response = get(&router(&dir.0, StaticOptions::default()), "/");

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "<h1>Root Index</h1>");
}

#[test]
fn supports_empty_index_array_to_disable_index_file_serving() {
    let dir = TempDir::new();
    dir.create("subdir/index.html", "<h1>Index Page</h1>");

    not_found_with(get(&index_router(&dir, &[]), "/subdir/"), "Fallback Handler");
}

#[test]
fn supports_index_false_to_disable_index_file_serving() {
    let dir = TempDir::new();
    dir.create("subdir/index.html", "<h1>Index Page</h1>");

    // `index: false` is an empty list.
    let options = StaticOptions { index: Vec::new(), ..StaticOptions::default() };
    not_found_with(get(&router(&dir.0, options), "/subdir/"), "Fallback Handler");
}

#[test]
fn supports_index_true_to_use_default_index_files() {
    let dir = TempDir::new();
    dir.create("subdir/index.html", "<h1>Index Page</h1>");

    // `index: true` is the default list.
    let options = StaticOptions { index: StaticOptions::default().index, ..StaticOptions::default() };
    let response = get(&router(&dir.0, options), "/subdir/");

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "<h1>Index Page</h1>");
}

// listFiles option

/// The contents of each `<tr class="file-row">`.
fn rows(body: &str) -> Vec<&str> {
    body.split("<tr class=\"file-row\">").skip(1).map(|row| row.split("</tr>").next().unwrap()).collect()
}

#[test]
fn lists_folders_without_sizes_and_preserves_file_metadata_and_navigation() {
    let dir = TempDir::new();
    dir.create("downloads/folder10/nested/file.txt", "Nested file");
    dir.mkdir("downloads/folder2");
    dir.create("downloads/index.html", "<h1>Index</h1>");
    dir.create("downloads/empty.txt", "");
    let router = Router::new().with(static_files(&dir.0, StaticOptions { list_files: true, index: Vec::new(), ..StaticOptions::default() }));

    let response = get(&router, "/downloads/");
    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Type").as_deref(), Some("text/html; charset=UTF-8"));

    let body = text(response);
    let rows = rows(&body);
    assert_eq!(rows.len(), 5);
    let parent = rows[0].split_once("href=\"/\"").expect("a link to the parent").1;
    assert!(parent.contains(".."), "{}", rows[0]);
    assert!(rows[1].contains("href=\"/downloads/folder2\""), "{}", rows[1]);
    assert!(rows[2].contains("href=\"/downloads/folder10\""), "{}", rows[2]);
    assert!(rows[1].contains("<td class=\"size-cell\"></td>"));
    assert!(rows[2].contains("<td class=\"size-cell\"></td>"));
    assert!(rows[1].contains("<td class=\"type-cell\">Folder</td>"));
    assert!(rows[2].contains("<td class=\"type-cell\">Folder</td>"));
    assert!(rows[3].contains("href=\"/downloads/empty.txt\""), "{}", rows[3]);
    assert!(rows[3].contains("<td class=\"size-cell\">0 B</td>"));
    assert!(rows[3].contains("<td class=\"type-cell\">text/plain</td>"));
    assert!(rows[4].contains("href=\"/downloads/index.html\""), "{}", rows[4]);
    assert!(rows[4].contains("<td class=\"size-cell\">14 B</td>"));
    assert!(rows[4].contains("<td class=\"type-cell\">text/html</td>"));
}

#[test]
fn only_reads_immediate_entries_on_repeated_get_and_head_listings() {
    // Remix counts fs.promises.readdir and stat calls with mocks; std::fs cannot be mocked.
    // What those counts guarantee is checked through the output instead: only the root's
    // immediate entries are listed, folders without a size, on every GET and HEAD.
    let dir = TempDir::new();
    dir.create("folder/nested/file.txt", "Nested file");
    dir.create("other/file.txt", "Other file");
    dir.create("readme.txt", "Read me");
    let router = Router::new().with(static_files(&dir.0, StaticOptions { list_files: true, ..StaticOptions::default() }));

    for method in [Method::GET, Method::GET, Method::HEAD] {
        let response = fetch(&router, method, "/", &[]);
        assert_eq!(response.status(), 200);
        let body = text(response);
        let rows = rows(&body);
        assert_eq!(rows.len(), 3, "{body}");
        assert!(rows[0].contains("href=\"/folder\"") && rows[0].contains("<td class=\"size-cell\"></td>"));
        assert!(rows[1].contains("href=\"/other\"") && rows[1].contains("<td class=\"size-cell\"></td>"));
        assert!(rows[2].contains("href=\"/readme.txt\"") && rows[2].contains("<td class=\"size-cell\">7 B</td>"));
    }
}

#[test]
fn serves_index_files_before_generating_a_listing() {
    let dir = TempDir::new();
    dir.create("index.html", "<h1>Index</h1>");
    dir.create("folder/nested/file.txt", "Nested file");
    let router = Router::new().with(static_files(&dir.0, StaticOptions { list_files: true, ..StaticOptions::default() }));

    let response = get(&router, "/");

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "<h1>Index</h1>");
}

// works with method-override middleware

fn post_form(router: &Router, body: &str) -> Response {
    let request = http::Request::post("https://remix.run/test.txt").header("Content-Type", "application/x-www-form-urlencoded").body(Body::from(body.to_string())).unwrap();
    block_on(router.fetch(request))
}

#[test]
fn ignores_overridden_post_requests() {
    let dir = TempDir::new();
    dir.create("test.txt", "Hello, World!");
    let mut router = Router::new().with(form_data()).with(method_override());
    router.post("/*path", action(|_: Context| async { "POST handler called" }).with(static_files(&dir.0, StaticOptions::default())));

    let response = post_form(&router, "_method=POST&name=test");

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "POST handler called");
}

#[test]
fn serves_files_with_overridden_get_requests() {
    let dir = TempDir::new();
    dir.create("test.txt", "Hello, World!");
    let mut router = Router::new().with(form_data()).with(method_override());
    router.get("/*path", with_static(&dir.0, StaticOptions::default(), "GET handler fallback"));

    let response = post_form(&router, "_method=GET");

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Type").as_deref(), Some("text/plain; charset=utf-8"));
    assert_eq!(text(response), "Hello, World!");
}
