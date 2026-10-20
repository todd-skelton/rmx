//! Browser assets served straight from source directories (Remix's `remix/assets`).
//!
//! An [`AssetServer`] maps public URLs under a base path onto mounted directories, serves
//! only files the allow and deny globs permit, and rewrites references between assets so
//! every URL it hands out is current:
//!
//! - CSS `url(...)` and `@import` references, and relative module specifiers in JavaScript
//!   (`import`, `import()`, `new URL('...', import.meta.url)`, as wasm-bindgen emits), become
//!   asset URLs.
//! - With `fingerprint: true`, URLs carry a content hash (`app.@k3j2h1x9.css`) and are served
//!   as immutable; otherwise they are revalidated with an `ETag`.
//! - Named request transforms run on demand (`image.svg?transform=recolor:8B5CF6`), after
//!   the global transforms that run for every file of their extensions.
//!
//! Files are read when requested, so edits show up on the next request without restarting.
//!
//! ```ignore
//! let assets = AssetServer::new(AssetOptions {
//!     root_dir: ".".into(),
//!     base_path: "/assets".into(),
//!     mounts: vec![("app".into(), "app".into())],
//!     allow_files: vec!["app/**/public/**".into()],
//!     ..Default::default()
//! })?;
//! let href = assets.href("app/public/styles/app.css")?;
//! router.get("/assets/*path", move |ctx: Context| { let assets = assets.clone(); async move {
//!     assets.fetch(&ctx).unwrap_or_else(response::not_found)
//! }});
//! ```

mod glob;
mod rewrite;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use http::{header, HeaderValue, Method, StatusCode};
use rmx_fetch::{Body, Response};
use rmx_router::Context;

pub use glob::glob_match;

/// A request transform's parameter: none, required (`name:param`) or optional.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Param {
    #[default]
    None,
    Required,
    Optional,
}

/// The output of a transform: new bytes, and optionally a new extension (`.webp`).
pub struct Transformed {
    pub content: Vec<u8>,
    pub extension: Option<String>,
}

impl From<Vec<u8>> for Transformed {
    fn from(content: Vec<u8>) -> Self {
        Transformed { content, extension: None }
    }
}

impl From<String> for Transformed {
    fn from(content: String) -> Self {
        Transformed { content: content.into_bytes(), extension: None }
    }
}

type TransformFn = dyn Fn(Vec<u8>, Option<&str>) -> Result<Transformed, String> + Send + Sync;

/// A named transform requested from asset URLs (`defineFileTransform`).
#[derive(Clone)]
pub struct FileTransform {
    /// Extensions (`.svg`) this transform accepts; empty accepts any.
    pub extensions: Vec<String>,
    pub param: Param,
    pub transform: Arc<TransformFn>,
}

impl FileTransform {
    pub fn new(
        extensions: &[&str],
        param: Param,
        transform: impl Fn(Vec<u8>, Option<&str>) -> Result<Transformed, String> + Send + Sync + 'static,
    ) -> FileTransform {
        FileTransform { extensions: extensions.iter().map(|e| e.to_string()).collect(), param, transform: Arc::new(transform) }
    }
}

/// A transform that runs for every served file with one of its extensions.
#[derive(Clone)]
pub struct GlobalTransform {
    pub extensions: Vec<String>,
    pub transform: Arc<dyn Fn(Vec<u8>) -> Result<Transformed, String> + Send + Sync>,
}

impl GlobalTransform {
    pub fn new(extensions: &[&str], transform: impl Fn(Vec<u8>) -> Result<Transformed, String> + Send + Sync + 'static) -> Self {
        GlobalTransform { extensions: extensions.iter().map(|e| e.to_string()).collect(), transform: Arc::new(transform) }
    }
}

/// Options for [`AssetServer::new`].
#[derive(Clone)]
pub struct AssetOptions {
    /// Relative paths in the other options resolve from here.
    pub root_dir: PathBuf,
    /// URL prefix, like `/assets`.
    pub base_path: String,
    /// `(url segment, directory)` pairs: `("app", "app")` serves `app/x.css` at
    /// `/assets/app/x.css`.
    pub mounts: Vec<(String, String)>,
    /// Globs (relative to `root_dir`) of files that may be served. Required.
    pub allow_files: Vec<String>,
    /// Globs that are never served, even when allowed.
    pub deny_files: Vec<String>,
    /// Put content hashes in URLs and serve them as immutable.
    pub fingerprint: bool,
    pub transforms: Vec<(String, FileTransform)>,
    pub global_transforms: Vec<GlobalTransform>,
    /// Most transforms one URL may request.
    pub max_request_transforms: usize,
}

impl Default for AssetOptions {
    fn default() -> Self {
        AssetOptions {
            root_dir: PathBuf::from("."),
            base_path: "/assets".into(),
            mounts: vec![("app".into(), "app".into())],
            allow_files: Vec::new(),
            deny_files: Vec::new(),
            fingerprint: false,
            transforms: Vec::new(),
            global_transforms: Vec::new(),
            max_request_transforms: 5,
        }
    }
}

/// Why an asset could not be served or linked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetError {
    /// Bad options, found by [`AssetServer::new`].
    Config(String),
    /// No mount contains the file, or the access rules refuse it.
    NotServable(String),
    NotFound(String),
    /// An unknown or invalid transform request.
    BadTransform(String),
    /// A transform failed.
    TransformFailed(String),
}

impl fmt::Display for AssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssetError::Config(m) => write!(f, "invalid asset options: {m}"),
            AssetError::NotServable(m) => write!(f, "not a servable asset: {m}"),
            AssetError::NotFound(m) => write!(f, "asset not found: {m}"),
            AssetError::BadTransform(m) => write!(f, "invalid transform: {m}"),
            AssetError::TransformFailed(m) => write!(f, "transform failed: {m}"),
        }
    }
}

impl std::error::Error for AssetError {}

/// A module script entry: its URL and the module URLs to `modulepreload`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptEntry {
    pub href: String,
    /// Every module the entry imports, shallowest first (the entry included).
    pub preloads: Vec<String>,
}

/// Serves assets; cheap to clone.
#[derive(Clone)]
pub struct AssetServer(Arc<Inner>);

struct Inner {
    root: PathBuf,
    base: String,
    /// `(url root, directory)`, longest URL root first.
    mounts: Vec<(String, PathBuf)>,
    allow: Vec<String>,
    deny: Vec<String>,
    fingerprint: bool,
    transforms: HashMap<String, FileTransform>,
    global: Vec<GlobalTransform>,
    max_transforms: usize,
    cache: Mutex<HashMap<(PathBuf, Vec<String>), Built>>,
}

/// A file after transforms and reference rewriting.
#[derive(Clone)]
struct Built {
    modified: Option<SystemTime>,
    len: u64,
    /// `(file, transforms, hash)` of every asset this one links to.
    deps: Vec<(PathBuf, Vec<String>, String)>,
    /// Module specifiers this file imports (for preloads).
    imports: Vec<(PathBuf, Vec<String>)>,
    body: Arc<Vec<u8>>,
    extension: String,
    hash: String,
}

impl AssetServer {
    pub fn new(options: AssetOptions) -> Result<AssetServer, AssetError> {
        if options.allow_files.is_empty() {
            return Err(AssetError::Config("allow_files must list the files that may be served".into()));
        }
        let root = options.root_dir.canonicalize().map_err(|e| AssetError::Config(format!("root_dir: {e}")))?;
        let base = format!("/{}", options.base_path.trim_matches('/'));
        let mut mounts = Vec::new();
        for (url, dir) in &options.mounts {
            let dir_path = root.join(dir);
            let dir_path = dir_path.canonicalize().unwrap_or(dir_path);
            let url_root = format!("{base}/{}", url.trim_matches('/')).trim_end_matches('/').to_string();
            if mounts.iter().any(|(u, d): &(String, PathBuf)| overlaps(u, &url_root) || d.starts_with(&dir_path) || dir_path.starts_with(d)) {
                return Err(AssetError::Config(format!("mount {url} overlaps another mount")));
            }
            mounts.push((url_root, dir_path));
        }
        mounts.sort_by_key(|(u, _)| std::cmp::Reverse(u.len()));
        let mut transforms = HashMap::new();
        for (name, t) in options.transforms {
            if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                return Err(AssetError::Config(format!("transform names use letters, digits and '-': {name}")));
            }
            transforms.insert(name, t);
        }
        Ok(AssetServer(Arc::new(Inner {
            root,
            base,
            mounts,
            allow: options.allow_files,
            deny: options.deny_files,
            fingerprint: options.fingerprint,
            transforms,
            global: options.global_transforms,
            max_transforms: options.max_request_transforms,
            cache: Mutex::new(HashMap::new()),
        })))
    }

    /// The URL for a file (absolute, or relative to `root_dir`).
    pub fn href(&self, file: impl AsRef<Path>) -> Result<String, AssetError> {
        self.href_with(file, &[])
    }

    /// The URL for a file with request transforms applied (`"recolor:8B5CF6"`).
    pub fn href_with(&self, file: impl AsRef<Path>, transforms: &[&str]) -> Result<String, AssetError> {
        let path = self.resolve(file.as_ref());
        let transforms: Vec<String> = transforms.iter().map(|t| t.to_string()).collect();
        self.0.href(&path, &transforms, &mut HashSet::new())
    }

    /// The URL of a module entry and the modules it imports (`getScriptEntry`).
    pub fn script_entry(&self, file: impl AsRef<Path>) -> Result<ScriptEntry, AssetError> {
        let path = self.resolve(file.as_ref());
        let href = self.0.href(&path, &[], &mut HashSet::new())?;
        let mut preloads = Vec::new();
        let mut seen = HashSet::new();
        let mut queue = std::collections::VecDeque::from([(path, Vec::<String>::new())]);
        while let Some((file, transforms)) = queue.pop_front() {
            if !seen.insert(file.clone()) {
                continue;
            }
            let built = self.0.build(&file, &transforms, &mut HashSet::new())?;
            if is_script(&built.extension) {
                preloads.push(self.0.href(&file, &transforms, &mut HashSet::new())?);
                queue.extend(built.imports.iter().cloned());
            }
        }
        Ok(ScriptEntry { href, preloads })
    }

    /// Serve an asset request. `None` when the URL is outside the base path (let the router
    /// continue); otherwise a response, `404` for files that are missing or not allowed.
    pub fn fetch(&self, ctx: &Context) -> Option<Response> {
        let head = *ctx.method() == Method::HEAD;
        if !head && *ctx.method() != Method::GET {
            return None;
        }
        let url = ctx.url();
        let pathname = url.pathname();
        if !pathname.starts_with(&format!("{}/", self.0.base)) {
            return None;
        }
        let (pathname, fingerprint) = parse_fingerprint(pathname);
        let Some(file) = self.0.file_for_url(&pathname) else { return Some(status(StatusCode::NOT_FOUND, "Not found")) };
        let transforms: Vec<String> = url.search_params().get_all("transform").into_iter().map(str::to_string).collect();
        let built = match self.0.build(&file, &transforms, &mut HashSet::new()) {
            Ok(b) => b,
            Err(AssetError::BadTransform(m)) | Err(AssetError::TransformFailed(m)) => return Some(status(StatusCode::BAD_REQUEST, &m)),
            Err(_) => return Some(status(StatusCode::NOT_FOUND, "Not found")),
        };
        if fingerprint.as_deref().is_some_and(|f| f != built.hash) {
            // A stale fingerprinted URL: never serve other content as immutable.
            return Some(status(StatusCode::NOT_FOUND, "Not found"));
        }
        let etag = format!("W/\"{}\"", built.hash);
        let cache_control = if fingerprint.is_some() { "public, max-age=31536000, immutable" } else { "no-cache" };
        let mut res = if ctx.header("if-none-match").is_some_and(|v| v.split(',').any(|t| t.trim() == etag)) {
            let mut res = Response::new(Body::empty());
            *res.status_mut() = StatusCode::NOT_MODIFIED;
            res
        } else {
            let mut res = Response::new(if head { Body::empty() } else { Body::from((*built.body).clone()) });
            let ct = rmx_mime::from_extension(built.extension.trim_start_matches('.'));
            if let Ok(v) = HeaderValue::from_str(ct) {
                res.headers_mut().insert(header::CONTENT_TYPE, v);
            }
            res.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from(built.body.len()));
            res
        };
        if let Ok(v) = HeaderValue::from_str(&etag) {
            res.headers_mut().insert(header::ETAG, v);
        }
        res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
        Some(res)
    }

    fn resolve(&self, file: &Path) -> PathBuf {
        let path = if file.is_absolute() { file.to_path_buf() } else { self.0.root.join(file) };
        path.canonicalize().unwrap_or(path)
    }
}

impl Inner {
    fn href(&self, file: &Path, transforms: &[String], visiting: &mut HashSet<PathBuf>) -> Result<String, AssetError> {
        let mut url = self.url_for_file(file).ok_or_else(|| AssetError::NotServable(file.display().to_string()))?;
        if !self.allowed(file) {
            return Err(AssetError::NotServable(file.display().to_string()));
        }
        if self.fingerprint && !visiting.contains(file) {
            let built = self.build(file, transforms, visiting)?;
            url = format_fingerprint(&url, &built.hash);
        }
        if !transforms.is_empty() {
            let mut params = rmx_url::SearchParams::new();
            for t in transforms {
                params.append("transform", t);
            }
            url = format!("{url}?{params}");
        }
        Ok(url)
    }

    fn relative(&self, file: &Path) -> Option<String> {
        let rel = file.strip_prefix(&self.root).ok()?;
        let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        Some(parts.join("/"))
    }

    fn allowed(&self, file: &Path) -> bool {
        let Some(rel) = self.relative(file) else { return false };
        self.allow.iter().any(|g| glob_match(g, &rel)) && !self.deny.iter().any(|g| glob_match(g, &rel))
    }

    fn url_for_file(&self, file: &Path) -> Option<String> {
        self.mounts.iter().find_map(|(url_root, dir)| {
            let rel = file.strip_prefix(dir).ok()?;
            let segments: Vec<String> = rel.components().map(|c| encode_segment(&c.as_os_str().to_string_lossy())).collect();
            Some(format!("{url_root}/{}", segments.join("/")))
        })
    }

    fn file_for_url(&self, pathname: &str) -> Option<PathBuf> {
        let (url_root, dir) = self.mounts.iter().find(|(u, _)| pathname.starts_with(&format!("{u}/")))?;
        let mut path = dir.clone();
        for segment in pathname[url_root.len() + 1..].split('/') {
            let segment = rmx_url::decode(segment);
            if segment.is_empty() || segment == "." || segment == ".." || segment.contains(['\\', '/', ':']) {
                return None;
            }
            path.push(segment);
        }
        if !matches!(path.strip_prefix(dir).ok()?.components().next(), Some(Component::Normal(_))) {
            return None;
        }
        let path = path.canonicalize().ok()?;
        (path.starts_with(dir) && path.is_file() && self.allowed(&path)).then_some(path)
    }

    /// Read, transform and rewrite a file, reusing the cached result while the file and
    /// everything it links to are unchanged.
    fn build(&self, file: &Path, transforms: &[String], visiting: &mut HashSet<PathBuf>) -> Result<Built, AssetError> {
        let meta = std::fs::metadata(file).map_err(|_| AssetError::NotFound(file.display().to_string()))?;
        let key = (file.to_path_buf(), transforms.to_vec());
        let cached = self.cache.lock().expect("asset cache").get(&key).cloned();
        if let Some(c) = cached {
            let fresh = c.modified == meta.modified().ok()
                && c.len == meta.len()
                && c.deps.iter().all(|(dep, t, hash)| {
                    visiting.contains(dep) || self.build(dep, t, visiting).is_ok_and(|b| &b.hash == hash)
                });
            if fresh {
                return Ok(c);
            }
        }
        visiting.insert(file.to_path_buf());
        let built = self.build_fresh(file, transforms, &meta, visiting);
        visiting.remove(file);
        let built = built?;
        self.cache.lock().expect("asset cache").insert(key, built.clone());
        Ok(built)
    }

    fn build_fresh(
        &self,
        file: &Path,
        transforms: &[String],
        meta: &std::fs::Metadata,
        visiting: &mut HashSet<PathBuf>,
    ) -> Result<Built, AssetError> {
        let mut body = std::fs::read(file).map_err(|_| AssetError::NotFound(file.display().to_string()))?;
        let mut extension = file.extension().map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase())).unwrap_or_default();
        for global in &self.global {
            if accepts(&global.extensions, &extension) {
                let out = (global.transform)(body).map_err(AssetError::TransformFailed)?;
                body = out.content;
                extension = out.extension.unwrap_or(extension);
            }
        }
        if transforms.len() > self.max_transforms {
            return Err(AssetError::BadTransform(format!("at most {} transforms per URL", self.max_transforms)));
        }
        for invocation in transforms {
            let (name, param) = match invocation.split_once(':') {
                Some((n, p)) => (n, Some(p)),
                None => (invocation.as_str(), None),
            };
            let t = self.transforms.get(name).ok_or_else(|| AssetError::BadTransform(format!("unknown transform {name}")))?;
            match (t.param, param) {
                (Param::None, Some(_)) => return Err(AssetError::BadTransform(format!("{name} takes no parameter"))),
                (Param::Required, None) => return Err(AssetError::BadTransform(format!("{name} needs a parameter"))),
                _ => {}
            }
            if !accepts(&t.extensions, &extension) {
                return Err(AssetError::BadTransform(format!("{name} does not accept {extension} files")));
            }
            let out = (t.transform)(body, param).map_err(AssetError::TransformFailed)?;
            body = out.content;
            extension = out.extension.unwrap_or(extension);
        }
        let mut deps = Vec::new();
        let mut imports = Vec::new();
        if extension == ".css" || is_script(&extension) {
            let text = String::from_utf8_lossy(&body).into_owned();
            let dir = file.parent().unwrap_or(&self.root).to_path_buf();
            let mut link = |spec: &str, is_import: bool| -> Option<String> {
                let (path_part, query) = spec.split_once('?').map_or((spec, None), |(p, q)| (p, Some(q)));
                let target = dir.join(path_part.replace('/', std::path::MAIN_SEPARATOR_STR));
                let target = target.canonicalize().ok()?;
                let target_transforms: Vec<String> = query
                    .map(|q| rmx_url::SearchParams::parse(q).get_all("transform").into_iter().map(str::to_string).collect())
                    .unwrap_or_default();
                let href = self.href(&target, &target_transforms, visiting).ok()?;
                if !visiting.contains(&target) {
                    if let Ok(b) = self.build(&target, &target_transforms, visiting) {
                        deps.push((target.clone(), target_transforms.clone(), b.hash));
                    }
                }
                if is_import {
                    imports.push((target, target_transforms));
                }
                Some(href)
            };
            let rewritten = if extension == ".css" { rewrite::css(&text, &mut link) } else { rewrite::script(&text, &mut link) };
            body = rewritten.into_bytes();
        }
        let hash = hash_content(&body);
        Ok(Built { modified: meta.modified().ok(), len: meta.len(), deps, imports, body: Arc::new(body), extension, hash })
    }
}

fn is_script(extension: &str) -> bool {
    matches!(extension, ".js" | ".mjs")
}

fn accepts(extensions: &[String], extension: &str) -> bool {
    extensions.is_empty() || extensions.iter().any(|e| e.eq_ignore_ascii_case(extension))
}

fn overlaps(a: &str, b: &str) -> bool {
    a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}

fn status(status: StatusCode, message: &str) -> Response {
    let mut res = rmx_fetch::response::text(message.to_string());
    *res.status_mut() = status;
    res
}

fn encode_segment(segment: &str) -> String {
    let mut out = String::new();
    for b in segment.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'@') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A short content hash for URLs: 64-bit FNV-1a in base64url. It only needs to change
/// when content changes; it is not a security measure.
fn hash_content(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    (0..8).map(|i| ALPHABET[((h >> (i * 6)) & 63) as usize] as char).collect()
}

/// `dir/name.@hash.ext` -> (`dir/name.ext`, `Some(hash)`).
fn parse_fingerprint(pathname: &str) -> (String, Option<String>) {
    let (dir, base) = pathname.rsplit_once('/').map_or(("", pathname), |(d, b)| (d, b));
    let Some(at) = base.find(".@") else { return (pathname.to_string(), None) };
    let rest = &base[at + 2..];
    let (hash, ext) = rest.split_once('.').map_or((rest, ""), |(h, e)| (h, e));
    if hash.is_empty() || !hash.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return (pathname.to_string(), None);
    }
    let name = if ext.is_empty() { base[..at].to_string() } else { format!("{}.{ext}", &base[..at]) };
    (format!("{dir}/{name}"), Some(hash.to_string()))
}

fn format_fingerprint(url: &str, hash: &str) -> String {
    let (dir, base) = url.rsplit_once('/').map_or(("", url), |(d, b)| (d, b));
    match base.rfind('.').filter(|&i| i > 0) {
        Some(dot) => format!("{dir}/{}.@{hash}{}", &base[..dot], &base[dot..]),
        None => format!("{dir}/{base}.@{hash}"),
    }
}

#[cfg(test)]
mod tests;
