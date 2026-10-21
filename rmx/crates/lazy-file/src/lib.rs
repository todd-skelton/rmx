//! Lazy, streaming files and blobs, ported from Remix's `lazy-file`.
//!
//! A [LazyFile] or [LazyBlob] knows its size and type up front and reads its content only
//! when streamed. Content is either a list of [BlobPart]s (bytes, strings, other lazy blobs
//! and files) or a [LazyContent] source such as a file on disk (`rmx_fs::open_lazy_file`).
//! [LazyBlob::slice] narrows the byte range without reading anything, so serving part of a
//! large file streams only that part.
//!
//! These are not the in-memory [File]: use [LazyFile::stream] for responses and other
//! streaming uses, or [LazyFile::to_file] when an API needs the whole file in memory. Neither
//! type implements `Display`, which is how Remix's throwing `toString()` shows up here: a
//! lazy file cannot be turned into a string by accident.
//!
//! ```
//! use rmx_lazy_file::{BlobPart, LazyFile, LazyFileOptions};
//!
//! let file = LazyFile::new(
//!     [BlobPart::from("hello "), BlobPart::from("world")],
//!     "hello.txt",
//!     LazyFileOptions { content_type: "text/plain".into(), ..Default::default() },
//! );
//! assert_eq!(file.size(), 11);
//! assert_eq!(rmx_fetch::block_on(file.slice(-5, None).text()).unwrap(), "world");
//! ```

use std::fmt;
use std::sync::Arc;

use bytes::Bytes;
use rmx_fetch::{Body, BodySender, BoxError, File};

/// A range of bytes: `start` is inclusive and `end` exclusive. Negative values count back from
/// the end of the content, and [ByteRange::END] (`i64::MAX`) stands for JavaScript's `Infinity`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteRange {
    pub start: i64,
    pub end: i64,
}

impl ByteRange {
    /// An offset past any content (`Infinity`).
    pub const END: i64 = i64::MAX;

    pub const fn new(start: i64, end: i64) -> ByteRange {
        ByteRange { start, end }
    }
}

/// The range resolved to absolute `[start, end)` indexes in content of `size` bytes. Both are
/// clamped to the content, and `end` is never before `start`.
pub fn get_indexes(range: ByteRange, size: u64) -> (u64, u64) {
    let size = i64::try_from(size).unwrap_or(i64::MAX);
    let resolve = |value: i64| if value < 0 { size.saturating_add(value) } else { value };
    let start = resolve(range.start).max(0).min(size);
    let end = resolve(range.end).max(start).min(size);
    (start as u64, end as u64)
}

/// How many bytes the range covers in content of `size` bytes.
pub fn get_byte_length(range: ByteRange, size: u64) -> u64 {
    let (start, end) = get_indexes(range, size);
    end - start
}

/// Content that is read on demand (Remix's `LazyContent`).
pub trait LazyContent: Send + Sync + 'static {
    /// The total length in bytes.
    fn byte_length(&self) -> u64;

    /// A stream of the bytes from `start` (inclusive) to `end` (exclusive). Both are within
    /// `0..=byte_length()` and `start <= end`.
    fn stream(&self, start: u64, end: u64) -> Body;
}

/// One piece of a blob's content (`BlobPart`, plus lazy blobs and files).
#[derive(Clone, Debug)]
pub enum BlobPart {
    Bytes(Bytes),
    Blob(LazyBlob),
    File(LazyFile),
}

impl BlobPart {
    pub fn size(&self) -> u64 {
        match self {
            BlobPart::Bytes(bytes) => bytes.len() as u64,
            BlobPart::Blob(blob) => blob.size(),
            BlobPart::File(file) => file.size(),
        }
    }
}

impl From<Bytes> for BlobPart {
    fn from(bytes: Bytes) -> BlobPart {
        BlobPart::Bytes(bytes)
    }
}

impl From<Vec<u8>> for BlobPart {
    fn from(bytes: Vec<u8>) -> BlobPart {
        BlobPart::Bytes(bytes.into())
    }
}

impl From<&[u8]> for BlobPart {
    fn from(bytes: &[u8]) -> BlobPart {
        BlobPart::Bytes(Bytes::copy_from_slice(bytes))
    }
}

/// Text is stored as UTF-8, like `TextEncoder`.
impl From<&str> for BlobPart {
    fn from(text: &str) -> BlobPart {
        BlobPart::Bytes(Bytes::copy_from_slice(text.as_bytes()))
    }
}

impl From<String> for BlobPart {
    fn from(text: String) -> BlobPart {
        BlobPart::Bytes(text.into())
    }
}

/// An in-memory file contributes its bytes, as a native `Blob` part does.
impl From<File> for BlobPart {
    fn from(file: File) -> BlobPart {
        BlobPart::Bytes(file.bytes)
    }
}

impl From<LazyBlob> for BlobPart {
    fn from(blob: LazyBlob) -> BlobPart {
        BlobPart::Blob(blob)
    }
}

impl From<LazyFile> for BlobPart {
    fn from(file: LazyFile) -> BlobPart {
        BlobPart::File(file)
    }
}

#[derive(Clone)]
enum Source {
    Parts(Arc<[BlobPart]>),
    Lazy(Arc<dyn LazyContent>),
}

#[derive(Clone)]
struct BlobContent {
    source: Source,
    total_size: u64,
    range: Option<ByteRange>,
    content_type: String,
}

impl BlobContent {
    fn parts(parts: impl IntoIterator<Item = BlobPart>, range: Option<ByteRange>, content_type: String) -> BlobContent {
        let parts: Arc<[BlobPart]> = parts.into_iter().collect();
        let total_size = parts.iter().map(BlobPart::size).sum();
        BlobContent { source: Source::Parts(parts), total_size, range, content_type }
    }

    fn lazy(content: Arc<dyn LazyContent>, range: Option<ByteRange>, content_type: String) -> BlobContent {
        BlobContent { total_size: content.byte_length(), source: Source::Lazy(content), range, content_type }
    }

    fn bounds(&self) -> (u64, u64) {
        match self.range {
            Some(range) => get_indexes(range, self.total_size),
            None => (0, self.total_size),
        }
    }

    fn size(&self) -> u64 {
        let (start, end) = self.bounds();
        end - start
    }

    fn slice(&self, start: i64, end: Option<i64>) -> LazyBlob {
        let (source_start, source_end) = self.bounds();
        let source_size = source_end - source_start;
        let end = end.unwrap_or(i64::try_from(source_size).unwrap_or(i64::MAX));
        let (slice_start, slice_end) = get_indexes(ByteRange { start, end }, source_size);
        let range = ByteRange { start: (source_start + slice_start) as i64, end: (source_start + slice_end) as i64 };
        LazyBlob { content: BlobContent { source: self.source.clone(), total_size: self.total_size, range: Some(range), content_type: String::new() } }
    }

    fn stream(&self) -> Body {
        let (start, end) = self.bounds();
        match &self.source {
            Source::Lazy(content) => content.stream(start, end),
            Source::Parts(parts) => stream_parts(parts.clone(), start, end),
        }
    }

    async fn bytes(&self) -> Result<Bytes, BoxError> {
        self.stream().bytes().await
    }

    async fn text(&self) -> Result<String, BoxError> {
        Ok(String::from_utf8_lossy(&self.bytes().await?).into_owned())
    }
}

/// Stream `[start, end)` of the concatenated parts, skipping whole parts before `start` and
/// stopping once `end` is reached.
fn stream_parts(parts: Arc<[BlobPart]>, start: u64, end: u64) -> Body {
    Body::from_task(move |tx| async move {
        let mut read = 0;
        for part in parts.iter() {
            if read >= end {
                break;
            }
            let size = part.size();
            if read + size <= start {
                read += size;
                continue;
            }
            let mut body = match part {
                BlobPart::Bytes(bytes) => {
                    if !push(&tx, bytes.clone(), read, start, end) {
                        return;
                    }
                    read += size;
                    continue;
                }
                BlobPart::Blob(blob) => blob.stream(),
                BlobPart::File(file) => file.stream(),
            };
            while let Some(chunk) = body.chunk().await {
                match chunk {
                    Ok(chunk) => {
                        let len = chunk.len() as u64;
                        if !push(&tx, chunk, read, start, end) {
                            return;
                        }
                        read += len;
                        if read >= end {
                            break;
                        }
                    }
                    Err(error) => {
                        tx.abort(error);
                        return;
                    }
                }
            }
        }
    })
}

/// Send the part of `chunk` (which starts at offset `read`) inside `[start, end)`. False once
/// the reader is gone.
fn push(tx: &BodySender, chunk: Bytes, read: u64, start: u64, end: u64) -> bool {
    let len = chunk.len() as u64;
    let from = start.saturating_sub(read).min(len);
    let to = end.saturating_sub(read).min(len);
    from >= to || tx.send(chunk.slice(from as usize..to as usize))
}

fn now_ms() -> u64 {
    // `SystemTime` panics in wasm32.
    #[cfg(target_arch = "wasm32")]
    return js_sys::Date::now() as u64;
    #[cfg(not(target_arch = "wasm32"))]
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Options for a [LazyBlob].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LazyBlobOptions {
    /// The bytes of the content to include. Default: all of it.
    pub range: Option<ByteRange>,
    /// The MIME type. Default `""`.
    pub content_type: String,
}

/// A lazy, streaming alternative to `Blob`.
#[derive(Clone)]
pub struct LazyBlob {
    content: BlobContent,
}

impl LazyBlob {
    /// A blob of `parts`, concatenated.
    pub fn new(parts: impl IntoIterator<Item = BlobPart>, options: LazyBlobOptions) -> LazyBlob {
        LazyBlob { content: BlobContent::parts(parts, options.range, options.content_type) }
    }

    /// A blob read on demand from `content`.
    pub fn from_content(content: impl LazyContent, options: LazyBlobOptions) -> LazyBlob {
        LazyBlob { content: BlobContent::lazy(Arc::new(content), options.range, options.content_type) }
    }

    /// `size` in bytes.
    pub fn size(&self) -> u64 {
        self.content.size()
    }

    /// `type`: the MIME type, or `""`.
    pub fn content_type(&self) -> &str {
        &self.content.content_type
    }

    /// The bytes from `start` to `end` (exclusive; `None` for the end), as a new blob with no
    /// type. Negative indexes count back from the end. Nothing is read.
    pub fn slice(&self, start: i64, end: impl Into<Option<i64>>) -> LazyBlob {
        self.content.slice(start, end.into())
    }

    /// The same content with another MIME type (`slice`'s `contentType` argument).
    pub fn with_type(mut self, content_type: impl Into<String>) -> LazyBlob {
        self.content.content_type = content_type.into();
        self
    }

    /// The content as a stream.
    pub fn stream(&self) -> Body {
        self.content.stream()
    }

    /// The content in memory (`bytes()` and `arrayBuffer()`).
    pub async fn bytes(&self) -> Result<Bytes, BoxError> {
        self.content.bytes().await
    }

    /// The content decoded as UTF-8, with invalid sequences replaced like `TextDecoder`.
    pub async fn text(&self) -> Result<String, BoxError> {
        self.content.text().await
    }

    /// The whole content in memory as a [File] with no name (rmx has no separate `Blob`
    /// type). Reads everything; prefer [LazyBlob::stream].
    pub async fn to_blob(&self) -> Result<File, BoxError> {
        Ok(File::new("", self.content_type(), self.bytes().await?))
    }
}

impl fmt::Debug for LazyBlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LazyBlob").field("size", &self.size()).field("type", &self.content_type()).finish()
    }
}

/// Options for a [LazyFile].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LazyFileOptions {
    /// The bytes of the content to include. Default: all of it.
    pub range: Option<ByteRange>,
    /// The MIME type. Default `""`.
    pub content_type: String,
    /// Milliseconds since the Unix epoch. Default: now.
    pub last_modified: Option<u64>,
}

/// A lazy, streaming alternative to `File`.
#[derive(Clone)]
pub struct LazyFile {
    content: BlobContent,
    name: String,
    last_modified: u64,
}

impl LazyFile {
    /// A file of `parts`, concatenated.
    pub fn new(parts: impl IntoIterator<Item = BlobPart>, name: impl Into<String>, options: LazyFileOptions) -> LazyFile {
        let last_modified = options.last_modified.unwrap_or_else(now_ms);
        LazyFile { content: BlobContent::parts(parts, options.range, options.content_type), name: name.into(), last_modified }
    }

    /// A file read on demand from `content`.
    pub fn from_content(content: impl LazyContent, name: impl Into<String>, options: LazyFileOptions) -> LazyFile {
        let last_modified = options.last_modified.unwrap_or_else(now_ms);
        LazyFile { content: BlobContent::lazy(Arc::new(content), options.range, options.content_type), name: name.into(), last_modified }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// `lastModified`: milliseconds since the Unix epoch.
    pub fn last_modified(&self) -> u64 {
        self.last_modified
    }

    /// `size` in bytes.
    pub fn size(&self) -> u64 {
        self.content.size()
    }

    /// `type`: the MIME type, or `""`.
    pub fn content_type(&self) -> &str {
        &self.content.content_type
    }

    /// The bytes from `start` to `end` (exclusive; `None` for the end). Like `File.slice()`,
    /// the result is a blob, with no type. Nothing is read.
    pub fn slice(&self, start: i64, end: impl Into<Option<i64>>) -> LazyBlob {
        self.content.slice(start, end.into())
    }

    /// The content as a stream.
    pub fn stream(&self) -> Body {
        self.content.stream()
    }

    /// The content in memory (`bytes()` and `arrayBuffer()`).
    pub async fn bytes(&self) -> Result<Bytes, BoxError> {
        self.content.bytes().await
    }

    /// The content decoded as UTF-8, with invalid sequences replaced like `TextDecoder`.
    pub async fn text(&self) -> Result<String, BoxError> {
        self.content.text().await
    }

    /// The whole file in memory, with its name, type and `lastModified`. Reads everything;
    /// prefer [LazyFile::stream].
    pub async fn to_file(&self) -> Result<File, BoxError> {
        let mut file = File::new(self.name.clone(), self.content_type(), self.bytes().await?);
        file.last_modified = self.last_modified;
        Ok(file)
    }

    /// The whole content in memory as a [File] with no name (rmx has no separate `Blob` type).
    pub async fn to_blob(&self) -> Result<File, BoxError> {
        Ok(File::new("", self.content_type(), self.bytes().await?))
    }
}

impl fmt::Debug for LazyFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LazyFile").field("name", &self.name).field("size", &self.size()).field("type", &self.content_type()).field("last_modified", &self.last_modified).finish()
    }
}
