//! Serve files from a directory (Remix's `remix/middleware/static`). Each file is opened as a
//! [LazyFile] and answered by [response::file] (Remix's `createFileResponse`), so conditional
//! requests and byte ranges behave exactly as for any other file response.

use std::cmp::Ordering;
use std::ffi::OsStr;
use std::fs::Metadata;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use http::{Method, StatusCode};
use rsmix_response as response;
use rsmix_response::FileResponseOptions;
use rsmix_fetch::Response;
use rsmix_fs::{open_lazy_file, OpenLazyFileOptions};
use rsmix_html_template::escape_html;
use rsmix_lazy_file::LazyFile;
use rsmix_fetch_router::{Context, Middleware, Next};

pub use rsmix_response::{Digest, DigestFn, ETag};

/// Decides per file whether to serve byte ranges (Remix's `AcceptRangesFunction`).
pub type AcceptRangesFn = Arc<dyn Fn(&LazyFile) -> bool + Send + Sync>;

/// Whether to serve byte ranges.
#[derive(Clone, Default)]
pub enum AcceptRanges {
    /// Only for types that are not compressible, since compressed responses cannot be
    /// sliced (the default).
    #[default]
    Auto,
    Always,
    Never,
    /// Decide per file. The file's name is its path relative to the root, and its type comes
    /// from the requested path, also when that is a symlink.
    With(AcceptRangesFn),
}

/// Decides whether a relative path may be served.
pub type PathFilter = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Options for [static_files] (Remix's `StaticFilesOptions`): the [FileResponseOptions] plus
/// the directory options.
#[derive(Clone)]
pub struct StaticOptions {
    /// `Cache-Control` for served files (default: none).
    pub cache_control: Option<String>,
    /// Default [ETag::Weak].
    pub etag: ETag,
    /// The digest for [ETag::Strong] (default: SHA-256).
    pub digest: Digest<LazyFile>,
    /// Send `Last-Modified` and honor `If-Modified-Since`/`If-Unmodified-Since` (default: true).
    pub last_modified: bool,
    pub accept_ranges: AcceptRanges,
    /// Files to try, in order, for a directory request (default: `index.html`, `index.htm`).
    /// Empty disables index files (Remix's `index: false`).
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
            digest: Digest::default(),
            last_modified: true,
            accept_ranges: AcceptRanges::Auto,
            index: vec!["index.html".into(), "index.htm".into()],
            list_files: false,
            filter: None,
        }
    }
}

/// Middleware serving `GET`/`HEAD` requests from `root`. Anything it cannot serve (a missing
/// file, a filtered path, a path that escapes the root, also through symlinks, or a file that
/// fails to read) falls through to the next handler.
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
            let rel = rsmix_url::decode(ctx.url().pathname().trim_start_matches('/'));
            if options.filter.as_ref().is_some_and(|filter| !filter(&rel)) {
                return next.run(ctx).await;
            }
            if !lexically_inside(&rel) {
                return next.run(ctx).await;
            }
            let Some((real, meta)) = resolve(&root, root.to_path_buf(), Path::new(&*rel)) else { return next.run(ctx).await };
            let mut found = None;
            if meta.is_file() {
                found = Some((real, rel.to_string()));
            } else if meta.is_dir() {
                for index in &options.index {
                    let Some((index_real, index_meta)) = resolve(&root, real.clone(), Path::new(index)) else { continue };
                    if index_meta.is_file() {
                        let name = if rel.is_empty() { index.clone() } else { format!("{}/{index}", rel.trim_end_matches('/')) };
                        found = Some((index_real, name));
                        break;
                    }
                }
                if found.is_none() && options.list_files {
                    return directory_listing(&real, ctx.url().pathname());
                }
            }
            if let Some((file, file_options)) = found.and_then(|(real, name)| open(&real, &name, &options)) {
                // The response future borrows the request only while it is created.
                let response = response::file(&file, ctx.request(), &file_options);
                if let Ok(res) = response.await {
                    return res;
                }
            }
            next.run(ctx).await
        }
    }
}

/// Whether `rel` names something below the root without leaving it lexically: no `..`, no
/// absolute path or prefix.
fn lexically_inside(rel: &str) -> bool {
    Path::new(rel).components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// `rel` below `base` (`root` itself or a path already resolved inside it), with its
/// metadata, when it exists and stays inside `root` (canonical) after following symlinks.
///
/// Each component is checked with one `symlink_metadata` call, so a missing path costs one
/// stat at the first missing name rather than resolving the whole path from `/` (what
/// `canonicalize` does, and what Remix's `realpath` does twice per request). Only a symlink,
/// or on Windows a name the system may rewrite, is resolved with `canonicalize` and checked
/// against `root`, as every path used to be.
fn resolve(root: &Path, base: PathBuf, rel: &Path) -> Option<(PathBuf, Metadata)> {
    let mut path = base;
    let mut meta = None;
    let mut components = rel.components();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else { continue };
        path.push(name);
        let slow = needs_resolving(name) || {
            let m = std::fs::symlink_metadata(&path).ok()?;
            let link = m.file_type().is_symlink();
            meta = Some(m);
            link
        };
        if slow {
            path.extend(components);
            return resolve_slow(root, &path);
        }
    }
    let meta = match meta {
        Some(meta) => meta,
        None => std::fs::metadata(&path).ok()?,
    };
    Some((path, meta))
}

/// `canonicalize` and the containment check, for paths through a symlink (or a junction).
fn resolve_slow(root: &Path, path: &Path) -> Option<(PathBuf, Metadata)> {
    let real = path.canonicalize().ok().filter(|real| real.starts_with(root))?;
    let meta = std::fs::metadata(&real).ok()?;
    Some((real, meta))
}

/// Windows rewrites some names before looking them up: trailing dots and spaces are dropped,
/// `:` opens an alternate data stream and `~` may be a short (8.3) name. Those go through
/// `canonicalize` so the containment check sees the name the system will open.
#[cfg(windows)]
fn needs_resolving(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    name.ends_with(['.', ' ']) || name.contains([':', '~'])
}

#[cfg(not(windows))]
fn needs_resolving(_: &OsStr) -> bool {
    false
}

/// The file at `real` as a [LazyFile] named and typed by the requested path `name`, with the
/// file response options for it.
fn open(real: &Path, name: &str, options: &StaticOptions) -> Option<(LazyFile, FileResponseOptions<LazyFile>)> {
    let content_type = rsmix_mime::detect_mime_type(name).unwrap_or_default().to_string();
    let file = open_lazy_file(real, OpenLazyFileOptions { name: Some(name.to_string()), content_type: Some(content_type), last_modified: None }).ok()?;
    let accept_ranges = match &options.accept_ranges {
        AcceptRanges::Auto => None,
        AcceptRanges::Always => Some(true),
        AcceptRanges::Never => Some(false),
        AcceptRanges::With(decide) => Some(decide(&file)),
    };
    let file_options = FileResponseOptions {
        cache_control: options.cache_control.clone(),
        etag: options.etag,
        digest: options.digest.clone(),
        last_modified: options.last_modified,
        accept_ranges,
    };
    Some((file, file_options))
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
                (size, rsmix_mime::detect_mime_type(&name).unwrap_or("application/octet-stream"))
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
