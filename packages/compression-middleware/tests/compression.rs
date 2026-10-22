//! Port of Remix's `compression-middleware/src/lib/compression.test.ts`, one test per case.
//! Remix's `encodings`, `zlib` and `brotli` options take a value or a function of the
//! response; here they are `.encodings(..)` and `.encodings_with(..)` and so on.

use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use http::HeaderValue;
use rmx_fetch::{block_on, Body, Response};
use rmx_compression_middleware::{compression, BrotliOptions, Encoding, ZlibOptions};
use rmx_fetch_router::{Context, Router};

fn response(body: &'static str, headers: &[(&'static str, &'static str)]) -> Response {
    let mut response = Response::new(Body::from(body));
    for (name, value) in headers {
        response.headers_mut().insert(*name, HeaderValue::from_static(value));
    }
    response
}

/// `router.get(path, () => new Response(body, { headers }))`.
fn serve(router: &mut Router, path: &str, body: &'static str, headers: &'static [(&'static str, &'static str)]) {
    router.get(path, move |_: Context| async move { response(body, headers) });
}

fn fetch(router: &Router, path: &str, accept_encoding: &str) -> Response {
    let request = http::Request::builder().uri(format!("https://remix.run{path}")).header("Accept-Encoding", accept_encoding).body(Body::empty()).unwrap();
    block_on(router.fetch(request))
}

fn header(response: &Response, name: &str) -> Option<String> {
    response.headers().get(name).map(|v| v.to_str().unwrap().to_string())
}

fn text(response: Response) -> String {
    String::from_utf8(block_on(response.into_body().bytes()).unwrap().to_vec()).unwrap()
}

fn gunzip(response: Response) -> String {
    let bytes = block_on(response.into_body().bytes()).unwrap();
    let mut out = String::new();
    flate2::read::GzDecoder::new(&bytes[..]).read_to_string(&mut out).unwrap();
    out
}

/// `/(?:^|,)\s*accept-encoding\s*(?:,|$)/i`.
fn varies_on_accept_encoding(response: &Response) -> bool {
    header(response, "Vary").unwrap_or_default().split(',').any(|name| name.trim().eq_ignore_ascii_case("accept-encoding"))
}

#[test]
fn compresses_compressible_content_types() {
    let mut router = Router::new().with(compression());
    serve(&mut router, "/", "Hello, World!", &[("Content-Type", "text/html")]);

    let response = fetch(&router, "/", "gzip");

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Encoding").as_deref(), Some("gzip"));
    assert_eq!(gunzip(response), "Hello, World!");
}

#[test]
fn does_not_compress_non_compressible_content_types() {
    let mut router = Router::new().with(compression());
    serve(&mut router, "/image.png", "fake image data", &[("Content-Type", "image/png")]);

    let response = fetch(&router, "/image.png", "gzip");

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Encoding"), None);
    assert_eq!(header(&response, "Vary"), None);
    assert_eq!(text(response), "fake image data");
}

#[test]
fn marks_identity_responses_as_accept_encoding_variants() {
    let mut router = Router::new().with(compression());
    serve(&mut router, "/app.js", "console.log(\"Hello, World!\")", &[("Content-Type", "text/javascript")]);

    let identity = fetch(&router, "/app.js", "identity");
    assert_eq!(header(&identity, "Content-Encoding"), None);
    assert!(varies_on_accept_encoding(&identity));
    assert_eq!(text(identity), "console.log(\"Hello, World!\")");

    let gzip = fetch(&router, "/app.js", "gzip");
    assert_eq!(header(&gzip, "Content-Encoding").as_deref(), Some("gzip"));
    assert!(varies_on_accept_encoding(&gzip));
}

#[test]
fn respects_threshold_option() {
    let mut router = Router::new().with(compression().threshold(10));
    serve(&mut router, "/", "Small", &[("Content-Type", "text/plain"), ("Content-Length", "5")]);

    let response = fetch(&router, "/", "gzip");

    assert_eq!(header(&response, "Content-Encoding"), None);
    assert_eq!(header(&response, "Vary"), None);
    assert_eq!(text(response), "Small");
}

#[test]
fn compresses_responses_when_content_length_is_not_set() {
    let mut router = Router::new().with(compression().threshold(1024));
    // A small response without Content-Length
    serve(&mut router, "/", "Small", &[("Content-Type", "text/plain")]);

    let response = fetch(&router, "/", "gzip");

    // Compressed, because the threshold needs a Content-Length
    assert_eq!(header(&response, "Content-Encoding").as_deref(), Some("gzip"));
}

fn json_and_html(router: &mut Router) {
    serve(router, "/data.json", "{\"data\":\"value\"}", &[("Content-Type", "application/json")]);
    serve(router, "/page.html", "<html>test</html>", &[("Content-Type", "text/html")]);
}

#[test]
fn respects_custom_filter_media_type() {
    // Only compress JSON
    let mut router = Router::new().with(compression().filter_media_type(|media_type| media_type.contains("json")));
    json_and_html(&mut router);

    assert_eq!(header(&fetch(&router, "/data.json", "gzip"), "Content-Encoding").as_deref(), Some("gzip"));

    let html = fetch(&router, "/page.html", "gzip");
    assert_eq!(header(&html, "Content-Encoding"), None);
    assert_eq!(text(html), "<html>test</html>");
}

#[test]
fn allows_custom_filter_media_type_to_use_is_compressible_mime_type() {
    // Only compress if it's compressible AND not HTML
    let filter = |media_type: &str| rmx_mime::is_compressible_mime_type(media_type) && !media_type.contains("html");
    let mut router = Router::new().with(compression().filter_media_type(filter));
    json_and_html(&mut router);

    assert_eq!(header(&fetch(&router, "/data.json", "gzip"), "Content-Encoding").as_deref(), Some("gzip"));

    // HTML is compressible, but the custom filter excludes it
    let html = fetch(&router, "/page.html", "gzip");
    assert_eq!(header(&html, "Content-Encoding"), None);
    assert_eq!(text(html), "<html>test</html>");
}

#[test]
fn supports_dynamic_encodings_based_on_response() {
    let mut router = Router::new().with(compression().encodings_with(|response| {
        if response.headers().get("Content-Type").is_some_and(|t| t == "text/event-stream") {
            vec![Encoding::Gzip, Encoding::Deflate]
        } else {
            vec![Encoding::Br, Encoding::Gzip, Encoding::Deflate]
        }
    }));
    serve(&mut router, "/events", "event: message\ndata: hello\n\n", &[("Content-Type", "text/event-stream")]);
    serve(&mut router, "/data.json", "{\"data\":\"value\"}", &[("Content-Type", "application/json")]);

    // SSE uses gzip (brotli excluded)
    assert_eq!(header(&fetch(&router, "/events", "br, gzip"), "Content-Encoding").as_deref(), Some("gzip"));
    // JSON uses brotli
    assert_eq!(header(&fetch(&router, "/data.json", "br, gzip"), "Content-Encoding").as_deref(), Some("br"));
}

#[test]
fn allows_disabling_compression_per_response_via_empty_encodings_array() {
    // Don't compress responses with an X-No-Compress header
    let mut router = Router::new().with(compression().encodings_with(|response| if response.headers().contains_key("X-No-Compress") { vec![] } else { vec![Encoding::Gzip] }));
    serve(&mut router, "/nocompress", "not compressed", &[("Content-Type", "text/plain"), ("X-No-Compress", "true")]);
    serve(&mut router, "/compress", "compressed", &[("Content-Type", "text/plain")]);

    let skipped = fetch(&router, "/nocompress", "gzip");
    assert_eq!(header(&skipped, "Content-Encoding"), None);
    assert_eq!(text(skipped), "not compressed");

    assert_eq!(header(&fetch(&router, "/compress", "gzip"), "Content-Encoding").as_deref(), Some("gzip"));
}

// rmx only: the zlib and brotli options can depend on the response too.

#[test]
fn zlib_and_brotli_options_per_response() {
    let (zlib_calls, brotli_calls) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (z, b) = (zlib_calls.clone(), brotli_calls.clone());
    let middleware = compression()
        .zlib_with(move |response| {
            z.fetch_add(1, Ordering::SeqCst);
            let level = if response.headers().contains_key("X-Fast") { 1 } else { 9 };
            ZlibOptions { level, ..ZlibOptions::default() }
        })
        .brotli_with(move |_| {
            b.fetch_add(1, Ordering::SeqCst);
            BrotliOptions { quality: 5, ..BrotliOptions::default() }
        });
    let mut router = Router::new().with(middleware);
    let content: &'static str = Box::leak("Hello, World! ".repeat(2000).into_boxed_str());
    serve(&mut router, "/fast", content, &[("Content-Type", "text/plain"), ("X-Fast", "1")]);
    serve(&mut router, "/small", content, &[("Content-Type", "text/plain")]);

    let fast = block_on(fetch(&router, "/fast", "gzip").into_body().bytes()).unwrap().len();
    let small = block_on(fetch(&router, "/small", "gzip").into_body().bytes()).unwrap().len();

    assert!(small < fast, "level 9 ({small}) is smaller than level 1 ({fast})");
    assert_eq!(header(&fetch(&router, "/small", "br"), "Content-Encoding").as_deref(), Some("br"));
    assert!(zlib_calls.load(Ordering::SeqCst) >= 2);
    assert!(brotli_calls.load(Ordering::SeqCst) >= 1);
}
