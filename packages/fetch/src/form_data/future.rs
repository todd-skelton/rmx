//! Streaming uploads (Remix's `form-data-parser/future`).
//!
//! [parse_with] hands each file to the upload handler as a [FileUpload] as soon as its
//! headers arrive, and the file's content streams from the request as the handler reads it,
//! so a large upload can go to storage without being held in memory. Parts are read in
//! order: the parser waits for the handler before moving on, and content the handler does
//! not read is discarded. A handler that returns the upload itself (and the default, with no
//! handler) gets it read into a [File].
//!
//! ```
//! use rsmix_fetch::form_data::future::{parse_with, FileUpload};
//! use rsmix_fetch::{Body, FormDataError, FormLimits};
//!
//! # rsmix_fetch::block_on(async {
//! let body = Body::from("--b\r\nContent-Disposition: form-data; name=\"cover\"; filename=\"a.txt\"\r\n\r\nhello\r\n--b--");
//! let form = parse_with(Some("multipart/form-data; boundary=b"), body, FormLimits::default(), |mut upload: FileUpload| async move {
//!     let mut stored = 0;
//!     while let Some(chunk) = upload.chunk().await {
//!         stored += chunk?.len(); // write the chunk to storage here
//!     }
//!     Ok::<_, FormDataError>(format!("/uploads/{}?size={stored}", upload.name))
//! })
//! .await
//! .unwrap();
//! assert_eq!(form.get("cover"), Some("/uploads/a.txt?size=5"));
//! # });
//! ```

use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

use bytes::Bytes;
use rsmix_multipart_parser::future::{parse_multipart_stream, MultipartStream};

use super::{form_body, File, FormBody, FormData, FormDataError, FormLimits, FormValue};
use crate::{Body, BoxError};

/// Boxed, so an upload that carries it stays small to move.
type Parts = Box<MultipartStream<Body>>;
type Home = Arc<Mutex<Option<Parts>>>;

/// A file part, given to an upload handler as soon as its headers arrive. Its content
/// streams from the request: read it with [FileUpload::chunk], [FileUpload::bytes],
/// [FileUpload::text] or [FileUpload::into_file]. It can be read once, while the handler
/// runs; the size is known once it has been read.
///
/// `name` and `content_type` are untrusted client input: never use the name as a filesystem
/// path, and validate the content before trusting the type.
pub struct FileUpload {
    /// The form field the file was sent under.
    pub field_name: String,
    /// The file name (`file-upload` when the part has none).
    pub name: String,
    /// The media type, normalized like [File::new] (`application/octet-stream` by default).
    pub content_type: String,
    /// When the upload was received, in milliseconds since the Unix epoch.
    pub last_modified: u64,
    /// The request's parts, on loan to this upload; given back when it is dropped.
    parts: Option<Parts>,
    home: Home,
    used: bool,
    read: u64,
    size: Option<u64>,
}

impl fmt::Debug for FileUpload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileUpload")
            .field("field_name", &self.field_name)
            .field("name", &self.name)
            .field("content_type", &self.content_type)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl Drop for FileUpload {
    fn drop(&mut self) {
        if let Some(parts) = self.parts.take() {
            *self.home.lock().unwrap_or_else(PoisonError::into_inner) = Some(parts);
        }
    }
}

impl FileUpload {
    /// The size in bytes, once the content has been read to the end. Limit uploads with
    /// [FormLimits::max_file_size].
    pub fn size(&self) -> Option<u64> {
        self.size
    }

    /// Whether reading the content has started.
    pub fn body_used(&self) -> bool {
        self.used
    }

    /// The next chunk of content, or `None` at its end. A limit error ends the request even
    /// if the handler ignores it.
    pub async fn chunk(&mut self) -> Option<Result<Bytes, FormDataError>> {
        self.used = true;
        let next = self.parts.as_mut()?.next_chunk().await;
        match next {
            Some(Ok(chunk)) => {
                self.read += chunk.len() as u64;
                Some(Ok(chunk))
            }
            Some(Err(e)) => Some(Err(FormDataError::from_multipart(e))),
            None => {
                self.size = Some(self.read);
                None
            }
        }
    }

    /// The whole content in one buffer.
    pub async fn bytes(mut self) -> Result<Bytes, FormDataError> {
        if self.used {
            return Err(misuse("The contents of a file upload can only be read once"));
        }
        let first = match self.chunk().await {
            None => return Ok(Bytes::new()),
            Some(chunk) => chunk?,
        };
        let Some(second) = self.chunk().await else { return Ok(first) };
        let mut all = first.to_vec();
        all.extend_from_slice(&second?);
        while let Some(chunk) = self.chunk().await {
            all.extend_from_slice(&chunk?);
        }
        Ok(all.into())
    }

    /// The content as UTF-8; invalid UTF-8 is a [FormDataError::Parse].
    pub async fn text(self) -> Result<String, FormDataError> {
        String::from_utf8(self.bytes().await?.into()).map_err(FormDataError::cannot_parse)
    }

    /// Read the content into a [File] with this upload's name, type and time.
    pub async fn into_file(mut self) -> Result<File, FormDataError> {
        let (name, content_type, last_modified) = (std::mem::take(&mut self.name), std::mem::take(&mut self.content_type), self.last_modified);
        let bytes = self.bytes().await?;
        Ok(File { name, content_type, bytes, last_modified })
    }
}

/// A value an upload handler puts in the form for a file.
#[derive(Debug)]
pub enum UploadValue {
    Text(String),
    File(File),
    /// The upload itself, unread: it is read into a [File].
    Upload(FileUpload),
}

impl From<String> for UploadValue {
    fn from(text: String) -> UploadValue {
        UploadValue::Text(text)
    }
}

impl From<&str> for UploadValue {
    fn from(text: &str) -> UploadValue {
        UploadValue::Text(text.to_string())
    }
}

impl From<File> for UploadValue {
    fn from(file: File) -> UploadValue {
        UploadValue::File(file)
    }
}

impl From<FileUpload> for UploadValue {
    fn from(upload: FileUpload) -> UploadValue {
        UploadValue::Upload(upload)
    }
}

impl From<FormValue> for UploadValue {
    fn from(value: FormValue) -> UploadValue {
        match value {
            FormValue::Text(text) => UploadValue::Text(text),
            FormValue::File(file) => UploadValue::File(file),
        }
    }
}

/// What an upload handler may return: a value for the form (text, a [File], a [FormValue],
/// the [FileUpload] itself), an `Option` of one (`None` and `()` leave the file out), or a
/// `Result` of either, whose error stops parsing. A [FormDataError] (such as a limit hit
/// while reading the upload) is passed through as it is; others become
/// [FormDataError::Upload].
pub trait UploadOutput {
    fn into_upload_output(self) -> Result<Option<UploadValue>, BoxError>;
}

impl UploadOutput for () {
    fn into_upload_output(self) -> Result<Option<UploadValue>, BoxError> {
        Ok(None)
    }
}

impl<T: Into<UploadValue>> UploadOutput for Option<T> {
    fn into_upload_output(self) -> Result<Option<UploadValue>, BoxError> {
        Ok(self.map(Into::into))
    }
}

impl<T: UploadOutput, E: Into<BoxError>> UploadOutput for Result<T, E> {
    fn into_upload_output(self) -> Result<Option<UploadValue>, BoxError> {
        self.map_err(Into::into)?.into_upload_output()
    }
}

macro_rules! upload_output {
    ($($ty:ty),*) => {$(
        impl UploadOutput for $ty {
            fn into_upload_output(self) -> Result<Option<UploadValue>, BoxError> {
                Ok(Some(self.into()))
            }
        }
    )*};
}

upload_output!(String, &str, File, FileUpload, FormValue, UploadValue);

/// Parse a body according to its `Content-Type`, URL-encoded or multipart, reading files
/// into memory. The same result as [FormData::parse], one part at a time.
pub async fn parse(content_type: Option<&str>, body: Body, limits: FormLimits) -> Result<FormData, FormDataError> {
    parse_with(content_type, body, limits, std::future::ready).await
}

/// Parse a body, streaming each file to `upload_handler` as soon as its headers arrive
/// (`parseFormData` from `form-data-parser/future`). The value the handler returns goes in
/// the form (see [UploadOutput]). Fields without a name are skipped.
pub async fn parse_with<F, Fut>(content_type: Option<&str>, body: Body, limits: FormLimits, mut upload_handler: F) -> Result<FormData, FormDataError>
where
    F: FnMut(FileUpload) -> Fut,
    Fut: Future,
    Fut::Output: UploadOutput,
{
    let (body, boundary) = match form_body(content_type, body, limits).await? {
        FormBody::UrlEncoded(form) => return Ok(form),
        FormBody::Multipart(body, boundary) => (body, boundary),
    };
    let home: Home = Arc::new(Mutex::new(None));
    let mut parts = Some(Box::new(parse_multipart_stream(body, &boundary, &limits.multipart()).map_err(FormDataError::from_multipart)?));
    let mut form = FormData::new();
    let mut files = 0;
    loop {
        let stream = parts.as_mut().ok_or_else(|| misuse("An upload handler cannot keep a file upload after it returns. Read it in the handler instead."))?;
        let Some(part) = stream.next_part().await else { break };
        let part = part.map_err(FormDataError::from_multipart)?;
        let headers = part.headers();
        let Some(name) = headers.name().filter(|n| !n.is_empty()) else { continue };
        if !headers.is_file() {
            form.append(name, part.text().await.map_err(FormDataError::from_multipart)?);
            continue;
        }
        files += 1;
        if files > limits.max_files {
            return Err(FormDataError::MaxFilesExceeded(limits.max_files));
        }
        let meta = File::new(headers.filename().unwrap_or_else(|| "file-upload".into()), headers.media_type().unwrap_or_else(|| "application/octet-stream".into()), Bytes::new());
        let upload = FileUpload {
            field_name: name.clone(),
            name: meta.name,
            content_type: meta.content_type,
            last_modified: meta.last_modified,
            parts: parts.take(),
            home: home.clone(),
            used: false,
            read: 0,
            size: None,
        };
        let output = upload_handler(upload).await.into_upload_output();
        parts = take(&home);
        let value = output.map_err(|e| match e.downcast::<FormDataError>() {
            Ok(e) => *e,
            Err(e) => FormDataError::Upload(Arc::from(e)),
        })?;
        match value {
            None => {}
            Some(UploadValue::Text(text)) => form.append(name, text),
            Some(UploadValue::File(file)) => form.append_file(name, file),
            Some(UploadValue::Upload(upload)) => {
                if upload.used {
                    return Err(misuse("An upload handler cannot return a file upload after reading its contents. Return the stored file or a reference to it instead."));
                }
                let file = upload.into_file().await;
                parts = take(&home);
                form.append_file(name, file?);
            }
        }
    }
    Ok(form)
}

fn take(home: &Home) -> Option<Parts> {
    home.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// A mistake in an upload handler, reported like an error it returned.
#[derive(Debug)]
struct Misuse(&'static str);

impl fmt::Display for Misuse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Misuse {}

fn misuse(message: &'static str) -> FormDataError {
    FormDataError::Upload(Arc::new(Misuse(message)))
}
