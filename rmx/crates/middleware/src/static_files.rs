//! Serve files from a directory, falling through to the next handler when no file matches.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use std::time::UNIX_EPOCH;

use http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use rmx_fetch::{Body, Response};
use rmx_headers::{format_http_date, parse_http_date, unix_seconds, ContentRange, IfMatch, IfNoneMatch, IfRange, Range};
use rmx_router::{Context, Middleware, Next};

/// Options for [`static_files`] and [`file_response`].
#[derive(Clone)]
pub struct StaticOptions {
    /// `Cache-Control` for served files (default: none).
    pub cache_control: Option<String>,
    /// Send a weak `ETag` and honor `If-Match`/`If-None-Match` (default: true).
    pub etag: bool,
    /// Send `Last-Modified` and honor `If-Modified-Since`/`If-Unmodified-Since` (default: true).
    pub last_modified: bool,
    /// Serve byte ranges; `None` (default) offers them for non-compressible types only,
    /// since compressed responses cannot be sliced.
    pub accept_ranges: Option<bool>,
    /// Serve `index.html` for directory requests (default: true).
    pub index: bool,
    /// Return `false` to refuse a relative path (for example hidden files).
    pub filter: Option<PathFilter>,
}

/// Decides whether a relative path may be served.
pub type PathFilter = Arc<dyn Fn(&str) -> bool + Send + Sync>;

impl Default for StaticOptions {
    fn default() -> Self {
        StaticOptions { cache_control: None, etag: true, last_modified: true, accept_ranges: None, index: true, filter: None }
    }
}

/// Middleware serving `GET`/`HEAD` requests from `root`.
pub fn static_files(root: impl Into<PathBuf>, options: StaticOptions) -> impl Middleware {
    let root: PathBuf = root.into();
    let root = Arc::new(root.canonicalize().unwrap_or(root));
    let options = Arc::new(options);
    move |ctx: Context, next: Next| {
        let root = root.clone();
        let options = options.clone();
        async move {
            if ctx.method() != Method::GET && ctx.method() != Method::HEAD {
                return next.run(ctx).await;
            }
            let rel = rmx_url::decode(ctx.url().pathname().trim_start_matches('/'));
            if let Some(filter) = &options.filter {
                if !filter(&rel) {
                    return next.run(ctx).await;
                }
            }
            let Some(mut path) = safe_join(&root, &rel) else { return next.run(ctx).await };
            if path.is_dir() {
                if !options.index {
                    return next.run(ctx).await;
                }
                path.push("index.html");
            }
            if !path.is_file() {
                return next.run(ctx).await;
            }
            match file_response(&path, ctx.headers(), ctx.method() == Method::HEAD, &options) {
                Ok(res) => res,
                Err(_) => next.run(ctx).await,
            }
        }
    }
}

/// Join a URL path onto `root`, refusing `..`, absolute paths and symlink escapes.
fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) && !rel.as_os_str().is_empty() {
        return None;
    }
    let joined = root.join(rel);
    match joined.canonicalize() {
        Ok(real) if real.starts_with(root) => Some(real),
        Ok(_) => None,
        Err(_) => Some(joined), // does not exist; caller checks is_file
    }
}

/// A file response with Remix's `createFileResponse` semantics: content type and
/// `nosniff`, a weak `ETag` and `Last-Modified`, `If-Match`/`If-Unmodified-Since` (412),
/// `If-None-Match`/`If-Modified-Since` (304), and single byte ranges with `If-Range`
/// (206, 400 for malformed ranges, 416 for unsatisfiable or multipart ones). Ranges are
/// offered for non-compressible types unless [`StaticOptions::accept_ranges`] says otherwise.
pub fn file_response(path: &Path, req_headers: &HeaderMap, head: bool, options: &StaticOptions) -> std::io::Result<Response> {
    let meta = std::fs::metadata(path)?;
    let size = meta.len();
    let modified = options.last_modified.then(|| meta.modified().ok()).flatten();
    let etag = options.etag.then(|| {
        let ms = meta.modified().ok().and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis());
        format!("W/\"{size}-{ms}\"")
    });
    let content_type = rmx_mime::from_path(&path.to_string_lossy());
    let accept_ranges = options.accept_ranges.unwrap_or_else(|| !rmx_mime::is_compressible(content_type));
    let get = |name: header::HeaderName| req_headers.get(name).and_then(|v| v.to_str().ok());

    let respond = |status: StatusCode, body: Body, full: bool| {
        let mut res = Response::new(body);
        *res.status_mut() = status;
        let h = res.headers_mut();
        h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
        let mut set = |name: header::HeaderName, value: Option<String>| {
            if let Some(v) = value.and_then(|v| HeaderValue::from_str(&v).ok()) {
                h.insert(name, v);
            }
        };
        if full {
            set(header::CONTENT_TYPE, Some(content_type.to_string()));
            set(header::CACHE_CONTROL, options.cache_control.clone());
        }
        set(header::ETAG, etag.clone());
        set(header::LAST_MODIFIED, modified.map(format_http_date));
        set(header::ACCEPT_RANGES, accept_ranges.then(|| "bytes".to_string()));
        res
    };
    let text = |status: StatusCode, msg: &'static str| respond(status, Body::from(msg), false);

    let has_if_match = req_headers.contains_key(header::IF_MATCH);
    if let (Some(tag), true) = (&etag, has_if_match) {
        if !IfMatch::parse(get(header::IF_MATCH)).matches(tag) {
            return Ok(text(StatusCode::PRECONDITION_FAILED, "Precondition Failed"));
        }
    }
    if let (Some(m), false) = (modified, has_if_match) {
        if let Some(since) = get(header::IF_UNMODIFIED_SINCE).and_then(parse_http_date) {
            if unix_seconds(m) > unix_seconds(since) {
                return Ok(text(StatusCode::PRECONDITION_FAILED, "Precondition Failed"));
            }
        }
    }
    let if_none_match = IfNoneMatch::parse(get(header::IF_NONE_MATCH));
    let not_modified = match (&etag, modified) {
        (Some(tag), _) if if_none_match.matches(tag) => true,
        (_, Some(m)) if if_none_match.tags.is_empty() => {
            get(header::IF_MODIFIED_SINCE).and_then(parse_http_date).is_some_and(|since| unix_seconds(m) <= unix_seconds(since))
        }
        _ => false,
    };
    if not_modified {
        return Ok(respond(StatusCode::NOT_MODIFIED, Body::empty(), false));
    }

    let read = |start: u64, end: u64| -> std::io::Result<Body> {
        if head || size == 0 {
            return Ok(Body::empty());
        }
        let bytes = std::fs::read(path)?;
        Ok(Body::from(bytes[start as usize..=end as usize].to_vec()))
    };
    if accept_ranges && !head && req_headers.contains_key(header::RANGE) {
        let range = Range::parse(get(header::RANGE));
        if range.ranges.is_empty() {
            return Ok(text(StatusCode::BAD_REQUEST, "Bad Request"));
        }
        if IfRange::parse(get(header::IF_RANGE)).matches(etag.as_deref(), modified) {
            let ranges = range.normalize(size);
            let [(start, end)] = ranges[..] else {
                let mut res = text(StatusCode::RANGE_NOT_SATISFIABLE, "Range Not Satisfiable");
                res.headers_mut().insert(header::CONTENT_RANGE, HeaderValue::from_str(&ContentRange::unsatisfied(size).to_string()).expect("ascii"));
                return Ok(res);
            };
            let mut res = respond(StatusCode::PARTIAL_CONTENT, read(start, end)?, true);
            res.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from(end - start + 1));
            res.headers_mut().insert(header::CONTENT_RANGE, HeaderValue::from_str(&ContentRange::bytes(start, end, size).to_string()).expect("ascii"));
            return Ok(res);
        }
    }
    let mut res = respond(StatusCode::OK, read(0, size.saturating_sub(1))?, true);
    res.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmx_fetch::block_on;
    use rmx_router::Router;

    fn setup() -> (PathBuf, Router) {
        let dir = std::env::temp_dir().join(format!("rmx-static-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("app.css"), "body{}").unwrap();
        std::fs::write(dir.join("logo.png"), b"0123456789").unwrap();
        std::fs::write(dir.join("sub").join("index.html"), "<p>i</p>").unwrap();
        let mut router = Router::new().with(static_files(&dir, StaticOptions::default()));
        router.get("/", |_: Context| async { "home" });
        (dir, router)
    }

    fn get(router: &Router, path: &str, headers: &[(&str, &str)]) -> Response {
        let mut b = http::Request::builder().uri(format!("http://x{path}"));
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        block_on(router.fetch(b.body(Body::empty()).unwrap()))
    }

    #[test]
    fn serves_files_conditionally_and_falls_through() {
        let (_dir, router) = setup();
        let res = get(&router, "/app.css", &[]);
        assert_eq!(res.status(), 200);
        assert_eq!(res.headers()["content-type"], "text/css; charset=utf-8");
        let etag = res.headers()["etag"].to_str().unwrap().to_string();
        assert_eq!(block_on(res.into_body().text()).unwrap(), "body{}");
        assert_eq!(get(&router, "/app.css", &[("if-none-match", &etag)]).status(), 304);
        let css = get(&router, "/app.css", &[("range", "bytes=1-3")]);
        assert_eq!((css.status().as_u16(), css.headers().get("accept-ranges")), (200, None), "compressible types are not ranged by default");
        assert_eq!(block_on(get(&router, "/sub/", &[]).into_body().text()).unwrap(), "<p>i</p>");
        assert_eq!(block_on(get(&router, "/", &[]).into_body().text()).unwrap(), "home");
        assert_eq!(get(&router, "/../Cargo.toml", &[]).status(), 404);
        assert_eq!(get(&router, "/%2e%2e/secret", &[]).status(), 404);
    }

    #[test]
    fn conditional_requests_and_ranges() {
        let (_dir, router) = setup();
        let res = get(&router, "/logo.png", &[]);
        assert_eq!(res.headers()["accept-ranges"], "bytes");
        assert_eq!(res.headers()["x-content-type-options"], "nosniff");
        let etag = res.headers()["etag"].to_str().unwrap().to_string();
        let modified = res.headers()["last-modified"].to_str().unwrap().to_string();

        let part = get(&router, "/logo.png", &[("range", "bytes=2-4")]);
        assert_eq!(part.status(), 206);
        assert_eq!(part.headers()["content-range"], "bytes 2-4/10");
        assert_eq!(block_on(part.into_body().text()).unwrap(), "234");
        assert_eq!(block_on(get(&router, "/logo.png", &[("range", "bytes=-3")]).into_body().text()).unwrap(), "789");
        assert_eq!(get(&router, "/logo.png", &[("range", "bytes=0-1,4-5")]).status(), 416, "no multipart ranges");
        let past_end = get(&router, "/logo.png", &[("range", "bytes=20-")]);
        assert_eq!((past_end.status().as_u16(), past_end.headers()["content-range"].to_str().unwrap()), (416, "bytes */10"));
        assert_eq!(get(&router, "/logo.png", &[("range", "lines=x")]).status(), 400);
        assert_eq!(get(&router, "/logo.png", &[("range", "bytes=2-4"), ("if-range", "\"other\"")]).status(), 200, "stale If-Range sends everything");
        assert_eq!(get(&router, "/logo.png", &[("range", "bytes=2-4"), ("if-range", &modified)]).status(), 206);

        assert_eq!(get(&router, "/logo.png", &[("if-match", "\"nope\"")]).status(), 412);
        assert_eq!(get(&router, "/logo.png", &[("if-unmodified-since", "Thu, 01 Jan 1970 00:00:00 GMT")]).status(), 412);
        assert_eq!(get(&router, "/logo.png", &[("if-modified-since", &modified)]).status(), 304);
        assert_eq!(get(&router, "/logo.png", &[("if-none-match", &etag), ("if-modified-since", "Thu, 01 Jan 1970 00:00:00 GMT")]).status(), 304, "If-None-Match wins");
        assert_eq!(get(&router, "/logo.png", &[("if-none-match", "\"x\""), ("if-modified-since", &modified)]).status(), 200);
    }
}
