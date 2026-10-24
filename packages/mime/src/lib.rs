//! MIME types by file extension (Remix's `mime` package): Remix's mime-db table, charset and
//! compressibility rules, and a registry for custom types.
//!
//! ```
//! use rsmix_mime::{define_mime_type, detect_content_type, detect_mime_type, MimeTypeDefinition};
//!
//! assert_eq!(detect_mime_type("app.min.js"), Some("text/javascript"));
//! assert_eq!(detect_content_type("style.css").as_deref(), Some("text/css; charset=utf-8"));
//! define_mime_type(MimeTypeDefinition::new(["mdx"], "text/mdx"));
//! assert_eq!(detect_content_type("post.mdx").as_deref(), Some("text/mdx; charset=utf-8"));
//! ```

mod generated;

use std::collections::HashMap;
use std::sync::{PoisonError, RwLock};

use generated::{COMPRESSIBLE_MIME_TYPES, MIME_TYPES};

/// A custom MIME type for [`define_mime_type`] (Remix's `MimeTypeDefinition`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MimeTypeDefinition {
    /// Extensions to register, with or without a leading dot, in any case. Multi-part
    /// extensions such as `be.pit` match the end of a file name.
    pub extensions: Vec<String>,
    pub mime_type: String,
    /// Whether the type is compressible; `None` keeps the default rules.
    pub compressible: Option<bool>,
    /// The charset [`mime_type_to_content_type`] adds; `None` keeps the default rules.
    pub charset: Option<String>,
}

impl MimeTypeDefinition {
    pub fn new<I, S>(extensions: I, mime_type: impl Into<String>) -> MimeTypeDefinition
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        MimeTypeDefinition {
            extensions: extensions.into_iter().map(Into::into).collect(),
            mime_type: mime_type.into(),
            ..MimeTypeDefinition::default()
        }
    }

    pub fn compressible(mut self, compressible: bool) -> MimeTypeDefinition {
        self.compressible = Some(compressible);
        self
    }

    pub fn charset(mut self, charset: impl Into<String>) -> MimeTypeDefinition {
        self.charset = Some(charset.into());
        self
    }
}

#[derive(Default)]
struct Registry {
    by_extension: HashMap<String, &'static str>,
    compressible: HashMap<String, bool>,
    charset: HashMap<String, String>,
}

static REGISTRY: RwLock<Option<Registry>> = RwLock::new(None);

fn with_registry<R>(read: impl FnOnce(&Registry) -> Option<R>) -> Option<R> {
    let guard = REGISTRY.read().unwrap_or_else(PoisonError::into_inner);
    guard.as_ref().and_then(read)
}

/// Register a MIME type for one or more extensions, overriding the built-in table (Remix's
/// `defineMimeType`). The latest registration for an extension wins. Registrations are
/// process-wide; the type name is kept for the life of the process so lookups can return
/// `&'static str`.
pub fn define_mime_type(definition: MimeTypeDefinition) {
    let mut guard = REGISTRY.write().unwrap_or_else(PoisonError::into_inner);
    let registry = guard.get_or_insert_with(Registry::default);
    let mime_type: &'static str = match registry.by_extension.values().find(|t| **t == definition.mime_type) {
        Some(existing) => existing,
        None => Box::leak(definition.mime_type.clone().into_boxed_str()),
    };
    for ext in &definition.extensions {
        let ext = ext.trim().to_lowercase();
        let ext = ext.strip_prefix('.').unwrap_or(&ext);
        registry.by_extension.insert(ext.to_string(), mime_type);
    }
    if let Some(compressible) = definition.compressible {
        registry.compressible.insert(definition.mime_type.clone(), compressible);
    }
    if let Some(charset) = definition.charset {
        registry.charset.insert(definition.mime_type, charset);
    }
}

/// Remove every [`define_mime_type`] registration (Remix's internal `resetMimeTypes`, for
/// test isolation).
pub fn reset_mime_types() {
    *REGISTRY.write().unwrap_or_else(PoisonError::into_inner) = None;
}

/// The MIME type for an extension (`"txt"`, `".txt"`) or a file name or path
/// (`"notes/a.txt"`), without parameters, or `None` when unknown (Remix's
/// `detectMimeType`). Case and surrounding whitespace are ignored; custom types win.
pub fn detect_mime_type(extension: &str) -> Option<&'static str> {
    let ext = extension.trim().to_lowercase();
    if let Some(custom) = with_registry(|r| detect_custom(r, &ext)) {
        return Some(custom);
    }
    let ext = ext.rfind('.').map_or(ext.as_str(), |i| &ext[i + 1..]);
    MIME_TYPES.binary_search_by(|(key, _)| (*key).cmp(ext)).ok().map(|i| MIME_TYPES[i].1)
}

fn detect_custom(registry: &Registry, ext: &str) -> Option<&'static str> {
    let get = |key: &str| registry.by_extension.get(key).copied();
    if let Some(found) = get(ext.strip_prefix('.').unwrap_or(ext)) {
        return Some(found);
    }
    let filename = ext.rfind(['/', '\\']).map_or(ext, |i| &ext[i + 1..]);
    if filename.contains('.') {
        if let Some(found) = get(filename.strip_prefix('.').unwrap_or(filename)) {
            return Some(found);
        }
    }
    // Every suffix after a dot, longest first, so `a.be.pit` finds `be.pit` before `pit`.
    filename.match_indices('.').find_map(|(i, _)| get(&filename[i + 1..]))
}

/// The `Content-Type` for an extension or file name: its MIME type with a charset where one
/// applies (Remix's `detectContentType`).
pub fn detect_content_type(extension: &str) -> Option<String> {
    detect_mime_type(extension).map(mime_type_to_content_type)
}

/// A `Content-Type` for a MIME type (Remix's `mimeTypeToContentType`): `; charset=utf-8`
/// for `text/*` (except `text/xml`, which declares its own encoding), `+json` types,
/// `application/json` and `application/javascript`, or the charset a custom definition
/// gives. A value that already names a charset is returned as is.
pub fn mime_type_to_content_type(mime_type: &str) -> String {
    if mime_type.contains("charset") || mime_type == "text/xml" {
        return mime_type.to_string();
    }
    if let Some(charset) = with_registry(|r| r.charset.get(mime_type).cloned()) {
        return format!("{mime_type}; charset={charset}");
    }
    let utf8 = mime_type.starts_with("text/")
        || mime_type.ends_with("+json")
        || mime_type == "application/json"
        || mime_type == "application/javascript";
    if utf8 {
        format!("{mime_type}; charset=utf-8")
    } else {
        mime_type.to_string()
    }
}

/// Whether a MIME type or `Content-Type` value is worth compressing (Remix's
/// `isCompressibleMimeType`): a custom definition's setting, mime-db's compressible types,
/// any `text/*`, and `+json`, `+text` or `+xml` suffixes. Parameters are ignored.
pub fn is_compressible_mime_type(mime_type: &str) -> bool {
    if mime_type.is_empty() {
        return false;
    }
    let essence = mime_type.split_once(';').map_or(mime_type, |(essence, _)| essence.trim());
    if let Some(custom) = with_registry(|r| r.compressible.get(essence).copied()) {
        return custom;
    }
    if COMPRESSIBLE_MIME_TYPES.binary_search(&essence).is_ok() {
        return true;
    }
    let lower = essence.to_ascii_lowercase();
    lower.starts_with("text/") || ["+json", "+text", "+xml"].iter().any(|suffix| lower.ends_with(suffix))
}

#[cfg(test)]
mod tests {
    use super::generated::{COMPRESSIBLE_MIME_TYPES, MIME_TYPES};

    #[test]
    fn tables_are_sorted_for_binary_search() {
        assert!(MIME_TYPES.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(COMPRESSIBLE_MIME_TYPES.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(MIME_TYPES.len(), 1239);
    }
}
