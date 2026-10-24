//! The file asset options (Remix's `files` option, `files/config.ts`): which extensions are
//! served as file assets, named request transforms (`image.svg?transform=recolor:8B5CF6`),
//! global transforms that run for every file asset, and an optional cache for transformed
//! output.

use std::collections::HashMap;
use std::sync::Arc;

use rsmix_fetch::File;

use crate::{AssetError, AssetErrorCode};

/// Whether a request transform takes a parameter (`name:param`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Param {
    /// `name` only; `name:x` is rejected.
    #[default]
    None,
    /// `name:param` only; `name` is rejected.
    Required,
    /// Either.
    Optional,
}

/// The output of a transform: new bytes, and optionally a new extension (`.webp`).
pub struct Transformed {
    pub content: Vec<u8>,
    pub extension: Option<String>,
}

impl Transformed {
    /// Output with a different extension, which later transforms in the pipeline see.
    pub fn with_extension(content: impl Into<Vec<u8>>, extension: &str) -> Transformed {
        Transformed { content: content.into(), extension: Some(extension.to_string()) }
    }
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

impl From<&str> for Transformed {
    fn from(content: &str) -> Self {
        Transformed { content: content.as_bytes().to_vec(), extension: None }
    }
}

/// What a transform is applied to.
pub struct TransformContext<'a> {
    /// The extension at this point in the pipeline (`.svg`), lowercase.
    pub extension: &'a str,
    /// The source file (absolute, `/`-separated).
    pub file_path: &'a str,
    /// The request transform's parameter; always `None` for global transforms.
    pub param: Option<&'a str>,
}

type RequestFn = dyn Fn(Vec<u8>, &TransformContext<'_>) -> Result<Transformed, String> + Send + Sync;
type GlobalFn = dyn Fn(Vec<u8>, &TransformContext<'_>) -> Result<Option<Transformed>, String> + Send + Sync;

/// A named transform requested from asset URLs (`defineFileTransform`). An `Err` fails the
/// request with a 500 through [`crate::AssetOptions::on_error`].
#[derive(Clone)]
pub struct FileTransform {
    pub(crate) extensions: Option<Vec<String>>,
    pub(crate) param: Param,
    pub(crate) transform: Arc<RequestFn>,
}

impl FileTransform {
    /// A transform that takes no parameter and accepts any extension.
    pub fn new(transform: impl Fn(Vec<u8>, &TransformContext<'_>) -> Result<Transformed, String> + Send + Sync + 'static) -> FileTransform {
        FileTransform { extensions: None, param: Param::None, transform: Arc::new(transform) }
    }

    pub fn param(mut self, param: Param) -> FileTransform {
        self.param = param;
        self
    }

    /// Only accept these extensions (`.svg`), matched against the extension at this point
    /// in the pipeline.
    pub fn extensions(mut self, extensions: &[&str]) -> FileTransform {
        self.extensions = Some(extensions.iter().map(|e| e.to_string()).collect());
        self
    }
}

/// A transform that runs for every file asset after the request transforms. Returning
/// `Ok(None)` skips it for that input.
#[derive(Clone)]
pub struct GlobalTransform {
    pub(crate) extensions: Option<Vec<String>>,
    pub(crate) name: Option<String>,
    pub(crate) transform: Arc<GlobalFn>,
}

impl GlobalTransform {
    pub fn new(
        transform: impl Fn(Vec<u8>, &TransformContext<'_>) -> Result<Option<Transformed>, String> + Send + Sync + 'static,
    ) -> GlobalTransform {
        GlobalTransform { extensions: None, name: None, transform: Arc::new(transform) }
    }

    /// Skip files with other extensions.
    pub fn extensions(mut self, extensions: &[&str]) -> GlobalTransform {
        self.extensions = Some(extensions.iter().map(|e| e.to_string()).collect());
        self
    }

    /// The name used in error messages (otherwise the transform's index).
    pub fn name(mut self, name: &str) -> GlobalTransform {
        self.name = Some(name.to_string());
        self
    }
}

/// A cache for transformed file output (Remix's `FileCache`). Implementations own
/// admission, eviction and persistence, and may drop entries at any time. An error fails
/// the request through [`AssetOptions::on_error`](crate::AssetOptions::on_error).
pub trait FileCache: Send + Sync {
    /// The cached file, keeping its bytes, name, type and `last_modified`; `None` on a miss.
    fn get(&self, key: &str) -> std::io::Result<Option<File>>;
    /// Store or replace a file. The cache may decline to store it.
    fn put(&self, key: &str, file: File) -> std::io::Result<()>;
}

/// Options for file assets: files that are served as they are or through transforms,
/// rather than as scripts or stylesheets.
#[derive(Clone)]
pub struct FilesOptions {
    /// Extensions served as file assets (`.png`, `.svg`). `.wasm` is always served.
    pub extensions: Vec<String>,
    /// Named transforms that asset URLs can request.
    pub transforms: Vec<(String, FileTransform)>,
    /// Transforms that run, in order, for every file asset.
    pub global_transforms: Vec<GlobalTransform>,
    /// Most request transforms in one URL. Defaults to 5.
    pub max_request_transforms: usize,
    /// Cache for transformed output; without one, transforms run on every request.
    pub cache: Option<Arc<dyn FileCache>>,
    /// Namespace for cached output, such as a commit SHA, so a restarted server reuses it.
    /// Defaults to a random value per server.
    pub cache_key: Option<String>,
}

impl Default for FilesOptions {
    fn default() -> Self {
        FilesOptions {
            extensions: Vec::new(),
            transforms: Vec::new(),
            global_transforms: Vec::new(),
            max_request_transforms: 5,
            cache: None,
            cache_key: None,
        }
    }
}

/// Extensions that are served as scripts or styles, so cannot be file assets.
const RESERVED: &[&str] = &[".ts", ".tsx", ".js", ".jsx", ".mts", ".mjs", ".css", ".map"];

/// Validated file options.
pub(crate) struct Files {
    pub(crate) extensions: Vec<String>,
    transforms: HashMap<String, FileTransform>,
    global: Vec<GlobalTransform>,
    max: usize,
    pub(crate) cache: Option<Arc<dyn FileCache>>,
    pub(crate) cache_key: String,
}

fn is_name(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// ` .SVG ` to `.svg`, or `None` when it is not `.ext` form.
pub(crate) fn normalize_extension(extension: &str) -> Option<String> {
    let e = extension.trim().to_ascii_lowercase();
    e.strip_prefix('.').is_some_and(is_name).then_some(e)
}

fn normalize_extensions(extensions: &[String], option: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for extension in extensions {
        let e = normalize_extension(extension).ok_or_else(|| format!("{option} values must use \".ext\" format. Received \"{extension}\"."))?;
        if !out.contains(&e) {
            out.push(e);
        }
    }
    Ok(out)
}

fn normalize_filter(extensions: &Option<Vec<String>>, option: &str) -> Result<Option<Vec<String>>, String> {
    match extensions {
        None => Ok(None),
        Some(list) if list.is_empty() => Err(format!("{option} must include at least one extension")),
        Some(list) => normalize_extensions(list, option).map(Some),
    }
}

impl Files {
    /// Validate the options with Remix's messages (`normalizeFilesOptions`).
    pub(crate) fn resolve(options: Option<FilesOptions>) -> Result<Files, String> {
        let options = options.unwrap_or_default();
        let mut extensions = normalize_extensions(&options.extensions, "files.extensions")?;
        if let Some(reserved) = extensions.iter().find(|e| RESERVED.contains(&e.as_str())) {
            return Err(format!("files.extensions cannot include compiled asset extensions like \"{reserved}\"."));
        }
        if !extensions.iter().any(|e| e == ".wasm") {
            extensions.push(".wasm".into());
        }
        let mut transforms = HashMap::new();
        for (name, transform) in options.transforms {
            if !is_name(&name) {
                return Err(format!("files.transforms keys must use \"transform-name\" format. Received \"{name}\"."));
            }
            let extensions = normalize_filter(&transform.extensions, &format!("files.transforms.{name}.extensions"))?;
            transforms.insert(name, FileTransform { extensions, ..transform });
        }
        let mut global = Vec::new();
        for (index, transform) in options.global_transforms.into_iter().enumerate() {
            let extensions = normalize_filter(&transform.extensions, &format!("files.global_transforms[{index}].extensions"))?;
            global.push(GlobalTransform { extensions, ..transform });
        }
        if options.max_request_transforms < 1 {
            return Err("files.max_request_transforms must be a positive integer".into());
        }
        if options.cache_key.as_deref() == Some("") {
            return Err("files.cache_key must be a non-empty string".into());
        }
        Ok(Files {
            extensions,
            transforms,
            global,
            max: options.max_request_transforms,
            cache: options.cache,
            cache_key: options.cache_key.unwrap_or_else(rsmix_crypto::random_uuid),
        })
    }

    /// Parse `transform` values (`name` or `name:param`) into their canonical form, so
    /// equivalent URLs share output (Remix's `parseAssetTransformInvocations`).
    pub(crate) fn parse(&self, query: &[String]) -> Result<Vec<String>, AssetError> {
        let invalid = |m: String| AssetError::new(AssetErrorCode::FileTransformQueryInvalid, m);
        if query.len() > self.max {
            return Err(invalid(format!("Expected at most {} request transforms", self.max)));
        }
        query
            .iter()
            .map(|invocation| {
                let (name, param) = match invocation.split_once(':') {
                    Some((n, p)) => (n, Some(p)),
                    None => (invocation.as_str(), None),
                };
                if !is_name(name) {
                    return Err(invalid("Expected each transform name to use \"transform-name\" format".into()));
                }
                let transform = self.transforms.get(name).ok_or_else(|| invalid(format!("Unknown file transform \"{name}\"")))?;
                match (transform.param, param) {
                    (Param::None, Some(_)) => Err(invalid(format!("File transform \"{name}\" does not accept a param"))),
                    (Param::Required, None) => Err(invalid(format!("File transform \"{name}\" requires a param"))),
                    (_, None) => Ok(name.to_string()),
                    (_, Some(p)) => Ok(format!("{name}:{p}")),
                }
            })
            .collect()
    }

    /// Whether a file with this extension goes through the transform pipeline for these
    /// request transforms: there are some, or a global transform may apply.
    pub(crate) fn has_pipeline(&self, transforms: &[String], extension: &str) -> bool {
        !transforms.is_empty() || self.global.iter().any(|g| accepts(&g.extensions, extension))
    }

    /// Run the request transforms (canonical form), then the global ones. `None` when no
    /// transform applied.
    pub(crate) fn apply(&self, file_path: &str, body: Vec<u8>, extension: String, transforms: &[String]) -> Result<Option<(Vec<u8>, String)>, AssetError> {
        let mut body = body;
        let mut extension = extension;
        let mut applied = false;
        for invocation in transforms {
            let (name, param) = match invocation.split_once(':') {
                Some((n, p)) => (n, Some(p)),
                None => (invocation.as_str(), None),
            };
            let Some(transform) = self.transforms.get(name) else {
                return Err(AssetError::new(
                    AssetErrorCode::FileTransformQueryInvalid,
                    format!("Unknown file transform \"{name}\" requested for {file_path}"),
                ));
            };
            let subject = format!("File transform \"{name}\"");
            if !accepts(&transform.extensions, &extension) {
                return Err(AssetError::new(
                    AssetErrorCode::FileTransformNotSupported,
                    format!("{subject} does not support {extension} inputs for {file_path}"),
                ));
            }
            let context = TransformContext { extension: &extension, file_path, param };
            let out = (transform.transform)(body, &context).map_err(|e| failed(&subject, file_path, &e))?;
            (body, extension) = output(out, extension, &subject, file_path)?;
            applied = true;
        }
        for (index, global) in self.global.iter().enumerate() {
            if !accepts(&global.extensions, &extension) {
                continue;
            }
            let subject = match &global.name {
                Some(name) => format!("Global file transform \"{name}\""),
                None => format!("Global file transform at index {index}"),
            };
            let context = TransformContext { extension: &extension, file_path, param: None };
            let input = body.clone();
            let Some(out) = (global.transform)(input, &context).map_err(|e| failed(&subject, file_path, &e))? else { continue };
            (body, extension) = output(out, extension, &subject, file_path)?;
            applied = true;
        }
        Ok(applied.then_some((body, extension)))
    }
}

fn accepts(extensions: &Option<Vec<String>>, extension: &str) -> bool {
    extensions.as_ref().is_none_or(|list| list.iter().any(|e| e == extension))
}

fn failed(subject: &str, file_path: &str, error: &str) -> AssetError {
    AssetError::new(AssetErrorCode::FileTransformFailed, format!("{subject} failed for {file_path}. {error}"))
}

fn output(out: Transformed, extension: String, subject: &str, file_path: &str) -> Result<(Vec<u8>, String), AssetError> {
    let extension = match out.extension {
        None => extension,
        Some(e) => normalize_extension(&e).ok_or_else(|| {
            AssetError::new(AssetErrorCode::FileTransformResultInvalid, format!("{subject} returned an invalid extension \"{e}\" for {file_path}"))
        })?,
    };
    Ok((out.content, extension))
}
