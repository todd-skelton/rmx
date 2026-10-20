//! Serve files from a directory, falling through to the next handler when no file matches.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use rmx_fetch::{Body, Response};
use rmx_router::{Context, Middleware, Next};

/// Options for [`static_files`] and [`file_response`].
#[derive(Clone)]
pub struct StaticOptions {
    /// `Cache-Control` for served files (default: none).
    pub cache_control: Option<String>,
    /// Send a weak `ETag` and honor `If-None-Match` (default: true).
    pub etag: bool,
    /// Send `Last-Modified` and honor `If-Modified-Since` (default: true).
    pub last_modified: bool,
    /// Serve `index.html` for directory requests (default: true).
    pub index: bool,
    /// Return `false` to refuse a relative path (for example hidden files).
    pub filter: Option<PathFilter>,
}

/// Decides whether a relative path may be served.
pub type PathFilter = Arc<dyn Fn(&str) -> bool + Send + Sync>;

impl Default for StaticOptions {
    fn default() -> Self {
        StaticOptions { cache_control: None, etag: true, last_modified: true, index: true, filter: None }
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

/// A full-featured file response: content type, ETag/Last-Modified with conditional
/// requests, single byte ranges, and HEAD.
pub fn file_response(path: &Path, req_headers: &HeaderMap, head: bool, options: &StaticOptions) -> std::io::Result<Response> {
    let meta = std::fs::metadata(path)?;
    let len = meta.len();
    let modified = meta.modified().ok();
    let etag = modified.map(|m| {
        let secs = m.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        format!("W/\"{len:x}-{secs:x}\"")
    });
    let last_modified = modified.map(crate::http_date::format);

    let mut res = Response::new(Body::empty());
    let headers = res.headers_mut();
    let content_type = rmx_mime::from_path(&path.to_string_lossy());
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Some(cc) = options.cache_control.as_deref().and_then(|v| HeaderValue::from_str(v).ok()) {
        headers.insert(header::CACHE_CONTROL, cc);
    }
    if options.etag {
        if let Some(v) = etag.as_deref().and_then(|v| HeaderValue::from_str(v).ok()) {
            headers.insert(header::ETAG, v);
        }
    }
    if options.last_modified {
        if let Some(v) = last_modified.as_deref().and_then(|v| HeaderValue::from_str(v).ok()) {
            headers.insert(header::LAST_MODIFIED, v);
        }
    }

    let header_str = |name: header::HeaderName| req_headers.get(name).and_then(|v| v.to_str().ok());
    let not_modified = if let (true, Some(inm), Some(tag)) = (options.etag, header_str(header::IF_NONE_MATCH), etag.as_deref()) {
        inm.split(',').any(|t| t.trim() == "*" || t.trim().trim_start_matches("W/") == tag.trim_start_matches("W/"))
    } else if let (true, Some(ims), Some(lm)) = (options.last_modified, header_str(header::IF_MODIFIED_SINCE), last_modified.as_deref()) {
        ims == lm
    } else {
        false
    };
    if not_modified {
        *res.status_mut() = StatusCode::NOT_MODIFIED;
        return Ok(res);
    }

    let range = header_str(header::RANGE).and_then(|r| parse_range(r, len));
    let (start, end) = match range {
        Some(Ok(r)) => {
            *res.status_mut() = StatusCode::PARTIAL_CONTENT;
            let v = format!("bytes {}-{}/{len}", r.0, r.1);
            res.headers_mut().insert(header::CONTENT_RANGE, HeaderValue::from_str(&v).expect("ascii"));
            r
        }
        Some(Err(())) => {
            *res.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
            let v = format!("bytes */{len}");
            res.headers_mut().insert(header::CONTENT_RANGE, HeaderValue::from_str(&v).expect("ascii"));
            return Ok(res);
        }
        None => (0, len.saturating_sub(1)),
    };
    let size = if len == 0 { 0 } else { end - start + 1 };
    res.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    if !head && size > 0 {
        let bytes = std::fs::read(path)?;
        *res.body_mut() = Body::from(bytes[start as usize..=end as usize].to_vec());
    }
    Ok(res)
}

/// Parse one `bytes=a-b` range. Multiple ranges are served as the full file.
fn parse_range(value: &str, len: u64) -> Option<Result<(u64, u64), ()>> {
    let spec = value.strip_prefix("bytes=")?;
    if spec.contains(',') || len == 0 {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            (len.saturating_sub(n), len - 1)
        }
        (a, "") => (a.parse().ok()?, len - 1),
        (a, b) => (a.parse().ok()?, b.parse::<u64>().ok()?.min(len - 1)),
    };
    Some(if start > end || start >= len { Err(()) } else { Ok((start, end)) })
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
        let ranged = get(&router, "/app.css", &[("range", "bytes=1-3")]);
        assert_eq!(ranged.status(), 206);
        assert_eq!(block_on(ranged.into_body().text()).unwrap(), "ody");
        assert_eq!(block_on(get(&router, "/sub/", &[]).into_body().text()).unwrap(), "<p>i</p>");
        assert_eq!(block_on(get(&router, "/", &[]).into_body().text()).unwrap(), "home");
        assert_eq!(get(&router, "/../Cargo.toml", &[]).status(), 404);
        assert_eq!(get(&router, "/%2e%2e/secret", &[]).status(), 404);
    }
}
