//! `FormData`: parsed form fields and uploaded files, from URL-encoded or multipart bodies.

use std::fmt;
use std::sync::Arc;

use bytes::Bytes;
use rmx_multipart_parser::{get_multipart_boundary, parse_multipart_stream, MultipartError, MultipartOptions};

use crate::{BoxError, Body, SearchParams};

/// A file, like the web's `File`: an uploaded file, or one read back from storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    /// The client-supplied file name. Untrusted: never use it as a filesystem path.
    pub name: String,
    pub content_type: String,
    pub bytes: Bytes,
    /// `lastModified`: milliseconds since the Unix epoch.
    pub last_modified: u64,
}

impl File {
    /// A file modified now. Like the web's `File` constructor, the type is lowercased, and a
    /// type with characters outside U+0020 to U+007E becomes empty.
    pub fn new(name: impl Into<String>, content_type: impl Into<String>, bytes: impl Into<Bytes>) -> File {
        // `SystemTime` panics in wasm32.
        #[cfg(target_arch = "wasm32")]
        let now = js_sys::Date::now() as u64;
        #[cfg(not(target_arch = "wasm32"))]
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        let content_type: String = content_type.into();
        let content_type = if content_type.bytes().all(|b| (0x20..=0x7e).contains(&b)) { content_type.to_ascii_lowercase() } else { String::new() };
        File { name: name.into(), content_type, bytes: bytes.into(), last_modified: now }
    }

    /// `size` in bytes.
    pub fn size(&self) -> usize {
        self.bytes.len()
    }
}

/// One form value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormValue {
    Text(String),
    File(File),
}

impl FormValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            FormValue::Text(s) => Some(s),
            FormValue::File(_) => None,
        }
    }

    pub fn as_file(&self) -> Option<&File> {
        match self {
            FormValue::File(f) => Some(f),
            FormValue::Text(_) => None,
        }
    }
}

/// Ordered form entries, like the web's `FormData`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormData {
    entries: Vec<(String, FormValue)>,
}

/// Parsing limits (Remix's `ParseFormDataOptions`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormLimits {
    /// Most file parts. Default 20.
    pub max_files: usize,
    /// Largest part (files and text fields). Default 2 MiB.
    pub max_file_size: usize,
    /// Largest header block per multipart part. Default 8 KiB.
    pub max_header_size: usize,
    /// Most parts or URL-encoded pairs. Default 1000.
    pub max_parts: usize,
    /// Largest body. Default `max_files * max_file_size + 1 MiB`.
    pub max_total_size: Option<usize>,
}

impl Default for FormLimits {
    fn default() -> Self {
        FormLimits { max_files: 20, max_file_size: 2 * 1024 * 1024, max_header_size: 8 * 1024, max_parts: 1000, max_total_size: None }
    }
}

impl FormLimits {
    /// The effective total limit.
    pub fn total_size(&self) -> usize {
        self.max_total_size.unwrap_or_else(|| self.max_files.saturating_mul(self.max_file_size).saturating_add(1024 * 1024))
    }

    fn multipart(&self) -> MultipartOptions {
        MultipartOptions { max_header_size: self.max_header_size, max_file_size: self.max_file_size, max_parts: self.max_parts, max_total_size: Some(self.total_size()) }
    }
}

/// Why a form could not be parsed.
#[derive(Clone, Debug)]
pub enum FormDataError {
    /// A malformed body or an unsupported media type (`FormDataParseError`).
    Parse(String),
    /// More file parts than `max_files` (`MaxFilesExceededError`).
    MaxFilesExceeded(usize),
    /// A multipart limit, or a URL-encoded body over `max_parts` or the total size.
    Limit(MultipartError),
    /// The upload handler failed; the error is passed through as it was returned (Remix lets
    /// errors thrown by the handler propagate).
    Upload(Arc<dyn std::error::Error + Send + Sync>),
}

impl PartialEq for FormDataError {
    fn eq(&self, other: &FormDataError) -> bool {
        match (self, other) {
            (FormDataError::Parse(a), FormDataError::Parse(b)) => a == b,
            (FormDataError::MaxFilesExceeded(a), FormDataError::MaxFilesExceeded(b)) => a == b,
            (FormDataError::Limit(a), FormDataError::Limit(b)) => a == b,
            (FormDataError::Upload(a), FormDataError::Upload(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for FormDataError {}

impl FormDataError {
    /// Whether a configured limit was exceeded (as opposed to a malformed body or a failed
    /// upload handler).
    pub fn is_limit(&self) -> bool {
        matches!(self, FormDataError::MaxFilesExceeded(_) | FormDataError::Limit(_))
    }

    fn cannot_parse(cause: impl fmt::Display) -> FormDataError {
        FormDataError::Parse(format!("Cannot parse form data: {cause}"))
    }

    fn from_multipart(e: MultipartError) -> FormDataError {
        if e.is_limit() {
            FormDataError::Limit(e)
        } else {
            FormDataError::cannot_parse(e)
        }
    }
}

impl fmt::Display for FormDataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormDataError::Parse(message) => f.write_str(message),
            FormDataError::MaxFilesExceeded(max) => write!(f, "Maximum number of files exceeded: {max}"),
            FormDataError::Limit(e) => e.fmt(f),
            FormDataError::Upload(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for FormDataError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FormDataError::Upload(e) => Some(&**e),
            _ => None,
        }
    }
}

/// What an upload handler may return: `Option<FormValue>` (`None` drops the file), or a
/// `Result` of one, whose error stops parsing with [`FormDataError::Upload`].
pub trait UploadResult {
    fn into_upload_result(self) -> Result<Option<FormValue>, BoxError>;
}

impl UploadResult for Option<FormValue> {
    fn into_upload_result(self) -> Result<Option<FormValue>, BoxError> {
        Ok(self)
    }
}

impl<E: Into<BoxError>> UploadResult for Result<Option<FormValue>, E> {
    fn into_upload_result(self) -> Result<Option<FormValue>, BoxError> {
        self.map_err(Into::into)
    }
}

/// A file part as it is parsed, before it reaches the form (`FileUpload`). Give it to an
/// upload handler with [`FormData::parse_with`].
#[derive(Clone, Debug)]
pub struct FileUpload {
    /// The form field the file was sent under.
    pub field_name: String,
    pub file: File,
}

impl FormData {
    pub fn new() -> FormData {
        FormData::default()
    }

    /// The first text value for `name` (files are skipped).
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries.iter().filter(|(k, _)| k == name).find_map(|(_, v)| v.as_text())
    }

    /// The first value for `name`, text or file.
    pub fn get_value(&self, name: &str) -> Option<&FormValue> {
        self.entries.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    pub fn get_all(&self, name: &str) -> Vec<&FormValue> {
        self.entries.iter().filter(|(k, _)| k == name).map(|(_, v)| v).collect()
    }

    /// The first file for `name`.
    pub fn file(&self, name: &str) -> Option<&File> {
        self.entries.iter().filter(|(k, _)| k == name).find_map(|(_, v)| v.as_file())
    }

    pub fn has(&self, name: &str) -> bool {
        self.entries.iter().any(|(k, _)| k == name)
    }

    pub fn append(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.entries.push((name.into(), FormValue::Text(value.into())));
    }

    pub fn append_file(&mut self, name: impl Into<String>, file: File) {
        self.entries.push((name.into(), FormValue::File(file)));
    }

    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        self.entries.retain(|(k, _)| *k != name);
        self.entries.push((name, FormValue::Text(value.into())));
    }

    pub fn delete(&mut self, name: &str) {
        self.entries.retain(|(k, _)| k != name);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &FormValue)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The text fields as `SearchParams` (files contribute their names).
    pub fn to_search_params(&self) -> SearchParams {
        self.entries
            .iter()
            .map(|(k, v)| match v {
                FormValue::Text(s) => (k.clone(), s.clone()),
                FormValue::File(f) => (k.clone(), f.name.clone()),
            })
            .collect()
    }

    /// Parse a body according to its `Content-Type` (`parseFormData`): URL-encoded or
    /// multipart (streamed, with files kept in memory). Other media types, and a missing
    /// `Content-Type`, are a [`FormDataError::Parse`] before the body is read.
    pub async fn parse(content_type: Option<&str>, body: Body, limits: FormLimits) -> Result<FormData, FormDataError> {
        FormData::parse_with(content_type, body, limits, |upload: FileUpload| std::future::ready(Some(FormValue::File(upload.file)))).await
    }

    /// [`FormData::parse`] with an upload handler (`parseFormData(request, options,
    /// uploadHandler)`): each file part is passed to `upload_handler` as soon as it has been
    /// read, and the value it returns (a stored file's URL, the file itself, or `None` to
    /// drop it) goes into the form. Fields without a name are skipped. Empty file inputs
    /// arrive as empty files, as in the browser's `FormData`. The handler may return a
    /// [`Result`] (see [`UploadResult`]); its error ends parsing as [`FormDataError::Upload`].
    pub async fn parse_with<F, Fut>(content_type: Option<&str>, body: Body, limits: FormLimits, mut upload_handler: F) -> Result<FormData, FormDataError>
    where
        F: FnMut(FileUpload) -> Fut,
        Fut: std::future::Future,
        Fut::Output: UploadResult,
    {
        let (body, boundary) = match form_body(content_type, body, limits).await? {
            FormBody::UrlEncoded(form) => return Ok(form),
            FormBody::Multipart(body, boundary) => (body, boundary),
        };
        let multipart_error = FormDataError::from_multipart;
        let mut parts = parse_multipart_stream(body, &boundary, &limits.multipart()).map_err(multipart_error)?;
        let mut form = FormData::new();
        let mut files = 0;
        while let Some(part) = parts.next_part().await {
            let part = part.map_err(multipart_error)?;
            let Some(name) = part.name().filter(|n| !n.is_empty()) else { continue };
            if part.is_file() {
                files += 1;
                if files > limits.max_files {
                    return Err(FormDataError::MaxFilesExceeded(limits.max_files));
                }
                let file_name = part.filename().unwrap_or_else(|| "file-upload".into());
                let media_type = part.media_type().unwrap_or_else(|| "application/octet-stream".into());
                let file = File::new(file_name, media_type, part.bytes());
                let value = upload_handler(FileUpload { field_name: name.clone(), file }).await.into_upload_result();
                match value.map_err(|e| FormDataError::Upload(Arc::from(e)))? {
                    Some(FormValue::Text(text)) => form.append(name, text),
                    Some(FormValue::File(file)) => form.append_file(name, file),
                    None => {}
                }
            } else {
                let text = part.text().map_err(FormDataError::cannot_parse)?;
                form.append(name, text);
            }
        }
        Ok(form)
    }
}

enum FormBody {
    UrlEncoded(FormData),
    Multipart(Body, String),
}

/// A URL-encoded form, or a multipart body and its boundary. Other media types, and a
/// missing `Content-Type`, are a [`FormDataError::Parse`] before the body is read.
async fn form_body(content_type: Option<&str>, body: Body, limits: FormLimits) -> Result<FormBody, FormDataError> {
    let ct = content_type.unwrap_or("");
    let essence = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if essence == "application/x-www-form-urlencoded" {
        let bytes = read_url_encoded(body, limits).await?;
        return Ok(FormBody::UrlEncoded(SearchParams::parse(&String::from_utf8_lossy(&bytes)).into()));
    }
    if !essence.starts_with("multipart/") {
        return Err(FormDataError::Parse("Cannot parse form data".into()));
    }
    let boundary = get_multipart_boundary(ct).ok_or_else(|| FormDataError::cannot_parse("Invalid Content-Type header: missing boundary"))?;
    Ok(FormBody::Multipart(body, boundary))
}

impl From<SearchParams> for FormData {
    fn from(p: SearchParams) -> FormData {
        FormData { entries: p.iter().map(|(k, v)| (k.to_string(), FormValue::Text(v.to_string()))).collect() }
    }
}

async fn read_limited(mut body: Body, max: usize) -> Result<Vec<u8>, FormDataError> {
    let mut buf = Vec::new();
    while let Some(chunk) = body.chunk().await {
        buf.extend_from_slice(&chunk.map_err(FormDataError::cannot_parse)?);
        if buf.len() > max {
            return Err(FormDataError::Limit(MultipartError::MaxTotalSizeExceeded(max)));
        }
    }
    Ok(buf)
}

/// Read a URL-encoded body, counting `&`-separated pairs against `max_parts` as it arrives.
async fn read_url_encoded(body: Body, limits: FormLimits) -> Result<Vec<u8>, FormDataError> {
    let bytes = read_limited(body, limits.total_size()).await?;
    let pairs = bytes.split(|b| *b == b'&').filter(|pair| !pair.is_empty()).count();
    if pairs > limits.max_parts {
        return Err(FormDataError::Limit(MultipartError::MaxPartsExceeded(limits.max_parts)));
    }
    Ok(bytes)
}

pub mod future;

/// A parameter from a header value such as `multipart/form-data; boundary=x`.
pub fn header_param(value: &str, name: &str) -> Option<String> {
    value.split(';').skip(1).find_map(|p| {
        let (k, v) = p.split_once('=')?;
        if !k.trim().eq_ignore_ascii_case(name) {
            return None;
        }
        let v = v.trim();
        Some(v.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(v).to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_on;

    #[test]
    fn parses_multipart() {
        let body = "--XyZ\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nHello world\r\n--XyZ\r\nContent-Disposition: form-data; name=\"cover\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nfile\r\nbody\r\n--XyZ--\r\n";
        let form = block_on(FormData::parse(Some("multipart/form-data; boundary=XyZ"), Body::from(body), FormLimits::default())).unwrap();
        assert_eq!(form.get("title"), Some("Hello world"));
        let f = form.file("cover").unwrap();
        assert_eq!(f.name, "a.txt");
        assert_eq!(f.content_type, "text/plain");
        assert_eq!(&f.bytes[..], b"file\r\nbody");
    }

    #[test]
    fn streams_files_to_an_upload_handler() {
        let body = "--b\r\nContent-Disposition: form-data; name=\"a\"; filename=\"one.txt\"\r\n\r\n1\r\n\
                    --b\r\nContent-Disposition: form-data; name=\"empty\"; filename=\"\"\r\nContent-Type: application/octet-stream\r\n\r\n\r\n\
                    --b\r\nContent-Disposition: form-data; name=\"raw\"\r\nContent-Type: application/octet-stream\r\n\r\nxyz\r\n\
                    --b\r\nContent-Disposition: form-data\r\n\r\nnameless\r\n--b--";
        let mut seen = Vec::new();
        let form = block_on(FormData::parse_with(Some("multipart/form-data; boundary=b"), Body::from(body), FormLimits::default(), |upload: FileUpload| {
            seen.push((upload.field_name.clone(), upload.file.name.clone(), upload.file.content_type.clone(), upload.file.size()));
            std::future::ready((upload.field_name != "a").then(|| FormValue::Text(format!("stored:{}", upload.field_name))))
        }))
        .unwrap();
        assert_eq!(
            seen,
            [
                ("a".into(), "one.txt".into(), "application/octet-stream".into(), 1),
                ("empty".into(), String::new(), "application/octet-stream".into(), 0),
                ("raw".into(), "file-upload".into(), "application/octet-stream".into(), 3),
            ]
        );
        assert_eq!(form.to_search_params().to_string(), "empty=stored%3Aempty&raw=stored%3Araw");

        let err = block_on(FormData::parse(Some("multipart/form-data; boundary=b"), Body::from(body), FormLimits { max_files: 2, ..Default::default() })).unwrap_err();
        assert_eq!(err, FormDataError::MaxFilesExceeded(2));
        let err = block_on(FormData::parse(Some("multipart/form-data"), Body::from(body), FormLimits::default())).unwrap_err();
        assert_eq!(err.to_string(), "Cannot parse form data: Invalid Content-Type header: missing boundary");
        assert!(!err.is_limit());
        let err = block_on(FormData::parse(Some("multipart/form-data; boundary=b"), Body::from(body), FormLimits { max_file_size: 2, ..Default::default() })).unwrap_err();
        assert_eq!(err.to_string(), "File size exceeds maximum allowed size of 2 bytes");
        let err = block_on(FormData::parse(Some("application/x-www-form-urlencoded"), Body::from("a=1234"), FormLimits { max_total_size: Some(3), ..Default::default() })).unwrap_err();
        assert!(err.is_limit());
    }

    #[test]
    fn parses_urlencoded_and_ignores_other_types() {
        let form = block_on(FormData::parse(Some("application/x-www-form-urlencoded"), Body::from("a=1&b=x%20y"), FormLimits::default())).unwrap();
        assert_eq!(form.get("b"), Some("x y"));
        let err = block_on(FormData::parse(Some("application/json"), Body::from("{}"), FormLimits::default())).unwrap_err();
        assert_eq!(err, FormDataError::Parse("Cannot parse form data".into()));
    }
}
