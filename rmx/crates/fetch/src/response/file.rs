//! File responses (Remix's `createFileResponse` from `remix/response/file`).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use bytes::Bytes;
use http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use rmx_crypto::DigestAlgorithm;
use rmx_headers::{format_http_date, parse_http_date, ContentRange, IfMatch, IfNoneMatch, IfRange, Range};

use crate::{Body, BoxError, File, Request, Response};

/// What [file] can serve (Remix's `FileLike`): the in-memory [File], or a lazy file that
/// reads its content only when streamed (`rmx_lazy_file::LazyFile`).
pub trait FileLike: Send + Sync {
    fn name(&self) -> &str;

    /// `size` in bytes.
    fn size(&self) -> u64;

    /// `type`: the MIME type, or `""`.
    fn content_type(&self) -> &str;

    /// `lastModified`: milliseconds since the Unix epoch.
    fn last_modified(&self) -> u64;

    /// The whole content as a stream.
    fn stream(&self) -> Body;

    /// The bytes from `start` (inclusive) to `end` (exclusive) as a stream
    /// (`slice(start, end).stream()`).
    fn stream_range(&self, start: u64, end: u64) -> Body;

    /// The whole content in memory (`arrayBuffer()`), for a strong `ETag`.
    fn bytes(&self) -> impl Future<Output = Result<Bytes, BoxError>> + Send;
}

impl FileLike for File {
    fn name(&self) -> &str {
        &self.name
    }

    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn content_type(&self) -> &str {
        &self.content_type
    }

    fn last_modified(&self) -> u64 {
        self.last_modified
    }

    fn stream(&self) -> Body {
        Body::from(self.bytes.clone())
    }

    fn stream_range(&self, start: u64, end: u64) -> Body {
        Body::from(self.bytes.slice(start as usize..end as usize))
    }

    fn bytes(&self) -> impl Future<Output = Result<Bytes, BoxError>> + Send {
        std::future::ready(Ok(self.bytes.clone()))
    }
}

/// Which `ETag` to send.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ETag {
    /// No `ETag`; `If-Match` and `If-None-Match` are ignored (`etag: false`).
    Off,
    /// `W/"<size>-<lastModified>"`, computed without reading the file (the default).
    #[default]
    Weak,
    /// A quoted digest of the content, by [FileResponseOptions::digest].
    Strong,
}

/// A custom digest for a strong `ETag` (Remix's `FileDigestFunction`): the file in, the
/// `ETag` value without quotes out.
pub type DigestFn<F> = Arc<dyn Fn(&F) -> Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync>;

/// How a strong `ETag` is computed: a Web Crypto digest of the content, hex encoded
/// (SHA-256 by default), or a function of the file. An algorithm name Web Crypto does not
/// support (`"MD5"`) fails to parse as a [DigestAlgorithm].
pub enum Digest<F = File> {
    Algorithm(DigestAlgorithm),
    With(DigestFn<F>),
}

impl<F> Digest<F> {
    /// A digest computed by `digest` from the file.
    pub fn with<Fut>(digest: impl Fn(&F) -> Fut + Send + Sync + 'static) -> Digest<F>
    where
        Fut: Future<Output = String> + Send + 'static,
    {
        Digest::With(Arc::new(move |file: &F| -> Pin<Box<dyn Future<Output = String> + Send>> { Box::pin(digest(file)) }))
    }
}

impl<F> Default for Digest<F> {
    fn default() -> Digest<F> {
        Digest::Algorithm(DigestAlgorithm::Sha256)
    }
}

impl<F> Clone for Digest<F> {
    fn clone(&self) -> Digest<F> {
        match self {
            Digest::Algorithm(algorithm) => Digest::Algorithm(*algorithm),
            Digest::With(digest) => Digest::With(digest.clone()),
        }
    }
}

impl<F> From<DigestAlgorithm> for Digest<F> {
    fn from(algorithm: DigestAlgorithm) -> Digest<F> {
        Digest::Algorithm(algorithm)
    }
}

/// Options for [file] (Remix's `FileResponseOptions`).
pub struct FileResponseOptions<F = File> {
    /// `Cache-Control` for full and partial responses (default: none).
    pub cache_control: Option<String>,
    /// Default [ETag::Weak].
    pub etag: ETag,
    /// How [ETag::Strong] is computed (default: SHA-256).
    pub digest: Digest<F>,
    /// Send `Last-Modified` and honor `If-Modified-Since`, `If-Unmodified-Since` and date
    /// `If-Range` values (default: true).
    pub last_modified: bool,
    /// Serve byte ranges. `None` (the default) serves them only for types that are not
    /// compressible, since a compressed response cannot be sliced.
    pub accept_ranges: Option<bool>,
}

impl<F> Default for FileResponseOptions<F> {
    fn default() -> FileResponseOptions<F> {
        FileResponseOptions { cache_control: None, etag: ETag::Weak, digest: Digest::default(), last_modified: true, accept_ranges: None }
    }
}

impl<F> Clone for FileResponseOptions<F> {
    fn clone(&self) -> FileResponseOptions<F> {
        FileResponseOptions {
            cache_control: self.cache_control.clone(),
            etag: self.etag,
            digest: self.digest.clone(),
            last_modified: self.last_modified,
            accept_ranges: self.accept_ranges,
        }
    }
}

/// A response for `file` that honors the request's conditional and range headers (Remix's
/// `createFileResponse(file, request, options)`):
///
/// 1. `Content-Type` from the file's type (with a charset where one applies), `Content-Length`,
///    an `ETag`, `Last-Modified`, `Cache-Control`, `Accept-Ranges`, and `X-Content-Type-Options:
///    nosniff` on every response.
/// 2. `If-Match`, then `If-Unmodified-Since` (without `If-Match`): 412.
/// 3. `If-None-Match`, then `If-Modified-Since` (without `If-None-Match`): 304.
/// 4. A single `Range` on a `GET`, unless `If-Range` says the file changed: 206, or 416 when
///    unsatisfiable or multipart, 400 when malformed.
///
/// A `HEAD` gets the headers without a body. Only the requested range is streamed, so a lazy
/// file reads just that part. Fails when computing a strong `ETag` fails to read the file.
///
/// The future does not borrow `request`. In a handler, create it before awaiting so the borrow
/// of the context ends first (a `Context` is not `Sync`):
///
/// ```ignore
/// let response = response::file(&file, ctx.request(), &options);
/// response.await
/// ```
pub fn file<'a, F: FileLike>(file: &'a F, request: &Request, options: &'a FileResponseOptions<F>) -> impl Future<Output = Result<Response, BoxError>> + Send + 'a {
    let method = request.method().clone();
    let headers = request.headers().clone();
    async move { respond(file, &method, &headers, options).await }
}

/// The parts of a response Remix's `buildResponseHeaders` may set.
#[derive(Default)]
struct Headers<'a> {
    content_type: Option<&'a str>,
    content_length: Option<u64>,
    content_range: Option<String>,
    etag: Option<&'a str>,
    last_modified: Option<u64>,
    cache_control: Option<&'a str>,
    accept_ranges: bool,
}

fn build(status: StatusCode, body: Body, values: Headers<'_>) -> Response {
    let mut res = Response::new(body);
    *res.status_mut() = status;
    let h = res.headers_mut();
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    let mut set = |name: HeaderName, value: Option<String>| {
        if let Some(value) = value.and_then(|v| HeaderValue::from_str(&v).ok()) {
            h.insert(name, value);
        }
    };
    set(header::CONTENT_TYPE, values.content_type.filter(|t| !t.is_empty()).map(str::to_string));
    set(header::CONTENT_LENGTH, values.content_length.map(|n| n.to_string()));
    set(header::CONTENT_RANGE, values.content_range);
    set(header::ETAG, values.etag.filter(|e| !e.is_empty()).map(str::to_string));
    set(header::LAST_MODIFIED, values.last_modified.map(|ms| format_http_date(UNIX_EPOCH + Duration::from_millis(ms))));
    set(header::CACHE_CONTROL, values.cache_control.filter(|c| !c.is_empty()).map(str::to_string));
    set(header::ACCEPT_RANGES, values.accept_ranges.then(|| "bytes".to_string()));
    res
}

async fn respond<F: FileLike>(file: &F, method: &Method, headers: &HeaderMap, options: &FileResponseOptions<F>) -> Result<Response, BoxError> {
    let get = |name: HeaderName| headers.get(name).and_then(|v| v.to_str().ok());
    let content_type = rmx_mime::mime_type_to_content_type(file.content_type());
    let size = file.size();
    let etag = match options.etag {
        ETag::Off => None,
        ETag::Weak => Some(format!("W/\"{size}-{}\"", file.last_modified())),
        ETag::Strong => {
            let digest = match &options.digest {
                Digest::Algorithm(algorithm) => rmx_crypto::hex(&algorithm.digest(&file.bytes().await?)),
                Digest::With(digest) => digest(file).await,
            };
            Some(format!("\"{digest}\""))
        }
    };
    let last_modified = options.last_modified.then(|| file.last_modified());
    // Remix tests these as JavaScript values, where 0 is false.
    let has_last_modified = last_modified.is_some_and(|ms| ms != 0);
    let accept_ranges = options.accept_ranges.unwrap_or_else(|| !rmx_mime::is_compressible_mime_type(&content_type));
    let validators = || Headers { etag: etag.as_deref(), last_modified, accept_ranges, ..Headers::default() };
    let seconds = |ms: u64| ms / 1000;
    let header_seconds = |name: HeaderName| get(name).and_then(parse_http_date).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs());

    let has_if_match = headers.contains_key(header::IF_MATCH);
    if let (Some(tag), true) = (&etag, has_if_match) {
        if !IfMatch::parse(get(header::IF_MATCH)).matches(tag) {
            return Ok(build(StatusCode::PRECONDITION_FAILED, Body::from("Precondition Failed"), validators()));
        }
    }
    if has_last_modified && !has_if_match {
        let modified = seconds(last_modified.unwrap_or_default());
        if header_seconds(header::IF_UNMODIFIED_SINCE).is_some_and(|since| modified > since) {
            return Ok(build(StatusCode::PRECONDITION_FAILED, Body::from("Precondition Failed"), validators()));
        }
    }
    if etag.is_some() || has_last_modified {
        let if_none_match = IfNoneMatch::parse(get(header::IF_NONE_MATCH));
        let not_modified = match &etag {
            Some(tag) if if_none_match.matches(tag) => true,
            _ if has_last_modified && if_none_match.tags.is_empty() => {
                let modified = seconds(last_modified.unwrap_or_default());
                header_seconds(header::IF_MODIFIED_SINCE).is_some_and(|since| modified <= since)
            }
            _ => false,
        };
        if not_modified {
            return Ok(build(StatusCode::NOT_MODIFIED, Body::empty(), validators()));
        }
    }

    let full = |status: StatusCode, body: Body, content_length: u64, content_range: Option<String>| {
        let values = Headers {
            content_type: Some(&content_type),
            content_length: Some(content_length),
            content_range,
            cache_control: options.cache_control.as_deref(),
            ..validators()
        };
        build(status, body, values)
    };
    if accept_ranges && method == Method::GET && headers.contains_key(header::RANGE) {
        let range = Range::parse(get(header::RANGE));
        if range.ranges.is_empty() {
            return Ok(build(StatusCode::BAD_REQUEST, Body::from("Bad Request"), Headers::default()));
        }
        let modified = last_modified.filter(|_| has_last_modified).map(|ms| UNIX_EPOCH + Duration::from_millis(ms));
        if IfRange::parse(get(header::IF_RANGE)).matches(etag.as_deref(), modified) {
            let [(start, end)] = range.normalize(size)[..] else {
                let unsatisfied = Headers { content_range: Some(ContentRange::unsatisfied(size).to_string()), ..Headers::default() };
                return Ok(build(StatusCode::RANGE_NOT_SATISFIABLE, Body::from("Range Not Satisfiable"), unsatisfied));
            };
            let content_range = ContentRange::bytes(start, end, size).to_string();
            return Ok(full(StatusCode::PARTIAL_CONTENT, file.stream_range(start, end + 1), end - start + 1, Some(content_range)));
        }
    }
    let body = if method == Method::HEAD { Body::empty() } else { file.stream() };
    Ok(full(StatusCode::OK, body, size, None))
}
