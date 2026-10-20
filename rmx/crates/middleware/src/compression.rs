//! Response compression: Remix's `remix/response/compress` (`compress_response`) and
//! `remix/middleware/compression` (`compression`).
//!
//! The encoding (br, gzip or deflate) is negotiated from `Accept-Encoding`: client q-values
//! win, server preference order breaks ties, and a client that refuses every encoding
//! including `identity` gets a 406. Bodies are compressed as they stream; HTML and SSE are
//! flushed after every chunk so they still arrive progressively.

use std::io::{self, Write};
use std::sync::Arc;

use flate2::write::{GzEncoder, ZlibEncoder};
use http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use rmx_fetch::{Body, Response};
use rmx_headers::{AcceptEncoding, CacheControl, Vary};
use rmx_router::{BoxFuture, Context, Middleware, Next};

/// A content coding [`compress_response`] can produce.
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

/// gzip and deflate settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZlibOptions {
    /// 0 (store) to 9 (smallest). Default 6, as in node:zlib.
    pub level: u32,
    /// Flush after every body chunk. `None` flushes `text/html` and `text/event-stream` only.
    pub flush: Option<bool>,
}

impl Default for ZlibOptions {
    fn default() -> Self {
        ZlibOptions { level: 6, flush: None }
    }
}

/// Brotli settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrotliOptions {
    /// 0 to 11. Default 11, as in node:zlib.
    pub quality: u32,
    /// Window size as a power of two, 10 to 24. Default 22, as in node:zlib.
    pub lgwin: u32,
    /// Flush after every body chunk. `None` flushes `text/html` and `text/event-stream` only.
    pub flush: Option<bool>,
}

impl Default for BrotliOptions {
    fn default() -> Self {
        BrotliOptions { quality: 11, lgwin: 22, flush: None }
    }
}

/// Settings for [`compress_response`].
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
    let below_threshold = header_str(headers, header::CONTENT_LENGTH)
        .and_then(|n| n.trim().parse::<u64>().ok())
        .is_some_and(|n| n < options.threshold);
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

    let media_type = header_str(&parts.headers, header::CONTENT_TYPE)
        .map(|ct| ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase())
        .unwrap_or_default();
    let streaming = media_type == "text/html" || media_type == "text/event-stream";

    set(&mut parts.headers, header::CONTENT_ENCODING, encoding.as_str());
    set(&mut parts.headers, header::ACCEPT_RANGES, "none");
    parts.headers.remove(header::CONTENT_LENGTH);
    if let Some(etag) = header_str(&parts.headers, header::ETAG).filter(|e| !e.starts_with("W/")).map(|e| format!("W/{e}")) {
        set(&mut parts.headers, header::ETAG, &etag);
    }
    if head {
        return Response::from_parts(parts, Body::empty());
    }

    let (encoder, flush) = Encoder::new(encoding, options, streaming);
    let body = Body::from_task(move |tx| async move {
        let mut body = body;
        let mut encoder = encoder;
        while let Some(chunk) = body.chunk().await {
            let Ok(chunk) = chunk else { return };
            if chunk.is_empty() {
                continue;
            }
            let Ok(out) = encoder.write(&chunk, flush) else { return };
            if !out.is_empty() && !tx.send(out) {
                return;
            }
        }
        if let Ok(rest) = encoder.finish() {
            if !rest.is_empty() {
                tx.send(rest);
            }
        }
    });
    Response::from_parts(parts, body)
}

fn not_acceptable(encodings: &[Encoding], vary: &str) -> Response {
    let list: Vec<String> = encodings.iter().map(|e| e.as_str()).chain(["identity"]).map(|e| format!("'{e}'")).collect();
    let mut res = Response::new(Body::from(format!("Only {} encodings are supported", list.join(", "))));
    *res.status_mut() = StatusCode::NOT_ACCEPTABLE;
    res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=UTF-8"));
    set(res.headers_mut(), header::VARY, vary);
    res
}

enum Encoder {
    Gzip(GzEncoder<Vec<u8>>),
    Deflate(ZlibEncoder<Vec<u8>>),
    Br(Box<brotli::CompressorWriter<Vec<u8>>>),
}

impl Encoder {
    /// The encoder, and whether to flush it after every chunk.
    fn new(encoding: Encoding, options: &CompressOptions, streaming: bool) -> (Encoder, bool) {
        let level = flate2::Compression::new(options.zlib.level.min(9));
        match encoding {
            Encoding::Gzip => (Encoder::Gzip(GzEncoder::new(Vec::new(), level)), options.zlib.flush.unwrap_or(streaming)),
            Encoding::Deflate => (Encoder::Deflate(ZlibEncoder::new(Vec::new(), level)), options.zlib.flush.unwrap_or(streaming)),
            Encoding::Br => {
                let BrotliOptions { quality, lgwin, flush } = options.brotli;
                let writer = brotli::CompressorWriter::new(Vec::new(), 4096, quality.min(11), lgwin.clamp(10, 24));
                (Encoder::Br(Box::new(writer)), flush.unwrap_or(streaming))
            }
        }
    }

    /// Feed `chunk` and return whatever compressed output is ready.
    fn write(&mut self, chunk: &[u8], flush: bool) -> io::Result<Vec<u8>> {
        fn step<W: Write>(w: &mut W, chunk: &[u8], flush: bool) -> io::Result<()> {
            w.write_all(chunk)?;
            if flush {
                w.flush()?;
            }
            Ok(())
        }
        match self {
            Encoder::Gzip(e) => step(e, chunk, flush).map(|_| std::mem::take(e.get_mut())),
            Encoder::Deflate(e) => step(e, chunk, flush).map(|_| std::mem::take(e.get_mut())),
            Encoder::Br(e) => step(e.as_mut(), chunk, flush).map(|_| std::mem::take(e.get_mut())),
        }
    }

    fn finish(self) -> io::Result<Vec<u8>> {
        match self {
            Encoder::Gzip(e) => e.finish(),
            Encoder::Deflate(e) => e.finish(),
            Encoder::Br(e) => Ok(e.into_inner()),
        }
    }
}

type PerResponse<T> = Arc<dyn Fn(&Response) -> T + Send + Sync>;

/// The compression middleware. Build it with [`compression`].
#[derive(Clone)]
pub struct Compression {
    threshold: u64,
    filter: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    encodings: PerResponse<Vec<Encoding>>,
    zlib: PerResponse<ZlibOptions>,
    brotli: PerResponse<BrotliOptions>,
}

/// Compress responses whose media type is compressible (see [`rmx_mime::is_compressible`]),
/// using [`compress_response`] with its defaults.
pub fn compression() -> Compression {
    Compression {
        threshold: 1024,
        filter: Arc::new(rmx_mime::is_compressible),
        encodings: Arc::new(|_| DEFAULT_ENCODINGS.to_vec()),
        zlib: Arc::new(|_| ZlibOptions::default()),
        brotli: Arc::new(|_| BrotliOptions::default()),
    }
}

impl Compression {
    /// Minimum `Content-Length` to compress. Default 1024.
    pub fn threshold(mut self, bytes: u64) -> Self {
        self.threshold = bytes;
        self
    }

    /// Which media types (lowercase-insensitive, no parameters) to compress.
    pub fn filter_media_type(mut self, filter: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        self.filter = Arc::new(filter);
        self
    }

    /// Encodings to offer, most preferred first.
    pub fn encodings(self, encodings: impl Into<Vec<Encoding>>) -> Self {
        let encodings = encodings.into();
        self.encodings_with(move |_| encodings.clone())
    }

    /// Encodings to offer, chosen per response. Return an empty list to skip compression.
    pub fn encodings_with(mut self, f: impl Fn(&Response) -> Vec<Encoding> + Send + Sync + 'static) -> Self {
        self.encodings = Arc::new(f);
        self
    }

    pub fn zlib(self, options: ZlibOptions) -> Self {
        self.zlib_with(move |_| options)
    }

    pub fn zlib_with(mut self, f: impl Fn(&Response) -> ZlibOptions + Send + Sync + 'static) -> Self {
        self.zlib = Arc::new(f);
        self
    }

    pub fn brotli(self, options: BrotliOptions) -> Self {
        self.brotli_with(move |_| options)
    }

    pub fn brotli_with(mut self, f: impl Fn(&Response) -> BrotliOptions + Send + Sync + 'static) -> Self {
        self.brotli = Arc::new(f);
        self
    }
}

impl Middleware for Compression {
    fn call(&self, ctx: Context, next: Next) -> BoxFuture<Response> {
        let this = self.clone();
        let method = ctx.method().clone();
        let accept_encoding = ctx.header(header::ACCEPT_ENCODING.as_str()).map(str::to_string);
        Box::pin(async move {
            let res = next.run(ctx).await;
            let wanted = header_str(res.headers(), header::CONTENT_TYPE)
                .map(|ct| ct.split(';').next().unwrap_or("").trim())
                .is_some_and(|media_type| !media_type.is_empty() && (this.filter)(media_type));
            if !wanted {
                return res;
            }
            let options = CompressOptions { threshold: this.threshold, encodings: (this.encodings)(&res), zlib: (this.zlib)(&res), brotli: (this.brotli)(&res) };
            compress(res, &method, accept_encoding.as_deref(), &options)
        })
    }
}

#[cfg(test)]
mod tests;
