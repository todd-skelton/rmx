//! Browser assets served straight from source directories (Remix's `remix/assets`).
//!
//! An [`AssetServer`] maps public URLs under a base path onto mounted directories, serves
//! only files the allow and deny globs permit, and rewrites references between assets so
//! every URL it hands out is current. It serves three kinds of files:
//!
//! - Scripts (`.js`, `.mjs`): relative module specifiers (`import`, `import()`) and
//!   `new URL('...', import.meta.url)` files, as wasm-bindgen emits, become asset URLs.
//! - Stylesheets (`.css`): `@import` and `url(...)` references become asset URLs.
//! - File assets: the extensions in [`FilesOptions::extensions`], plus `.wasm`. Named request
//!   transforms run on demand (`image.svg?transform=recolor:8B5CF6`), then the global
//!   transforms.
//!
//! With `fingerprint: true`, URLs carry a content hash (`app.@k3j2h1.css`), are served as
//! immutable, and only fingerprinted URLs are served; otherwise responses are revalidated
//! with an `ETag`. Files are read when requested, so edits show up on the next request
//! without restarting.
//!
//! Requests the server does not handle (missing, not allowed, unsupported types, stale
//! fingerprints) return `None` so the router can continue; failures go to
//! [`AssetOptions::on_error`] and become a `500`.
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

mod cache;
mod glob;
mod inspect;
pub mod paths;
mod rewrite;
pub mod routes;
mod transforms;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use http::{header, HeaderValue, Method, StatusCode};
use rmx_fetch::{Body, File, Response};
use rmx_headers::IfNoneMatch;
use rmx_router::Context;

pub use cache::{FsFileCache, FsFileCacheOptions, DEFAULT_CACHE_DIRECTORY};
pub use glob::glob_match;
pub use inspect::{assets_command, assets_command_output, AssetAccess, AssetDetails, AssetKind, AssetStatus};
pub use transforms::{FileCache, FileTransform, FilesOptions, GlobalTransform, Param, TransformContext, Transformed};

/// Called with errors that fail a request (Remix's `onError`). Return a response to send
/// instead of the default `500 Internal Server Error`.
pub type ErrorHandler = Arc<dyn Fn(&AssetError) -> Option<Response> + Send + Sync>;

/// Options for [`AssetServer::new`].
#[derive(Clone)]
pub struct AssetOptions {
    /// Relative paths in the other options resolve from here.
    pub root_dir: PathBuf,
    /// URL prefix, like `/assets`. Empty serves from the root.
    pub base_path: String,
    /// `(url segment, directory)` pairs: `("app", "app")` serves `app/x.css` at
    /// `/assets/app/x.css`.
    pub mounts: Vec<(String, String)>,
    /// Globs (relative to `root_dir`) of files that may be served. Required.
    pub allow_files: Vec<String>,
    /// Globs that are never served, even when allowed.
    pub deny_files: Vec<String>,
    /// Put content hashes in URLs and serve them as immutable. Only fingerprinted URLs
    /// are served.
    pub fingerprint: bool,
    /// File assets: extensions served as they are or through transforms.
    pub files: Option<FilesOptions>,
    /// Called for requests that fail; the default logs the error to stderr.
    pub on_error: Option<ErrorHandler>,
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
            files: None,
            on_error: None,
        }
    }
}

/// What went wrong (Remix's `AssetServerCompilationError` codes, plus `Config` for invalid
/// options and arguments).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetErrorCode {
    Config,
    FileNotFound,
    FileNotAllowed,
    FileNotSupported,
    FileOutsideMounts,
    FileTransformQueryInvalid,
    FileTransformNotSupported,
    FileTransformResultInvalid,
    FileTransformFailed,
    EmitFailed,
    ImportResolutionFailed,
    ImportNotSupported,
    ImportNotAllowed,
    ImportOutsideMounts,
    UrlResolutionFailed,
    UrlNotSupported,
    UrlNotAllowed,
    UrlOutsideMounts,
}

impl AssetErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            AssetErrorCode::Config => "CONFIG",
            AssetErrorCode::FileNotFound => "FILE_NOT_FOUND",
            AssetErrorCode::FileNotAllowed => "FILE_NOT_ALLOWED",
            AssetErrorCode::FileNotSupported => "FILE_NOT_SUPPORTED",
            AssetErrorCode::FileOutsideMounts => "FILE_OUTSIDE_MOUNTS",
            AssetErrorCode::FileTransformQueryInvalid => "FILE_TRANSFORM_QUERY_INVALID",
            AssetErrorCode::FileTransformNotSupported => "FILE_TRANSFORM_NOT_SUPPORTED",
            AssetErrorCode::FileTransformResultInvalid => "FILE_TRANSFORM_RESULT_INVALID",
            AssetErrorCode::FileTransformFailed => "FILE_TRANSFORM_FAILED",
            AssetErrorCode::EmitFailed => "EMIT_FAILED",
            AssetErrorCode::ImportResolutionFailed => "IMPORT_RESOLUTION_FAILED",
            AssetErrorCode::ImportNotSupported => "IMPORT_NOT_SUPPORTED",
            AssetErrorCode::ImportNotAllowed => "IMPORT_NOT_ALLOWED",
            AssetErrorCode::ImportOutsideMounts => "IMPORT_OUTSIDE_MOUNTS",
            AssetErrorCode::UrlResolutionFailed => "URL_RESOLUTION_FAILED",
            AssetErrorCode::UrlNotSupported => "URL_NOT_SUPPORTED",
            AssetErrorCode::UrlNotAllowed => "URL_NOT_ALLOWED",
            AssetErrorCode::UrlOutsideMounts => "URL_OUTSIDE_MOUNTS",
        }
    }
}

/// Why an asset could not be served or linked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetError {
    pub code: AssetErrorCode,
    pub message: String,
}

impl AssetError {
    pub fn new(code: AssetErrorCode, message: impl Into<String>) -> AssetError {
        AssetError { code, message: message.into() }
    }

    fn config(message: impl Into<String>) -> AssetError {
        AssetError::new(AssetErrorCode::Config, message)
    }
}

impl fmt::Display for AssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
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

/// A mounted directory: `dir` (as configured, `file_root`) served below `url_root`.
struct Mount {
    url_root: String,
    dir: PathBuf,
    file_root: String,
}

struct Inner {
    root: PathBuf,
    /// Longest URL root first.
    mounts: Vec<Mount>,
    allow: Vec<String>,
    deny: Vec<String>,
    fingerprint: bool,
    files: transforms::Files,
    on_error: Option<ErrorHandler>,
    /// Emitted scripts, styles and untransformed files.
    cache: Mutex<HashMap<PathBuf, Built>>,
    /// Each file's last seen `(modified, len)` and a version that changes with it, for
    /// transformed-output cache keys.
    versions: Mutex<HashMap<PathBuf, (Snapshot, u64)>>,
}

type Snapshot = (Option<SystemTime>, u64);

/// A file after transforms and reference rewriting.
#[derive(Clone)]
struct Built {
    snapshot: Snapshot,
    /// `(file, transforms, hash)` of every asset this one links to.
    deps: Vec<(PathBuf, Vec<String>, String)>,
    /// Scripts a script imports, or stylesheets a stylesheet `@import`s (for preloads).
    imports: Vec<PathBuf>,
    body: Arc<Vec<u8>>,
    extension: String,
    hash: String,
}

impl AssetServer {
    pub fn new(options: AssetOptions) -> Result<AssetServer, AssetError> {
        if options.allow_files.is_empty() {
            return Err(AssetError::config("allow_files must list the files that may be served"));
        }
        if options.mounts.is_empty() {
            return Err(AssetError::config("mounts must include at least one entry"));
        }
        let files = transforms::Files::resolve(options.files).map_err(AssetError::config)?;
        let root = options.root_dir.canonicalize().map_err(|e| AssetError::config(format!("root_dir: {e}")))?;
        let root_dir = paths::strip_verbatim(&root.to_string_lossy());
        let config = routes::MountConfig { mounts: options.mounts.clone(), root_dir };
        let compiled = routes::compile_routes(&options.base_path, &[config]).map_err(AssetError::config)?;
        let mut mounts = Vec::new();
        for mount in compiled.mounts {
            let dir_path = root.join(&mount.file_root_value);
            let dir_path = dir_path.canonicalize().unwrap_or(dir_path);
            mounts.push(Mount { url_root: mount.url_root, dir: dir_path, file_root: mount.file_root_value });
        }
        mounts.sort_by_key(|m| std::cmp::Reverse(m.url_root.len()));
        Ok(AssetServer(Arc::new(Inner {
            root,
            mounts,
            allow: options.allow_files,
            deny: options.deny_files,
            fingerprint: options.fingerprint,
            files,
            on_error: options.on_error,
            cache: Mutex::new(HashMap::new()),
            versions: Mutex::new(HashMap::new()),
        })))
    }

    /// The URL for a file: a path relative to `root_dir`, an absolute path or a `file://`
    /// URL (`getHref`).
    pub fn href(&self, file: impl AsRef<Path>) -> Result<String, AssetError> {
        self.href_with(file, &[])
    }

    /// The URL for a file asset with request transforms applied, each `"name"` or
    /// `"name:param"` (`getHref` with `transform`). Only configured file assets take
    /// transforms.
    pub fn href_with(&self, file: impl AsRef<Path>, transforms: &[&str]) -> Result<String, AssetError> {
        let file = self.0.served_file(&self.0.file_input(file.as_ref())?)?;
        let kind = self.0.kind(&file);
        if !transforms.is_empty() && kind != AssetKind::File {
            return Err(AssetError::config("AssetServer::href_with only supports transforms for configured file assets"));
        }
        let query: Vec<String> = transforms.iter().map(|t| t.to_string()).collect();
        let transforms = self.0.files.parse(&query).map_err(|e| AssetError::config(e.message))?;
        if kind == AssetKind::Style {
            // Resolve the import graph, so missing or circular imports fail here.
            self.0.build(&file, &[], &mut Vec::new())?;
        }
        self.0.href(&file, &transforms, &mut Vec::new())
    }

    /// The URL of a module entry and the modules it imports (`getScriptEntry`).
    pub fn script_entry(&self, file: impl AsRef<Path>) -> Result<ScriptEntry, AssetError> {
        let href = self.href(file.as_ref())?;
        let preloads = self.preloads(&[file.as_ref()])?;
        Ok(ScriptEntry { href, preloads })
    }

    /// The URLs to preload for scripts, stylesheets and file assets (`getPreloads`): the
    /// roots, then what they import, shallowest first, each URL once. Scripts follow their
    /// imports and stylesheets their `@import`s; file assets are listed as they are.
    pub fn preloads<P: AsRef<Path>>(&self, files: &[P]) -> Result<Vec<String>, AssetError> {
        let mut order: Vec<AssetKind> = Vec::new();
        let mut groups: HashMap<AssetKind, Vec<PathBuf>> = HashMap::new();
        for file in files {
            let file = self.0.served_file(&self.0.file_input(file.as_ref())?)?;
            let kind = self.0.kind(&file);
            if !order.contains(&kind) {
                order.push(kind);
            }
            groups.entry(kind).or_default().push(file);
        }
        let mut layered = Vec::new();
        for kind in &order {
            let roots = &groups[kind];
            layered.push(match kind {
                AssetKind::File => vec![roots.iter().map(|f| self.0.href(f, &[], &mut Vec::new())).collect::<Result<Vec<_>, _>>()?],
                _ => self.0.layers(roots)?,
            });
        }
        let mut urls = Vec::new();
        let mut seen = HashSet::new();
        let depth = layered.iter().map(Vec::len).max().unwrap_or(0);
        for d in 0..depth {
            for layers in &layered {
                for url in layers.get(d).into_iter().flatten() {
                    if seen.insert(url.clone()) {
                        urls.push(url.clone());
                    }
                }
            }
        }
        Ok(urls)
    }

    /// Serve an asset request (`fetch`). `None` when this server does not handle it: not
    /// GET or HEAD, outside every mount, missing, not allowed, an unsupported file type,
    /// or (when fingerprinting) a URL without the current fingerprint. Malformed transform
    /// requests are `400`; other failures go to [`AssetOptions::on_error`].
    pub fn fetch(&self, ctx: &Context) -> Option<Response> {
        let head = *ctx.method() == Method::HEAD;
        if !head && *ctx.method() != Method::GET {
            return None;
        }
        let url = ctx.url();
        let (pathname, fingerprint) = parse_fingerprint(url.pathname());
        let (mount, mapped) = self.0.mapped_url(&pathname)?;
        if self.0.fingerprint && fingerprint.is_none() {
            return None;
        }
        let query: Vec<String> = url.search_params().get_all("transform").into_iter().map(str::to_string).collect();
        match self.0.kind(&mapped) {
            AssetKind::Unsupported => return None,
            AssetKind::Script | AssetKind::Style if !query.is_empty() => {
                return Some(plain(StatusCode::BAD_REQUEST, "Asset request transforms are only supported for configured file assets"));
            }
            _ => {}
        }
        let file = mapped.canonicalize().ok()?;
        if !file.starts_with(&mount.dir) || !file.is_file() || !self.0.allowed(&file) {
            return None;
        }
        let built = self.0.files.parse(&query).and_then(|transforms| self.0.build(&file, &transforms, &mut Vec::new()));
        let built = match built {
            Ok(built) => built,
            Err(e) => {
                return match e.code {
                    AssetErrorCode::FileNotFound | AssetErrorCode::FileNotAllowed => None,
                    AssetErrorCode::FileTransformQueryInvalid => Some(plain(StatusCode::BAD_REQUEST, &e.message)),
                    _ => Some(self.0.error_response(&e)),
                };
            }
        };
        if fingerprint.as_deref().is_some_and(|f| f != built.hash) {
            return None;
        }
        let etag = format!("W/\"{}\"", built.hash);
        let etag_value = HeaderValue::from_str(&etag).expect("hash is header-safe");
        if IfNoneMatch::parse(ctx.header("if-none-match")).matches(&etag) {
            let mut res = Response::new(Body::empty());
            *res.status_mut() = StatusCode::NOT_MODIFIED;
            res.headers_mut().insert(header::ETAG, etag_value);
            return Some(res);
        }
        let mut res = Response::new(if head { Body::empty() } else { Body::from((*built.body).clone()) });
        let ct = rmx_mime::detect_content_type(&built.extension).unwrap_or_else(|| "application/octet-stream".into());
        if let Ok(v) = HeaderValue::from_str(&ct) {
            res.headers_mut().insert(header::CONTENT_TYPE, v);
        }
        res.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from(built.body.len()));
        res.headers_mut().insert(header::ETAG, etag_value);
        let cache_control = if fingerprint.is_some() { "public, max-age=31536000, immutable" } else { "no-cache" };
        res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
        Some(res)
    }
}

/// A `text/plain; charset=utf-8` response.
fn plain(status: StatusCode, message: &str) -> Response {
    let mut res = Response::new(Body::from(message.to_string()));
    *res.status_mut() = status;
    res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
    res
}

/// A file path for messages: absolute, `/`-separated.
/// A [`FileCache`] failure. Remix passes the cache's own error to `onError`; here it is an
/// `EmitFailed` error carrying that message.
fn cache_error(e: std::io::Error) -> AssetError {
    AssetError::new(AssetErrorCode::EmitFailed, format!("File cache failed: {e}"))
}

fn display(path: &Path) -> String {
    paths::normalize_windows_path(&paths::strip_verbatim(&path.to_string_lossy()))
}

fn extension_of(path: &Path) -> String {
    path.extension().map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase())).unwrap_or_default()
}

impl Inner {
    /// How a file is served, by extension.
    fn kind(&self, file: &Path) -> AssetKind {
        let extension = extension_of(file);
        if is_script(&extension) {
            AssetKind::Script
        } else if extension == ".css" {
            AssetKind::Style
        } else if self.files.extensions.contains(&extension) {
            AssetKind::File
        } else {
            AssetKind::Unsupported
        }
    }

    /// A `file://` URL, an absolute path, or a path relative to the root.
    fn file_input(&self, input: &Path) -> Result<PathBuf, AssetError> {
        let text = input.to_string_lossy();
        if let Some(rest) = text.strip_prefix("file://") {
            let path = rmx_url::decode(rest.split(['?', '#']).next().unwrap_or_default());
            // `file:///C:/x` on Windows.
            let path = if path.len() > 2 && path.as_bytes()[2] == b':' { &path[1..] } else { &path[..] };
            return Ok(PathBuf::from(path));
        }
        if text.contains("://") {
            return Err(AssetError::config(format!("Expected a file path or file:// URL, received \"{text}\"")));
        }
        Ok(if input.is_absolute() { input.to_path_buf() } else { self.root.join(input) })
    }

    /// The real path of a file the server can serve, or why not (`resolveServedFileOrThrow`).
    fn served_file(&self, file: &Path) -> Result<PathBuf, AssetError> {
        let real = file.canonicalize().ok().filter(|f| f.is_file());
        let Some(file) = real else {
            return Err(AssetError::new(AssetErrorCode::FileNotFound, format!("File not found: {}", display(file))));
        };
        if self.kind(&file) == AssetKind::Unsupported {
            return Err(AssetError::new(AssetErrorCode::FileNotSupported, format!("File type is not supported: {}", display(&file))));
        }
        if !self.allowed(&file) {
            return Err(AssetError::new(
                AssetErrorCode::FileNotAllowed,
                format!(
                    "File \"{}\" is not allowed by the asset server access configuration. Add a matching allow_files rule, or remove a conflicting deny_files rule.",
                    display(&file)
                ),
            ));
        }
        if self.url_for_file(&file).is_none() {
            return Err(AssetError::new(AssetErrorCode::FileOutsideMounts, format!("File {} is outside all configured mounts.", display(&file))));
        }
        Ok(file)
    }

    /// The URL of a servable file, fingerprinted when fingerprinting is on.
    fn href(&self, file: &Path, transforms: &[String], stack: &mut Vec<PathBuf>) -> Result<String, AssetError> {
        let hash = if self.fingerprint { Some(self.build(file, transforms, stack)?.hash) } else { None };
        self.url_with(file, transforms, hash.as_deref())
    }

    /// The URL of a file with its transforms, fingerprinted with `hash` when fingerprinting
    /// is on.
    fn url_with(&self, file: &Path, transforms: &[String], hash: Option<&str>) -> Result<String, AssetError> {
        let mut url = self.url_for_file(file).ok_or_else(|| {
            AssetError::new(AssetErrorCode::FileOutsideMounts, format!("File {} is outside all configured mounts.", display(file)))
        })?;
        if let (true, Some(hash)) = (self.fingerprint, hash) {
            url = format_fingerprint(&url, hash);
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

    /// Preload layers for scripts or stylesheets: the roots, then each level of imports.
    fn layers(&self, roots: &[PathBuf]) -> Result<Vec<Vec<String>>, AssetError> {
        let mut visited: HashSet<PathBuf> = HashSet::new();
        let mut frontier: Vec<PathBuf> = roots.iter().filter(|r| visited.insert((*r).clone())).cloned().collect();
        let mut layers = Vec::new();
        while !frontier.is_empty() {
            let mut layer = Vec::new();
            let mut next = Vec::new();
            for file in frontier {
                let built = self.build(&file, &[], &mut Vec::new())?;
                layer.push(self.url_with(&file, &[], Some(&built.hash))?);
                next.extend(built.imports.iter().filter(|i| visited.insert((*i).clone())).cloned());
            }
            layers.push(layer);
            frontier = next;
        }
        Ok(layers)
    }

    fn error_response(&self, error: &AssetError) -> Response {
        let custom = match &self.on_error {
            Some(handler) => handler(error),
            None => {
                eprintln!("{}: {error}", error.code.as_str());
                None
            }
        };
        custom.unwrap_or_else(|| plain(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error"))
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
        self.mounts.iter().find_map(|Mount { url_root, dir, .. }| {
            let rel = file.strip_prefix(dir).ok()?;
            let segments: Vec<String> = rel.components().map(|c| rmx_url::encode_component(&c.as_os_str().to_string_lossy())).collect();
            Some(format!("{}/{}", url_root.trim_end_matches('/'), segments.join("/")))
        })
    }

    /// The mount and file a URL pathname maps to, whether or not the file exists.
    fn mapped_url(&self, pathname: &str) -> Option<(&Mount, PathBuf)> {
        let mount = self.mounts.iter().find(|m| m.url_root == "/" || pathname.starts_with(&format!("{}/", m.url_root)))?;
        let mut path = mount.dir.clone();
        let rest = if mount.url_root == "/" { &pathname[1..] } else { &pathname[mount.url_root.len() + 1..] };
        for segment in rest.split('/') {
            let segment = rmx_url::decode(segment);
            if segment.is_empty() || segment == "." || segment == ".." || segment.contains(['\\', '/', ':']) {
                return None;
            }
            path.push(segment);
        }
        matches!(path.strip_prefix(&mount.dir).ok()?.components().next(), Some(Component::Normal(_))).then_some((mount, path))
    }

    /// The version of a file's content this server has seen, bumped when its size or
    /// modification time changes.
    fn version(&self, file: &Path, snapshot: Snapshot) -> u64 {
        let mut versions = self.versions.lock().expect("asset versions");
        let entry = versions.entry(file.to_path_buf()).or_insert((snapshot, 0));
        if entry.0 != snapshot {
            *entry = (snapshot, entry.1 + 1);
        }
        entry.1
    }

    /// Read, transform and rewrite a file. Scripts, styles and untransformed files are kept
    /// while the file and everything it links to are unchanged; transformed files are
    /// recomputed unless a [`FileCache`] has them.
    fn build(&self, file: &Path, transforms: &[String], stack: &mut Vec<PathBuf>) -> Result<Built, AssetError> {
        let meta = std::fs::metadata(file).map_err(|_| AssetError::new(AssetErrorCode::FileNotFound, format!("File not found: {}", display(file))))?;
        let snapshot = (meta.modified().ok(), meta.len());
        let kind = self.kind(file);
        if kind == AssetKind::File && self.files.has_pipeline(transforms, &extension_of(file)) {
            return self.build_transformed(file, transforms, snapshot);
        }
        let cached = self.cache.lock().expect("asset cache").get(file).cloned();
        if let Some(c) = cached {
            let fresh = c.snapshot == snapshot
                && c.deps.iter().all(|(dep, t, hash)| stack.contains(dep) || self.build(dep, t, stack).is_ok_and(|b| &b.hash == hash));
            if fresh {
                return Ok(c);
            }
        }
        stack.push(file.to_path_buf());
        let built = self.build_fresh(file, kind, snapshot, stack);
        stack.pop();
        let built = built?;
        self.cache.lock().expect("asset cache").insert(file.to_path_buf(), built.clone());
        Ok(built)
    }

    fn build_transformed(&self, file: &Path, transforms: &[String], snapshot: Snapshot) -> Result<Built, AssetError> {
        let file_path = display(file);
        let version = self.version(file, snapshot);
        let key = self.files.cache.as_ref().map(|_| {
            let identity = format!("{file_path:?},{version},{transforms:?}");
            let digest = rmx_crypto::sha256(format!("[\"transformed-file-v3\",{:?},[{identity}]]", self.files.cache_key).as_bytes());
            rmx_crypto::hex(&digest)
        });
        let built = |body: Vec<u8>, extension: String| {
            let hash = hash_content(&body);
            Built { snapshot, deps: Vec::new(), imports: Vec::new(), body: Arc::new(body), extension, hash }
        };
        if let (Some(cache), Some(key)) = (&self.files.cache, &key) {
            if let Some(cached) = cache.get(key).map_err(cache_error)? {
                return Ok(built(cached.bytes.to_vec(), extension_of(Path::new(&cached.name))));
            }
        }
        let source = std::fs::read(file).map_err(|_| AssetError::new(AssetErrorCode::FileNotFound, format!("File not found: {file_path}")))?;
        let Some((body, extension)) = self.files.apply(&file_path, source.clone(), extension_of(file), transforms)? else {
            return Ok(built(source, extension_of(file)));
        };
        if let (Some(cache), Some(key)) = (&self.files.cache, &key) {
            let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let content_type = rmx_mime::detect_content_type(&extension).unwrap_or_else(|| "application/octet-stream".into());
            cache.put(key, File::new(format!("{stem}{extension}"), content_type, body.clone())).map_err(cache_error)?;
        }
        Ok(built(body, extension))
    }

    fn build_fresh(&self, file: &Path, kind: AssetKind, snapshot: Snapshot, stack: &mut Vec<PathBuf>) -> Result<Built, AssetError> {
        let file_path = display(file);
        let mut body = std::fs::read(file).map_err(|_| AssetError::new(AssetErrorCode::FileNotFound, format!("File not found: {file_path}")))?;
        let mut deps = Vec::new();
        let mut imports = Vec::new();
        if matches!(kind, AssetKind::Style | AssetKind::Script) {
            let text = String::from_utf8_lossy(&body).into_owned();
            let dir = file.parent().unwrap_or(&self.root).to_path_buf();
            let mut failure = None;
            let mut link = |spec: &str, is_import: bool| -> Option<String> {
                if failure.is_some() {
                    return None;
                }
                let linked = if kind == AssetKind::Style {
                    self.link_style(spec, is_import, &dir, &file_path, stack)
                } else {
                    self.link_script(spec, is_import, &dir, &file_path, stack)
                };
                match linked {
                    Ok(Some(link)) => {
                        if let Some(hash) = link.hash {
                            deps.push((link.target.clone(), link.transforms, hash));
                        }
                        if link.import {
                            imports.push(link.target);
                        }
                        Some(link.href)
                    }
                    Ok(None) => None,
                    Err(e) => {
                        failure = Some(e);
                        None
                    }
                }
            };
            let rewritten = if kind == AssetKind::Style { rewrite::css(&text, &mut link) } else { rewrite::script(&text, &mut link) };
            if let Some(e) = failure {
                return Err(e);
            }
            body = rewritten.into_bytes();
        }
        let hash = hash_content(&body);
        Ok(Built { snapshot, deps, imports, body: Arc::new(body), extension: extension_of(file), hash })
    }

    /// A stylesheet reference: `@import` (another stylesheet) or `url()` (a file asset),
    /// relative to the stylesheet. Query strings and fragments are kept; `transform`
    /// parameters on file URLs are requests for transformed output.
    fn link_style(&self, spec: &str, is_import: bool, dir: &Path, importer: &str, stack: &mut Vec<PathBuf>) -> Result<Option<Link>, AssetError> {
        let (path_part, query, fragment) = split_url(spec);
        if path_part.is_empty() {
            return Ok(None);
        }
        let target = dir.join(rmx_url::decode(path_part).replace('/', std::path::MAIN_SEPARATOR_STR)).canonicalize().ok().filter(|t| t.is_file());
        if is_import {
            let Some(target) = target.filter(|t| extension_of(t) == ".css") else {
                return Err(AssetError::new(AssetErrorCode::ImportResolutionFailed, format!("Failed to resolve import \"{spec}\" in {importer}.")));
            };
            self.check_reference(&target, spec, importer, "Import", AssetErrorCode::ImportNotAllowed, AssetErrorCode::ImportOutsideMounts)?;
            if let Some(start) = stack.iter().position(|f| *f == target) {
                let cycle: Vec<String> = stack[start..].iter().chain([&target]).map(|f| display(f)).collect();
                return Err(AssetError::new(AssetErrorCode::EmitFailed, format!("Circular CSS imports are not supported: {}", cycle.join(" -> "))));
            }
            let built = self.build(&target, &[], stack)?;
            let href = self.url_with(&target, &[], Some(&built.hash))?;
            return Ok(Some(Link { href: format!("{href}{query}{fragment}"), hash: Some(built.hash), transforms: Vec::new(), import: true, target }));
        }
        let Some(target) = target else {
            return Err(AssetError::new(AssetErrorCode::UrlResolutionFailed, format!("Failed to resolve url(\"{spec}\") in {importer}.")));
        };
        if self.kind(&target) != AssetKind::File {
            return Err(AssetError::new(
                AssetErrorCode::UrlNotSupported,
                format!(
                    "URL \"{spec}\" in {importer}, resolved to \"{}\", is not a supported file asset. Add the file extension to files.extensions to serve this asset.",
                    display(&target)
                ),
            ));
        }
        self.check_reference(&target, spec, importer, "URL", AssetErrorCode::UrlNotAllowed, AssetErrorCode::UrlOutsideMounts)?;
        let mut params = rmx_url::SearchParams::parse(query);
        let requested: Vec<String> = params.get_all("transform").into_iter().map(str::to_string).collect();
        params.delete("transform");
        let rest = if params.is_empty() { String::new() } else { format!("?{params}") };
        let suffix = format!("{rest}{fragment}");
        let (href, built, transforms) = match self.files.parse(&requested) {
            Ok(transforms) => {
                let built = self.build(&target, &transforms, stack)?;
                (self.url_with(&target, &transforms, Some(&built.hash))?, built, transforms)
            }
            Err(e) => {
                // Leave the request as written, so the browser gets the 400 for it.
                eprintln!("Invalid file transform request \"{}\" in CSS asset {importer} for {}: {}", requested.join(","), display(&target), e.message);
                let mut query = rmx_url::SearchParams::new();
                for t in &requested {
                    query.append("transform", t);
                }
                let built = self.build(&target, &[], stack)?;
                (format!("{}?{query}", self.url_with(&target, &[], Some(&built.hash))?), built, Vec::new())
            }
        };
        let href = match suffix.strip_prefix('?') {
            Some(more) if href.contains('?') => format!("{href}&{more}"),
            _ => format!("{href}{suffix}"),
        };
        Ok(Some(Link { href, hash: Some(built.hash), transforms, import: false, target }))
    }

    /// A script reference: a relative module specifier (`import`, `export ... from`,
    /// `import()`) or a `new URL('...', import.meta.url)` file, as wasm-bindgen emits.
    /// Unresolvable `new URL` references are left as written.
    fn link_script(&self, spec: &str, is_import: bool, dir: &Path, importer: &str, stack: &mut Vec<PathBuf>) -> Result<Option<Link>, AssetError> {
        let (path_part, query, fragment) = split_url(spec);
        let target = dir.join(rmx_url::decode(path_part).replace('/', std::path::MAIN_SEPARATOR_STR)).canonicalize().ok().filter(|t| t.is_file());
        if !is_import {
            let Some(target) = target.filter(|t| self.kind(t) == AssetKind::File && self.allowed(t)) else { return Ok(None) };
            let Ok(built) = self.build(&target, &[], stack) else { return Ok(None) };
            let Ok(href) = self.url_with(&target, &[], Some(&built.hash)) else { return Ok(None) };
            return Ok(Some(Link { href: format!("{href}{query}{fragment}"), hash: Some(built.hash), transforms: Vec::new(), import: false, target }));
        }
        let Some(target) = target else {
            return Err(AssetError::new(
                AssetErrorCode::ImportResolutionFailed,
                format!("Failed to resolve import \"{spec}\" in {importer}. Ensure it resolves to a file within a configured asset server mount, or mark it as external."),
            ));
        };
        if self.kind(&target) != AssetKind::Script {
            return Err(AssetError::new(
                AssetErrorCode::ImportNotSupported,
                format!("Import \"{spec}\" in {importer}, resolved to \"{}\", is not a supported script file. Supported extensions are .js, .mjs.", display(&target)),
            ));
        }
        self.check_reference(&target, spec, importer, "Import", AssetErrorCode::ImportNotAllowed, AssetErrorCode::ImportOutsideMounts)?;
        let on_stack = stack.iter().position(|f| *f == target);
        if let (true, Some(start)) = (self.fingerprint, on_stack) {
            // A fingerprint covers the URLs a module imports, so a cycle has none.
            let cycle: Vec<String> = stack[start..].iter().chain([&target]).map(|f| display(f)).collect();
            return Err(AssetError::new(
                AssetErrorCode::EmitFailed,
                format!("Circular module imports are not supported with fingerprinting: {}", cycle.join(" -> ")),
            ));
        }
        let hash = if on_stack.is_some() { None } else { Some(self.build(&target, &[], stack)?.hash) };
        let href = self.url_with(&target, &[], hash.as_deref())?;
        Ok(Some(Link { href: format!("{href}{query}{fragment}"), hash, transforms: Vec::new(), import: true, target }))
    }

    /// The access and mount checks for a file another asset references.
    fn check_reference(&self, target: &Path, spec: &str, importer: &str, what: &str, not_allowed: AssetErrorCode, outside: AssetErrorCode) -> Result<(), AssetError> {
        let resolved = display(target);
        if !self.allowed(target) {
            return Err(AssetError::new(
                not_allowed,
                format!(
                    "{what} \"{spec}\" in {importer}, resolved to \"{resolved}\", is not allowed by the asset server access configuration. Add a matching allow_files rule, or remove a conflicting deny_files rule."
                ),
            ));
        }
        if self.url_for_file(target).is_none() {
            return Err(AssetError::new(
                outside,
                format!("{what} \"{spec}\" in {importer}, resolved to \"{resolved}\", is outside all configured mounts. Add a matching mount for this file path."),
            ));
        }
        Ok(())
    }
}

/// A reference another asset makes.
struct Link {
    href: String,
    target: PathBuf,
    transforms: Vec<String>,
    /// The target's hash, so the referencing asset is rebuilt when it changes.
    hash: Option<String>,
    /// Followed for preloads.
    import: bool,
}

/// `path?query#fragment` -> (`path`, `?query`, `#fragment`).
fn split_url(spec: &str) -> (&str, &str, &str) {
    let (rest, fragment) = spec.find('#').map_or((spec, ""), |i| (&spec[..i], &spec[i..]));
    let (path, query) = rest.find('?').map_or((rest, ""), |i| (&rest[..i], &rest[i..]));
    (path, query, fragment)
}

fn is_script(extension: &str) -> bool {
    matches!(extension, ".js" | ".mjs")
}

/// A content hash for URLs and ETags (Remix's `hashContent`): the first six characters of
/// the base64url SHA-256 digest.
fn hash_content(bytes: &[u8]) -> String {
    let mut hash = rmx_crypto::base64url_encode(&rmx_crypto::sha256(bytes));
    hash.truncate(6);
    hash
}

/// `dir/name.@hash.ext` -> (`dir/name.ext`, `Some(hash)`).
/// The pathname without its fingerprint, and the fingerprint (Remix's
/// `parseFingerprintSuffix`): `name.@hash.ext` with a single-part extension, or
/// `name.@hash`.
fn parse_fingerprint(pathname: &str) -> (String, Option<String>) {
    let (dir, base) = pathname.rsplit_once('/').map_or(("", pathname), |(d, b)| (d, b));
    let is_hash = |h: &str| !h.is_empty() && h.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    let is_ext = |e: &str| !e.is_empty() && !e.contains(['.', '/']);
    let split = |name: &str, hash: &str, ext: Option<&str>| {
        let name = match ext {
            Some(ext) => format!("{name}.{ext}"),
            None => name.to_string(),
        };
        let path = if pathname.contains('/') { format!("{dir}/{name}") } else { name };
        (path, Some(hash.to_string()))
    };
    if let Some((stem, ext)) = base.rsplit_once('.') {
        if let Some((name, hash)) = stem.rsplit_once(".@") {
            if !name.is_empty() && is_hash(hash) && is_ext(ext) {
                return split(name, hash, Some(ext));
            }
        }
    }
    if let Some((name, hash)) = base.rsplit_once(".@") {
        if !name.is_empty() && is_hash(hash) {
            return split(name, hash, None);
        }
    }
    (pathname.to_string(), None)
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
