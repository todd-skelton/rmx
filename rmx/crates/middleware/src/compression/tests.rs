//! Ports of Remix's `response/compress.test.ts` and `compression-middleware` tests.

use std::io::Read;

use rmx_fetch::{block_on, response, Body, Response};
use rmx_router::{Context, Router};

use super::*;

fn request(accept_encoding: Option<&str>) -> http::Request<Body> {
    let mut b = http::Request::builder().uri("https://remix.run/");
    if let Some(value) = accept_encoding {
        b = b.header("accept-encoding", value);
    }
    b.body(Body::empty()).unwrap()
}

fn head(accept_encoding: &str) -> http::Request<Body> {
    http::Request::builder().method("HEAD").uri("https://remix.run/").header("accept-encoding", accept_encoding).body(Body::empty()).unwrap()
}

fn text(body: &str) -> Response {
    Response::new(Body::from(body.to_string()))
}

fn with(mut res: Response, headers: &[(&'static str, &str)]) -> Response {
    for (k, v) in headers {
        res.headers_mut().insert(*k, v.parse().unwrap());
    }
    res
}

fn compress_default(res: Response, accept: Option<&str>) -> Response {
    compress_response(res, &request(accept), &CompressOptions::default())
}

fn offering(encodings: &[Encoding]) -> CompressOptions {
    CompressOptions { encodings: encodings.to_vec(), ..CompressOptions::default() }
}

fn encoding(res: &Response) -> Option<&str> {
    res.headers().get("content-encoding").map(|v| v.to_str().unwrap())
}

fn varies(res: &Response) -> bool {
    Vary::from_headers(res.headers().get_all("vary").iter().map(|v| v.to_str().unwrap())).has("accept-encoding")
}

fn decode(res: Response) -> String {
    let enc = encoding(&res).map(str::to_string);
    let bytes = block_on(res.into_body().bytes()).unwrap();
    let mut out = String::new();
    match enc.as_deref() {
        Some("gzip") => flate2::read::GzDecoder::new(&bytes[..]).read_to_string(&mut out).unwrap(),
        Some("deflate") => flate2::read::ZlibDecoder::new(&bytes[..]).read_to_string(&mut out).unwrap(),
        Some("br") => brotli::Decompressor::new(&bytes[..], 4096).read_to_string(&mut out).unwrap(),
        _ => return String::from_utf8(bytes.to_vec()).unwrap(),
    };
    out
}

#[test]
fn compresses_with_each_encoding() {
    let gz = compress_default(text("Hello, World!"), Some("gzip"));
    assert_eq!(encoding(&gz), Some("gzip"));
    assert_eq!(gz.headers()["accept-ranges"], "none");
    assert!(varies(&gz));
    assert_eq!(decode(gz), "Hello, World!");

    let br = compress_default(text("Hello, World!"), Some("br, gzip"));
    assert_eq!(encoding(&br), Some("br"));
    assert_eq!(decode(br), "Hello, World!");

    let deflate = compress_response(text("Hello, World!"), &request(Some("deflate")), &offering(&[Encoding::Deflate]));
    assert_eq!(encoding(&deflate), Some("deflate"));
    assert_eq!(decode(deflate), "Hello, World!");
}

#[test]
fn identity_keeps_representation_headers() {
    let res = with(text("Hello, World!"), &[("content-length", "2048"), ("etag", "\"abc123\""), ("vary", "Cookie")]);
    let mut res = res;
    *res.status_mut() = http::StatusCode::CREATED;
    let out = compress_default(res, Some("identity"));
    assert_eq!(out.status(), 201);
    assert_eq!(encoding(&out), None);
    assert_eq!(out.headers()["content-length"], "2048");
    assert_eq!(out.headers().get("accept-ranges"), None);
    assert_eq!(out.headers()["etag"], "\"abc123\"");
    let vary = Vary::parse(out.headers()["vary"].to_str().ok());
    assert!(vary.has("cookie") && vary.has("accept-encoding"));
    assert_eq!(decode(out), "Hello, World!");

    let once = compress_default(with(text("x"), &[("vary", "Cookie, Accept-Encoding")]), Some("identity"));
    assert_eq!(once.headers()["vary"].to_str().unwrap().to_lowercase().matches("accept-encoding").count(), 1);

    let none = compress_default(text("Hello, World!"), None);
    assert_eq!(encoding(&none), None);
    assert!(varies(&none), "no Accept-Encoding still varies");
}

#[test]
fn threshold_and_skips() {
    assert_eq!(encoding(&compress_default(text("Small"), Some("gzip"))), Some("gzip"), "no Content-Length compresses");

    let small = compress_default(with(text("Small"), &[("content-length", "5")]), Some("gzip"));
    assert_eq!((encoding(&small), small.headers().get("vary")), (None, None));
    assert_eq!(decode(small), "Small");
    let custom = CompressOptions { threshold: 3, ..CompressOptions::default() };
    assert_eq!(encoding(&compress_response(with(text("Small"), &[("content-length", "5")]), &request(Some("gzip")), &custom)), Some("gzip"));

    for headers in [&[("content-encoding", "gzip")][..], &[("cache-control", "public, no-transform, max-age=3600")], &[("accept-ranges", "bytes")]] {
        let out = compress_default(with(text("Already"), headers), Some("gzip"));
        assert_eq!(out.headers().get("vary"), None, "{headers:?}");
        assert_eq!(out.headers().get("content-encoding").map(|v| v.to_str().unwrap()), headers.iter().find(|h| h.0 == "content-encoding").map(|h| h.1));
    }
    let mut empty = Response::new(Body::empty());
    *empty.status_mut() = http::StatusCode::NO_CONTENT;
    assert_eq!(compress_default(empty, Some("gzip")).headers().get("vary"), None);
    let mut partial = text("part");
    *partial.status_mut() = http::StatusCode::PARTIAL_CONTENT;
    assert_eq!(encoding(&compress_default(partial, Some("gzip"))), None);

    let disabled = compress_response(text("Hello"), &request(Some("gzip, br")), &offering(&[]));
    assert_eq!((encoding(&disabled), disabled.headers().get("vary")), (None, None));
}

#[test]
fn levels_and_brotli_window_change_output() {
    let content = "Hello, World!".repeat(1000);
    let level = |level| {
        let options = CompressOptions { zlib: ZlibOptions { level, flush: None }, ..CompressOptions::default() };
        block_on(compress_response(text(&content), &request(Some("gzip")), &options).into_body().bytes()).unwrap().len()
    };
    assert!(level(9) < level(1));

    let content = "Hello, World! ".repeat(10000);
    let window = |lgwin| {
        let options = CompressOptions { brotli: BrotliOptions { lgwin, ..BrotliOptions::default() }, ..offering(&[Encoding::Br]) };
        let res = compress_response(text(&content), &request(Some("br")), &options);
        let len = block_on(res.into_body().bytes()).unwrap().len();
        let res = compress_response(text(&content), &request(Some("br")), &options);
        assert_eq!(decode(res), content);
        len
    };
    assert_ne!(window(10), window(22));
}

#[test]
fn negotiation_follows_client_weights_then_server_order() {
    let pick = |accept: &str, encodings: &[Encoding]| {
        let res = compress_response(text("Hello, World!"), &request(Some(accept)), &offering(encodings));
        (res.status().as_u16(), encoding(&res).map(str::to_string))
    };
    let all = &DEFAULT_ENCODINGS[..];
    let ok = |e: &str| (200, Some(e.to_string()));
    assert_eq!(pick("br, gzip, deflate", &[Encoding::Gzip, Encoding::Deflate]), ok("gzip"));
    assert_eq!(pick("gzip;q=0.8, deflate;q=1.0", all), ok("deflate"));
    assert_eq!(pick("br;q=0.5, gzip;q=1.0, deflate;q=0.8", all), ok("gzip"));
    assert_eq!(pick("gzip, deflate, br", &[Encoding::Deflate, Encoding::Br, Encoding::Gzip]), ok("deflate"));
    assert_eq!(pick("gzip;q=0, deflate;q=0, br;q=1.0", &[Encoding::Gzip, Encoding::Deflate, Encoding::Br]), ok("br"));
    assert_eq!(pick("gzip;q=0, deflate;q=0, br;q=0, identity", all), (200, None));
    assert_eq!(pick("identity;q=0, gzip", all), ok("gzip"));
    assert_eq!(pick("gzip, *;q=0.5", &[Encoding::Br, Encoding::Deflate]), ok("br"));
    assert_eq!(pick("gzip, *;q=0.8", all), ok("gzip"));
    assert_eq!(pick("gzip, *;q=0", &[Encoding::Br, Encoding::Deflate]).0, 406);
    assert_eq!(pick("identity, *;q=0", all), (200, None));
    assert_eq!(pick("*;q=1.0, identity;q=0", &[Encoding::Gzip, Encoding::Deflate, Encoding::Br]), ok("gzip"));

    let refused = compress_response(with(text("x"), &[("vary", "Cookie")]), &request(Some("gzip;q=0, deflate;q=0, br;q=0, identity;q=0")), &CompressOptions::default());
    assert_eq!(refused.status(), 406);
    let vary = Vary::parse(refused.headers()["vary"].to_str().ok());
    assert!(vary.has("cookie") && vary.has("accept-encoding"));
}

#[test]
fn etags_and_head_requests() {
    let strong = compress_default(with(text("Hello"), &[("etag", "\"abc\"")]), Some("gzip"));
    assert_eq!(strong.headers()["etag"], "W/\"abc\"");
    let weak = compress_default(with(text("Hello"), &[("etag", "W/\"abc\"")]), Some("gzip"));
    assert_eq!(weak.headers()["etag"], "W/\"abc\"");
    assert_eq!(compress_default(text("Hello"), Some("gzip")).headers().get("etag"), None);

    let res = with(text("Hello, World!"), &[("content-length", "2048"), ("etag", "\"abc\"")]);
    let out = compress_response(res, &head("br, gzip"), &CompressOptions::default());
    assert_eq!(encoding(&out), Some("br"));
    assert_eq!(out.headers()["etag"], "W/\"abc\"");
    assert_eq!(out.headers().get("content-length"), None);
    assert!(block_on(out.into_body().bytes()).unwrap().is_empty());
    let empty = compress_response(Response::new(Body::empty()), &head("gzip"), &CompressOptions::default());
    assert_eq!(encoding(&empty), Some("gzip"), "HEAD negotiates even without a body");
    let identity = compress_response(text("x"), &head("identity"), &CompressOptions::default());
    assert_eq!(encoding(&identity), None);
}

fn chunked(parts: Vec<&'static [u8]>) -> Body {
    Body::from_task(move |tx| async move {
        for part in parts {
            tx.send(part);
        }
    })
}

#[test]
fn streams_round_trip_and_flush_html() {
    let binary: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    for enc in DEFAULT_ENCODINGS {
        let parts: Vec<&'static [u8]> = vec![b"a", b"", b"bc", b"\xc3\xa9 unicode \xe2\x9c\x93"];
        let res = compress_response(Response::new(chunked(parts)), &request(Some(enc.as_str())), &CompressOptions::default());
        assert_eq!(decode(res), "abc\u{e9} unicode \u{2713}", "{enc:?}");

        let res = compress_response(Response::new(Body::from(binary.clone())), &request(Some(enc.as_str())), &CompressOptions::default());
        let bytes = block_on(res.into_body().bytes()).unwrap();
        let mut out = Vec::new();
        match enc {
            Encoding::Gzip => flate2::read::GzDecoder::new(&bytes[..]).read_to_end(&mut out).unwrap(),
            Encoding::Deflate => flate2::read::ZlibDecoder::new(&bytes[..]).read_to_end(&mut out).unwrap(),
            Encoding::Br => brotli::Decompressor::new(&bytes[..], 4096).read_to_end(&mut out).unwrap(),
        };
        assert_eq!(out, binary, "{enc:?} binary");

        let res = compress_response(Response::new(chunked(vec![])), &request(Some(enc.as_str())), &CompressOptions::default());
        assert_eq!(decode(res), "", "{enc:?} empty stream");

        // The first HTML chunk must come out before the source finishes.
        let (tx, body) = Body::channel();
        let res = compress_response(response::html_stream(body), &request(Some(enc.as_str())), &CompressOptions::default());
        tx.send("<p>first</p>");
        let mut body = res.into_body();
        let first = block_on(body.chunk()).unwrap().unwrap();
        assert!(!first.is_empty(), "{enc:?} flushed");
        drop(tx);
        let rest = block_on(body.bytes()).unwrap();
        let mut all = first.to_vec();
        all.extend_from_slice(&rest);
        let mut res = Response::new(Body::from(all));
        res.headers_mut().insert("content-encoding", enc.as_str().parse().unwrap());
        assert_eq!(decode(res), "<!DOCTYPE html><p>first</p>");
    }
}

fn router(middleware: Compression) -> Router {
    let mut router = Router::new().with(middleware);
    router.get("/page", |_: Context| async { response::html("<p>hello</p>".repeat(200)) });
    router.get("/data.json", |_: Context| async { with(text("{\"data\":\"value\"}"), &[("content-type", "application/json")]) });
    router.get("/events", |_: Context| async { with(text("event: message\ndata: hello\n\n"), &[("content-type", "text/event-stream")]) });
    router.get("/image.png", |_: Context| async { with(text("PNG"), &[("content-type", "image/png")]) });
    router.get("/small", |_: Context| async { with(text("Small"), &[("content-type", "text/plain"), ("content-length", "5")]) });
    router.get("/nocompress", |_: Context| async { with(text("not compressed"), &[("content-type", "text/plain"), ("x-no-compress", "true")]) });
    router
}

fn get(router: &Router, path: &str, accept: &str) -> Response {
    let req = http::Request::builder().uri(format!("https://remix.run{path}")).header("accept-encoding", accept).body(Body::empty()).unwrap();
    block_on(router.fetch(req))
}

#[test]
fn middleware_filters_media_types() {
    let r = router(compression());
    let page = get(&r, "/page", "gzip");
    assert_eq!(encoding(&page), Some("gzip"));
    assert!(decode(page).starts_with("<!DOCTYPE html><p>hello</p>"));
    assert_eq!(encoding(&get(&r, "/image.png", "gzip")), None);
    assert_eq!(get(&r, "/image.png", "gzip").headers().get("vary"), None);
    let identity = get(&r, "/data.json", "identity");
    assert!(varies(&identity));
    assert_eq!(encoding(&get(&r, "/small", "gzip")), None);
    assert_eq!(encoding(&get(&r, "/page", "br, gzip;q=0")), Some("br"));

    let json_only = router(compression().filter_media_type(|t| t.contains("json")));
    assert_eq!(encoding(&get(&json_only, "/data.json", "gzip")), Some("gzip"));
    assert_eq!(encoding(&get(&json_only, "/page", "gzip")), None);
}

#[test]
fn middleware_options_per_response() {
    let r = router(compression().threshold(1).encodings_with(|res| {
        if res.headers().contains_key("x-no-compress") {
            vec![]
        } else if res.headers().get("content-type").is_some_and(|t| t == "text/event-stream") {
            vec![Encoding::Gzip, Encoding::Deflate]
        } else {
            DEFAULT_ENCODINGS.to_vec()
        }
    }));
    assert_eq!(encoding(&get(&r, "/events", "br, gzip")), Some("gzip"));
    assert_eq!(encoding(&get(&r, "/data.json", "br, gzip")), Some("br"));
    assert_eq!(encoding(&get(&r, "/small", "gzip")), Some("gzip"), "threshold 1");
    let skipped = get(&r, "/nocompress", "gzip");
    assert_eq!(encoding(&skipped), None);
    assert_eq!(decode(skipped), "not compressed");
}

#[test]
fn skips_partial_content() {
    let partial = with(text("Partial content"), &[("content-range", "bytes 0-9/100")]);
    let mut partial = partial;
    *partial.status_mut() = http::StatusCode::PARTIAL_CONTENT;
    let res = compress_default(partial, Some("gzip"));
    assert_eq!((res.status().as_u16(), encoding(&res), varies(&res)), (206, None, false));
    assert_eq!(decode(res), "Partial content");
}

/// A source that sends two chunks and then fails, like Remix's `createErrorStream(2)`.
fn failing_source() -> Body {
    Body::from_task(|tx| async move {
        tx.send("chunk 1\n".repeat(50));
        tx.send("chunk 2\n".repeat(50));
        tx.abort("Stream error");
    })
}

#[test]
fn propagates_errors_from_the_source() {
    for encoding in [Encoding::Br, Encoding::Gzip, Encoding::Deflate] {
        let res = Response::new(failing_source());
        let res = compress_response(with(res, &[("content-type", "text/plain")]), &request(Some(encoding.as_str())), &offering(&[encoding]));
        let mut body = res.into_body();
        let error = block_on(async {
            loop {
                match body.chunk().await {
                    Some(Ok(_)) => continue,
                    Some(Err(error)) => return Some(error.to_string()),
                    None => return None,
                }
            }
        });
        assert_eq!(error.as_deref(), Some("Stream error"), "{}", encoding.as_str());
    }
}

#[test]
fn dropping_the_compressed_body_cancels_the_source() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct OnDrop(Arc<AtomicBool>);
    impl Drop for OnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let cancelled = Arc::new(AtomicBool::new(false));
    let guard = OnDrop(cancelled.clone());
    let source = Body::from_task(move |tx| async move {
        let _guard = guard;
        // One chunk, then a wait that never ends, like a source waiting on the network.
        tx.send("chunk\n".repeat(100));
        std::future::pending::<()>().await;
    });
    // HTML flushes every chunk, so the first compressed chunk arrives while the source is
    // still streaming (Remix reads once from a Node stream; buffered brotli would hold it).
    let res = compress_response(with(Response::new(source), &[("content-type", "text/html")]), &request(Some("br")), &offering(&[Encoding::Br]));
    let mut body = res.into_body();
    assert!(block_on(body.chunk()).is_some_and(|c| c.is_ok()));
    assert!(!cancelled.load(Ordering::SeqCst), "still streaming while read");
    drop(body);
    assert!(cancelled.load(Ordering::SeqCst), "the source was dropped with the compressed body");
}
