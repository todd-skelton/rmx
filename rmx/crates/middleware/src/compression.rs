//! gzip response compression, streaming-aware.

use std::io::Write;

use flate2::write::GzEncoder;
use flate2::Compression;
use http::{header, HeaderValue};
use rmx_fetch::{Body, Response};
use rmx_router::{Context, Middleware, Next};

/// Compress compressible responses with gzip when the client accepts it. Streamed bodies
/// are compressed chunk by chunk with a sync flush, so streamed HTML still arrives
/// progressively.
pub fn compression() -> impl Middleware {
    |ctx: Context, next: Next| async move {
        let accepts_gzip = rmx_headers::AcceptEncoding::parse(ctx.header(header::ACCEPT_ENCODING.as_str())).accepts("gzip");
        let res = next.run(ctx).await;
        if !accepts_gzip { return res }
        compress(res)
    }
}

fn compress(res: Response) -> Response {
    let headers = res.headers();
    let compressible = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(rmx_mime::is_compressible);
    let small = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok()?.parse::<u64>().ok())
        .is_some_and(|n| n < 1024);
    if !compressible || small || headers.contains_key(header::CONTENT_ENCODING) || res.status().as_u16() == 206 || res.status().as_u16() == 304 || res.body().is_empty_hint() {
        return res;
    }
    let (mut parts, body) = res.into_parts();
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    parts.headers.append(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    let body = Body::from_task(move |tx| async move {
        let mut body = body;
        let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
        while let Some(chunk) = body.chunk().await {
            let Ok(chunk) = chunk else { return };
            if enc.write_all(&chunk).and_then(|_| enc.flush()).is_err() {
                return;
            }
            let out = std::mem::take(enc.get_mut());
            if !out.is_empty() && !tx.send(out) {
                return;
            }
        }
        if let Ok(rest) = enc.finish() {
            if !rest.is_empty() {
                tx.send(rest);
            }
        }
    });
    Response::from_parts(parts, body)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use rmx_fetch::{block_on, response, Body};
    use rmx_router::{Context, Router};

    #[test]
    fn gzips_large_html() {
        let mut router = Router::new().with(super::compression());
        router.get("/", |_: Context| async { response::html("<p>hello</p>".repeat(200)) });
        let req = http::Request::builder().uri("http://x/").header("accept-encoding", "gzip, br").body(Body::empty()).unwrap();
        let res = block_on(router.fetch(req));
        assert_eq!(res.headers()["content-encoding"], "gzip");
        let bytes = block_on(res.into_body().bytes()).unwrap();
        let mut out = String::new();
        flate2::read::GzDecoder::new(&bytes[..]).read_to_string(&mut out).unwrap();
        assert!(out.starts_with("<!DOCTYPE html><p>hello</p>"));

        let refused = http::Request::builder().uri("http://x/").header("accept-encoding", "br, gzip;q=0").body(Body::empty()).unwrap();
        assert!(block_on(router.fetch(refused)).headers().get("content-encoding").is_none(), "q=0 refuses gzip");
    }
}
