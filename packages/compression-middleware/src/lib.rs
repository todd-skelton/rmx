//! The compression middleware (Remix's `remix/middleware/compression`): compresses
//! responses with a compressible media type through [`compress_response`] (Remix's
//! `remix/response/compress`, which lives in `rmx_fetch::response::compress` and is
//! re-exported here).

use std::sync::Arc;

use http::header;
use rmx_fetch::Response;
use rmx_router::{BoxFuture, Context, Middleware, Next};

pub use rmx_fetch::response::compress::{
    compress_response, compress_stream, create_compression_options, BrotliOptions, CompressOptions, Compressor, Encoding, ZlibOptions, DEFAULT_ENCODINGS,
};

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

/// Compress responses whose media type is compressible (see [`rmx_mime::is_compressible_mime_type`]),
/// using [`compress_response`] with its defaults.
pub fn compression() -> Compression {
    Compression {
        threshold: 1024,
        filter: Arc::new(rmx_mime::is_compressible_mime_type),
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
            let wanted = res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok())
                .map(|ct| ct.split(';').next().unwrap_or("").trim())
                .is_some_and(|media_type| !media_type.is_empty() && (this.filter)(media_type));
            if !wanted {
                return res;
            }
            let options = CompressOptions { threshold: this.threshold, encodings: (this.encodings)(&res), zlib: (this.zlib)(&res), brotli: (this.brotli)(&res) };
            let mut request = http::Request::builder().method(method);
            if let Some(accept_encoding) = &accept_encoding {
                request = request.header(header::ACCEPT_ENCODING, accept_encoding);
            }
            match request.body(()) {
                Ok(request) => compress_response(res, &request, &options),
                Err(_) => res,
            }
        })
    }
}

