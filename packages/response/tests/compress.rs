//! Port of Remix's `response/src/lib/compress.test.ts`, one test per case.
//!
//! Differences that follow from Rust and are kept deliberately:
//!
//! 1. `assert.equal(compressed, response)` (the same object back) becomes "unchanged": no
//!    `Vary`, no `Content-Encoding` and the original body.
//! 2. `statusText` does not exist on an `http` response; the status and its canonical reason
//!    are checked instead.
//! 3. node:zlib flush constants are a `bool`: `Z_SYNC_FLUSH` and `BROTLI_OPERATION_FLUSH` are
//!    `Some(true)`, and the explicitly configured non-default values (`Z_FULL_FLUSH`,
//!    `BROTLI_OPERATION_PROCESS`) are written as an explicit `Some(false)`.
//! 4. The mocked node:zlib compressor is a [Compressor]: data events are the pieces `write`
//!    and `end` return, an error event or write-callback error is an `Err`, `destroy()` is
//!    `Drop`, and cancelling a stream is dropping the body (so there is no cancel reason and
//!    no cancel error to propagate). Node's `drain` backpressure is [BodySender::ready]: a
//!    source chunk is read only after the previous output was taken.

use std::future::Future;
use std::io::{self, Read, Write};
use std::pin::pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use rmx_response::compress::{compress_response, compress_stream, create_compression_options, BrotliOptions, CompressOptions, Compressor, Encoding, ZlibOptions};
use rmx_fetch::{block_on, Body, BoxError, Request, Response};
use rmx_headers::Vary;

fn request(method: Method, accept_encoding: Option<&str>) -> Request {
    let mut builder = http::Request::builder().method(method).uri("https://remix.run");
    if let Some(value) = accept_encoding {
        builder = builder.header("Accept-Encoding", value);
    }
    builder.body(Body::empty()).unwrap()
}

fn accepting(accept_encoding: &str) -> Request {
    request(Method::GET, Some(accept_encoding))
}

fn response(body: impl Into<Body>, headers: &[(&'static str, &str)]) -> Response {
    let mut response = Response::new(body.into());
    for (name, value) in headers {
        response.headers_mut().append(*name, HeaderValue::from_str(value).unwrap());
    }
    response
}

fn text(body: &str) -> Response {
    response(body.to_string(), &[])
}

fn offering(encodings: &[Encoding]) -> CompressOptions {
    CompressOptions { encodings: encodings.to_vec(), ..CompressOptions::default() }
}

fn compress(response: Response, request: &Request) -> Response {
    compress_response(response, request, &CompressOptions::default())
}

fn header(response: &Response, name: &str) -> Option<String> {
    response.headers().get(name).map(|v| v.to_str().unwrap().to_string())
}

fn vary(response: &Response) -> Vary {
    Vary::from_headers(response.headers().get_all("vary").iter().map(|v| v.to_str().unwrap()))
}

fn body_bytes(response: Response) -> Vec<u8> {
    block_on(response.into_body().bytes()).unwrap().to_vec()
}

fn body_text(response: Response) -> String {
    String::from_utf8(body_bytes(response)).unwrap()
}

fn decompress(encoding: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    match encoding {
        "gzip" => flate2::read::GzDecoder::new(bytes).read_to_end(&mut out).unwrap(),
        "deflate" => flate2::read::ZlibDecoder::new(bytes).read_to_end(&mut out).unwrap(),
        "br" => brotli::Decompressor::new(bytes, 4096).read_to_end(&mut out).unwrap(),
        other => panic!("unexpected encoding {other}"),
    };
    out
}

/// The decompressed body, by the response's `Content-Encoding`.
fn decoded(response: Response) -> String {
    let encoding = header(&response, "content-encoding").expect("a Content-Encoding");
    String::from_utf8(decompress(&encoding, &body_bytes(response))).unwrap()
}

/// `assert.equal(compressed, response)`: the response came back as it was.
fn assert_unchanged(compressed: Response, body: &str) {
    assert_eq!(header(&compressed, "Content-Encoding"), None);
    assert_eq!(header(&compressed, "Vary"), None);
    assert_eq!(body_text(compressed), body);
}

#[test]
fn compresses_response_with_gzip_when_client_accepts_it() {
    let compressed = compress(text("Hello, World!"), &accepting("gzip"));

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("gzip"));
    assert_eq!(header(&compressed, "Accept-Ranges").as_deref(), Some("none"));
    assert!(vary(&compressed).has("Accept-Encoding"));
    assert_eq!(decoded(compressed), "Hello, World!");
}

#[test]
fn compresses_response_with_brotli_when_client_prefers_it() {
    let compressed = compress(text("Hello, World!"), &accepting("br, gzip"));

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("br"));
    assert_eq!(decoded(compressed), "Hello, World!");
}

#[test]
fn compresses_response_with_deflate() {
    let compressed = compress_response(text("Hello, World!"), &accepting("deflate"), &offering(&[Encoding::Deflate]));

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("deflate"));
    assert_eq!(header(&compressed, "Accept-Ranges").as_deref(), Some("none"));
    assert!(vary(&compressed).has("Accept-Encoding"));
    assert_eq!(decoded(compressed), "Hello, World!");
}

#[test]
fn selects_identity_without_changing_representation_headers() {
    let mut original = response("Hello, World!", &[("Content-Length", "2048"), ("ETag", "\"abc123\"")]);
    *original.status_mut() = StatusCode::CREATED;

    let compressed = compress(original, &accepting("identity"));

    assert_eq!(compressed.status(), StatusCode::CREATED);
    assert_eq!(compressed.status().canonical_reason(), Some("Created"));
    assert_eq!(header(&compressed, "Content-Encoding"), None);
    assert_eq!(header(&compressed, "Content-Length").as_deref(), Some("2048"));
    assert_eq!(header(&compressed, "Accept-Ranges"), None);
    assert_eq!(header(&compressed, "ETag").as_deref(), Some("\"abc123\""));
    assert!(vary(&compressed).has("Accept-Encoding"));
    assert_eq!(body_text(compressed), "Hello, World!");
}

#[test]
fn preserves_existing_vary_values_when_identity_is_selected() {
    let compressed = compress(response("Hello, World!", &[("Vary", "Cookie")]), &accepting("identity"));
    let vary = vary(&compressed);

    assert!(vary.has("Cookie"));
    assert!(vary.has("Accept-Encoding"));
}

#[test]
fn does_not_duplicate_accept_encoding_in_vary_header() {
    let compressed = compress(response("Hello, World!", &[("Vary", "Cookie, Accept-Encoding")]), &accepting("identity"));

    let vary_header = header(&compressed, "Vary").unwrap_or_default();
    assert!(Vary::parse(Some(&vary_header)).has("Cookie"));
    assert_eq!(vary_header.to_ascii_lowercase().matches("accept-encoding").count(), 1);
}

#[test]
fn selects_identity_when_client_does_not_send_accept_encoding() {
    let compressed = compress(text("Hello, World!"), &request(Method::GET, None));

    // Per RFC 7231, when no Accept-Encoding header is present, the server should use
    // identity (uncompressed) for compatibility
    assert_eq!(header(&compressed, "Content-Encoding"), None);
    assert!(vary(&compressed).has("Accept-Encoding"));
    assert_eq!(body_text(compressed), "Hello, World!");
}

#[test]
fn compresses_responses_when_content_length_is_not_set() {
    let compressed = compress(text("Small"), &accepting("gzip"));

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("gzip"));
}

#[test]
fn skips_compression_when_content_length_is_below_threshold() {
    let compressed = compress(response("Small", &[("Content-Length", "5")]), &accepting("gzip"));

    assert_unchanged(compressed, "Small");
}

#[test]
fn respects_custom_threshold_option() {
    let options = CompressOptions { threshold: 3, ..CompressOptions::default() };
    let compressed = compress_response(response("Small", &[("Content-Length", "5")]), &accepting("gzip"), &options);

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("gzip"));
}

#[test]
fn skips_compression_for_already_compressed_responses() {
    let compressed = compress(response("Already compressed", &[("Content-Encoding", "gzip")]), &accepting("gzip"));

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("gzip"));
    assert_eq!(header(&compressed, "Vary"), None);
    assert_eq!(body_text(compressed), "Already compressed");
}

#[test]
fn skips_compression_when_cache_control_no_transform_is_present() {
    let compressed = compress(response("Do not transform", &[("Cache-Control", "public, no-transform, max-age=3600")]), &accepting("gzip"));

    assert_unchanged(compressed, "Do not transform");
}

#[test]
fn skips_compression_when_response_has_no_body() {
    let mut original = Response::new(Body::empty());
    *original.status_mut() = StatusCode::NO_CONTENT;

    let compressed = compress(original, &accepting("gzip"));

    assert_eq!(compressed.status(), StatusCode::NO_CONTENT);
    assert_unchanged(compressed, "");
}

#[test]
fn compresses_with_custom_compression_level() {
    let request = accepting("gzip");
    // A large, repetitive string where compression level makes a difference
    let content = "Hello, World!".repeat(1000);
    let at = |level: u32| {
        let options = CompressOptions { zlib: ZlibOptions { level, ..ZlibOptions::default() }, ..CompressOptions::default() };
        compress_response(text(&content), &request, &options)
    };

    let level1 = at(1);
    let level9 = at(9);
    assert_eq!(header(&level1, "Content-Encoding").as_deref(), Some("gzip"));
    assert_eq!(header(&level9, "Content-Encoding").as_deref(), Some("gzip"));
    let (level1, level9) = (body_bytes(level1), body_bytes(level9));

    assert_eq!(decompress("gzip", &level1), content.as_bytes());
    assert_eq!(decompress("gzip", &level9), content.as_bytes());
    // Level 9 should produce smaller output than level 1
    assert!(level9.len() < level1.len(), "{} < {}", level9.len(), level1.len());
}

#[test]
fn compresses_with_custom_brotli_options() {
    let request = accepting("br");
    // Repetitive content where window size affects compression ratio
    let content = "Hello, World! ".repeat(10000);
    let with_window = |lgwin: u32| {
        let options = CompressOptions { brotli: BrotliOptions { lgwin, ..BrotliOptions::default() }, ..offering(&[Encoding::Br]) };
        compress_response(text(&content), &request, &options)
    };

    let small = with_window(10);
    let large = with_window(22);
    assert_eq!(header(&small, "Content-Encoding").as_deref(), Some("br"));
    assert_eq!(header(&large, "Content-Encoding").as_deref(), Some("br"));
    let (small, large) = (body_bytes(small), body_bytes(large));

    assert_eq!(decompress("br", &small), content.as_bytes());
    assert_eq!(decompress("br", &large), content.as_bytes());
    // Different window sizes produce different output (both would match if the option were dropped)
    assert_ne!(small.len(), large.len());
}

#[test]
fn limits_to_specified_encodings() {
    let compressed = compress_response(text("Hello, World!"), &accepting("br, gzip, deflate"), &offering(&[Encoding::Gzip, Encoding::Deflate]));

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("gzip"));
}

#[test]
fn returns_uncompressed_when_encodings_is_empty_array() {
    let compressed = compress_response(text("Hello, World!"), &accepting("gzip, br"), &offering(&[]));

    assert_unchanged(compressed, "Hello, World!");
}

#[test]
fn handles_quality_factors_in_accept_encoding() {
    let compressed = compress(text("Hello, World!"), &accepting("gzip;q=0.8, deflate;q=1.0"));

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("deflate"));
}

#[test]
fn client_quality_factors_override_server_preference_order() {
    // The client strongly prefers gzip over br; the server prefers br > gzip > deflate
    let compressed = compress_response(text("Hello, World!"), &accepting("br;q=0.5, gzip;q=1.0, deflate;q=0.8"), &offering(&[Encoding::Br, Encoding::Gzip, Encoding::Deflate]));

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("gzip"));
}

#[test]
fn server_preference_order_breaks_ties_when_client_has_equal_quality_factors() {
    // The server prefers deflate first, so it wins the tie
    let compressed = compress_response(text("Hello, World!"), &accepting("gzip, deflate, br"), &offering(&[Encoding::Deflate, Encoding::Br, Encoding::Gzip]));

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("deflate"));
}

#[test]
fn respects_explicit_rejection_with_q_0() {
    let compressed = compress_response(text("Hello, World!"), &accepting("gzip;q=0, deflate;q=0, br;q=1.0"), &offering(&[Encoding::Gzip, Encoding::Deflate, Encoding::Br]));

    // br, since the others are explicitly rejected
    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("br"));
}

#[test]
fn returns_uncompressed_when_all_encodings_are_rejected_but_identity_is_acceptable() {
    let compressed = compress_response(text("Hello, World!"), &accepting("gzip;q=0, deflate;q=0, br;q=0, identity"), &offering(&[Encoding::Gzip, Encoding::Deflate, Encoding::Br]));

    assert_eq!(header(&compressed, "Content-Encoding"), None);
    assert!(vary(&compressed).has("Accept-Encoding"));
    assert_eq!(body_text(compressed), "Hello, World!");
}

#[test]
fn requires_compression_when_identity_is_explicitly_rejected() {
    let compressed = compress(text("Hello, World!"), &accepting("identity;q=0, gzip"));

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("gzip"));
}

#[test]
fn returns_406_when_all_encodings_including_identity_are_rejected() {
    let original = response("Hello, World!", &[("Vary", "Cookie")]);
    let result = compress_response(original, &accepting("gzip;q=0, deflate;q=0, br;q=0, identity;q=0"), &offering(&[Encoding::Gzip, Encoding::Deflate, Encoding::Br]));

    // 406 Not Acceptable per RFC 7231
    assert_eq!(result.status(), StatusCode::NOT_ACCEPTABLE);
    assert_eq!(result.status().canonical_reason(), Some("Not Acceptable"));
    let vary = vary(&result);
    assert!(vary.has("Cookie"));
    assert!(vary.has("Accept-Encoding"));
}

#[test]
fn handles_wildcard_with_quality_factor() {
    // gzip preferred, anything else at q=0.5; the server offers no gzip
    let compressed = compress_response(text("Hello, World!"), &accepting("gzip, *;q=0.5"), &offering(&[Encoding::Br, Encoding::Deflate]));

    // br matches the wildcard
    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("br"));
}

#[test]
fn prefers_explicit_encoding_over_wildcard() {
    let compressed = compress_response(text("Hello, World!"), &accepting("gzip, *;q=0.8"), &offering(&[Encoding::Br, Encoding::Gzip, Encoding::Deflate]));

    // Explicit gzip (q=1.0) over br matched by the wildcard (q=0.8)
    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("gzip"));
}

#[test]
fn respects_wildcard_rejection_with_q_0() {
    // Only gzip is accepted; *;q=0 rejects everything else, identity included
    let result = compress_response(text("Hello, World!"), &accepting("gzip, *;q=0"), &offering(&[Encoding::Br, Encoding::Deflate]));

    assert_eq!(result.status(), StatusCode::NOT_ACCEPTABLE);
    assert_eq!(result.status().canonical_reason(), Some("Not Acceptable"));
}

#[test]
fn allows_identity_when_wildcard_rejects_compression_but_identity_is_explicit() {
    let compressed = compress_response(text("Hello, World!"), &accepting("identity, *;q=0"), &offering(&[Encoding::Gzip, Encoding::Deflate, Encoding::Br]));

    assert_eq!(header(&compressed, "Content-Encoding"), None);
    assert!(vary(&compressed).has("Accept-Encoding"));
    assert_eq!(body_text(compressed), "Hello, World!");
}

#[test]
fn handles_wildcard_with_identity_rejection() {
    let compressed = compress_response(text("Hello, World!"), &accepting("*;q=1.0, identity;q=0"), &offering(&[Encoding::Gzip, Encoding::Deflate, Encoding::Br]));

    // Must compress since identity is rejected: the first supported encoding
    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("gzip"));
}

#[test]
fn preserves_response_status_and_status_text() {
    let mut original = text("Created");
    *original.status_mut() = StatusCode::CREATED;

    let compressed = compress(original, &accepting("gzip"));

    assert_eq!(compressed.status(), StatusCode::CREATED);
    assert_eq!(compressed.status().canonical_reason(), Some("Created"));
}

#[test]
fn removes_content_length_header() {
    let large = "Hello, World!".repeat(100);
    let compressed = compress(response(large.clone(), &[("Content-Length", &large.len().to_string())]), &accepting("gzip"));

    assert_eq!(header(&compressed, "Content-Length"), None);
}

#[test]
fn sets_accept_ranges_to_none() {
    let compressed = compress(text("Hello, World!"), &accepting("gzip"));

    assert_eq!(header(&compressed, "Accept-Ranges").as_deref(), Some("none"));
}

#[test]
fn converts_strong_etags_to_weak() {
    let compressed = compress(response("Hello, World!", &[("ETag", "\"abc123\"")]), &accepting("gzip"));

    assert_eq!(header(&compressed, "ETag").as_deref(), Some("W/\"abc123\""));
}

#[test]
fn preserves_weak_etags() {
    let compressed = compress(response("Hello, World!", &[("ETag", "W/\"abc123\"")]), &accepting("gzip"));

    assert_eq!(header(&compressed, "ETag").as_deref(), Some("W/\"abc123\""));
}

#[test]
fn does_not_add_etag_when_not_present() {
    let compressed = compress(text("Hello, World!"), &accepting("gzip"));

    assert_eq!(header(&compressed, "ETag"), None);
}

#[test]
fn converts_strong_etags_to_weak_for_head_requests() {
    let original = response("x".repeat(2000), &[("Content-Type", "text/plain"), ("ETag", "\"xyz789\"")]);
    let compressed = compress(original, &request(Method::HEAD, Some("gzip")));

    assert_eq!(header(&compressed, "ETag").as_deref(), Some("W/\"xyz789\""));
}

#[test]
fn skips_responses_with_accept_ranges_bytes() {
    let compressed = compress(response("Hello, World!", &[("Accept-Ranges", "bytes")]), &accepting("gzip"));

    assert_eq!(header(&compressed, "Accept-Ranges").as_deref(), Some("bytes"));
    assert_unchanged(compressed, "Hello, World!");
}

#[test]
fn skips_206_partial_content_responses() {
    let mut original = response("Partial content", &[("Content-Range", "bytes 0-9/100")]);
    *original.status_mut() = StatusCode::PARTIAL_CONTENT;

    let compressed = compress(original, &accepting("gzip"));

    assert_eq!(compressed.status(), StatusCode::PARTIAL_CONTENT);
    assert_unchanged(compressed, "Partial content");
}

#[test]
fn sets_compression_headers_for_head_requests_without_compressing() {
    // Content larger than the default threshold (1024 bytes)
    let original = response("x".repeat(2000), &[("Content-Type", "text/plain")]);
    let compressed = compress(original, &request(Method::HEAD, Some("gzip")));

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("gzip"));
    assert_eq!(header(&compressed, "Accept-Ranges").as_deref(), Some("none"));
    assert_eq!(header(&compressed, "Content-Length"), None);
    assert!(vary(&compressed).has("Accept-Encoding"));
    assert!(compressed.body().is_empty_hint());
}

#[test]
fn negotiates_encoding_for_head_requests() {
    let original = response("x".repeat(2000), &[("Content-Type", "text/plain")]);
    let compressed = compress(original, &request(Method::HEAD, Some("br")));

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("br"));
}

#[test]
fn returns_identity_for_head_when_client_does_not_accept_compression() {
    let content = "x".repeat(2000);
    let compressed = compress(response(content.clone(), &[("Content-Type", "text/plain")]), &request(Method::HEAD, None));

    assert_eq!(header(&compressed, "Content-Encoding"), None);
    assert!(vary(&compressed).has("Accept-Encoding"));
    assert_eq!(body_text(compressed), content);
}

#[test]
fn sets_compression_headers_for_head_requests_even_when_body_is_already_null() {
    // A body already removed at the route level
    let original = response(Body::empty(), &[("Content-Type", "text/plain"), ("Content-Length", "2000")]);
    let compressed = compress(original, &request(Method::HEAD, Some("gzip")));

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some("gzip"));
    assert_eq!(header(&compressed, "Accept-Ranges").as_deref(), Some("none"));
    assert_eq!(header(&compressed, "Content-Length"), None);
    assert!(vary(&compressed).has("Accept-Encoding"));
    assert!(compressed.body().is_empty_hint());
}

// Streaming response types

fn content_type(value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_str(value).unwrap());
    headers
}

/// Remix's `{ zlib: { level: 9 }, brotli: { params: { QUALITY: 11 } } }`.
fn custom_levels() -> CompressOptions {
    CompressOptions { zlib: ZlibOptions { level: 9, ..ZlibOptions::default() }, brotli: BrotliOptions { quality: 11, ..BrotliOptions::default() }, ..CompressOptions::default() }
}

/// An explicitly configured flush that is not the streaming default (`Z_FULL_FLUSH`,
/// `BROTLI_OPERATION_PROCESS` in Remix).
fn explicit_flush() -> CompressOptions {
    CompressOptions { zlib: ZlibOptions { flush: Some(false), ..ZlibOptions::default() }, brotli: BrotliOptions { flush: Some(false), ..BrotliOptions::default() }, ..CompressOptions::default() }
}

#[test]
fn applies_flush_defaults_to_sse_while_preserving_custom_compression_options() {
    let options = create_compression_options(&content_type("text/event-stream"), &custom_levels());

    assert_eq!(options.zlib.level, 9);
    assert_eq!(options.zlib.flush, Some(true));
    assert_eq!(options.brotli.quality, 11);
    assert_eq!(options.brotli.flush, Some(true));
}

#[test]
fn applies_html_flush_defaults_unless_explicitly_configured() {
    let headers = content_type("text/html; charset=UTF-8");
    let defaults = create_compression_options(&headers, &CompressOptions::default());

    assert_eq!(defaults.zlib.flush, Some(true));
    assert_eq!(defaults.brotli.flush, Some(true));

    let configured = create_compression_options(&headers, &explicit_flush());

    assert_eq!(configured.zlib.flush, Some(false));
    assert_eq!(configured.brotli.flush, Some(false));
}

#[test]
fn matches_streaming_media_types_case_insensitively() {
    let html = create_compression_options(&content_type("Text/HTML; charset=UTF-8"), &CompressOptions::default());
    let sse = create_compression_options(&content_type("TEXT/Event-Stream"), &CompressOptions::default());

    assert_eq!(html.zlib.flush, Some(true));
    assert_eq!(html.brotli.flush, Some(true));
    assert_eq!(sse.zlib.flush, Some(true));
    assert_eq!(sse.brotli.flush, Some(true));
}

#[test]
fn preserves_explicit_sse_flush_options() {
    let options = create_compression_options(&content_type("text/event-stream"), &explicit_flush());

    assert_eq!(options.zlib.flush, Some(false));
    assert_eq!(options.brotli.flush, Some(false));
}

#[test]
fn does_not_apply_flush_defaults_to_other_response_types() {
    let options = create_compression_options(&content_type("text/plain"), &custom_levels());

    assert_eq!(options.zlib.level, 9);
    assert_eq!(options.zlib.flush, None);
    assert_eq!(options.brotli.quality, 11);
    assert_eq!(options.brotli.flush, None);
}

fn assert_sse_compression(encoding: Encoding) {
    let body = "event: message\ndata: test-payload\n\n";
    let original = response(body, &[("Content-Type", "text/event-stream")]);
    let options = CompressOptions { encodings: vec![encoding], ..custom_levels() };

    let compressed = compress_response(original, &accepting(encoding.as_str()), &options);

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some(encoding.as_str()));
    assert_eq!(decoded(compressed), body);
}

#[test]
fn compresses_sse_with_br() {
    assert_sse_compression(Encoding::Br);
}

#[test]
fn compresses_sse_with_gzip() {
    assert_sse_compression(Encoding::Gzip);
}

#[test]
fn compresses_sse_with_deflate() {
    assert_sse_compression(Encoding::Deflate);
}

// Streaming compression

/// `createChunkedStream(chunks)`.
fn chunked(chunks: Vec<Vec<u8>>) -> Body {
    Body::from_task(move |tx| async move {
        for chunk in chunks {
            tx.send(chunk);
        }
    })
}

/// `createErrorStream(n)`: `n` chunks, then an error.
fn error_stream(chunks: usize) -> Body {
    Body::from_task(move |tx| async move {
        for _ in 0..chunks {
            tx.send("chunk");
        }
        tx.abort("Stream error");
    })
}

/// Poll a future once, without waiting.
fn poll_once<F: Future>(future: F) -> Poll<F::Output> {
    let mut future = pin!(future);
    future.as_mut().poll(&mut Context::from_waker(Waker::noop()))
}

/// Read the body to its end or its first error.
fn read_all(body: &mut Body) -> Result<Vec<Bytes>, BoxError> {
    let mut chunks = Vec::new();
    while let Some(chunk) = block_on(body.chunk()) {
        chunks.push(chunk?);
    }
    Ok(chunks)
}

fn concat(chunks: &[Bytes]) -> Vec<u8> {
    chunks.iter().flat_map(|c| c.iter().copied()).collect()
}

/// A decompressor fed one compressed chunk at a time (`decompressStream`).
enum StreamDecoder {
    Gzip(flate2::write::GzDecoder<Vec<u8>>),
    Deflate(flate2::write::ZlibDecoder<Vec<u8>>),
    Br(Box<brotli::DecompressorWriter<Vec<u8>>>),
}

impl StreamDecoder {
    fn new(encoding: Encoding) -> StreamDecoder {
        match encoding {
            Encoding::Gzip => StreamDecoder::Gzip(flate2::write::GzDecoder::new(Vec::new())),
            Encoding::Deflate => StreamDecoder::Deflate(flate2::write::ZlibDecoder::new(Vec::new())),
            Encoding::Br => StreamDecoder::Br(Box::new(brotli::DecompressorWriter::new(Vec::new(), 4096))),
        }
    }

    /// Feed `chunk` and return everything decoded so far.
    fn feed(&mut self, chunk: &[u8]) -> String {
        let decoded = match self {
            StreamDecoder::Gzip(d) => {
                d.write_all(chunk).unwrap();
                d.flush().unwrap();
                d.get_ref().clone()
            }
            StreamDecoder::Deflate(d) => {
                d.write_all(chunk).unwrap();
                d.flush().unwrap();
                d.get_ref().clone()
            }
            StreamDecoder::Br(d) => {
                d.write_all(chunk).unwrap();
                d.flush().unwrap();
                d.get_ref().clone()
            }
        };
        String::from_utf8(decoded).unwrap()
    }
}

/// `roundTrip(input, encoding)`.
fn round_trip(input: &str, encoding: Encoding) -> String {
    let original = response(chunked(vec![input.as_bytes().to_vec()]), &[("Content-Type", "text/plain")]);
    let compressed = compress_response(original, &accepting(encoding.as_str()), &offering(&[encoding]));

    assert_eq!(header(&compressed, "Content-Encoding").as_deref(), Some(encoding.as_str()));
    String::from_utf8(decompress(encoding.as_str(), &body_bytes(compressed))).unwrap()
}

/// `assertHtmlStreamsBeforeCompletion`: the first HTML chunk decodes from the compressed body
/// while the source is still open. Remix races each read against a one-second timeout; here
/// a read that is not ready at once means the compressor buffered the chunk.
fn assert_html_streams_before_completion(encoding: Encoding) {
    let initial_html = "<!DOCTYPE html><main>Initial content</main>";
    let frame_html = "<template>Deferred frame</template>";
    let (source, body) = Body::channel();
    source.send(initial_html);
    let original = response(body, &[("Content-Type", "text/html; charset=UTF-8")]);
    let compressed = compress_response(original, &accepting(encoding.as_str()), &offering(&[encoding]));
    let mut body = compressed.into_body();
    let mut decoder = StreamDecoder::new(encoding);

    let mut html = String::new();
    while html.len() < initial_html.len() {
        match poll_once(body.chunk()) {
            Poll::Ready(Some(Ok(chunk))) => html = decoder.feed(&chunk),
            Poll::Ready(other) => panic!("Compressed stream should contain the initial HTML: {other:?}"),
            Poll::Pending => panic!("Initial HTML was buffered by {} compression", encoding.as_str()),
        }
    }
    assert_eq!(html, initial_html);

    // releaseFrame()
    source.send(frame_html);
    drop(source);
    for chunk in read_all(&mut body).unwrap() {
        html = decoder.feed(&chunk);
    }
    assert_eq!(html, format!("{initial_html}{frame_html}"));
}

#[test]
fn flushes_the_initial_html_through_brotli_before_the_source_completes() {
    assert_html_streams_before_completion(Encoding::Br);
}

#[test]
fn flushes_the_initial_html_through_gzip_before_the_source_completes() {
    assert_html_streams_before_completion(Encoding::Gzip);
}

#[test]
fn flushes_the_initial_html_through_deflate_before_the_source_completes() {
    assert_html_streams_before_completion(Encoding::Deflate);
}

#[test]
fn handles_binary_data_byte_perfectly() {
    // Binary data with all byte values (0-255)
    let binary: Vec<u8> = (0..=255u8).collect();
    let original = response(chunked(vec![binary.clone()]), &[("Content-Type", "application/octet-stream")]);

    let compressed = compress_response(original, &accepting("br"), &offering(&[Encoding::Br]));

    assert_eq!(decompress("br", &body_bytes(compressed)), binary);
}

const UNICODE: &str = "Hello \u{4e16}\u{754c} \u{1f30d} \u{e9}moji";

#[test]
fn gzip_compresses_and_decompresses_simple_text() {
    assert_eq!(round_trip("hello world", Encoding::Gzip), "hello world");
}

#[test]
fn gzip_compresses_and_decompresses_empty_string() {
    assert_eq!(round_trip("", Encoding::Gzip), "");
}

#[test]
fn gzip_compresses_and_decompresses_unicode() {
    assert_eq!(round_trip(UNICODE, Encoding::Gzip), UNICODE);
}

#[test]
fn deflate_compresses_and_decompresses_simple_text() {
    assert_eq!(round_trip("hello world", Encoding::Deflate), "hello world");
}

#[test]
fn deflate_compresses_and_decompresses_empty_string() {
    assert_eq!(round_trip("", Encoding::Deflate), "");
}

#[test]
fn deflate_compresses_and_decompresses_unicode() {
    assert_eq!(round_trip(UNICODE, Encoding::Deflate), UNICODE);
}

#[test]
fn br_compresses_and_decompresses_simple_text() {
    assert_eq!(round_trip("hello world", Encoding::Br), "hello world");
}

#[test]
fn br_compresses_and_decompresses_empty_string() {
    assert_eq!(round_trip("", Encoding::Br), "");
}

#[test]
fn br_compresses_and_decompresses_unicode() {
    assert_eq!(round_trip(UNICODE, Encoding::Br), UNICODE);
}

fn compress_br_text(source: Body) -> String {
    let original = response(source, &[("Content-Type", "text/plain")]);
    let compressed = compress_response(original, &accepting("br"), &offering(&[Encoding::Br]));
    String::from_utf8(decompress("br", &body_bytes(compressed))).unwrap()
}

#[test]
fn handles_multiple_chunks_correctly() {
    assert_eq!(compress_br_text(chunked(vec![b"hello".to_vec(), b" ".to_vec(), b"world".to_vec()])), "hello world");
}

#[test]
fn handles_single_byte_chunks() {
    let bytes = ["h", "e", "l", "l", "o"].map(|b| b.as_bytes().to_vec()).to_vec();

    assert_eq!(compress_br_text(chunked(bytes)), "hello");
}

#[test]
fn handles_empty_chunks_in_stream() {
    let chunks = vec![Vec::new(), b"hello ".to_vec(), Vec::new(), b"world".to_vec()];

    assert_eq!(compress_br_text(chunked(chunks)), "hello world");
}

#[test]
fn handles_stream_that_closes_immediately_without_data() {
    assert_eq!(compress_br_text(chunked(Vec::new())), "");
}

#[test]
fn propagates_errors_from_input_stream() {
    let original = response(error_stream(2), &[("Content-Type", "text/plain")]);
    let compressed = compress_response(original, &accepting("br"), &offering(&[Encoding::Br]));

    let error = read_all(&mut compressed.into_body()).expect_err("the source error");

    assert_eq!(error.to_string(), "Stream error");
}

/// Sets a flag when dropped: a source or compressor being cancelled or destroyed.
struct OnDrop(Arc<AtomicUsize>);

impl Drop for OnDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// A source that sends a chunk and then waits forever, dropping `guard` when cancelled.
fn endless_source(guard: OnDrop) -> Body {
    Body::from_task(move |tx| async move {
        let _guard = guard;
        tx.send("chunk\n".repeat(100));
        std::future::pending::<()>().await;
    })
}

#[test]
fn handles_output_stream_cancellation_and_stops_processing_source_stream() {
    let cancelled = Arc::new(AtomicUsize::new(0));
    let original = response(endless_source(OnDrop(cancelled.clone())), &[("Content-Type", "text/plain")]);
    let compressed = compress_response(original, &accepting("br"), &offering(&[Encoding::Br]));
    let mut body = compressed.into_body();

    // Start reading to activate the stream
    let _ = poll_once(body.chunk());
    assert_eq!(cancelled.load(Ordering::SeqCst), 0, "still streaming");

    // Cancel the output stream; there is no cancel reason in Rust, dropping is the cancel
    drop(body);

    assert_eq!(cancelled.load(Ordering::SeqCst), 1, "Source stream should be cancelled");
}

// Compressor interactions

type WriteFn = Box<dyn FnMut(&[u8]) -> io::Result<Vec<Bytes>> + Send>;
type EndFn = Box<dyn FnMut() -> io::Result<Vec<Bytes>> + Send>;

/// A scripted [Compressor] (`createMockCompressor`).
struct Mock {
    write: WriteFn,
    end: EndFn,
    _destroyed: Option<OnDrop>,
}

impl Mock {
    fn new(write: impl FnMut(&[u8]) -> io::Result<Vec<Bytes>> + Send + 'static) -> Mock {
        Mock { write: Box::new(write), end: Box::new(|| Ok(Vec::new())), _destroyed: None }
    }

    fn on_end(mut self, end: impl FnMut() -> io::Result<Vec<Bytes>> + Send + 'static) -> Mock {
        self.end = Box::new(end);
        self
    }

    fn on_destroy(mut self, count: Arc<AtomicUsize>) -> Mock {
        self._destroyed = Some(OnDrop(count));
        self
    }
}

impl Compressor for Mock {
    fn write(&mut self, chunk: &[u8]) -> io::Result<Vec<Bytes>> {
        (self.write)(chunk)
    }

    fn end(&mut self) -> io::Result<Vec<Bytes>> {
        (self.end)()
    }
}

fn failed(message: &str) -> io::Error {
    io::Error::other(message.to_string())
}

/// A source of `chunks` that counts how often it is read, and never ends when `endless`
/// (Remix's `pull` streams).
struct Counting {
    chunks: Vec<Bytes>,
    endless: bool,
    reads: Arc<AtomicUsize>,
}

impl http_body::Body for Counting {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(mut self: std::pin::Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Result<http_body::Frame<Bytes>, BoxError>>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.endless {
            return Poll::Ready(Some(Ok(http_body::Frame::data(Bytes::from_static(b"chunk")))));
        }
        if self.chunks.is_empty() {
            return Poll::Ready(None);
        }
        let chunk = self.chunks.remove(0);
        Poll::Ready(Some(Ok(http_body::Frame::data(chunk))))
    }
}

fn counting(chunks: &[&'static str], reads: &Arc<AtomicUsize>) -> Body {
    Body::wrap(Counting { chunks: chunks.iter().map(|c| Bytes::from_static(c.as_bytes())).collect(), endless: false, reads: reads.clone() })
}

fn endless(reads: &Arc<AtomicUsize>) -> Body {
    Body::wrap(Counting { chunks: Vec::new(), endless: true, reads: reads.clone() })
}

fn passthrough(chunk: &[u8]) -> io::Result<Vec<Bytes>> {
    Ok(vec![Bytes::copy_from_slice(chunk)])
}

#[test]
fn ignores_data_events_emitted_after_error() {
    // After a failed write, nothing else goes out: end() would emit data, but is never called.
    let error_emitted = Arc::new(AtomicBool::new(false));
    let emitted = error_emitted.clone();
    let mock = Mock::new(move |_| {
        emitted.store(true, Ordering::SeqCst);
        Err(failed("Compressor failed"))
    })
    .on_end(|| Ok(vec![Bytes::from_static(b"test")]));

    let mut compressed = compress_stream(chunked(vec![b"test".to_vec()]), mock);
    let mut chunks = Vec::new();
    let error = loop {
        match block_on(compressed.chunk()) {
            Some(Ok(chunk)) => chunks.push(chunk),
            Some(Err(error)) => break error,
            None => panic!("expected an error"),
        }
    };

    assert_eq!(error.to_string(), "Compressor failed");
    assert_eq!(chunks.len(), 0, "Should not receive data chunks after error");
    assert!(error_emitted.load(Ordering::SeqCst), "Error should have been emitted");
}

#[test]
fn ignores_data_events_emitted_after_cancellation() {
    // The compressor holds the chunk without output; cancelling destroys it, and nothing it
    // could still produce reaches the reader.
    let destroyed = Arc::new(AtomicUsize::new(0));
    let mock = Mock::new(|_| Ok(Vec::new())).on_destroy(destroyed.clone());
    let (source, input) = Body::channel();
    source.send("test");
    let mut compressed = compress_stream(input, mock);

    assert!(poll_once(compressed.chunk()).is_pending(), "blocked with no output");
    drop(compressed);

    assert_eq!(destroyed.load(Ordering::SeqCst), 1, "the compressor was destroyed on cancel");
    drop(source);
}

#[test]
fn handles_multiple_data_chunks_emitted_during_single_write() {
    // Real zlib can emit several data chunks for one write
    let mock = Mock::new(|chunk| {
        let (third, two_thirds) = (chunk.len() / 3, chunk.len() * 2 / 3);
        Ok(vec![Bytes::copy_from_slice(&chunk[..third]), Bytes::copy_from_slice(&chunk[third..two_thirds]), Bytes::copy_from_slice(&chunk[two_thirds..])])
    });

    let chunks = read_all(&mut compress_stream(chunked(vec![b"input".to_vec()]), mock)).unwrap();

    assert_eq!(chunks.len(), 3, "Should receive all data chunks from single write");
    assert_eq!(concat(&chunks), b"input", "Chunks should reassemble to original input");
}

#[test]
fn handles_backpressure_write_returns_false_then_drain() {
    // The next source chunk is read only after the reader took the previous output.
    let (reads, writes) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let counted = writes.clone();
    let mock = Mock::new(move |chunk| {
        counted.fetch_add(1, Ordering::SeqCst);
        passthrough(chunk)
    });
    let mut compressed = compress_stream(counting(&["chunk1", "chunk2", "chunk3"], &reads), mock);

    let first = block_on(compressed.chunk()).unwrap().unwrap();
    assert_eq!(&first[..], b"chunk1");
    assert_eq!(reads.load(Ordering::SeqCst), 1, "the source waits for the reader");

    let mut all = first.to_vec();
    all.extend(concat(&read_all(&mut compressed).unwrap()));
    assert_eq!(String::from_utf8(all).unwrap(), "chunk1chunk2chunk3");
    assert_eq!(writes.load(Ordering::SeqCst), 3, "Should have written all chunks");
}

#[test]
fn handles_cancel_while_waiting_for_drain() {
    let destroyed = Arc::new(AtomicUsize::new(0));
    let mock = Mock::new(passthrough).on_destroy(destroyed.clone());
    let reads = Arc::new(AtomicUsize::new(0));
    let mut compressed = compress_stream(endless(&reads), mock);

    // One chunk out; the compressor now waits for the reader to take it
    assert!(matches!(poll_once(compressed.chunk()), Poll::Ready(Some(Ok(_)))));
    drop(compressed);

    assert_eq!(destroyed.load(Ordering::SeqCst), 1, "Compressor should be destroyed on cancel");
}

/// A mock whose write number `fail_at` fails with `message`; counts writes.
fn failing_at(fail_at: usize, message: &'static str, writes: &Arc<AtomicUsize>) -> Mock {
    let writes = writes.clone();
    Mock::new(move |chunk| {
        let n = writes.fetch_add(1, Ordering::SeqCst) + 1;
        if n == fail_at {
            Err(failed(message))
        } else {
            passthrough(chunk)
        }
    })
}

#[test]
fn handles_error_emitted_while_waiting_for_drain_and_stops_loop() {
    let (reads, writes) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let mut compressed = compress_stream(endless(&reads), failing_at(1, "Compressor failed", &writes));

    let error = block_on(compressed.chunk()).unwrap().expect_err("the compressor error");
    assert_eq!(error.to_string(), "Compressor failed");
    assert!(block_on(compressed.chunk()).is_none(), "the stream ends after the error");

    assert_eq!(writes.load(Ordering::SeqCst), 1, "Should only write once before error stops loop");
}

#[test]
fn handles_compressor_error_after_successful_chunks() {
    let (reads, writes) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let mut compressed = compress_stream(endless(&reads), failing_at(2, "Compressor failed mid-stream", &writes));

    let error = read_all(&mut compressed).expect_err("the compressor error");

    assert_eq!(error.to_string(), "Compressor failed mid-stream");
    assert_eq!(writes.load(Ordering::SeqCst), 2, "Should only write twice before error stops loop");
}

#[test]
fn stops_calling_reader_read_after_error() {
    let (reads, writes) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let mut compressed = compress_stream(endless(&reads), failing_at(1, "Compressor failed", &writes));

    let error = block_on(compressed.chunk()).unwrap().expect_err("the compressor error");
    assert_eq!(error.to_string(), "Compressor failed");
    let _ = block_on(compressed.chunk());

    assert_eq!(reads.load(Ordering::SeqCst), 1, "Should only read the source once");
    assert_eq!(writes.load(Ordering::SeqCst), 1, "Should only write once");
}

#[test]
fn propagates_reader_cancel_errors() {
    // A Web stream's cancel() can reject; dropping a Rust body cannot fail. What carries over:
    // cancelling mid-read reaches the source's cancellation exactly once.
    let cancelled = Arc::new(AtomicUsize::new(0));
    let mut compressed = compress_stream(endless_source(OnDrop(cancelled.clone())), Mock::new(passthrough));

    let _ = poll_once(compressed.chunk());
    drop(compressed);

    assert_eq!(cancelled.load(Ordering::SeqCst), 1);
}

#[test]
fn handles_error_passed_to_write_callback() {
    let (source, input) = Body::channel();
    source.send("test");
    let mut compressed = compress_stream(input, Mock::new(|_| Err(failed("Write callback error"))));

    let error = block_on(compressed.chunk()).unwrap().expect_err("the write error");

    assert_eq!(error.to_string(), "Write callback error");
    drop(source);
}

#[test]
fn prevents_duplicate_error_reporting_if_destroy_also_emits_error() {
    let destroyed = Arc::new(AtomicUsize::new(0));
    let (source, input) = Body::channel();
    source.send("test");
    let mock = Mock::new(|_| Err(failed("Write callback error"))).on_destroy(destroyed.clone());
    let mut compressed = compress_stream(input, mock);

    let error = block_on(compressed.chunk()).unwrap().expect_err("the write error");
    assert_eq!(error.to_string(), "Write callback error");

    // The compressor is destroyed once, and the reader sees the end, not a second error
    assert!(block_on(compressed.chunk()).is_none());
    assert_eq!(destroyed.load(Ordering::SeqCst), 1, "destroyed once");
    drop(source);
}

#[test]
fn calls_compressor_end_when_input_stream_is_done() {
    let end_called = Arc::new(AtomicBool::new(false));
    let called = end_called.clone();
    let mock = Mock::new(passthrough).on_end(move || {
        called.store(true, Ordering::SeqCst);
        Ok(Vec::new())
    });

    read_all(&mut compress_stream(chunked(vec![b"test".to_vec()]), mock)).unwrap();

    assert!(end_called.load(Ordering::SeqCst), "compressor.end() should be called when input stream is done");
}

#[test]
fn closes_output_stream_when_compressor_emits_end_event() {
    let chunks = read_all(&mut compress_stream(chunked(vec![b"test".to_vec()]), Mock::new(passthrough))).unwrap();

    assert_eq!(chunks.len(), 1, "Should receive data chunk");
    assert_eq!(&chunks[0][..], b"test");
}

#[test]
fn handles_end_event_with_final_data_chunk() {
    let mock = Mock::new(passthrough).on_end(|| Ok(vec![Bytes::from_static(b" final")]));

    let chunks = read_all(&mut compress_stream(chunked(vec![b"test".to_vec()]), mock)).unwrap();

    assert_eq!(chunks.len(), 2, "Should receive all data chunks including final");
    assert_eq!(concat(&chunks), b"test final");
}
