//! Serve files from a directory (Remix's `remix/middleware/static`), and file responses with
//! conditional requests and byte ranges (`remix/response/file`).

use std::cmp::Ordering;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use rmx_fetch::{response, Body, Bytes, File, Response};
use rmx_headers::{format_http_date, parse_http_date, unix_seconds, ContentRange, IfMatch, IfNoneMatch, IfRange, Range};
use rmx_html_template::escape_html;
use rmx_router::{Context, Middleware, Next};

/// Which `ETag` to send.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ETag {
    /// No `ETag`; `If-Match` and `If-None-Match` are ignored.
    Off,
    /// `W/"<size>-<lastModified ms>"`, computed without reading the file (the default).
    #[default]
    Weak,
    /// A quoted digest of the content: SHA-256 hex unless [`StaticOptions::digest`] is set.
    Strong,
}

/// Facts about a file passed to an [`AcceptRanges::With`] function.
#[derive(Clone, Copy, Debug)]
pub struct FileInfo<'a> {
    /// The path relative to the static root, or the file's own name.
    pub name: &'a str,
    /// The MIME type without parameters; empty when unknown.
    pub mime_type: &'a str,
    pub size: u64,
    pub last_modified: Option<SystemTime>,
}

/// Whether to serve byte ranges.
#[derive(Clone, Default)]
pub enum AcceptRanges {
    /// Only for types that are not compressible, since compressed responses cannot be
    /// sliced (the default).
    #[default]
    Auto,
    Always,
    Never,
    /// Decide per file.
    With(Arc<dyn Fn(&FileInfo) -> bool + Send + Sync>),
}

/// Computes a strong `ETag` value (without quotes) from the file content.
pub type DigestFn = Arc<dyn Fn(&[u8]) -> String + Send + Sync>;

/// Decides whether a relative path may be served.
pub type PathFilter = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Options for [`static_files`] and [`file_response`].
#[derive(Clone)]
pub struct StaticOptions {
    /// `Cache-Control` for served files (default: none).
    pub cache_control: Option<String>,
    /// Default [`ETag::Weak`].
    pub etag: ETag,
    /// The digest for [`ETag::Strong`]; SHA-256 hex when `None`.
    pub digest: Option<DigestFn>,
    /// Send `Last-Modified` and honor `If-Modified-Since`/`If-Unmodified-Since` (default: true).
    pub last_modified: bool,
    pub accept_ranges: AcceptRanges,
    /// Files to try, in order, for a directory request (default: `index.html`, `index.htm`).
    /// Empty disables index files.
    pub index: Vec<String>,
    /// List a directory's entries when no index file matches (default: false).
    pub list_files: bool,
    /// Return `false` to refuse a relative path (for example hidden files).
    pub filter: Option<PathFilter>,
}

impl Default for StaticOptions {
    fn default() -> Self {
        StaticOptions {
            cache_control: None,
            etag: ETag::Weak,
            digest: None,
            last_modified: true,
            accept_ranges: AcceptRanges::Auto,
            index: vec!["index.html".into(), "index.htm".into()],
            list_files: false,
            filter: None,
        }
    }
}

/// Middleware serving `GET`/`HEAD` requests from `root`. Anything it cannot serve (a missing
/// file, a filtered path, a path that escapes the root, also through symlinks) falls through
/// to the next handler.
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
            if options.filter.as_ref().is_some_and(|filter| !filter(&rel)) {
                return next.run(ctx).await;
            }
            let head = ctx.method() == Method::HEAD;
            let Some(requested) = join(&root, &rel) else { return next.run(ctx).await };
            let Some(real) = contained(&root, &requested) else { return next.run(ctx).await };
            if real.is_file() {
                if let Ok(res) = path_response(&real, &rel, ctx.headers(), head, &options) {
                    return res;
                }
            } else if real.is_dir() {
                for index in &options.index {
                    let Some(index_real) = contained(&root, &requested.join(index)) else { continue };
                    if index_real.is_file() {
                        let name = if rel.is_empty() { index.clone() } else { format!("{}/{index}", rel.trim_end_matches('/')) };
                        if let Ok(res) = path_response(&index_real, &name, ctx.headers(), head, &options) {
                            return res;
                        }
                    }
                }
                if options.list_files {
                    return directory_listing(&real, ctx.url().pathname());
                }
            }
            next.run(ctx).await
        }
    }
}

/// Join a URL path onto `root` lexically, refusing `..`, absolute paths and prefixes.
fn join(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    rel.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir)).then(|| root.join(rel))
}

/// The real path of `path` when it exists and stays inside `root` after resolving symlinks.
fn contained(root: &Path, path: &Path) -> Option<PathBuf> {
    path.canonicalize().ok().filter(|real| real.starts_with(root))
}

fn path_response(real: &Path, name: &str, req_headers: &HeaderMap, head: bool, options: &StaticOptions) -> io::Result<Response> {
    let meta = std::fs::metadata(real)?;
    let mime_type = rmx_mime::detect_mime_type(name).unwrap_or_default();
    let info = FileInfo { name, mime_type, size: meta.len(), last_modified: meta.modified().ok() };
    let read = |start: u64, len: u64| -> io::Result<Body> {
        let mut file = std::fs::File::open(real)?;
        file.seek(SeekFrom::Start(start))?;
        Ok(Body::from_reader(file.take(len)))
    };
    let contents = || std::fs::read(real).map(Bytes::from);
    respond_with_file(&info, read, contents, req_headers, head, options)
}

/// A file response with Remix's `createFileResponse` semantics: content type and `nosniff`,
/// an `ETag` and `Last-Modified`, `If-Match`/`If-Unmodified-Since` (412),
/// `If-None-Match`/`If-Modified-Since` (304), and single byte ranges with `If-Range` (206,
/// 400 for malformed ranges, 416 for unsatisfiable or multipart ones). The body is read from
/// disk as it is sent. The type comes from the path's extension; unknown types get no
/// `Content-Type`.
pub fn file_response(path: &Path, req_headers: &HeaderMap, head: bool, options: &StaticOptions) -> io::Result<Response> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path_response(path, &name, req_headers, head, options)
}

/// [`file_response`] for a [`File`] held in memory, such as one from file storage
/// (`createFileResponse(file, request)`). The type comes from the file, or its name when
/// it has none.
pub fn file_response_for(file: &File, req_headers: &HeaderMap, head: bool, options: &StaticOptions) -> Response {
    let mime_type = if file.content_type.is_empty() { rmx_mime::detect_mime_type(&file.name).unwrap_or_default() } else { file.content_type.as_str() };
    let last_modified = UNIX_EPOCH.checked_add(Duration::from_millis(file.last_modified));
    let info = FileInfo { name: &file.name, mime_type, size: file.size() as u64, last_modified };
    let read = |start: u64, len: u64| Ok(Body::from(file.bytes.slice(start as usize..(start + len) as usize)));
    let contents = || Ok(file.bytes.clone());
    respond_with_file(&info, read, contents, req_headers, head, options).expect("reading memory cannot fail")
}

fn respond_with_file(
    file: &FileInfo,
    read_range: impl Fn(u64, u64) -> io::Result<Body>,
    contents: impl Fn() -> io::Result<Bytes>,
    req_headers: &HeaderMap,
    head: bool,
    options: &StaticOptions,
) -> io::Result<Response> {
    let size = file.size;
    let content_type = (!file.mime_type.is_empty()).then(|| rmx_mime::to_content_type(file.mime_type));
    let modified = file.last_modified.filter(|_| options.last_modified);
    let etag = match options.etag {
        ETag::Off => None,
        ETag::Weak => {
            let ms = file.last_modified.and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis());
            Some(format!("W/\"{size}-{ms}\""))
        }
        ETag::Strong => {
            let bytes = contents()?;
            let digest = match &options.digest {
                Some(digest) => digest(&bytes),
                None => rmx_crypto::hex(&rmx_crypto::sha256(&bytes)),
            };
            Some(format!("\"{digest}\""))
        }
    };
    let accept_ranges = match &options.accept_ranges {
        AcceptRanges::Auto => !rmx_mime::is_compressible(content_type.as_deref().unwrap_or_default()),
        AcceptRanges::Always => true,
        AcceptRanges::Never => false,
        AcceptRanges::With(decide) => decide(file),
    };
    let get = |name: HeaderName| req_headers.get(name).and_then(|v| v.to_str().ok());

    // Remix's buildResponseHeaders: `full` adds the representation headers, `validators`
    // the caching ones; nosniff is always set.
    let respond = |status: StatusCode, body: Body, full: bool, validators: bool| {
        let mut res = Response::new(body);
        *res.status_mut() = status;
        let h = res.headers_mut();
        h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
        let mut set = |name: HeaderName, value: Option<String>| {
            if let Some(v) = value.and_then(|v| HeaderValue::from_str(&v).ok()) {
                h.insert(name, v);
            }
        };
        if full {
            set(header::CONTENT_TYPE, content_type.clone());
            set(header::CACHE_CONTROL, options.cache_control.clone());
        }
        if validators {
            set(header::ETAG, etag.clone());
            set(header::LAST_MODIFIED, modified.map(format_http_date));
            set(header::ACCEPT_RANGES, accept_ranges.then(|| "bytes".to_string()));
        }
        res
    };
    let failed = || respond(StatusCode::PRECONDITION_FAILED, Body::from("Precondition Failed"), false, true);

    let has_if_match = req_headers.contains_key(header::IF_MATCH);
    if let (Some(tag), true) = (&etag, has_if_match) {
        if !IfMatch::parse(get(header::IF_MATCH)).matches(tag) {
            return Ok(failed());
        }
    }
    if let (Some(m), false) = (modified, has_if_match) {
        if get(header::IF_UNMODIFIED_SINCE).and_then(parse_http_date).is_some_and(|since| unix_seconds(m) > unix_seconds(since)) {
            return Ok(failed());
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
        return Ok(respond(StatusCode::NOT_MODIFIED, Body::empty(), false, true));
    }

    let read = |start: u64, len: u64| -> io::Result<Body> {
        if head || len == 0 {
            return Ok(Body::empty());
        }
        read_range(start, len)
    };
    if accept_ranges && !head && req_headers.contains_key(header::RANGE) {
        let range = Range::parse(get(header::RANGE));
        if range.ranges.is_empty() {
            return Ok(respond(StatusCode::BAD_REQUEST, Body::from("Bad Request"), false, false));
        }
        if IfRange::parse(get(header::IF_RANGE)).matches(etag.as_deref(), modified) {
            let [(start, end)] = range.normalize(size)[..] else {
                let mut res = respond(StatusCode::RANGE_NOT_SATISFIABLE, Body::from("Range Not Satisfiable"), false, false);
                res.headers_mut().insert(header::CONTENT_RANGE, HeaderValue::from_str(&ContentRange::unsatisfied(size).to_string()).expect("ascii"));
                return Ok(res);
            };
            let mut res = respond(StatusCode::PARTIAL_CONTENT, read(start, end - start + 1)?, true, true);
            res.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from(end - start + 1));
            res.headers_mut().insert(header::CONTENT_RANGE, HeaderValue::from_str(&ContentRange::bytes(start, end, size).to_string()).expect("ascii"));
            return Ok(res);
        }
    }
    let mut res = respond(StatusCode::OK, read(0, size)?, true, true);
    res.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    Ok(res)
}

struct Entry {
    name: String,
    is_dir: bool,
    size: u64,
    mime_type: &'static str,
}

/// An HTML index of `dir`'s immediate entries: folders first, then files in natural order.
fn directory_listing(dir: &Path, pathname: &str) -> Response {
    let Ok(read) = std::fs::read_dir(dir) else { return response::status(StatusCode::INTERNAL_SERVER_ERROR, "Error reading directory") };
    let mut entries: Vec<Entry> = read
        .flatten()
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            let (size, mime_type) = if is_dir {
                (0, "")
            } else {
                let size = std::fs::metadata(entry.path()).map_or(0, |m| m.len());
                (size, rmx_mime::detect_mime_type(&name).unwrap_or("application/octet-stream"))
            };
            Entry { name, is_dir, size, mime_type }
        })
        .collect();
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| natural_cmp(&a.name, &b.name)));

    let mut rows = String::new();
    let mut row = |href: &str, icon: &str, label: &str, size: &str, kind: &str| {
        rows.push_str(&format!(
            r#"<tr class="file-row"><td class="name-cell"><a href="{}">{icon} {}</a></td><td class="size-cell">{size}</td><td class="type-cell">{}</td></tr>"#,
            escape_html(href),
            escape_html(label),
            escape_html(kind)
        ));
    };
    if pathname != "/" && !pathname.is_empty() {
        let trimmed = pathname.strip_suffix('/').unwrap_or(pathname);
        let parent = trimmed.rsplit_once('/').map_or("", |(parent, _)| parent);
        row(if parent.is_empty() { "/" } else { parent }, FOLDER_ICON, "..", "", "");
    }
    for entry in &entries {
        let href = if pathname.ends_with('/') { format!("{pathname}{}", entry.name) } else { format!("{pathname}/{}", entry.name) };
        let (icon, size, kind) = if entry.is_dir { (FOLDER_ICON, String::new(), "Folder") } else { (FILE_ICON, format_size(entry.size), entry.mime_type) };
        row(&href, icon, &entry.name, &size, kind);
    }
    let title = escape_html(pathname);
    response::html(format!(
        r#"<html lang="en"><head><meta charset="utf-8" /><meta name="viewport" content="width=device-width, initial-scale=1" /><title>Index of {title}</title><style>{LISTING_CSS}</style></head><body><div class="container"><h1>Index of {title}</h1><table><thead><tr><th class="name">Name</th><th class="size">Size</th><th class="type">Type</th></tr></thead><tbody>{rows}</tbody></table></div></body></html>"#
    ))
}

/// `localeCompare(b, undefined, { numeric: true })`, approximately: digit runs compare as
/// numbers, letters ignore case first.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    fn digits(it: &mut std::iter::Peekable<std::str::Chars>) -> String {
        let mut s = String::new();
        while let Some(c) = it.next_if(char::is_ascii_digit) {
            s.push(c);
        }
        s.trim_start_matches('0').to_string()
    }
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let (m, n) = (digits(&mut x), digits(&mut y));
                let order = m.len().cmp(&n.len()).then_with(|| m.cmp(&n));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(c), Some(d)) => {
                x.next();
                y.next();
                let order = c.to_lowercase().cmp(d.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".into();
    }
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    let i = ((bytes as f64).ln() / 1024f64.ln()).floor().min(4.0) as usize;
    let size = bytes as f64 / 1024f64.powi(i as i32);
    if i == 0 {
        format!("{size:.0} {}", UNITS[i])
    } else {
        format!("{size:.1} {}", UNITS[i])
    }
}

const FOLDER_ICON: &str = r#"<svg class="icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M2 3.5h4.5l1.5 1.5h6v8h-12z" /></svg>"#;
const FILE_ICON: &str = r#"<svg class="icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M3 2h7l3 3v9H3z" /><path d="M10 2v3h3" /></svg>"#;
const LISTING_CSS: &str = "* { box-sizing: border-box; } \
body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, 'Open Sans', 'Helvetica Neue', sans-serif; margin: 0; padding: 2rem 1rem; background: #fff; color: #333; font-size: 14px; } \
.container { max-width: 1200px; margin: 0 auto; } \
h1 { margin: 0 0 1rem 0; padding: 0; font-size: 1.25rem; font-weight: 600; color: #333; } \
.icon { width: 14px; height: 14px; display: inline-block; vertical-align: text-top; margin-right: 0.5rem; color: #666; } \
table { width: 100%; border-collapse: collapse; border: 1px solid #e0e0e0; } \
thead tr { background: #fafafa; } \
th { padding: 0.5rem 1rem; text-align: left; font-weight: 600; font-size: 0.8125rem; color: #666; border-bottom: 1px solid #e0e0e0; } \
td { border-bottom: 1px solid #f0f0f0; } \
tr:last-child td { border-bottom: none; } \
.file-row { position: relative; } \
.file-row:hover { background: #fafafa; } \
.name-cell { width: 50%; padding: 0; } \
.name-cell a { display: block; padding: 0.375rem 1rem; color: #0066cc; text-decoration: none; } \
.name-cell a::after { content: ''; position: absolute; top: 0; left: 0; right: 0; bottom: 0; } \
.file-row:hover .name-cell a { text-decoration: underline; } \
.size-cell { width: 25%; padding: 0.375rem 1rem; text-align: left; color: #666; font-variant-numeric: tabular-nums; } \
.type-cell { width: 25%; padding: 0.375rem 1rem; color: #666; white-space: nowrap; } \
@media (max-width: 768px) { body { padding: 1rem 0.5rem; } h1 { font-size: 1.125rem; margin-bottom: 0.75rem; } thead { display: none; } \
.name-cell a, .size-cell, .type-cell { padding: 0.375rem 0.75rem; } .type-cell { display: none; } .name-cell { width: 60%; } .size-cell { width: 40%; text-align: right; } } \
@media (max-width: 480px) { body { font-size: 13px; } .name-cell a, .size-cell, .type-cell { padding: 0.375rem 0.5rem; } h1 { font-size: 1rem; } .icon { width: 12px; height: 12px; } }";

#[cfg(test)]
mod tests;
