//! Remix's asset-server.test.ts: the cases about serving, file assets, request and global
//! transforms, the transformed-output cache, CSS rewriting, fingerprints, preloads, access
//! rules, mounts, option validation and error handling. Cases about TypeScript and JSX
//! compilation, source maps, import maps, bare and package imports, barrel files, module
//! loaders, CommonJS detection, watch mode and HMR have no counterpart: rmx serves
//! JavaScript as written (scripts are .js and .mjs, usually wasm-bindgen output).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use http::{header, Method, StatusCode};
use rmx_assets::*;
use rmx_fetch::{Body, File, Response};
use rmx_fetch_router::Context;

struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!("rmx-asset-server-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Dir(path.canonicalize().unwrap())
    }

    fn write(&self, rel: &str, content: &str) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Remix's `createTestServer`: `app/**` allowed, served under `/assets`.
fn options(dir: &Dir) -> AssetOptions {
    AssetOptions { root_dir: dir.0.clone(), allow_files: vec!["app/**".into()], ..Default::default() }
}

fn server(options: AssetOptions) -> AssetServer {
    AssetServer::new(options).unwrap()
}

fn files(extensions: &[&str]) -> FilesOptions {
    FilesOptions { extensions: extensions.iter().map(|e| e.to_string()).collect(), ..Default::default() }
}

fn with_files(dir: &Dir, files: FilesOptions) -> AssetOptions {
    AssetOptions { files: Some(files), ..options(dir) }
}

/// Records the errors passed to `on_error`.
#[derive(Clone, Default)]
struct Errors(Arc<Mutex<Vec<AssetError>>>);

impl Errors {
    fn handler(&self) -> Option<ErrorHandler> {
        let errors = self.0.clone();
        Some(Arc::new(move |e: &AssetError| {
            errors.lock().unwrap().push(e.clone());
            None
        }))
    }

    fn count(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    fn last(&self) -> AssetError {
        self.0.lock().unwrap().last().cloned().expect("an error")
    }
}

struct Res {
    status: StatusCode,
    headers: http::HeaderMap,
    body: String,
}

fn request(server: &AssetServer, method: Method, url: &str, headers: &[(&str, &str)]) -> Option<Res> {
    let mut req = http::Request::builder().method(method).uri(format!("http://localhost{url}"));
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let ctx = Context::new(req.body(Body::empty()).unwrap());
    let res: Response = server.fetch(&ctx)?;
    let (parts, body) = res.into_parts();
    Some(Res { status: parts.status, headers: parts.headers, body: rmx_fetch::block_on(body.text()).unwrap() })
}

fn get(server: &AssetServer, url: &str) -> Option<Res> {
    request(server, Method::GET, url, &[])
}

fn get_ok(server: &AssetServer, url: &str) -> Res {
    get(server, url).unwrap_or_else(|| panic!("{url} is served"))
}

fn header(res: &Res, name: header::HeaderName) -> &str {
    res.headers.get(name).map_or("", |v| v.to_str().unwrap())
}

const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// `/assets/app/x.@hash.svg`: the URL with its fingerprint removed, and the fingerprint.
fn unfingerprint(url: &str) -> (String, String) {
    let at = url.rfind(".@").unwrap_or_else(|| panic!("{url} is fingerprinted"));
    let rest = &url[at + 2..];
    let end = rest.find(['.', '?']).unwrap_or(rest.len());
    (format!("{}{}", &url[..at], &rest[end..]), rest[..end].to_string())
}

fn upper() -> FileTransform {
    FileTransform::new(|bytes, _| Ok(String::from_utf8_lossy(&bytes).to_uppercase().into()))
}

fn identity() -> FileTransform {
    FileTransform::new(|bytes, _| Ok(bytes.into()))
}

/// A transform that returns its input and counts its calls.
fn counting(calls: &Arc<AtomicUsize>) -> FileTransform {
    let calls = calls.clone();
    FileTransform::new(move |bytes, _| {
        calls.fetch_add(1, Ordering::Relaxed);
        Ok(String::from_utf8_lossy(&bytes).to_uppercase().into())
    })
}

fn recolor() -> FileTransform {
    FileTransform::new(|bytes, c| Ok(String::from_utf8_lossy(&bytes).replace("currentColor", c.param.unwrap()).into())).param(Param::Required)
}

const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>\n";
const COLOR_SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" color=\"#0EA5E9\"><rect fill=\"currentColor\" /></svg>\n";

/// A cache that keeps everything in memory (Remix's `createMemoryFileCache`).
#[derive(Default)]
struct MemoryCache(Mutex<HashMap<String, File>>);

impl FileCache for MemoryCache {
    fn get(&self, key: &str) -> std::io::Result<Option<File>> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }

    fn put(&self, key: &str, file: File) -> std::io::Result<()> {
        self.0.lock().unwrap().insert(key.to_string(), file);
        Ok(())
    }
}

// Serving

#[test]
fn handles_get_and_head_requests_but_ignores_post() {
    let dir = Dir::new();
    dir.write("app/entry.js", "export const value = 1");
    let s = server(options(&dir));
    assert_eq!(get_ok(&s, "/assets/app/entry.js").status, 200);
    let head = request(&s, Method::HEAD, "/assets/app/entry.js", &[]).unwrap();
    assert_eq!(head.status, 200);
    assert_eq!(head.body, "");
    assert!(request(&s, Method::POST, "/assets/app/entry.js", &[]).is_none());
}

#[test]
fn serves_entry_points_with_no_cache_and_etags() {
    let dir = Dir::new();
    dir.write("app/entry.js", "export const value = 1");
    let s = server(options(&dir));
    let res = get_ok(&s, "/assets/app/entry.js");
    assert_eq!(header(&res, header::CACHE_CONTROL), "no-cache");
    let etag = header(&res, header::ETAG).to_string();
    assert!(!etag.is_empty());
    let not_modified = request(&s, Method::GET, "/assets/app/entry.js", &[("if-none-match", &etag)]).unwrap();
    assert_eq!(not_modified.status, 304);
}

#[test]
fn matches_a_stripped_weak_etag_prefix_in_if_none_match() {
    let dir = Dir::new();
    dir.write("app/entry.js", "export const value = 1");
    let s = server(options(&dir));
    let etag = header(&get_ok(&s, "/assets/app/entry.js"), header::ETAG).to_string();
    let strong = etag.strip_prefix("W/").unwrap();
    assert_eq!(request(&s, Method::GET, "/assets/app/entry.js", &[("if-none-match", strong)]).unwrap().status, 304);
}

#[test]
fn serves_allow_listed_internal_modules_directly_by_default_with_etags() {
    let dir = Dir::new();
    dir.write("app/entry.js", "import './dep.js'\nexport const entry = true");
    dir.write("app/dep.js", "export const dep = 1");
    let s = server(options(&dir));
    let res = get_ok(&s, "/assets/app/dep.js");
    assert_eq!(res.status, 200);
    assert_eq!(header(&res, header::CACHE_CONTROL), "no-cache");
    assert!(!header(&res, header::ETAG).is_empty());
}

#[test]
fn supports_mjs_modules() {
    let dir = Dir::new();
    dir.write("app/entry.mjs", "export const value = 1");
    let s = server(AssetOptions { fingerprint: true, ..options(&dir) });
    let href = s.href("app/entry.mjs").unwrap();
    assert_eq!(get_ok(&s, &href).status, 200);
}

#[test]
fn serves_css_assets_directly_and_ignores_unsupported_direct_requests_for_other_non_compiled_files() {
    let dir = Dir::new();
    dir.write("app/data.json", "{\"ok\":true}");
    dir.write("app/styles.css", "body { color: red; }");
    dir.write("app/logo.svg", SVG);
    dir.write("app/.env", "SECRET=123");
    let s = server(options(&dir));
    let style = get_ok(&s, "/assets/app/styles.css");
    assert_eq!(style.status, 200);
    assert_eq!(header(&style, header::CONTENT_TYPE), "text/css; charset=utf-8");
    assert!(style.body.contains("color: red"));
    assert!(get(&s, "/assets/app/data.json").is_none());
    assert!(get(&s, "/assets/app/logo.svg").is_none());
    assert!(get(&s, "/assets/app/.env").is_none());
}

#[test]
fn serves_direct_css_assets_with_no_cache_etags_and_head_support() {
    let dir = Dir::new();
    dir.write("app/styles.css", "body { color: red; }\n");
    let s = server(options(&dir));
    let res = get_ok(&s, "/assets/app/styles.css");
    assert_eq!(header(&res, header::CACHE_CONTROL), "no-cache");
    assert_eq!(header(&res, header::CONTENT_TYPE), "text/css; charset=utf-8");
    let head = request(&s, Method::HEAD, "/assets/app/styles.css", &[]).unwrap();
    assert_eq!(head.status, 200);
    assert_eq!(header(&head, header::CONTENT_TYPE), "text/css; charset=utf-8");
    assert_eq!(head.body, "");
    let etag = header(&res, header::ETAG).to_string();
    assert_eq!(request(&s, Method::GET, "/assets/app/styles.css", &[("if-none-match", &etag)]).unwrap().status, 304);
}

#[test]
fn serves_configured_file_assets_with_no_cache_etags_and_head_support() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    let s = server(with_files(&dir, files(&[".svg"])));
    let res = get_ok(&s, "/assets/app/images/logo.svg");
    assert_eq!(res.status, 200);
    assert_eq!(header(&res, header::CONTENT_TYPE), "image/svg+xml");
    assert_eq!(header(&res, header::CACHE_CONTROL), "no-cache");
    assert!(res.body.contains("<svg"));
    let head = request(&s, Method::HEAD, "/assets/app/images/logo.svg", &[]).unwrap();
    assert_eq!(head.status, 200);
    assert_eq!(header(&head, header::CONTENT_TYPE), "image/svg+xml");
    assert_eq!(head.body, "");
    let etag = header(&res, header::ETAG).to_string();
    assert_eq!(request(&s, Method::GET, "/assets/app/images/logo.svg", &[("if-none-match", &etag)]).unwrap().status, 304);
}

#[test]
fn picks_up_source_file_changes_without_watch_mode() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello\n");
    let s = server(with_files(&dir, files(&[".txt"])));
    let first = get_ok(&s, "/assets/app/content/value.txt");
    assert_eq!(first.body, "hello\n");
    dir.write("app/content/value.txt", "world\n");
    let second = get_ok(&s, "/assets/app/content/value.txt");
    assert_eq!(second.body, "world\n");
    assert_ne!(header(&second, header::ETAG), header(&first, header::ETAG));
}

// Request transforms

#[test]
fn serves_request_transformed_file_assets() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", "<svg xmlns=\"http://www.w3.org/2000/svg\" color=\"#0EA5E9\"><title>Demo image</title><rect fill=\"currentColor\" /></svg>\n");
    let recolor = FileTransform::new(|bytes, c| {
        assert_eq!(c.extension, ".svg");
        Ok(String::from_utf8_lossy(&bytes).replace("currentColor", c.param.unwrap()).into())
    })
    .param(Param::Required);
    let retitle = FileTransform::new(|bytes, c| {
        Ok(String::from_utf8_lossy(&bytes).replace("<title>Demo image</title>", &format!("<title>{}</title>", c.param.unwrap())).into())
    })
    .param(Param::Required);
    let s = server(with_files(&dir, FilesOptions { transforms: vec![("recolor".into(), recolor), ("retitle".into(), retitle)], ..files(&[".svg"]) }));
    let href = s.href_with("app/images/logo.svg", &["recolor:#8B5CF6", "retitle:Purple demo image"]).unwrap();
    let res = get_ok(&s, &href);
    assert_eq!(res.status, 200);
    assert_eq!(header(&res, header::CONTENT_TYPE), "image/svg+xml");
    assert!(res.body.contains("#8B5CF6"));
    assert!(res.body.contains("Purple demo image"));
}

#[test]
fn serializes_typed_transform_arguments_and_no_arg_shorthand_in_hrefs() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "base\n");
    let repeat = FileTransform::new(|bytes, c| {
        let count: usize = c.param.unwrap().parse().map_err(|_| "Expected an integer repeat count".to_string())?;
        let text = String::from_utf8_lossy(&bytes).trim().to_string();
        Ok(vec![text; count].join("|").into())
    })
    .param(Param::Required);
    let s = server(with_files(&dir, FilesOptions { transforms: vec![("upper".into(), upper()), ("repeat".into(), repeat)], ..files(&[".txt"]) }));
    let href = s.href_with("app/content/value.txt", &["repeat:3", "upper"]).unwrap();
    assert_eq!(href, "/assets/app/content/value.txt?transform=repeat%3A3&transform=upper");
    assert_eq!(get_ok(&s, &href).body, "BASE|BASE|BASE");
}

#[test]
fn supports_transform_params_with_delimiter_like_characters() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", "<svg xmlns=\"http://www.w3.org/2000/svg\"><title>Demo image</title></svg>\n");
    let retitle = FileTransform::new(|bytes, c| {
        Ok(String::from_utf8_lossy(&bytes).replace("<title>Demo image</title>", &format!("<title>{}</title>", c.param.unwrap())).into())
    })
    .param(Param::Required);
    let s = server(with_files(&dir, FilesOptions { transforms: vec![("retitle".into(), retitle)], ..files(&[".svg"]) }));
    let href = s.href_with("app/images/logo.svg", &["retitle:Purple | demo (v2)"]).unwrap();
    assert_eq!(href, "/assets/app/images/logo.svg?transform=retitle%3APurple+%7C+demo+%28v2%29");
    assert!(get_ok(&s, &href).body.contains("Purple | demo (v2)"));
}

#[test]
fn treats_an_empty_transform_list_in_href_with_as_no_transform() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    let s = server(with_files(&dir, FilesOptions { transforms: vec![("optimize".into(), identity())], ..files(&[".svg"]) }));
    assert_eq!(s.href_with("app/images/logo.svg", &[]).unwrap(), s.href("app/images/logo.svg").unwrap());
    assert_eq!(s.href_with("app/images/logo.svg", &[]).unwrap(), "/assets/app/images/logo.svg");
}

#[test]
fn returns_400_for_malformed_file_transform_queries_without_calling_on_error() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    let errors = Errors::default();
    let s = server(AssetOptions {
        on_error: errors.handler(),
        ..with_files(&dir, FilesOptions { transforms: vec![("optimize".into(), identity())], ..files(&[".svg"]) })
    });
    let res = get_ok(&s, "/assets/app/images/logo.svg?transform=:wat");
    assert_eq!(res.status, 400);
    assert_eq!(header(&res, header::CONTENT_TYPE), "text/plain; charset=utf-8");
    assert!(res.body.contains("Expected each transform name to use \"transform-name\" format"), "{}", res.body);
    assert_eq!(errors.count(), 0);
}

#[test]
fn returns_400_for_invalid_file_transform_requests_without_calling_on_error() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    let errors = Errors::default();
    let s = server(AssetOptions {
        on_error: errors.handler(),
        ..with_files(&dir, FilesOptions { transforms: vec![("recolor".into(), identity().param(Param::Required))], ..files(&[".svg"]) })
    });
    let res = get_ok(&s, "/assets/app/images/logo.svg?transform=recolor");
    assert_eq!(res.status, 400);
    assert_eq!(header(&res, header::CONTENT_TYPE), "text/plain; charset=utf-8");
    assert!(res.body.contains("File transform \"recolor\" requires a param"), "{}", res.body);
    assert_eq!(errors.count(), 0);
}

#[test]
fn returns_400_for_transform_queries_on_served_script_assets() {
    let dir = Dir::new();
    dir.write("app/entry.js", "export const entry = true");
    let s = server(options(&dir));
    assert_eq!(get_ok(&s, "/assets/app/entry.js").status, 200);
    let res = get_ok(&s, "/assets/app/entry.js?transform=resize");
    assert_eq!(res.status, 400);
    assert_eq!(header(&res, header::CONTENT_TYPE), "text/plain; charset=utf-8");
    assert!(res.body.contains("Asset request transforms are only supported for configured file assets"));
}

#[test]
fn returns_400_for_transform_queries_on_served_style_assets() {
    let dir = Dir::new();
    dir.write("app/styles.css", "body { color: red; }\n");
    let s = server(options(&dir));
    assert_eq!(get_ok(&s, "/assets/app/styles.css").status, 200);
    let res = get_ok(&s, "/assets/app/styles.css?transform=resize");
    assert_eq!(res.status, 400);
    assert!(res.body.contains("Asset request transforms are only supported for configured file assets"));
}

#[test]
fn validates_transform_params_when_generating_hrefs() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", COLOR_SVG);
    let s = server(with_files(&dir, FilesOptions { transforms: vec![("recolor".into(), recolor()), ("optimize".into(), identity())], ..files(&[".svg"]) }));
    assert_eq!(s.href_with("app/images/logo.svg", &["recolor"]).unwrap_err().message, "File transform \"recolor\" requires a param");
    assert_eq!(s.href_with("app/images/logo.svg", &["optimize:x"]).unwrap_err().message, "File transform \"optimize\" does not accept a param");
    assert_eq!(s.href_with("app/images/logo.svg", &["missing"]).unwrap_err().message, "Unknown file transform \"missing\"");
}

#[test]
fn allows_global_file_transforms_to_skip_by_returning_none() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    dir.write("app/content/info.txt", "plain text file\n");
    let global = GlobalTransform::new(|bytes, c| {
        if c.extension != ".svg" {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(&bytes).replace("</svg>", "<!-- transformed --></svg>").into()))
    });
    let s = server(with_files(&dir, FilesOptions { global_transforms: vec![global], ..files(&[".svg", ".txt"]) }));
    assert!(get_ok(&s, "/assets/app/images/logo.svg").body.contains("transformed"));
    assert_eq!(get_ok(&s, "/assets/app/content/info.txt").body, "plain text file\n");
}

#[test]
fn skips_global_file_transforms_when_configured_extensions_do_not_match() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    dir.write("app/content/info.txt", "plain text file\n");
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let global = GlobalTransform::new(move |bytes, _| {
        counter.fetch_add(1, Ordering::Relaxed);
        Ok(Some(String::from_utf8_lossy(&bytes).replace("</svg>", "<!-- transformed --></svg>").into()))
    })
    .extensions(&[".svg"]);
    let s = server(with_files(&dir, FilesOptions { global_transforms: vec![global], ..files(&[".svg", ".txt"]) }));
    assert!(get_ok(&s, "/assets/app/images/logo.svg").body.contains("transformed"));
    assert_eq!(get_ok(&s, "/assets/app/content/info.txt").body, "plain text file\n");
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn reports_named_global_file_transform_errors_with_the_name() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello\n");
    let errors = Errors::default();
    let global = GlobalTransform::new(|_, _| Err("no output".into())).name("annotateFile");
    let s = server(AssetOptions { on_error: errors.handler(), ..with_files(&dir, FilesOptions { global_transforms: vec![global], ..files(&[".txt"]) }) });
    let res = get_ok(&s, "/assets/app/content/value.txt");
    assert_eq!(res.status, 500);
    assert_eq!(header(&res, header::CONTENT_TYPE), "text/plain; charset=utf-8");
    assert_eq!(res.body, "Internal Server Error");
    let error = errors.last();
    assert_eq!(error.code, AssetErrorCode::FileTransformFailed);
    assert!(error.message.starts_with("Global file transform \"annotateFile\" failed for "), "{}", error.message);
    assert!(error.message.contains("app/content/value.txt"), "{}", error.message);
}

#[test]
fn reports_anonymous_global_file_transform_errors_with_the_transform_index() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello\n");
    let errors = Errors::default();
    let globals = vec![GlobalTransform::new(|_, _| Ok(Some("hello\n".into()))), GlobalTransform::new(|_, _| Err("no output".into()))];
    let s = server(AssetOptions { on_error: errors.handler(), ..with_files(&dir, FilesOptions { global_transforms: globals, ..files(&[".txt"]) }) });
    assert_eq!(get_ok(&s, "/assets/app/content/value.txt").status, 500);
    let error = errors.last();
    assert!(error.message.starts_with("Global file transform at index 1 failed for "), "{}", error.message);
}

#[test]
fn allows_request_file_transforms_to_return_string_content_directly() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello\n");
    let s = server(with_files(&dir, FilesOptions { transforms: vec![("upper".into(), upper())], ..files(&[".txt"]) }));
    let href = s.href_with("app/content/value.txt", &["upper"]).unwrap();
    assert_eq!(get_ok(&s, &href).body, "HELLO\n");
}

#[test]
fn rejects_request_file_transforms_when_configured_extensions_do_not_match() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello\n");
    let errors = Errors::default();
    let annotate = FileTransform::new(|bytes, _| Ok(format!("{}!", String::from_utf8_lossy(&bytes)).into())).extensions(&[".svg"]);
    let s = server(AssetOptions { on_error: errors.handler(), ..with_files(&dir, FilesOptions { transforms: vec![("annotate".into(), annotate)], ..files(&[".txt"]) }) });
    let href = s.href_with("app/content/value.txt", &["annotate"]).unwrap();
    let res = get_ok(&s, &href);
    assert_eq!(res.status, 500);
    assert_eq!(header(&res, header::CONTENT_TYPE), "text/plain; charset=utf-8");
    let error = errors.last();
    assert_eq!(error.code, AssetErrorCode::FileTransformNotSupported);
    assert!(error.message.starts_with("File transform \"annotate\" does not support .txt inputs for "), "{}", error.message);
    assert!(error.message.ends_with("app/content/value.txt"), "{}", error.message);
}

#[test]
fn reports_invalid_request_file_transform_results_with_a_specific_error_code() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello\n");
    let errors = Errors::default();
    let broken = FileTransform::new(|bytes, _| Ok(Transformed::with_extension(bytes, "txt")));
    let s = server(AssetOptions { on_error: errors.handler(), ..with_files(&dir, FilesOptions { transforms: vec![("broken".into(), broken)], ..files(&[".txt"]) }) });
    assert_eq!(get_ok(&s, "/assets/app/content/value.txt?transform=broken").status, 500);
    let error = errors.last();
    assert_eq!(error.code, AssetErrorCode::FileTransformResultInvalid);
    assert!(error.message.starts_with("File transform \"broken\" returned an invalid extension \"txt\" for "), "{}", error.message);
}

#[test]
fn reports_failed_request_file_transforms_as_transform_failures() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello\n");
    let errors = Errors::default();
    let broken = FileTransform::new(|_, _| Err("boom".into()));
    let s = server(AssetOptions { on_error: errors.handler(), ..with_files(&dir, FilesOptions { transforms: vec![("broken".into(), broken)], ..files(&[".txt"]) }) });
    assert_eq!(get_ok(&s, "/assets/app/content/value.txt?transform=broken").status, 500);
    let error = errors.last();
    assert_eq!(error.code, AssetErrorCode::FileTransformFailed);
    assert!(error.message.starts_with("File transform \"broken\" failed for "), "{}", error.message);
    assert!(error.message.ends_with("app/content/value.txt. boom"), "{}", error.message);
}

#[test]
fn matches_transform_extension_filters_against_the_current_extension_in_the_pipeline() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello\n");
    let global = GlobalTransform::new(|bytes, c| Ok(Some(format!("{}|global:{}", String::from_utf8_lossy(&bytes), c.extension).into()))).extensions(&[".done"]);
    let finalize = FileTransform::new(|bytes, c| Ok(Transformed::with_extension(format!("{}|finalize:{}", String::from_utf8_lossy(&bytes), c.extension), ".done"))).extensions(&[".txt"]);
    let publish = FileTransform::new(|bytes, c| Ok(format!("{}|publish:{}", String::from_utf8_lossy(&bytes), c.extension).into())).extensions(&[".done"]);
    let s = server(with_files(&dir, FilesOptions {
        global_transforms: vec![global],
        transforms: vec![("finalize".into(), finalize), ("publish".into(), publish)],
        ..files(&[".txt"])
    }));
    let href = s.href_with("app/content/value.txt", &["finalize", "publish"]).unwrap();
    let res = get_ok(&s, &href);
    // Remix falls back to the source file's type; rmx uses the output extension's, and
    // `.done` has none.
    assert_eq!(header(&res, header::CONTENT_TYPE), "application/octet-stream");
    assert_eq!(res.body, "hello\n|finalize:.txt|publish:.done|global:.done");
}

#[test]
fn accepts_no_arg_transforms_and_serializes_them_as_names() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    let s = server(with_files(&dir, FilesOptions { transforms: vec![("optimize".into(), identity())], ..files(&[".svg"]) }));
    assert_eq!(s.href_with("app/images/logo.svg", &["optimize"]).unwrap(), "/assets/app/images/logo.svg?transform=optimize");
}

#[test]
fn limits_request_transforms_to_5_by_default_when_generating_hrefs() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    let s = server(with_files(&dir, FilesOptions { transforms: vec![("optimize".into(), identity())], ..files(&[".svg"]) }));
    let error = s.href_with("app/images/logo.svg", &["optimize"; 6]).unwrap_err();
    assert_eq!(error.message, "Expected at most 5 request transforms");
}

#[test]
fn limits_request_transforms_to_5_by_default_when_parsing_requests() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    let errors = Errors::default();
    let s = server(AssetOptions {
        on_error: errors.handler(),
        ..with_files(&dir, FilesOptions { transforms: vec![("optimize".into(), identity())], ..files(&[".svg"]) })
    });
    let query = ["transform=optimize"; 6].join("&");
    let res = get_ok(&s, &format!("/assets/app/images/logo.svg?{query}"));
    assert_eq!(res.status, 400);
    assert!(res.body.contains("Expected at most 5 request transforms"));
    assert_eq!(errors.count(), 0);
}

#[test]
fn supports_overriding_files_max_request_transforms() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    let s = server(with_files(&dir, FilesOptions { max_request_transforms: 6, transforms: vec![("optimize".into(), identity())], ..files(&[".svg"]) }));
    let href = s.href_with("app/images/logo.svg", &["optimize"; 6]).unwrap();
    assert_eq!(get_ok(&s, &href).status, 200);
}

#[test]
fn treats_inherited_object_property_names_as_unknown_file_transform_requests() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    let errors = Errors::default();
    let s = server(AssetOptions {
        on_error: errors.handler(),
        ..with_files(&dir, FilesOptions { transforms: vec![("optimize".into(), identity())], ..files(&[".svg"]) })
    });
    for name in ["constructor", "toString", "__proto__"] {
        let res = get_ok(&s, &format!("/assets/app/images/logo.svg?transform={name}"));
        assert_eq!(res.status, 400);
        assert!(res.body.contains(&format!("Unknown file transform \"{name}\"")), "{}", res.body);
    }
    assert_eq!(errors.count(), 0);
}

// The transformed-output cache

#[test]
fn recomputes_transformed_outputs_on_each_request_without_a_cache() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello");
    let calls = Arc::new(AtomicUsize::new(0));
    let s = server(with_files(&dir, FilesOptions { transforms: vec![("upper".into(), counting(&calls))], ..files(&[".txt"]) }));
    for _ in 0..2 {
        assert_eq!(get_ok(&s, "/assets/app/content/value.txt?transform=upper").body, "HELLO");
    }
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert!(!dir.0.join("node_modules").exists());
}

#[test]
fn uses_a_get_and_put_only_cache_for_transformed_assets() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    let calls = Arc::new(AtomicUsize::new(0));
    let s = server(with_files(&dir, FilesOptions {
        cache: Some(Arc::new(MemoryCache::default())),
        transforms: vec![("optimize".into(), counting(&calls))],
        ..files(&[".svg"])
    }));
    let href = s.href_with("app/images/logo.svg", &["optimize"]).unwrap();
    assert_eq!(get_ok(&s, &href).status, 200);
    assert_eq!(get_ok(&s, &href).status, 200);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn deduplicates_equivalent_transform_urls_and_preserves_distinct_pipeline_inputs() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello");
    let cache = Arc::new(MemoryCache::default());
    let calls = Arc::new(Mutex::new(Vec::new()));
    let seen = calls.clone();
    let append = FileTransform::new(move |bytes, c| {
        seen.lock().unwrap().push(c.param.map(str::to_string));
        Ok(format!("{}{}", String::from_utf8_lossy(&bytes), c.param.unwrap_or("default")).into())
    })
    .param(Param::Optional);
    let s = server(with_files(&dir, FilesOptions { cache: Some(cache.clone()), transforms: vec![("append".into(), append)], ..files(&[".txt"]) }));
    for url in ["/assets/app/content/value.txt?transform=append:a+b", "/assets/app/content/value.txt?transform=append%3Aa%20b&v=2"] {
        assert_eq!(get_ok(&s, url).body, "helloa b");
    }
    assert_eq!(*calls.lock().unwrap(), vec![Some("a b".to_string())]);
    assert_eq!(cache.0.lock().unwrap().len(), 1);
    assert_eq!(get_ok(&s, "/assets/app/content/value.txt?transform=append").body, "hellodefault");
    assert_eq!(get_ok(&s, "/assets/app/content/value.txt?transform=append:").body, "hello");
    assert_eq!(get_ok(&s, "/assets/app/content/value.txt?transform=append:a&transform=append:b").body, "helloab");
    assert_eq!(get_ok(&s, "/assets/app/content/value.txt?transform=append:b&transform=append:a").body, "helloba");
}

fn cached_server(dir: &Dir, cache: &Arc<MemoryCache>, cache_key: &str, calls: &Arc<AtomicUsize>) -> AssetServer {
    server(with_files(dir, FilesOptions {
        cache: Some(cache.clone()),
        cache_key: Some(cache_key.into()),
        transforms: vec![("upper".into(), counting(calls))],
        ..files(&[".svg", ".txt"])
    }))
}

#[test]
fn reuses_transformed_file_cache_entries_across_servers_with_the_same_cache_key() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    let (cache, calls) = (Arc::new(MemoryCache::default()), Arc::new(AtomicUsize::new(0)));
    for _ in 0..2 {
        let s = cached_server(&dir, &cache, "commit-a", &calls);
        let href = s.href_with("app/images/logo.svg", &["upper"]).unwrap();
        assert_eq!(get_ok(&s, &href).status, 200);
    }
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn does_not_reuse_transformed_file_cache_entries_across_different_cache_keys() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    let (cache, calls) = (Arc::new(MemoryCache::default()), Arc::new(AtomicUsize::new(0)));
    for key in ["commit-a", "commit-b"] {
        let s = cached_server(&dir, &cache, key, &calls);
        let href = s.href_with("app/images/logo.svg", &["upper"]).unwrap();
        assert_eq!(get_ok(&s, &href).status, 200);
    }
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

#[test]
fn lets_the_cache_key_control_transformed_file_cache_invalidation_across_servers() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello\n");
    let (cache, calls) = (Arc::new(MemoryCache::default()), Arc::new(AtomicUsize::new(0)));
    let first = cached_server(&dir, &cache, "commit-a", &calls);
    assert_eq!(get_ok(&first, &first.href_with("app/content/value.txt", &["upper"]).unwrap()).body, "HELLO\n");
    dir.write("app/content/value.txt", "world\n");
    let second = cached_server(&dir, &cache, "commit-a", &calls);
    assert_eq!(get_ok(&second, &second.href_with("app/content/value.txt", &["upper"]).unwrap()).body, "HELLO\n");
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn invalidates_transformed_file_cache_entries_when_source_files_change() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello\n");
    let (cache, calls) = (Arc::new(MemoryCache::default()), Arc::new(AtomicUsize::new(0)));
    let s = cached_server(&dir, &cache, "commit-a", &calls);
    let href = s.href_with("app/content/value.txt", &["upper"]).unwrap();
    assert_eq!(get_ok(&s, &href).body, "HELLO\n");
    dir.write("app/content/value.txt", "world!\n");
    assert_eq!(get_ok(&s, &href).body, "WORLD!\n");
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

/// A server for `app/content/value.txt` with one request transform.
fn txt_server(dir: &Dir, cache: Arc<dyn FileCache>, cache_key: Option<&str>, name: &str, transform: FileTransform) -> AssetServer {
    server(with_files(dir, FilesOptions {
        cache: Some(cache),
        cache_key: cache_key.map(str::to_string),
        transforms: vec![(name.into(), transform)],
        ..files(&[".txt"])
    }))
}

fn fs_cache(directory: PathBuf, max_entries: usize) -> Arc<dyn FileCache> {
    Arc::new(FsFileCache::new(FsFileCacheOptions { directory, max_entries, ..Default::default() }).unwrap())
}

#[test]
fn reuses_a_filesystem_cache_in_a_custom_directory_across_server_restarts() {
    let dir = Dir::new();
    let root = dir.0.join("custom-cached-assets");
    std::fs::create_dir_all(root.join("app/content")).unwrap();
    std::fs::write(root.join("app/content/value.txt"), "hello").unwrap();
    let cache_dir = dir.0.join("custom-asset-cache");
    let calls = Arc::new(AtomicUsize::new(0));
    for _ in 0..2 {
        let s = server(AssetOptions {
            root_dir: root.clone(),
            allow_files: vec!["app/**".into()],
            files: Some(FilesOptions {
                cache: Some(fs_cache(cache_dir.clone(), 1024)),
                cache_key: Some("custom-build".into()),
                transforms: vec![("upper".into(), counting(&calls))],
                ..files(&[".txt"])
            }),
            ..Default::default()
        });
        assert_eq!(get_ok(&s, "/assets/app/content/value.txt?transform=upper").body, "HELLO");
    }
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(cache_dir.exists());
    assert!(!root.join("target").exists());
}

#[test]
fn accepts_file_storage_backends_as_file_cache_implementations() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello");
    let caches: Vec<Arc<dyn FileCache>> = vec![
        Arc::new(rmx_file_storage::MemoryFileStorage::new()),
        Arc::new(rmx_file_storage::FsFileStorage::new(dir.0.join("custom-file-storage-cache")).unwrap()),
    ];
    for cache in caches {
        let calls = Arc::new(AtomicUsize::new(0));
        let s = txt_server(&dir, cache, None, "upper", counting(&calls));
        for _ in 0..2 {
            assert_eq!(get_ok(&s, "/assets/app/content/value.txt?transform=upper").body, "HELLO");
        }
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}

#[test]
fn lets_custom_caches_retain_files_larger_than_the_filesystem_cache_limit() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello");
    let cache = Arc::new(MemoryCache::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let expand = FileTransform::new(move |_, _| {
        seen.fetch_add(1, Ordering::Relaxed);
        Ok(Transformed::with_extension(vec![b'A'; 4 * 1024 * 1024 + 1], ".svg"))
    });
    let s = txt_server(&dir, cache.clone(), None, "expand", expand);
    for _ in 0..2 {
        let res = get_ok(&s, "/assets/app/content/value.txt?transform=expand");
        assert!(header(&res, header::CONTENT_TYPE).contains("image/svg+xml"));
        assert_eq!(res.body.len(), 4 * 1024 * 1024 + 1);
    }
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let files = cache.0.lock().unwrap();
    assert_eq!(files.len(), 1);
    let file = files.values().next().unwrap();
    assert_eq!(file.name, "value.svg");
    assert!(file.content_type.contains("image/svg+xml"));
    assert_eq!(file.size(), 4 * 1024 * 1024 + 1);
}

#[test]
fn consults_custom_cache_eviction_before_answering_conditional_requests() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello");
    let cache = Arc::new(MemoryCache::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let append = FileTransform::new(move |_, _| Ok(format!("output-{}", seen.fetch_add(1, Ordering::Relaxed) + 1).into()));
    let s = txt_server(&dir, cache.clone(), None, "append", append);
    let href = "/assets/app/content/value.txt?transform=append";
    let first = get_ok(&s, href);
    let etag = header(&first, header::ETAG).to_string();
    assert!(!etag.is_empty());
    cache.0.lock().unwrap().clear();
    let second = request(&s, Method::GET, href, &[("if-none-match", &etag)]).unwrap();
    assert_eq!(second.status, 200);
    assert_eq!(second.body, "output-2");
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

/// `append` adds its param to the file text and counts its calls.
fn appending(calls: &Arc<AtomicUsize>) -> FileTransform {
    let calls = calls.clone();
    FileTransform::new(move |bytes, c| {
        calls.fetch_add(1, Ordering::Relaxed);
        Ok(format!("{}{}", String::from_utf8_lossy(&bytes), c.param.unwrap()).into())
    })
    .param(Param::Required)
}

#[test]
fn recomputes_evicted_filesystem_cache_entries_for_conditional_requests() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello");
    let calls = Arc::new(AtomicUsize::new(0));
    let s = txt_server(&dir, fs_cache(dir.0.join("conditional-cache"), 3), None, "append", appending(&calls));
    let mut etags = Vec::new();
    for index in 0..4 {
        let res = get_ok(&s, &format!("/assets/app/content/value.txt?transform=append:{index}"));
        assert_eq!(res.body, format!("hello{index}"));
        etags.push(header(&res, header::ETAG).to_string());
    }
    assert_eq!(calls.load(Ordering::Relaxed), 4);
    for (index, etag) in etags.iter().enumerate() {
        let res = request(&s, Method::GET, &format!("/assets/app/content/value.txt?transform=append:{index}"), &[("if-none-match", etag)]).unwrap();
        assert_eq!(res.status, 304);
        assert_eq!(header(&res, header::ETAG), etag);
    }
    assert!(calls.load(Ordering::Relaxed) > 4);
}

#[test]
fn bounds_the_filesystem_cache_with_configured_limits_across_server_restarts_and_namespaces() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello");
    let calls = Arc::new(AtomicUsize::new(0));
    for round in 0..2 {
        let s = txt_server(&dir, fs_cache(dir.0.join("cache"), 5), None, "append", appending(&calls));
        for index in 0..6 {
            let param = format!("{round}:{index}");
            assert_eq!(get_ok(&s, &format!("/assets/app/content/value.txt?transform=append:{param}")).body, format!("hello{param}"));
        }
        let bodies = walk(&dir.0.join("cache")).into_iter().filter(|p| p.to_string_lossy().ends_with(".dat")).count();
        assert_eq!(bodies, 5);
    }
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(walk(&path));
        }
        out.push(path);
    }
    out
}

#[test]
fn serves_oversized_outputs_without_admitting_them_to_the_filesystem_cache() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello");
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let expand = FileTransform::new(move |_, _| {
        seen.fetch_add(1, Ordering::Relaxed);
        Ok(vec![b'A'; 4 * 1024 * 1024 + 1].into())
    });
    let s = txt_server(&dir, fs_cache(dir.0.join("cache"), 1024), None, "expand", expand);
    for _ in 0..2 {
        let res = get_ok(&s, "/assets/app/content/value.txt?transform=expand");
        assert_eq!(res.status, 200);
        assert_eq!(res.body.len(), 4 * 1024 * 1024 + 1);
        assert!(res.body.bytes().all(|b| b == b'A'));
    }
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

#[test]
fn reuses_the_filesystem_cache_with_a_stable_namespace_and_isolates_new_namespaces() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello");
    let calls = Arc::new(AtomicUsize::new(0));
    for key in ["default-build-a", "default-build-a", "default-build-b"] {
        let seen = calls.clone();
        let append = FileTransform::new(move |bytes, _| {
            seen.fetch_add(1, Ordering::Relaxed);
            Ok(bytes.into())
        });
        let s = txt_server(&dir, fs_cache(dir.0.join("cache"), 1024), Some(key), "append", append);
        assert_eq!(get_ok(&s, "/assets/app/content/value.txt?transform=append").body, "hello");
    }
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

/// A cache whose reads and writes fail.
struct BrokenCache;

impl FileCache for BrokenCache {
    fn get(&self, _: &str) -> std::io::Result<Option<File>> {
        Err(std::io::Error::other("disk on fire"))
    }

    fn put(&self, _: &str, _: File) -> std::io::Result<()> {
        Err(std::io::Error::other("disk on fire"))
    }
}

#[test]
fn reports_file_cache_failures_through_on_error() {
    let dir = Dir::new();
    dir.write("app/content/value.txt", "hello");
    let errors = Errors::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let s = server(AssetOptions { on_error: errors.handler(), ..txt_options(&dir, Arc::new(BrokenCache), counting(&calls)) });
    let res = get_ok(&s, "/assets/app/content/value.txt?transform=upper");
    assert_eq!(res.status, 500);
    assert_eq!(errors.count(), 1);
    assert_eq!(errors.last().code, AssetErrorCode::EmitFailed);
    assert!(errors.last().message.contains("disk on fire"), "{}", errors.last().message);
}

fn txt_options(dir: &Dir, cache: Arc<dyn FileCache>, transform: FileTransform) -> AssetOptions {
    with_files(dir, FilesOptions { cache: Some(cache), transforms: vec![("upper".into(), transform)], ..files(&[".txt"]) })
}

// CSS

#[test]
fn rewrites_css_imports_and_relative_file_urls_to_served_urls() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", "@import \"./reset.css\";\n.hero { background: url(\"../images/logo.svg\"); }\n");
    dir.write("app/styles/reset.css", "body { color: red; }\n");
    dir.write("app/images/logo.svg", SVG);
    let s = server(with_files(&dir, files(&[".svg"])));
    let res = get_ok(&s, &s.href("app/styles/app.css").unwrap());
    assert_eq!(res.status, 200);
    assert_eq!(header(&res, header::CONTENT_TYPE), "text/css; charset=utf-8");
    assert!(res.body.contains("@import \"/assets/app/styles/reset.css\";"), "{}", res.body);
    assert!(res.body.contains("url(\"/assets/app/images/logo.svg\")"), "{}", res.body);
    assert_eq!(s.preloads(&["app/styles/app.css"]).unwrap(), ["/assets/app/styles/app.css", "/assets/app/styles/reset.css"]);
    assert_eq!(get_ok(&s, "/assets/app/images/logo.svg").status, 200);
}

fn recolor_server(dir: &Dir, fingerprint: bool) -> AssetServer {
    server(AssetOptions { fingerprint, ..with_files(dir, FilesOptions { transforms: vec![("recolor".into(), recolor())], ..files(&[".svg"]) }) })
}

/// The URL inside the first `url('...')`.
fn first_url(css: &str) -> &str {
    let start = css.find("url('").unwrap() + 5;
    &css[start..start + css[start..].find('\'').unwrap()]
}

#[test]
fn encodes_handwritten_query_syntax_when_rewriting_css_file_urls() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", ".hero { background-image: url('../images/logo.svg?transform=recolor:red'); }\n");
    dir.write("app/images/logo.svg", COLOR_SVG);
    let s = recolor_server(&dir, false);
    let css = get_ok(&s, &s.href("app/styles/app.css").unwrap()).body;
    assert_eq!(first_url(&css), "/assets/app/images/logo.svg?transform=recolor%3Ared");
    assert!(get_ok(&s, "/assets/app/images/logo.svg?transform=recolor:red").body.contains("red"));
}

#[test]
fn rewrites_fingerprinted_css_file_urls_through_the_file_href_pipeline() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", ".hero { background-image: url('../images/logo.svg?transform=recolor:red#icon'); }\n");
    dir.write("app/images/logo.svg", COLOR_SVG);
    let s = recolor_server(&dir, true);
    let css = get_ok(&s, &s.href("app/styles/app.css").unwrap()).body;
    let url = first_url(&css);
    assert_eq!(unfingerprint(url).0, "/assets/app/images/logo.svg?transform=recolor%3Ared#icon");
    let res = get_ok(&s, url.trim_end_matches("#icon"));
    assert_eq!(res.status, 200);
    assert_eq!(header(&res, header::CACHE_CONTROL), IMMUTABLE);
    assert!(res.body.contains("red"));
}

#[test]
fn preserves_non_transform_css_file_query_params_when_rewriting_transformed_urls() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", ".hero { background-image: url('../images/logo.svg?transform=recolor:red&v=1&mode=dark'); }\n");
    dir.write("app/images/logo.svg", COLOR_SVG);
    let s = recolor_server(&dir, false);
    let css = get_ok(&s, &s.href("app/styles/app.css").unwrap()).body;
    assert_eq!(first_url(&css), "/assets/app/images/logo.svg?transform=recolor%3Ared&v=1&mode=dark");
    let res = get_ok(&s, "/assets/app/images/logo.svg?transform=recolor:red&v=1&mode=dark");
    assert_eq!(res.status, 200);
    assert!(res.body.contains("red"));
}

#[test]
fn preserves_non_transform_css_file_query_params_and_hashes_for_fingerprinted_rewritten_urls() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", ".hero { background-image: url('../images/logo.svg?transform=recolor:red&v=1#icon'); }\n");
    dir.write("app/images/logo.svg", COLOR_SVG);
    let s = recolor_server(&dir, true);
    let css = get_ok(&s, &s.href("app/styles/app.css").unwrap()).body;
    let url = first_url(&css);
    assert_eq!(unfingerprint(url).0, "/assets/app/images/logo.svg?transform=recolor%3Ared&v=1#icon");
    let res = get_ok(&s, url.trim_end_matches("#icon"));
    assert_eq!(header(&res, header::CACHE_CONTROL), IMMUTABLE);
    assert!(res.body.contains("red"));
}

#[test]
fn leaves_external_css_imports_unchanged() {
    let dir = Dir::new();
    let imports = [
        "@import \"data:text/css,body{color:red}\"",
        "@import \"//cdn.example.com/theme.css\"",
        "@import \"http://example.com/theme.css\"",
        "@import \"https://example.com/theme.css\"",
        "@import \"/themes/dark.css\"",
        "@import \"#theme\"",
    ];
    dir.write("app/styles/app.css", &format!("{};\n", imports.join(";\n")));
    let s = server(AssetOptions { fingerprint: true, ..options(&dir) });
    let css = get_ok(&s, &s.href("app/styles/app.css").unwrap()).body;
    for import in imports {
        assert!(css.contains(&format!("{import};")), "{css}");
    }
}

#[test]
fn leaves_external_css_url_dependencies_unchanged() {
    let dir = Dir::new();
    let urls = [
        "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg'%3E%3C/svg%3E\")",
        "url(\"//cdn.example.com/bg.png\")",
        "url(\"http://example.com/bg.png\")",
        "url(\"https://example.com/bg.png\")",
        "url(\"/images/bg.png\")",
        "url(\"#my-filter\")",
    ];
    let rules: Vec<String> = urls.iter().map(|u| format!(".x {{ background-image: {u}; }}")).collect();
    dir.write("app/styles/app.css", &format!("{}\n", rules.join("\n")));
    let s = server(AssetOptions { fingerprint: true, ..options(&dir) });
    let css = get_ok(&s, &s.href("app/styles/app.css").unwrap()).body;
    for url in urls {
        assert!(css.contains(url), "{css}");
    }
}

/// The files in a "Circular CSS imports" message.
fn cycle(message: &str) -> Vec<String> {
    let list = message.strip_prefix("Circular CSS imports are not supported: ").unwrap_or_else(|| panic!("{message}"));
    list.split(" -> ").map(|f| f.rsplit('/').next().unwrap().to_string()).collect()
}

#[test]
fn rejects_self_importing_css_without_fingerprinting() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", "@import \"./app.css\";\nbody { color: black; }\n");
    let s = server(options(&dir));
    assert_eq!(cycle(&s.href("app/styles/app.css").unwrap_err().message), ["app.css", "app.css"]);
}

#[test]
fn rejects_multi_file_css_import_cycles_when_fingerprinting() {
    let dir = Dir::new();
    dir.write("app/styles/a.css", "@import \"./b.css\";\n.a { color: red; }\n");
    dir.write("app/styles/b.css", "@import \"./a.css\";\n.b { color: blue; }\n");
    let s = server(AssetOptions { fingerprint: true, ..options(&dir) });
    assert_eq!(cycle(&s.href("app/styles/b.css").unwrap_err().message), ["b.css", "a.css", "b.css"]);
    assert_eq!(cycle(&s.preloads(&["app/styles/a.css"]).unwrap_err().message), ["a.css", "b.css", "a.css"]);
}

#[test]
fn preloads_accept_multiple_style_roots_and_dedupe_shared_imports() {
    let dir = Dir::new();
    dir.write("app/styles/a.css", "@import \"./shared.css\";\n.a { color: red; }\n");
    dir.write("app/styles/b.css", "@import \"./shared.css\";\n.b { color: blue; }\n");
    dir.write("app/styles/shared.css", "body { color: black; }\n");
    let s = server(options(&dir));
    assert_eq!(
        s.preloads(&["app/styles/a.css", "app/styles/b.css"]).unwrap(),
        ["/assets/app/styles/a.css", "/assets/app/styles/b.css", "/assets/app/styles/shared.css"]
    );
}

// Fingerprints

#[test]
fn href_returns_fingerprinted_urls_for_configured_file_assets_when_fingerprinting() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    let s = server(AssetOptions { fingerprint: true, ..with_files(&dir, files(&[".svg"])) });
    let href = s.href("app/images/logo.svg").unwrap();
    let (plain, hash) = unfingerprint(&href);
    assert_eq!(plain, "/assets/app/images/logo.svg");
    assert_eq!(hash.len(), 6);
    let res = get_ok(&s, &href);
    assert_eq!(header(&res, header::CACHE_CONTROL), IMMUTABLE);
    assert_eq!(header(&res, header::ETAG), format!("W/\"{hash}\""));
    assert!(get(&s, "/assets/app/images/logo.svg").is_none());
}

#[test]
fn uses_immutable_caching_for_fingerprinted_file_requests_and_returns_none_on_mismatch() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    let s = server(AssetOptions { fingerprint: true, ..with_files(&dir, files(&[".svg"])) });
    let res = get_ok(&s, &s.href("app/images/logo.svg").unwrap());
    assert_eq!(header(&res, header::CACHE_CONTROL), IMMUTABLE);
    assert!(!header(&res, header::ETAG).is_empty());
    assert!(get(&s, "/assets/app/images/logo.@wronghash.svg").is_none());
}

#[test]
fn updates_source_file_fingerprints_when_source_files_change() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", "<svg><title>hello</title></svg>\n");
    let s = server(AssetOptions { fingerprint: true, ..with_files(&dir, files(&[".svg"])) });
    let first = s.href("app/images/logo.svg").unwrap();
    assert!(get_ok(&s, &first).body.contains("hello"));
    dir.write("app/images/logo.svg", "<svg><title>world</title></svg>\n");
    let second = s.href("app/images/logo.svg").unwrap();
    assert_ne!(second, first);
    assert!(get_ok(&s, &second).body.contains("world"));
}

#[test]
fn href_returns_fingerprinted_urls_for_transformed_file_assets_when_fingerprinting() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", COLOR_SVG);
    let s = recolor_server(&dir, true);
    let href = s.href_with("app/images/logo.svg", &["recolor:#8B5CF6"]).unwrap();
    assert_eq!(unfingerprint(&href).0, "/assets/app/images/logo.svg?transform=recolor%3A%238B5CF6");
    let res = get_ok(&s, &href);
    assert_eq!(header(&res, header::CACHE_CONTROL), IMMUTABLE);
    assert!(res.body.contains("#8B5CF6"));
    assert!(get(&s, "/assets/app/images/logo.svg?transform=recolor:%238B5CF6").is_none());
}

#[test]
fn returns_none_for_fingerprint_mismatches_on_transformed_file_requests() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", COLOR_SVG);
    let s = recolor_server(&dir, true);
    let res = get_ok(&s, &s.href_with("app/images/logo.svg", &["recolor:#8B5CF6"]).unwrap());
    assert_eq!(header(&res, header::CACHE_CONTROL), IMMUTABLE);
    assert!(get(&s, "/assets/app/images/logo.@wronghash.svg?transform=recolor:%238B5CF6").is_none());
}

#[test]
fn fingerprints_transformed_files_from_their_emitted_bytes() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", COLOR_SVG);
    let s = recolor_server(&dir, true);
    let purple = s.href_with("app/images/logo.svg", &["recolor:#8B5CF6"]).unwrap();
    let red = s.href_with("app/images/logo.svg", &["recolor:#ef4444"]).unwrap();
    let (_, purple_hash) = unfingerprint(&purple);
    let (_, red_hash) = unfingerprint(&red);
    assert_eq!(header(&get_ok(&s, &purple), header::ETAG), format!("W/\"{purple_hash}\""));
    assert_eq!(header(&get_ok(&s, &red), header::ETAG), format!("W/\"{red_hash}\""));
    assert_ne!(purple_hash, red_hash);
}

#[test]
fn fingerprints_css_import_graphs_when_fingerprinting() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", "@import \"./reset.css\";\nbody { color: black; }\n");
    dir.write("app/styles/reset.css", "body { color: red; }\n");
    let s = server(AssetOptions { fingerprint: true, ..options(&dir) });
    let app = s.href("app/styles/app.css").unwrap();
    let reset = s.href("app/styles/reset.css").unwrap();
    assert_eq!(unfingerprint(&app).0, "/assets/app/styles/app.css");
    assert_eq!(unfingerprint(&reset).0, "/assets/app/styles/reset.css");
    assert!(get_ok(&s, &app).body.contains(&format!("@import \"{reset}\";")));
    assert!(get(&s, "/assets/app/styles/app.css").is_none());
    assert_eq!(s.preloads(&["app/styles/app.css"]).unwrap(), [app, reset]);
}

#[test]
fn uses_immutable_caching_for_fingerprinted_style_requests_and_returns_none_on_mismatch() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", "@import \"./reset.css\";\nbody { color: black; }\n");
    dir.write("app/styles/reset.css", "body { color: red; }\n");
    let s = server(AssetOptions { fingerprint: true, ..options(&dir) });
    let app = get_ok(&s, &s.href("app/styles/app.css").unwrap());
    assert_eq!(header(&app, header::CACHE_CONTROL), IMMUTABLE);
    let reset_href = s.href("app/styles/reset.css").unwrap();
    assert!(app.body.contains(&reset_href));
    let reset = get_ok(&s, &reset_href);
    assert_eq!(header(&reset, header::CACHE_CONTROL), IMMUTABLE);
    assert!(!header(&reset, header::ETAG).is_empty());
    assert!(get(&s, "/assets/app/styles/reset.css").is_none());
    assert!(get(&s, "/assets/app/styles/reset.@wronghash.css").is_none());
}

#[test]
fn cascades_fingerprinted_style_graphs_when_dependencies_change() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", "@import \"./mid.css\";\nbody { color: black; }\n");
    dir.write("app/styles/mid.css", "@import \"./leaf.css\";\nbody { color: red; }\n");
    dir.write("app/styles/leaf.css", "body { color: blue; }\n");
    let before = server(AssetOptions { fingerprint: true, ..options(&dir) }).preloads(&["app/styles/app.css"]).unwrap();
    dir.write("app/styles/leaf.css", "body { color: green; }\n");
    let after = server(AssetOptions { fingerprint: true, ..options(&dir) }).preloads(&["app/styles/app.css"]).unwrap();
    assert_ne!(after[1], before[1], "mid.css");
    assert_ne!(after[2], before[2], "leaf.css");
}

#[test]
fn keeps_fingerprinted_style_graphs_stable_across_servers_when_contents_are_unchanged() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", "@import \"./dep.css\";\nbody { color: black; }\n");
    dir.write("app/styles/dep.css", "body { color: red; }\n");
    let a = server(AssetOptions { fingerprint: true, ..options(&dir) });
    let b = server(AssetOptions { fingerprint: true, ..options(&dir) });
    assert_eq!(get_ok(&a, &a.href("app/styles/app.css").unwrap()).body, get_ok(&b, &b.href("app/styles/app.css").unwrap()).body);
}

#[test]
fn uses_immutable_caching_for_fingerprinted_script_requests() {
    let dir = Dir::new();
    dir.write("app/entry.js", "import './dep.js'\nexport const entry = true");
    dir.write("app/dep.js", "export const dep = 1");
    let s = server(AssetOptions { fingerprint: true, ..options(&dir) });
    let entry = get_ok(&s, &s.href("app/entry.js").unwrap());
    let dep = get_ok(&s, &s.href("app/dep.js").unwrap());
    assert_eq!(header(&entry, header::CACHE_CONTROL), IMMUTABLE);
    assert_eq!(header(&dep, header::CACHE_CONTROL), IMMUTABLE);
}

#[test]
fn fingerprints_all_modules_with_filename_fingerprint_ext_urls_and_returns_none_on_mismatch() {
    let dir = Dir::new();
    dir.write("app/entry.js", "import './dep.js'\nexport const entry = true");
    dir.write("app/dep.js", "export const dep = 1");
    let s = server(AssetOptions { fingerprint: true, ..options(&dir) });
    let entry = s.href("app/entry.js").unwrap();
    let dep = s.href("app/dep.js").unwrap();
    let body = get_ok(&s, &entry).body;
    assert_eq!(unfingerprint(&entry).0, "/assets/app/entry.js");
    assert_eq!(unfingerprint(&dep).0, "/assets/app/dep.js");
    // Remix keeps the specifier and maps it with an import map; rmx rewrites it.
    assert!(body.contains(&format!("import '{dep}'")), "{body}");
    let res = get_ok(&s, &dep);
    assert_eq!(res.status, 200);
    assert!(!header(&res, header::ETAG).is_empty());
    assert!(get(&s, "/assets/app/dep.js").is_none());
    assert!(get(&s, "/assets/app/dep.@wronghash.js").is_none());
}

// Hrefs and preloads

#[test]
fn href_accepts_relative_and_absolute_paths_and_file_urls() {
    let dir = Dir::new();
    let entry = dir.write("app/entry.js", "export const entry = true");
    let s = server(AssetOptions { fingerprint: true, ..options(&dir) });
    let href = s.href("app/entry.js").unwrap();
    assert_eq!(unfingerprint(&href).0, "/assets/app/entry.js");
    assert_eq!(s.href(&entry).unwrap(), href);
    let path = entry.display().to_string();
    let path = path.strip_prefix(r"\\?\").unwrap_or(&path).replace('\\', "/");
    let url = format!("file:///{}", path.trim_start_matches('/'));
    assert_eq!(s.href(&url).unwrap(), href);
    assert_eq!(s.href(format!("{url}?tsx-namespace=123")).unwrap(), href);
}

#[test]
fn preloads_are_file_oriented_and_return_the_fingerprinted_root_href_first() {
    let dir = Dir::new();
    dir.write("app/entry.js", "import './a.js'\nimport './b.js'\nexport const entry = true");
    dir.write("app/a.js", "import './c.js'\nexport const a = true");
    dir.write("app/b.js", "export const b = true");
    dir.write("app/c.js", "export const c = true");
    let s = server(AssetOptions { fingerprint: true, ..options(&dir) });
    let urls: Vec<String> = s.preloads(&["app/entry.js"]).unwrap().iter().map(|u| unfingerprint(u).0).collect();
    assert_eq!(urls, ["/assets/app/entry.js", "/assets/app/a.js", "/assets/app/b.js", "/assets/app/c.js"]);
}

#[test]
fn preloads_accept_arbitrary_file_roots() {
    let dir = Dir::new();
    dir.write("app/entry.js", "import './a.js'\nexport const entry = true");
    dir.write("app/a.js", "import './b.js'\nexport const a = true");
    dir.write("app/b.js", "export const b = true");
    let s = server(AssetOptions { fingerprint: true, ..options(&dir) });
    let urls: Vec<String> = s.preloads(&["app/a.js"]).unwrap().iter().map(|u| unfingerprint(u).0).collect();
    assert_eq!(urls, ["/assets/app/a.js", "/assets/app/b.js"]);
}

#[test]
fn preloads_accept_multiple_file_paths_and_dedupe_shared_dependencies() {
    let dir = Dir::new();
    dir.write("app/a.js", "import './shared.js'\nexport const a = true");
    dir.write("app/b.js", "import './shared.js'\nexport const b = true");
    dir.write("app/shared.js", "export const shared = true");
    let s = server(AssetOptions { fingerprint: true, ..options(&dir) });
    let urls: Vec<String> = s.preloads(&["app/a.js", "app/b.js"]).unwrap().iter().map(|u| unfingerprint(u).0).collect();
    assert_eq!(urls, ["/assets/app/a.js", "/assets/app/b.js", "/assets/app/shared.js"]);
}

#[test]
fn preloads_stay_shallowest_first_across_multiple_roots() {
    let dir = Dir::new();
    for root in ["a", "b", "c"] {
        dir.write(&format!("app/{root}.js"), &format!("import './{root}-1.js'\nexport const x = true"));
        dir.write(&format!("app/{root}-1.js"), &format!("import './{root}-2.js'\nexport const x = true"));
        dir.write(&format!("app/{root}-2.js"), "export const x = true");
    }
    let s = server(AssetOptions { fingerprint: true, ..options(&dir) });
    let urls: Vec<String> = s.preloads(&["app/a.js", "app/b.js", "app/c.js"]).unwrap().iter().map(|u| unfingerprint(u).0).collect();
    let expected: Vec<String> = ["a", "b", "c", "a-1", "b-1", "c-1", "a-2", "b-2", "c-2"].iter().map(|n| format!("/assets/app/{n}.js")).collect();
    assert_eq!(urls, expected);
}

#[test]
fn preloads_return_file_asset_roots_as_direct_hrefs() {
    let dir = Dir::new();
    dir.write("app/images/logo.svg", SVG);
    let s = server(AssetOptions { fingerprint: true, ..with_files(&dir, files(&[".svg"])) });
    let urls = s.preloads(&["app/images/logo.svg"]).unwrap();
    assert_eq!(urls.len(), 1);
    assert_eq!(unfingerprint(&urls[0]).0, "/assets/app/images/logo.svg");
}

#[test]
fn preloads_stay_shallowest_first_across_mixed_asset_roots() {
    let dir = Dir::new();
    dir.write("app/a.js", "import './a-1.js'\nexport const a = true");
    dir.write("app/a-1.js", "import './shared.js'\nexport const a1 = true");
    dir.write("app/b.js", "import './b-1.js'\nexport const b = true");
    dir.write("app/b-1.js", "import './shared.js'\nexport const b1 = true");
    dir.write("app/styles/app.css", "@import \"./reset.css\";\n@import \"./shared.css\";\nbody { color: black; }\n");
    dir.write("app/styles/reset.css", "@import \"./shared.css\";\nbody { color: red; }\n");
    dir.write("app/styles/shared.css", "body { color: purple; }\n");
    dir.write("app/images/logo.svg", SVG);
    dir.write("app/shared.js", "export const shared = true");
    let s = server(AssetOptions { fingerprint: true, ..with_files(&dir, files(&[".svg"])) });
    let urls: Vec<String> =
        s.preloads(&["app/a.js", "app/styles/app.css", "app/images/logo.svg", "app/b.js"]).unwrap().iter().map(|u| unfingerprint(u).0).collect();
    assert_eq!(
        urls,
        [
            "/assets/app/a.js",
            "/assets/app/b.js",
            "/assets/app/styles/app.css",
            "/assets/app/images/logo.svg",
            "/assets/app/a-1.js",
            "/assets/app/b-1.js",
            "/assets/app/styles/reset.css",
            "/assets/app/styles/shared.css",
            "/assets/app/shared.js",
        ]
    );
}

#[test]
fn preloads_reject_script_request_urls() {
    let dir = Dir::new();
    dir.write("app/entry.js", "import './dep.js'\nexport const entry = true");
    dir.write("app/dep.js", "export const dep = 1");
    let s = server(AssetOptions { fingerprint: true, ..options(&dir) });
    let error = s.preloads(&["/assets/app/dep.@abc123.js"]).unwrap_err();
    assert!(error.message.starts_with("File not found:"), "{}", error.message);
    let error = s.preloads(&["app/entry.js", "/assets/app/dep.@abc123.js"]).unwrap_err();
    assert!(error.message.starts_with("File not found:"), "{}", error.message);
}

#[test]
fn href_rejects_modules_outside_the_allowed_files() {
    let dir = Dir::new();
    dir.write("other.js", "export const value = 1");
    let s = server(options(&dir));
    assert!(s.href("other.js").unwrap_err().message.contains("not allowed by the asset server access configuration"));
}

#[test]
fn preloads_reject_denied_modules() {
    let dir = Dir::new();
    dir.write("app/entry.js", "export const entry = true");
    let s = server(AssetOptions { deny_files: vec!["app/entry.js".into()], ..options(&dir) });
    let error = s.preloads(&["app/entry.js"]).unwrap_err();
    assert_eq!(error.code, AssetErrorCode::FileNotAllowed);
    assert!(error.message.contains("not allowed by the asset server access configuration"));
}

// Mounts and access rules

#[test]
fn supports_absolute_entry_point_paths() {
    let dir = Dir::new();
    let entry = dir.write("app/entry-abs.js", "export const abs = true");
    let s = server(AssetOptions { fingerprint: true, ..options(&dir) });
    assert_eq!(get_ok(&s, &s.href(&entry).unwrap()).status, 200);
}

#[test]
fn treats_an_empty_base_path_as_root() {
    let dir = Dir::new();
    dir.write("app/entry.js", "export const value = 1");
    let s = server(AssetOptions { base_path: String::new(), ..options(&dir) });
    assert_eq!(get_ok(&s, "/app/entry.js").status, 200);
    assert_eq!(s.href("app/entry.js").unwrap(), "/app/entry.js");
}

#[test]
fn rejects_absolute_mount_file_roots() {
    let dir = Dir::new();
    let app = dir.0.join("app").display().to_string();
    let error = AssetServer::new(AssetOptions { mounts: vec![("app".into(), app)], fingerprint: true, ..options(&dir) }).err().unwrap();
    assert!(error.message.starts_with("mounts values must be relative to rootDir"), "{}", error.message);
}

#[test]
fn rejects_empty_mounts_during_startup() {
    let dir = Dir::new();
    let error = AssetServer::new(AssetOptions { mounts: vec![], ..options(&dir) }).err().unwrap();
    assert_eq!(error.message, "mounts must include at least one entry");
}

#[test]
fn supports_mount_url_roots_with_multiple_path_segments() {
    let dir = Dir::new();
    dir.write("packages/runtime/entry.js", "export const value = true");
    let s = server(AssetOptions {
        allow_files: vec!["packages/runtime/**".into()],
        mounts: vec![("/internal/runtime/".into(), "packages/runtime".into())],
        ..options(&dir)
    });
    let href = s.href("packages/runtime/entry.js").unwrap();
    assert_eq!(href, "/assets/internal/runtime/entry.js");
    assert_eq!(get_ok(&s, &href).status, 200);
}

fn mount_error(dir: &Dir, mounts: &[(&str, &str)]) -> String {
    let mounts = mounts.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    AssetServer::new(AssetOptions { mounts, ..options(dir) }).err().expect("rejected").message
}

#[test]
fn rejects_incompatible_overlapping_mount_url_roots_during_startup() {
    let dir = Dir::new();
    assert_eq!(mount_error(&dir, &[("app", "app"), ("app/routes", "routes")]), "mounts keys must not overlap. Received \"app\" and \"app/routes\".");
}

#[test]
fn rejects_compatible_but_redundant_overlapping_mounts_during_startup() {
    let dir = Dir::new();
    let error = mount_error(&dir, &[("app", "app"), ("app/vendor", "app/vendor")]);
    assert!(error.starts_with("mounts values must not overlap. Received \"app\" and \"app/vendor\""), "{error}");
}

#[test]
fn rejects_overlapping_mount_file_roots_during_startup() {
    let dir = Dir::new();
    let error = mount_error(&dir, &[("app", "app"), ("routes", "app/routes")]);
    assert!(error.starts_with("mounts values must not overlap. Received \"app\" and \"app/routes\""), "{error}");
}

#[test]
fn supports_glob_style_allow_files_and_deny_files_rules() {
    let dir = Dir::new();
    dir.write("app/features/allowed.js", "export const allowed = true");
    dir.write("app/features/private/blocked.js", "export const blocked = true");
    let s = server(AssetOptions { allow_files: vec!["app/**/*.js".into()], deny_files: vec!["app/**/private/**".into()], ..options(&dir) });
    assert_eq!(get_ok(&s, "/assets/app/features/allowed.js").status, 200);
    assert!(get(&s, "/assets/app/features/private/blocked.js").is_none());
}

#[test]
fn matches_dot_prefixed_files_and_directories_in_glob_style_allow_files_rules() {
    let dir = Dir::new();
    dir.write("app/.dotfile.js", "export const dotfile = true");
    dir.write("app/.dotdir/example/index.js", "export const dotdir = true");
    let s = server(AssetOptions { allow_files: vec!["app/**/*".into()], ..options(&dir) });
    assert_eq!(get_ok(&s, "/assets/app/.dotfile.js").status, 200);
    assert_eq!(get_ok(&s, "/assets/app/.dotdir/example/index.js").status, 200);
}

#[test]
fn does_not_call_on_error_for_denied_direct_requests() {
    let dir = Dir::new();
    dir.write("app/blocked.js", "export const blocked = true");
    let errors = Errors::default();
    let s = server(AssetOptions { deny_files: vec!["app/blocked.js".into()], on_error: errors.handler(), ..options(&dir) });
    assert!(get(&s, "/assets/app/blocked.js").is_none());
    assert_eq!(errors.count(), 0);
}

// Option validation

fn files_error(dir: &Dir, files: FilesOptions) -> String {
    AssetServer::new(with_files(dir, files)).err().expect("rejected").message
}

#[test]
fn rejects_files_extensions_for_compiled_asset_types() {
    let dir = Dir::new();
    assert!(files_error(&dir, files(&[".css"])).starts_with("files.extensions cannot include compiled asset extensions"));
}

#[test]
fn rejects_files_extensions_for_script_module_types() {
    let dir = Dir::new();
    assert!(files_error(&dir, files(&[".ts"])).starts_with("files.extensions cannot include compiled asset extensions"));
}

#[test]
fn rejects_non_positive_files_max_request_transforms_values() {
    let dir = Dir::new();
    assert_eq!(files_error(&dir, FilesOptions { max_request_transforms: 0, ..files(&[".svg"]) }), "files.max_request_transforms must be a positive integer");
}

#[test]
fn rejects_empty_files_cache_key_values() {
    let dir = Dir::new();
    assert_eq!(files_error(&dir, FilesOptions { cache_key: Some(String::new()), ..files(&[".svg"]) }), "files.cache_key must be a non-empty string");
}

#[test]
fn rejects_invalid_request_transform_extension_filters() {
    let dir = Dir::new();
    let error = files_error(&dir, FilesOptions { transforms: vec![("optimize".into(), identity().extensions(&["svg"]))], ..files(&[".svg"]) });
    assert!(error.starts_with("files.transforms.optimize.extensions values must use \".ext\" format"), "{error}");
}

#[test]
fn rejects_empty_global_transform_extension_filters() {
    let dir = Dir::new();
    let global = GlobalTransform::new(|bytes, _| Ok(Some(bytes.into()))).extensions(&[]);
    let error = files_error(&dir, FilesOptions { global_transforms: vec![global], ..files(&[".svg"]) });
    assert_eq!(error, "files.global_transforms[0].extensions must include at least one extension");
}

// Errors

#[test]
fn calls_on_error_for_unsupported_imported_file_types() {
    let dir = Dir::new();
    dir.write("app/entry.js", "import './data.json'\nexport const entry = true");
    dir.write("app/data.json", "{\"ok\":true}");
    let errors = Errors::default();
    let s = server(AssetOptions { on_error: errors.handler(), ..options(&dir) });
    let res = get_ok(&s, "/assets/app/entry.js");
    assert_eq!(res.status, 500);
    assert_eq!(res.body, "Internal Server Error");
    let error = errors.last();
    assert_eq!(error.code, AssetErrorCode::ImportNotSupported);
    assert!(error.message.contains("not a supported script file"));
    assert!(error.message.contains("\"./data.json\""));
    assert!(error.message.contains("app/entry.js") && error.message.contains("app/data.json"), "{}", error.message);
}

#[test]
fn calls_on_error_for_import_resolution_failures() {
    let dir = Dir::new();
    dir.write("app/entry.js", "import './missing.js'\nexport const entry = true");
    let errors = Errors::default();
    let s = server(AssetOptions { on_error: errors.handler(), ..options(&dir) });
    assert_eq!(get_ok(&s, "/assets/app/entry.js").status, 500);
    let error = errors.last();
    assert_eq!(error.code, AssetErrorCode::ImportResolutionFailed);
    assert!(error.message.contains("Failed to resolve import") && error.message.contains("\"./missing.js\""), "{}", error.message);
}

#[test]
fn calls_on_error_for_css_import_resolution_failures() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", "@import \"./missing.css\";\nbody { color: red; }\n");
    let errors = Errors::default();
    let s = server(AssetOptions { on_error: errors.handler(), ..options(&dir) });
    assert_eq!(get_ok(&s, "/assets/app/styles/app.css").status, 500);
    let error = errors.last();
    assert_eq!(error.code, AssetErrorCode::ImportResolutionFailed);
    assert!(error.message.contains("Failed to resolve import") && error.message.contains("\"./missing.css\""), "{}", error.message);
}

#[test]
fn calls_on_error_for_unsupported_css_url_dependencies_when_files_are_not_configured() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", "body { background-image: url(\"../images/logo.svg\"); }\n");
    dir.write("app/images/logo.svg", SVG);
    let errors = Errors::default();
    let s = server(AssetOptions { on_error: errors.handler(), ..options(&dir) });
    assert_eq!(get_ok(&s, "/assets/app/styles/app.css").status, 500);
    let error = errors.last();
    assert_eq!(error.code, AssetErrorCode::UrlNotSupported);
    assert!(error.message.contains("files.extensions") && error.message.contains("\"../images/logo.svg\""), "{}", error.message);
}

#[test]
fn calls_on_error_for_css_url_resolution_failures() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", "body { background-image: url(\"../images/missing.svg\"); }\n");
    let errors = Errors::default();
    let s = server(AssetOptions { on_error: errors.handler(), ..with_files(&dir, files(&[".svg"])) });
    assert_eq!(get_ok(&s, "/assets/app/styles/app.css").status, 500);
    let error = errors.last();
    assert_eq!(error.code, AssetErrorCode::UrlResolutionFailed);
    assert!(error.message.contains("Failed to resolve url") && error.message.contains("\"../images/missing.svg\""), "{}", error.message);
}

/// A stylesheet whose `url()` requests an invalid transform still works: the request is
/// left as written (Remix also logs a warning), and the browser gets the 400 for it.
fn css_transform_request(transform_query: &str, transforms: Vec<(String, FileTransform)>) {
    let dir = Dir::new();
    dir.write("app/styles/app.css", &format!("body {{ background-image: url(\"../images/logo.svg?{transform_query}\"); }}\n"));
    dir.write("app/images/logo.svg", SVG);
    let errors = Errors::default();
    let s = server(AssetOptions { on_error: errors.handler(), ..with_files(&dir, FilesOptions { transforms, ..files(&[".svg"]) }) });
    let res = get_ok(&s, "/assets/app/styles/app.css");
    assert_eq!(res.status, 200, "{}", res.body);
    assert_eq!(errors.count(), 0);
    assert!(res.body.contains(&format!("url(\"/assets/app/images/logo.svg?{transform_query}\")")), "{}", res.body);
    assert_eq!(get_ok(&s, &format!("/assets/app/images/logo.svg?{transform_query}")).status, 400);
}

#[test]
fn keeps_malformed_css_authored_file_transform_queries_without_failing_the_stylesheet() {
    css_transform_request("transform=wat", vec![("optimize".into(), identity())]);
}

#[test]
fn keeps_invalid_css_authored_file_transform_requests_without_failing_the_stylesheet() {
    css_transform_request("transform=recolor", vec![("recolor".into(), identity().param(Param::Required))]);
}

#[test]
fn keeps_over_limit_css_authored_file_transform_requests_without_failing_the_stylesheet() {
    css_transform_request(&["transform=optimize"; 6].join("&"), vec![("optimize".into(), identity())]);
}

#[test]
fn calls_on_error_for_disallowed_css_imports() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", "@import \"../../secret.css\";\nbody { color: red; }\n");
    dir.write("secret.css", "body { color: black; }\n");
    let errors = Errors::default();
    let s = server(AssetOptions { on_error: errors.handler(), ..options(&dir) });
    assert_eq!(get_ok(&s, "/assets/app/styles/app.css").status, 500);
    let error = errors.last();
    assert_eq!(error.code, AssetErrorCode::ImportNotAllowed);
    assert!(error.message.contains("not allowed by the asset server access configuration"));
    assert!(error.message.contains("allow_files") && error.message.contains("deny_files"));
    assert!(error.message.contains("\"../../secret.css\"") && error.message.contains("app/styles/app.css") && error.message.contains("secret.css"));
}

#[test]
fn calls_on_error_for_disallowed_css_url_dependencies() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", "body { background-image: url(\"../../secret.svg\"); }\n");
    dir.write("secret.svg", SVG);
    let errors = Errors::default();
    let s = server(AssetOptions { on_error: errors.handler(), ..with_files(&dir, files(&[".svg"])) });
    assert_eq!(get_ok(&s, "/assets/app/styles/app.css").status, 500);
    let error = errors.last();
    assert_eq!(error.code, AssetErrorCode::UrlNotAllowed);
    assert!(error.message.contains("not allowed by the asset server access configuration") && error.message.contains("\"../../secret.svg\""));
}

#[test]
fn calls_on_error_when_a_css_import_is_outside_configured_mounts() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", "@import \"../../shared/reset.css\";\nbody { color: red; }\n");
    dir.write("shared/reset.css", "body { color: black; }\n");
    let errors = Errors::default();
    let s = server(AssetOptions { allow_files: vec!["app/**".into(), "shared/**".into()], on_error: errors.handler(), ..options(&dir) });
    assert_eq!(get_ok(&s, "/assets/app/styles/app.css").status, 500);
    let error = errors.last();
    assert_eq!(error.code, AssetErrorCode::ImportOutsideMounts);
    assert!(error.message.contains("outside all configured mounts") && error.message.contains("\"../../shared/reset.css\""), "{}", error.message);
}

#[test]
fn calls_on_error_when_a_css_url_dependency_is_outside_configured_mounts() {
    let dir = Dir::new();
    dir.write("app/styles/app.css", "body { background-image: url(\"../../shared/logo.svg\"); }\n");
    dir.write("shared/logo.svg", SVG);
    let errors = Errors::default();
    let s = server(AssetOptions { allow_files: vec!["app/**".into(), "shared/**".into()], on_error: errors.handler(), ..with_files(&dir, files(&[".svg"])) });
    assert_eq!(get_ok(&s, "/assets/app/styles/app.css").status, 500);
    let error = errors.last();
    assert_eq!(error.code, AssetErrorCode::UrlOutsideMounts);
    assert!(error.message.contains("outside all configured mounts") && error.message.contains("\"../../shared/logo.svg\""), "{}", error.message);
}

#[test]
fn calls_on_error_for_disallowed_imported_scripts() {
    let dir = Dir::new();
    dir.write("app/entry.js", "import '../secret.js'\nexport const entry = true");
    dir.write("secret.js", "export const secret = true");
    let errors = Errors::default();
    let s = server(AssetOptions { on_error: errors.handler(), ..options(&dir) });
    assert_eq!(get_ok(&s, "/assets/app/entry.js").status, 500);
    let error = errors.last();
    assert_eq!(error.code, AssetErrorCode::ImportNotAllowed);
    assert!(error.message.contains("not allowed by the asset server access configuration") && error.message.contains("\"../secret.js\""));
}

#[test]
fn calls_on_error_when_an_imported_module_is_outside_configured_mounts() {
    let dir = Dir::new();
    dir.write("app/entry.js", "import '../shared/util.js'\nexport const entry = util");
    dir.write("shared/util.js", "export const util = true");
    let errors = Errors::default();
    let s = server(AssetOptions { allow_files: vec!["app/**".into(), "shared/**".into()], on_error: errors.handler(), ..options(&dir) });
    assert_eq!(get_ok(&s, "/assets/app/entry.js").status, 500);
    let error = errors.last();
    assert_eq!(error.code, AssetErrorCode::ImportOutsideMounts);
    assert!(error.message.contains("outside all configured mounts") && error.message.contains("\"../shared/util.js\""));
}

#[test]
fn uses_a_custom_response_returned_from_on_error() {
    let dir = Dir::new();
    dir.write("app/entry.js", "import './missing.js'\nexport const entry = true");
    let s = server(AssetOptions {
        on_error: Some(Arc::new(|_| Some(rmx_response::status(StatusCode::IM_A_TEAPOT, "Custom build error")))),
        ..options(&dir)
    });
    let res = get_ok(&s, "/assets/app/entry.js");
    assert_eq!(res.status, 418);
    assert_eq!(res.body, "Custom build error");
}
