//! Response compression (Remix's `remix/response/compress`): [compress_response],
//! [create_compression_options] and [compress_stream].
//!
//! The encoding (br, gzip or deflate) is negotiated from `Accept-Encoding`: client q-values
//! win, server preference order breaks ties, and a client that refuses every encoding
//! including `identity` gets a 406. Bodies are compressed as they stream, one source chunk
//! at a time as the compressed body is read; HTML and SSE are flushed after every chunk so
//! they still arrive progressively.

use std::io::{self, Write};

use bytes::Bytes;
use flate2::write::{GzEncoder, ZlibEncoder};
use http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use rsmix_headers::{AcceptEncoding, CacheControl, Vary};

use rsmix_fetch::{Body, Response};

/// A content coding [compress_response] can produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Br,
    Gzip,
    Deflate,
}

impl Encoding {
    pub fn as_str(self) -> &'static str {
        match self {
            Encoding::Br => "br",
            Encoding::Gzip => "gzip",
            Encoding::Deflate => "deflate",
        }
    }
}

/// The default server preference: br, then gzip, then deflate.
pub const DEFAULT_ENCODINGS: [Encoding; 3] = [Encoding::Br, Encoding::Gzip, Encoding::Deflate];

/// gzip and deflate settings (Remix's `zlib` options).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZlibOptions {
    /// 0 (store) to 9 (smallest). Default 6, as in node:zlib.
    pub level: u32,
    /// `Some(true)` flushes after every body chunk (`Z_SYNC_FLUSH`), `Some(false)` never
    /// (`Z_NO_FLUSH`). `None` lets [create_compression_options] decide: flush for
    /// `text/html` and `text/event-stream`.
    pub flush: Option<bool>,
}

impl Default for ZlibOptions {
    fn default() -> Self {
        ZlibOptions { level: 6, flush: None }
    }
}

/// Brotli settings (Remix's `brotli` options).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrotliOptions {
    /// 0 to 11 (`BROTLI_PARAM_QUALITY`). Default 11, as in node:zlib.
    pub quality: u32,
    /// Window size as a power of two, 10 to 24 (`BROTLI_PARAM_LGWIN`). Default 22.
    pub lgwin: u32,
    /// `Some(true)` flushes after every body chunk (`BROTLI_OPERATION_FLUSH`), `Some(false)`
    /// never (`BROTLI_OPERATION_PROCESS`). `None` lets [create_compression_options] decide.
    pub flush: Option<bool>,
}

impl Default for BrotliOptions {
    fn default() -> Self {
        BrotliOptions { quality: 11, lgwin: 22, flush: None }
    }
}

/// Settings for [compress_response] (Remix's `CompressResponseOptions`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompressOptions {
    /// Skip responses whose `Content-Length` is below this many bytes. Responses without a
    /// `Content-Length` are compressed regardless. Default 1024.
    pub threshold: u64,
    /// Encodings the server offers, most preferred first. Empty disables compression.
    pub encodings: Vec<Encoding>,
    pub zlib: ZlibOptions,
    pub brotli: BrotliOptions,
}

impl Default for CompressOptions {
    fn default() -> Self {
        CompressOptions { threshold: 1024, encodings: DEFAULT_ENCODINGS.to_vec(), zlib: ZlibOptions::default(), brotli: BrotliOptions::default() }
    }
}

/// Compress `response` for `request` according to its `Accept-Encoding`.
///
/// Returned unchanged: empty bodies (except for HEAD), responses that already have a
/// `Content-Encoding`, a `Content-Length` below the threshold, `Cache-Control: no-transform`,
/// `Accept-Ranges: bytes`, and 206 responses. Every other response gets `Vary:
/// Accept-Encoding`, even when `identity` is chosen. A compressed response loses its
/// `Content-Length`, gets `Accept-Ranges: none`, and has a strong `ETag` made weak.
pub fn compress_response<B>(response: Response, request: &http::Request<B>, options: &CompressOptions) -> Response {
    let accept_encoding = request.headers().get(header::ACCEPT_ENCODING).and_then(|v| v.to_str().ok());
    compress(response, request.method(), accept_encoding, options)
}

/// `options` with the flush defaults for `headers`' content type: HTML and SSE flush after
/// every chunk unless `flush` is set explicitly. The media type matches case-insensitively.
pub fn create_compression_options(headers: &HeaderMap, options: &CompressOptions) -> CompressOptions {
    let media_type = header_str(headers, header::CONTENT_TYPE).map(|ct| ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase()).unwrap_or_default();
    let should_flush = media_type == "text/event-stream" || media_type == "text/html";
    let mut options = options.clone();
    if should_flush {
        options.zlib.flush = options.zlib.flush.or(Some(true));
        options.brotli.flush = options.brotli.flush.or(Some(true));
    }
    options
}

/// A streaming compressor for [compress_stream] (node:zlib's `Gzip`, `Deflate` and
/// `BrotliCompress` in Remix). Dropping it is `destroy()`.
pub trait Compressor: Send + 'static {
    /// Compress `chunk`, returning the output that is ready, in any number of pieces.
    fn write(&mut self, chunk: &[u8]) -> io::Result<Vec<Bytes>>;

    /// The input is done (`end()`): return the rest of the output.
    fn end(&mut self) -> io::Result<Vec<Bytes>>;
}

/// `input` compressed by `compressor` as it is read. A source chunk is read only after the
/// output of the previous one was taken, so a fast source does not run ahead of the reader.
/// An error from the source or the compressor fails the compressed body, and nothing more is
/// read or written after it. Dropping the compressed body drops the compressor and the
/// source, which cancels it.
pub fn compress_stream(input: Body, compressor: impl Compressor) -> Body {
    Body::from_task(move |tx| async move {
        let (mut input, mut compressor) = (input, compressor);
        while let Some(chunk) = input.chunk().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => return tx.abort(error),
            };
            if chunk.is_empty() {
                continue;
            }
            match compressor.write(&chunk) {
                Ok(out) => {
                    for piece in out.into_iter().filter(|piece| !piece.is_empty()) {
                        if !tx.send(piece) {
                            return;
                        }
                    }
                }
                Err(error) => return tx.abort(error),
            }
            if !tx.ready().await {
                return;
            }
        }
        match compressor.end() {
            Ok(out) => {
                for piece in out.into_iter().filter(|piece| !piece.is_empty()) {
                    tx.send(piece);
                }
            }
            Err(error) => tx.abort(error),
        }
    })
}

enum Selected {
    Identity,
    Encoded(Encoding),
    NotAcceptable,
}

fn negotiate(accept: &AcceptEncoding, supported: &[Encoding]) -> Selected {
    if accept.is_empty() {
        return Selected::Identity;
    }
    let names: Vec<&str> = supported.iter().map(|e| e.as_str()).collect();
    match accept.preferred(&names).and_then(|name| supported.iter().find(|e| e.as_str() == name)) {
        Some(encoding) => Selected::Encoded(*encoding),
        // A client can refuse identity explicitly (`identity;q=0`) or with `*;q=0`.
        None if accept.weight("identity") == 0.0 => Selected::NotAcceptable,
        None => Selected::Identity,
    }
}

fn header_str(headers: &HeaderMap, name: HeaderName) -> Option<&str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn set(headers: &mut HeaderMap, name: HeaderName, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        headers.insert(name, value);
    }
}

fn compress(response: Response, method: &Method, accept_encoding: Option<&str>, options: &CompressOptions) -> Response {
    let head = method == Method::HEAD;
    let headers = response.headers();
    let below_threshold = header_str(headers, header::CONTENT_LENGTH).and_then(|n| n.trim().parse::<u64>().ok()).is_some_and(|n| n < options.threshold);
    if options.encodings.is_empty()
        || (!head && response.body().is_empty_hint())
        || headers.contains_key(header::CONTENT_ENCODING)
        || below_threshold
        || CacheControl::parse(header_str(headers, header::CACHE_CONTROL)).no_transform
        || header_str(headers, header::ACCEPT_RANGES) == Some("bytes")
        || response.status() == StatusCode::PARTIAL_CONTENT
    {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let mut vary = Vary::from_headers(parts.headers.get_all(header::VARY).iter().filter_map(|v| v.to_str().ok()));
    vary.add("Accept-Encoding");
    let vary = vary.to_string();
    set(&mut parts.headers, header::VARY, &vary);

    let selected = match accept_encoding {
        Some(value) if !value.trim().is_empty() => negotiate(&AcceptEncoding::parse(Some(value)), &options.encodings),
        _ => Selected::Identity,
    };
    let encoding = match selected {
        Selected::Identity => return Response::from_parts(parts, body),
        Selected::NotAcceptable => return not_acceptable(&options.encodings, &vary),
        Selected::Encoded(encoding) => encoding,
    };

    let options = create_compression_options(&parts.headers, options);
    set(&mut parts.headers, header::CONTENT_ENCODING, encoding.as_str());
    set(&mut parts.headers, header::ACCEPT_RANGES, "none");
    parts.headers.remove(header::CONTENT_LENGTH);
    if let Some(etag) = header_str(&parts.headers, header::ETAG).filter(|e| !e.starts_with("W/")).map(|e| format!("W/{e}")) {
        set(&mut parts.headers, header::ETAG, &etag);
    }
    if head {
        return Response::from_parts(parts, Body::empty());
    }
    Response::from_parts(parts, compress_stream(body, Encoder::new(encoding, &options)))
}

fn not_acceptable(encodings: &[Encoding], vary: &str) -> Response {
    let list: Vec<String> = encodings.iter().map(|e| e.as_str()).chain(["identity"]).map(|e| format!("'{e}'")).collect();
    let mut res = Response::new(Body::from(format!("Only {} encodings are supported", list.join(", "))));
    *res.status_mut() = StatusCode::NOT_ACCEPTABLE;
    res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=UTF-8"));
    set(res.headers_mut(), header::VARY, vary);
    res
}

enum Kind {
    Gzip(GzEncoder<Vec<u8>>),
    Deflate(ZlibEncoder<Vec<u8>>),
    Br(Box<brotli::CompressorWriter<Vec<u8>>>),
}

/// The built-in compressors, flushing after every chunk when asked to.
struct Encoder {
    kind: Option<Kind>,
    flush: bool,
}

impl Encoder {
    fn new(encoding: Encoding, options: &CompressOptions) -> Encoder {
        let level = flate2::Compression::new(options.zlib.level.min(9));
        let (kind, flush) = match encoding {
            Encoding::Gzip => (Kind::Gzip(GzEncoder::new(Vec::new(), level)), options.zlib.flush),
            Encoding::Deflate => (Kind::Deflate(ZlibEncoder::new(Vec::new(), level)), options.zlib.flush),
            Encoding::Br => {
                let BrotliOptions { quality, lgwin, flush } = options.brotli;
                (Kind::Br(Box::new(brotli::CompressorWriter::new(Vec::new(), 4096, quality.min(11), lgwin.clamp(10, 24)))), flush)
            }
        };
        Encoder { kind: Some(kind), flush: flush.unwrap_or(false) }
    }
}

fn ended() -> io::Error {
    io::Error::other("write after end")
}

impl Compressor for Encoder {
    fn write(&mut self, chunk: &[u8]) -> io::Result<Vec<Bytes>> {
        fn step<W: Write>(w: &mut W, chunk: &[u8], flush: bool) -> io::Result<()> {
            w.write_all(chunk)?;
            if flush {
                w.flush()?;
            }
            Ok(())
        }
        let flush = self.flush;
        let out = match self.kind.as_mut().ok_or_else(ended)? {
            Kind::Gzip(e) => step(e, chunk, flush).map(|_| std::mem::take(e.get_mut()))?,
            Kind::Deflate(e) => step(e, chunk, flush).map(|_| std::mem::take(e.get_mut()))?,
            Kind::Br(e) => step(e.as_mut(), chunk, flush).map(|_| std::mem::take(e.get_mut()))?,
        };
        Ok(vec![Bytes::from(out)])
    }

    fn end(&mut self) -> io::Result<Vec<Bytes>> {
        let out = match self.kind.take().ok_or_else(ended)? {
            Kind::Gzip(e) => e.finish()?,
            Kind::Deflate(e) => e.finish()?,
            Kind::Br(e) => e.into_inner(),
        };
        Ok(vec![Bytes::from(out)])
    }
}
