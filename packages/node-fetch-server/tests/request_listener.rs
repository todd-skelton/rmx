//! Port of Remix's `node-fetch-server/src/lib/request-listener.test.ts`, one test per case.
//!
//! Remix drives `createRequestListener` with mocked Node request and response objects; here
//! every case runs against a real server on a loopback port with a raw TCP client (or hyper's
//! HTTP/2 client), so the mocked events become what a client does: `res.emit('close')` is the
//! client closing the connection, `req.emit('aborted')` is the client half-closing mid-upload,
//! `req.emit('error')` is a malformed body. A thrown handler error is a panic. Responses have
//! no `statusText` in Rust, so the status line carries the canonical reason.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http::{Method, StatusCode, Version};
use hyper_util::rt::{TokioExecutor, TokioIo};
use rmx_fetch::{AbortError, Body, Request, RequestExt};
use rmx_fetch_router::{Context, Router};
use rmx_node_fetch_server::{create_request, serve_with, ClientAddress, ServeError, ServeOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const TIMEOUT: Duration = Duration::from_secs(5);

async fn start(router: Router, options: ServeOptions) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(serve_with(listener, router, options));
    addr
}

/// `t.mock.fn()` for `onError`: records each error and keeps the default response.
fn recording() -> (ServeOptions, Arc<Mutex<Vec<String>>>) {
    let errors = Arc::new(Mutex::new(Vec::new()));
    let log = errors.clone();
    let options = ServeOptions::default().on_error(move |error| {
        log.lock().unwrap().push(error.to_string());
        None
    });
    (options, errors)
}

fn count(errors: &Arc<Mutex<Vec<String>>>) -> usize {
    errors.lock().unwrap().len()
}

/// Send `request` and read until the server closes the connection.
async fn exchange(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    let _ = tokio::time::timeout(TIMEOUT, stream.read_to_end(&mut out)).await.expect("the server closes the connection");
    String::from_utf8_lossy(&out).into_owned()
}

/// A `GET` of `path` with `headers`, closing after the response.
async fn get(addr: SocketAddr, path: &str, headers: &[&str]) -> String {
    let mut request = format!("GET {path} HTTP/1.1\r\nConnection: close\r\n");
    for header in headers {
        request.push_str(header);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    exchange(addr, &request).await
}

/// The body of a raw HTTP/1.1 response, de-chunked when it is chunked.
fn body_of(raw: &str) -> String {
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((raw, ""));
    if !head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        return body.to_string();
    }
    let (mut rest, mut out) = (body, String::new());
    while let Some((size, after)) = rest.split_once("\r\n") {
        let Ok(size) = usize::from_str_radix(size.trim(), 16) else { break };
        if size == 0 || after.len() < size {
            break;
        }
        out.push_str(&after[..size]);
        rest = after[size..].strip_prefix("\r\n").unwrap_or(&after[size..]);
    }
    out
}

/// A router that answers every `GET` (and `POST`) to `/` and `/*path` with `handler`.
fn router<H: rmx_fetch_router::Handler + Clone>(handler: H) -> Router {
    let mut router = Router::new();
    router.any("/", handler.clone());
    router.any("/*path", handler);
    router
}

/// Wait until `check` holds, or fail with `message`.
async fn eventually(message: &str, check: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while !check() {
        assert!(tokio::time::Instant::now() < deadline, "{message}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// A channel the test reads what a handler observed from.
fn report<T: Send + 'static>() -> (tokio::sync::mpsc::UnboundedSender<T>, tokio::sync::mpsc::UnboundedReceiver<T>) {
    tokio::sync::mpsc::unbounded_channel()
}

async fn next<T>(rx: &mut tokio::sync::mpsc::UnboundedReceiver<T>) -> T {
    tokio::time::timeout(TIMEOUT, rx.recv()).await.expect("a report from the handler").expect("an open channel")
}

/// Send an HTTP/2 request with prior knowledge (Node's `http2.connect`).
async fn send_h2(addr: SocketAddr, request: http::Request<Body>) -> (StatusCode, http::HeaderMap, String) {
    let stream = TcpStream::connect(addr).await.unwrap();
    let (mut sender, connection) = hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream)).await.unwrap();
    tokio::spawn(connection);
    let response = sender.send_request(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let text = Body::wrap(body).text().await.unwrap();
    (parts.status, parts.headers, text)
}

#[tokio::test]
async fn returns_a_request_listener() {
    let addr = start(router(|_: Context| async { "Hello, world!" }), ServeOptions::default()).await;

    let raw = get(addr, "/", &["Host: localhost"]).await;

    assert_eq!(body_of(&raw), "Hello, world!");
}

fn created() -> http::Response<Body> {
    let mut response = http::Response::new(Body::from("Hello, world!"));
    *response.status_mut() = StatusCode::CREATED;
    response.headers_mut().insert("X-A", "A".parse().unwrap());
    response.headers_mut().insert("X-B", "B".parse().unwrap());
    response
}

#[tokio::test]
async fn returns_custom_status_status_text_and_header_values_http_1() {
    let addr = start(router(|_: Context| async { created() }), ServeOptions::default()).await;

    let raw = get(addr, "/", &["Host: localhost"]).await;
    let head = raw.split("\r\n\r\n").next().unwrap().to_ascii_lowercase();

    assert!(raw.starts_with("HTTP/1.1 201 Created\r\n"), "{raw}");
    assert!(head.contains("\r\nx-a: a") && head.contains("\r\nx-b: b"), "{raw}");
}

#[tokio::test]
async fn returns_custom_status_status_text_and_header_values_http_2() {
    let addr = start(router(|_: Context| async { created() }), ServeOptions::default()).await;

    let request = http::Request::builder().uri("http://localhost/").version(Version::HTTP_2).body(Body::empty()).unwrap();
    let (status, headers, body) = send_h2(addr, request).await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(headers["x-a"], "A");
    assert_eq!(headers["x-b"], "B");
    assert_eq!(body, "Hello, world!");
}

fn boom() -> &'static str {
    panic!("boom!")
}

#[tokio::test]
async fn calls_on_error_when_an_error_is_thrown_in_the_request_handler() {
    let (options, errors) = recording();
    let addr = start(router(|_: Context| async { boom() }), options).await;

    get(addr, "/", &["Host: localhost"]).await;

    assert_eq!(*errors.lock().unwrap(), ["request handler panicked: boom!"]);
}

#[tokio::test]
async fn returns_a_500_internal_server_error_response_when_an_error_is_thrown_in_the_request_handler() {
    let (options, _errors) = recording();
    let addr = start(router(|_: Context| async { boom() }), options).await;

    let raw = get(addr, "/", &["Host: localhost"]).await;

    assert!(raw.starts_with("HTTP/1.1 500"), "{raw}");
    assert_eq!(body_of(&raw), "Internal Server Error");
}

/// Connect, send a `GET`, and close once `ready` reports the handler started.
async fn close_after_start(addr: SocketAddr, started: &mut tokio::sync::mpsc::UnboundedReceiver<()>) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
    next(started).await;
    drop(stream);
}

#[tokio::test]
async fn does_not_forward_request_abort_errors_to_on_error_when_the_response_closes_before_the_handler_returns() {
    let (options, errors) = recording();
    let (started, mut started_rx) = report();
    let (aborted, mut aborted_rx) = report();
    let handler = move |ctx: Context| {
        let (started, aborted, signal) = (started.clone(), aborted.clone(), ctx.signal().clone());
        async move {
            tokio::spawn(async move {
                signal.until_aborted().await;
                let _ = aborted.send(());
            });
            let _ = started.send(());
            // The handler rejects with the abort: here it panics once the signal aborts.
            ctx.signal().until_aborted().await;
            if ctx.signal().aborted() {
                panic!("{}", AbortError);
            }
            StatusCode::OK
        }
    };
    let addr = start(router(handler), options).await;

    close_after_start(addr, &mut started_rx).await;
    next(&mut aborted_rx).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(count(&errors), 0);
}

#[tokio::test]
async fn drops_the_response_without_writing_when_the_request_was_aborted_before_send() {
    let (options, errors) = recording();
    let (started, mut started_rx) = report();
    let handler = move |ctx: Context| {
        let started = started.clone();
        async move {
            let _ = started.send(());
            // Resolve with a normal-looking response after the abort, as an outer catch would.
            ctx.signal().until_aborted().await;
            rmx_response::status(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error")
        }
    };
    let addr = start(router(handler), options).await;

    close_after_start(addr, &mut started_rx).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(count(&errors), 0);
}

/// A handler that answers only when the test sends the response, without reading the signal.
fn late_handler(started: tokio::sync::mpsc::UnboundedSender<()>, release: Arc<tokio::sync::Notify>, finished: Arc<AtomicBool>) -> impl rmx_fetch_router::Handler + Clone {
    move |_: Context| {
        let (started, release, finished) = (started.clone(), release.clone(), finished.clone());
        async move {
            let _ = started.send(());
            release.notified().await;
            finished.store(true, Ordering::SeqCst);
            "late"
        }
    }
}

#[tokio::test]
async fn drops_a_late_response_after_close_when_the_handler_does_not_read_the_request_signal() {
    let (options, errors) = recording();
    let (started, mut started_rx) = report();
    let (release, finished) = (Arc::new(tokio::sync::Notify::new()), Arc::new(AtomicBool::new(false)));
    let addr = start(router(late_handler(started, release.clone(), finished)), options).await;

    close_after_start(addr, &mut started_rx).await;
    release.notify_waiters();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // The response has nowhere to go; the server neither writes it nor reports an error.
    assert_eq!(count(&errors), 0);
}

#[tokio::test]
async fn drops_a_late_zero_argument_handler_response_after_close() {
    // Every rmx handler receives the context; this one ignores it entirely.
    let (options, errors) = recording();
    let (started, mut started_rx) = report();
    let (release, finished) = (Arc::new(tokio::sync::Notify::new()), Arc::new(AtomicBool::new(false)));
    let addr = start(router(late_handler(started, release.clone(), finished)), options).await;

    close_after_start(addr, &mut started_rx).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    release.notify_waiters();
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(count(&errors), 0);
}

#[tokio::test]
async fn does_not_forward_request_abort_errors_to_on_error_while_streaming_the_response_body() {
    let (options, errors) = recording();
    let (aborted, mut aborted_rx) = report();
    let handler = move |ctx: Context| {
        let (signal, watched, aborted) = (ctx.signal().clone(), ctx.signal().clone(), aborted.clone());
        async move {
            // A JS stream source keeps running after the stream is cancelled; a Rust body task is
            // dropped with the body, so a separate task reports the abort.
            tokio::spawn(async move {
                watched.until_aborted().await;
                let _ = aborted.send(());
            });
            http::Response::new(Body::from_task(move |tx| async move {
                tx.send("first");
                // The body errors with the request's abort once the client is gone.
                signal.until_aborted().await;
                tx.abort(AbortError);
            }))
        }
    };
    let addr = start(router(handler), options).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
    let mut buf = [0u8; 512];
    let n = stream.read(&mut buf).await.unwrap();
    assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 200"));
    drop(stream);
    next(&mut aborted_rx).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(count(&errors), 0);
}

/// A body that sends "partial" and then fails.
fn failing_body() -> http::Response<Body> {
    http::Response::new(Body::from_task(|tx| async move {
        tx.send("partial");
        tokio::time::sleep(Duration::from_millis(10)).await;
        tx.abort("boom after commit");
    }))
}

#[tokio::test]
async fn does_not_send_an_error_response_when_a_response_stream_errors_after_headers_are_sent() {
    let errors = Arc::new(Mutex::new(Vec::new()));
    let log = errors.clone();
    let options = ServeOptions::default().on_error(move |error| {
        log.lock().unwrap().push(error.clone());
        Some(rmx_response::status(StatusCode::INTERNAL_SERVER_ERROR, "fallback"))
    });
    let addr = start(router(|_: Context| async { failing_body() }), options).await;

    let raw = get(addr, "/", &["Host: localhost"]).await;

    assert!(raw.starts_with("HTTP/1.1 200"), "{raw}");
    assert_eq!(body_of(&raw), "partial");
    assert!(!raw.contains("fallback") && !raw.ends_with("0\r\n\r\n"), "the connection closes without a complete body: {raw:?}");
    eventually("the error is reported", || errors.lock().unwrap().len() == 1).await;
    assert_eq!(*errors.lock().unwrap(), [ServeError::ResponseBody("boom after commit".into())]);
}

#[tokio::test]
async fn destroys_the_response_without_waiting_for_a_slow_error_handler_for_committed_stream_errors() {
    let (started, finished) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let release_rx = Mutex::new(release_rx);
    let (s, f) = (started.clone(), finished.clone());
    let options = ServeOptions::default().on_error(move |_| {
        s.store(true, Ordering::SeqCst);
        let _ = release_rx.lock().unwrap().recv();
        f.store(true, Ordering::SeqCst);
        None
    });
    let addr = start(router(|_: Context| async { failing_body() }), options).await;

    // The connection closes while the error handler is still running.
    let raw = get(addr, "/", &["Host: localhost"]).await;
    assert_eq!(body_of(&raw), "partial");
    eventually("the error handler starts", || started.load(Ordering::SeqCst)).await;
    assert!(!finished.load(Ordering::SeqCst));

    release_tx.send(()).unwrap();
    eventually("the error handler finishes", || finished.load(Ordering::SeqCst)).await;
}

/// What a handler saw when it read the request body in a task of its own: the error and
/// whether the signal was aborted.
type BodyRead = (Result<String, String>, bool);

/// A handler that reads the body with `read` in a task (so the read finishes even when the
/// connection goes away) and reports the outcome.
fn body_reader(reads: tokio::sync::mpsc::UnboundedSender<BodyRead>, wait_for_abort: bool, buffered: bool) -> impl rmx_fetch_router::Handler + Clone {
    move |mut ctx: Context| {
        let reads = reads.clone();
        async move {
            let (mut body, signal) = (ctx.take_body(), ctx.signal().clone());
            tokio::spawn(async move {
                if wait_for_abort {
                    signal.until_aborted().await;
                }
                let read = if buffered {
                    body.bytes().await.map(|b| String::from_utf8_lossy(&b).into_owned())
                } else {
                    let mut text = String::new();
                    loop {
                        match body.chunk().await {
                            Some(Ok(chunk)) => text.push_str(&String::from_utf8_lossy(&chunk)),
                            Some(Err(error)) => break Err(error),
                            None => break Ok(text),
                        }
                    }
                };
                let _ = reads.send((read.map_err(|e| e.to_string()), signal.aborted()));
            });
            std::future::pending::<()>().await;
            "never"
        }
    }
}

/// A `POST` that announces 100 bytes and sends 7.
const PARTIAL_POST: &[u8] = b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100\r\n\r\npartial";

#[tokio::test]
async fn rejects_request_body_reads_when_the_request_aborts_before_ending() {
    let (reads, mut reads_rx) = report();
    let addr = start(router(body_reader(reads, false, false)), ServeOptions::default()).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(PARTIAL_POST).await.unwrap();
    // The client stops sending (half-closes) before the body ended.
    stream.shutdown().await.unwrap();
    let (read, aborted) = next(&mut reads_rx).await;

    assert_eq!(read, Err(AbortError.to_string()));
    assert!(aborted);
}

#[tokio::test]
async fn rejects_buffered_request_body_reads_when_the_request_closes_before_ending() {
    let (reads, mut reads_rx) = report();
    let addr = start(router(body_reader(reads, false, true)), ServeOptions::default()).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(PARTIAL_POST).await.unwrap();
    drop(stream);
    let (read, aborted) = next(&mut reads_rx).await;

    assert_eq!(read, Err(AbortError.to_string()));
    assert!(aborted);
}

/// A chunked `POST` whose second chunk size is not hex: a body error that is not a disconnect.
const MALFORMED_POST: &[u8] = b"POST / HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n7\r\npartial\r\nzz\r\n";

#[tokio::test]
async fn treats_request_body_errors_as_request_abort_errors() {
    let (reads, mut reads_rx) = report();
    let addr = start(router(body_reader(reads, false, true)), ServeOptions::default()).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(MALFORMED_POST).await.unwrap();
    let (read, aborted) = next(&mut reads_rx).await;

    // The body's own error comes through, and it aborts the request signal.
    let error = read.expect_err("the body error");
    assert_ne!(error, AbortError.to_string());
    assert!(aborted);
}

#[tokio::test]
async fn does_not_forward_a_handler_failure_from_a_failed_request_body_to_on_error() {
    // rmx: the handler rethrows (panics on) the body error, which is the signal's abort reason
    // in Remix, so `onError` is skipped (`isRequestAbortError`).
    let (options, errors) = recording();
    let (reads, mut reads_rx) = report();
    let handler = move |mut ctx: Context| {
        let reads = reads.clone();
        async move {
            let read = ctx.take_body().text().await;
            let _ = reads.send(read.is_err());
            read.unwrap()
        }
    };
    let addr = start(router(handler), options).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(MALFORMED_POST).await.unwrap();
    assert!(next(&mut reads_rx).await);
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(count(&errors), 0);
}

#[tokio::test]
async fn treats_request_body_errors_as_request_abort_errors_after_response_close() {
    let (options, errors) = recording();
    let (reads, mut reads_rx) = report();
    let addr = start(router(body_reader(reads, false, true)), options).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(PARTIAL_POST).await.unwrap();
    drop(stream);
    let (read, aborted) = next(&mut reads_rx).await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    assert!(read.is_err() && aborted);
    assert_eq!(count(&errors), 0);
}

#[tokio::test]
async fn rejects_request_body_reads_when_the_request_already_aborted() {
    let (reads, mut reads_rx) = report();
    // The read starts only after the signal aborted.
    let addr = start(router(body_reader(reads, true, true)), ServeOptions::default()).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(PARTIAL_POST).await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    drop(stream);
    let (read, aborted) = next(&mut reads_rx).await;

    assert_eq!(read, Err(AbortError.to_string()));
    assert!(aborted);
}

#[tokio::test]
async fn keeps_an_error_listener_after_already_aborted_request_body_reads() {
    // Node needs an error listener so a late socket error does not crash the process; in Rust
    // the equivalent is that reading an aborted body again keeps failing the same way.
    let (options, errors) = recording();
    let (reads, mut reads_rx) = report::<(String, String)>();
    let handler = move |mut ctx: Context| {
        let reads = reads.clone();
        async move {
            let (mut body, signal) = (ctx.take_body(), ctx.signal().clone());
            tokio::spawn(async move {
                signal.until_aborted().await;
                let first = body.chunk().await.and_then(Result::err).map(|e| e.to_string()).unwrap_or_default();
                let second = body.chunk().await.and_then(Result::err).map(|e| e.to_string()).unwrap_or_default();
                let _ = reads.send((first, second));
            });
            std::future::pending::<()>().await;
            "never"
        }
    };
    let addr = start(router(handler), options).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(PARTIAL_POST).await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    drop(stream);
    let (first, second) = next(&mut reads_rx).await;

    assert_eq!(first, AbortError.to_string());
    assert_eq!(second, AbortError.to_string());
    assert_eq!(count(&errors), 0);
}

#[tokio::test]
async fn returns_a_stable_signal_from_handler_requests() {
    let handler = |ctx: Context| async move {
        let (signal, again) = (ctx.signal().clone(), ctx.request().signal());
        // Clones share one state: neither is aborted, and they agree.
        format!("{} {} {}", ctx.url().href(), signal.aborted(), again.aborted() == signal.aborted())
    };
    let addr = start(router(handler), ServeOptions::default()).await;

    let raw = get(addr, "/", &[]).await;

    assert_eq!(body_of(&raw), "http://localhost/ false true");
}

async fn url_with(options: ServeOptions, headers: &[&str]) -> String {
    let addr = start(router(|ctx: Context| async move { ctx.url().href().to_string() }), options).await;
    body_of(&get(addr, "/", headers).await)
}

#[tokio::test]
async fn uses_the_host_header_to_construct_the_url_by_default() {
    assert_eq!(url_with(ServeOptions::default(), &["Host: example.com"]).await, "http://example.com/");
}

#[tokio::test]
async fn provides_native_request_instances_to_request_handlers() {
    let handler = |ctx: Context| async move {
        // A new request built from the handler's request, with an extra header.
        let original: &Request = ctx.request();
        let mut forwarded = http::Request::builder().method(original.method().clone()).uri(original.uri().clone());
        for (name, value) in original.headers() {
            forwarded = forwarded.header(name, value);
        }
        let forwarded = forwarded.header("X-Forwarded", "yes").body(Body::empty()).unwrap();
        format!("{} {} {}", forwarded.uri(), forwarded.method(), forwarded.headers()["x-forwarded"].to_str().unwrap())
    };
    let addr = start(router(handler), ServeOptions::default()).await;

    let raw = get(addr, "/", &["Host: example.com"]).await;

    assert_eq!(body_of(&raw), "http://example.com/ GET yes");
}

/// A server whose handler records that it ran, behind an `onError` that answers 500 "boom".
async fn construction_error_server(handler_called: Arc<AtomicBool>, reads_client: bool) -> (SocketAddr, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let options = ServeOptions::default().on_error(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
        Some(rmx_response::status(StatusCode::INTERNAL_SERVER_ERROR, "boom"))
    });
    let handler = move |ctx: Context| {
        let called = handler_called.clone();
        async move {
            called.store(true, Ordering::SeqCst);
            if reads_client {
                let _ = ctx.get::<ClientAddress>();
            }
            "ok"
        }
    };
    (start(router(handler), options).await, calls)
}

// Remix's TRACE method cannot become a `Request`; in Rust every method can, so the request
// that cannot be created here is one whose `Host` makes an invalid URL.

#[tokio::test]
async fn routes_request_construction_errors_to_the_error_handler_in_the_request_only_handler_path() {
    let handler_called = Arc::new(AtomicBool::new(false));
    let (addr, calls) = construction_error_server(handler_called.clone(), false).await;

    let raw = get(addr, "/", &["Host: exa mple.com"]).await;

    assert!(raw.starts_with("HTTP/1.1 500"), "{raw}");
    assert!(!handler_called.load(Ordering::SeqCst));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn routes_request_construction_errors_to_the_error_handler_in_the_client_info_handler_path() {
    let handler_called = Arc::new(AtomicBool::new(false));
    let (addr, calls) = construction_error_server(handler_called.clone(), true).await;

    let raw = get(addr, "/", &["Host: exa mple.com"]).await;

    assert!(raw.starts_with("HTTP/1.1 500"), "{raw}");
    assert!(!handler_called.load(Ordering::SeqCst));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn applies_backpressure_and_pauses_the_request_when_the_body_is_not_read() {
    // The handler never reads the body: the server answers without waiting for or buffering
    // the rest of a 10 MB upload, of which only 1 KiB was sent.
    let addr = start(router(|_: Context| async { "ok" }), ServeOptions::default()).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 10000000\r\n\r\n").await.unwrap();
    stream.write_all(&[0u8; 1024]).await.unwrap();
    let mut buf = [0u8; 1024];
    let n = tokio::time::timeout(TIMEOUT, stream.read(&mut buf)).await.expect("a response before the body ended").unwrap();
    let raw = String::from_utf8_lossy(&buf[..n]);

    assert!(raw.starts_with("HTTP/1.1 200") && raw.ends_with("ok"), "{raw}");
}

#[tokio::test]
async fn uses_the_authority_header_to_construct_the_url_for_http_2_requests() {
    let addr = start(router(|ctx: Context| async move { ctx.url().href().to_string() }), ServeOptions::default()).await;

    let request = http::Request::builder().uri("http://example.com/").version(Version::HTTP_2).body(Body::empty()).unwrap();
    let (_, _, body) = send_h2(addr, request).await;

    assert_eq!(body, "http://example.com/");
}

#[tokio::test]
async fn uses_the_host_option_to_override_the_host_header() {
    assert_eq!(url_with(ServeOptions::default().host("remix.run"), &["Host: example.com"]).await, "http://remix.run/");
}

#[tokio::test]
async fn uses_the_protocol_option_to_construct_the_url() {
    assert_eq!(url_with(ServeOptions::default().protocol("https:"), &["Host: example.com"]).await, "https://example.com/");
}

fn trusted() -> ServeOptions {
    ServeOptions::default().trust_proxy(true)
}

#[tokio::test]
async fn ignores_proxy_headers_by_default() {
    let headers = ["Host: example.com", "Forwarded: for=203.0.113.43; proto=https; host=remix.run", "X-Forwarded-Host: remix.run", "X-Forwarded-Proto: https"];

    assert_eq!(url_with(ServeOptions::default(), &headers).await, "http://example.com/");
}

#[tokio::test]
async fn uses_the_forwarded_proto_and_host_parameters_when_proxies_are_trusted() {
    let headers = ["Host: example.com", "Forwarded: for=203.0.113.43; proto=https; host=remix.run"];

    assert_eq!(url_with(trusted(), &headers).await, "https://remix.run/");
}

#[tokio::test]
async fn uses_quoted_forwarded_proto_and_host_values_when_proxies_are_trusted() {
    let headers = ["Host: example.com", "Forwarded: for=\"[2001:db8:cafe::17]\"; proto=\"https\"; host=\"remix.run\""];

    assert_eq!(url_with(trusted(), &headers).await, "https://remix.run/");
}

#[tokio::test]
async fn uses_x_forwarded_host_and_x_forwarded_proto_when_proxies_are_trusted() {
    let headers = ["Host: example.com", "X-Forwarded-Host: remix.run", "X-Forwarded-Proto: https"];

    assert_eq!(url_with(trusted(), &headers).await, "https://remix.run/");
}

#[tokio::test]
async fn uses_the_first_x_forwarded_host_and_x_forwarded_proto_values_when_proxies_are_trusted() {
    let headers = ["Host: example.com", "X-Forwarded-Host: remix.run, example.com", "X-Forwarded-Proto: https, http"];

    assert_eq!(url_with(trusted(), &headers).await, "https://remix.run/");
}

#[tokio::test]
async fn uses_the_host_option_before_trusted_proxy_host_headers() {
    let headers = ["Host: example.com", "Forwarded: host=remix.run", "X-Forwarded-Host: remix.run"];

    assert_eq!(url_with(trusted().host("app.example.com"), &headers).await, "http://app.example.com/");
}

#[tokio::test]
async fn ignores_invalid_forwarded_protocol_header_values_when_proxies_are_trusted() {
    let headers = ["Host: example.com", "Forwarded: proto=javascript", "X-Forwarded-Proto: file"];

    assert_eq!(url_with(trusted(), &headers).await, "http://example.com/");
}

#[tokio::test]
async fn uses_the_protocol_option_before_trusted_proxy_protocol_headers() {
    let headers = ["Host: example.com", "Forwarded: proto=https"];

    assert_eq!(url_with(trusted().protocol("http:"), &headers).await, "http://example.com/");
}

/// The `ClientAddress` a handler sees, and the client's own port.
async fn client_with(options: ServeOptions, headers: &[&str]) -> (String, u16) {
    let handler = |ctx: Context| async move {
        let client = ctx.get::<ClientAddress>().copied().unwrap();
        format!("{} {} {}", client.address, client.family(), client.port)
    };
    let addr = start(router(handler), options).await;
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let port = stream.local_addr().unwrap().port();
    let mut request = String::from("GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
    for header in headers {
        request.push_str(&format!("{header}\r\n"));
    }
    stream.write_all(format!("{request}\r\n").as_bytes()).await.unwrap();
    let mut out = String::new();
    stream.read_to_string(&mut out).await.unwrap();
    (body_of(&out), port)
}

#[tokio::test]
async fn uses_x_forwarded_for_for_client_address_when_proxies_are_trusted() {
    let (client, port) = client_with(trusted(), &["X-Forwarded-For: 203.0.113.43, 10.0.0.1"]).await;

    assert_eq!(client, format!("203.0.113.43 IPv4 {port}"));
}

#[tokio::test]
async fn uses_the_forwarded_for_parameter_for_client_address_when_proxies_are_trusted() {
    let (client, _) = client_with(trusted(), &["Forwarded: for=\"[2001:db8:cafe::17]:4711\"", "X-Forwarded-For: 203.0.113.43"]).await;

    assert_eq!(client, "2001:db8:cafe::17 IPv6 4711");
}

#[tokio::test]
async fn ignores_proxy_client_address_headers_by_default() {
    let (client, port) = client_with(ServeOptions::default(), &["Forwarded: for=203.0.113.43", "X-Forwarded-For: 203.0.113.43"]).await;

    assert_eq!(client, format!("127.0.0.1 IPv4 {port}"));
}

#[tokio::test]
async fn reads_request_method_headers_and_body_text() {
    let handler = |mut ctx: Context| async move {
        assert_eq!(ctx.method(), Method::POST);
        assert_eq!(ctx.headers()["x-test"], "yes");
        let text = ctx.take_body().text().await.unwrap();
        assert_eq!(text, "Hello, world!");
        // The body has been taken: what is left is empty.
        assert!(ctx.take_body().is_empty_hint());
        "ok"
    };
    let addr = start(router(handler), ServeOptions::default()).await;

    let raw = exchange(addr, "POST / HTTP/1.1\r\nHost: localhost\r\nX-Test: yes\r\nContent-Length: 13\r\nConnection: close\r\n\r\nHello, world!").await;

    assert_eq!(body_of(&raw), "ok");
}

#[tokio::test]
async fn sets_multiple_set_cookie_headers() {
    let handler = |_: Context| async {
        let mut response = http::Response::new(Body::from("Hello, world!"));
        response.headers_mut().insert("Content-Type", "text/plain".parse().unwrap());
        response.headers_mut().append("Set-Cookie", "a=1".parse().unwrap());
        response.headers_mut().append("Set-Cookie", "b=2".parse().unwrap());
        response
    };
    let addr = start(router(handler), ServeOptions::default()).await;

    let raw = get(addr, "/", &["Host: localhost"]).await.to_ascii_lowercase();

    assert!(raw.contains("\r\ncontent-type: text/plain\r\n"), "{raw}");
    assert!(raw.contains("\r\nset-cookie: a=1\r\n") && raw.contains("\r\nset-cookie: b=2\r\n"), "{raw}");
}

#[tokio::test]
async fn truncates_the_response_body_when_the_request_method_is_head() {
    let addr = start(router(|_: Context| async { "Hello, world!" }), ServeOptions::default()).await;

    let raw = exchange(addr, "HEAD / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").await;

    assert!(raw.starts_with("HTTP/1.1 200"), "{raw}");
    assert_eq!(raw.split_once("\r\n\r\n").unwrap().1, "");
}

#[tokio::test]
async fn handles_backpressure_when_writing_response_chunks() {
    let handler = |_: Context| async {
        http::Response::new(Body::from_task(|tx| async move {
            for chunk in ["chunk1", "chunk2", "chunk3", "chunk4", "chunk5"] {
                tx.send(chunk);
                if !tx.ready().await {
                    return;
                }
            }
        }))
    };
    let addr = start(router(handler), ServeOptions::default()).await;

    let raw = get(addr, "/", &["Host: localhost"]).await;

    assert_eq!(body_of(&raw), "chunk1chunk2chunk3chunk4chunk5");
}

#[tokio::test]
async fn writes_the_first_response_stream_chunk_before_waiting_for_the_second_chunk() {
    let release = Arc::new(tokio::sync::Notify::new());
    let gate = release.clone();
    let handler = move |_: Context| {
        let gate = gate.clone();
        async move {
            http::Response::new(Body::from_task(move |tx| async move {
                tx.send("first");
                gate.notified().await;
                tx.send("second");
            }))
        }
    };
    let addr = start(router(handler), ServeOptions::default()).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut raw = Vec::new();
    while !body_of(&String::from_utf8_lossy(&raw)).contains("first") {
        let mut buf = [0u8; 512];
        let n = tokio::time::timeout(TIMEOUT, stream.read(&mut buf)).await.expect("the first chunk").unwrap();
        raw.extend_from_slice(&buf[..n]);
    }
    assert_eq!(body_of(&String::from_utf8_lossy(&raw)), "first");

    release.notify_one();
    stream.read_to_end(&mut raw).await.unwrap();
    assert_eq!(body_of(&String::from_utf8_lossy(&raw)), "firstsecond");
}

/// Counts drops: a body task being cancelled.
struct OnDrop(Arc<AtomicUsize>);

impl Drop for OnDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn cancels_a_streaming_response_body_when_the_response_closes_before_it_finishes() {
    let (cancelled, aborted) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)));
    let (c, a) = (cancelled.clone(), aborted.clone());
    let handler = move |ctx: Context| {
        let (guard, aborted, signal) = (OnDrop(c.clone()), a.clone(), ctx.signal().clone());
        async move {
            tokio::spawn(async move {
                signal.until_aborted().await;
                aborted.store(true, Ordering::SeqCst);
            });
            http::Response::new(Body::from_task(move |tx| async move {
                let _guard = guard;
                tx.send("first");
                std::future::pending::<()>().await;
            }))
        }
    };
    let addr = start(router(handler), ServeOptions::default()).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
    let mut buf = [0u8; 512];
    let n = stream.read(&mut buf).await.unwrap();
    assert!(String::from_utf8_lossy(&buf[..n]).contains("first"));
    drop(stream);

    eventually("the request signal aborts", || aborted.load(Ordering::SeqCst)).await;
    eventually("the streaming body is cancelled", || cancelled.load(Ordering::SeqCst) == 1).await;
}

// createRequest HTTP/2 authority

#[test]
fn throws_when_host_and_authority_identify_different_hosts() {
    let request = http::Request::builder().uri("http://example.com/").version(Version::HTTP_2).header("Host", "other.example.com").body(Body::empty()).unwrap();

    let error = create_request(request, "127.0.0.1:1".parse().unwrap(), &ServeOptions::default()).unwrap_err();

    assert_eq!(error.to_string(), "Host header does not match :authority");
}

// createRequest abort behavior

/// A handler that reports when its signal aborts, and answers when `answer` is notified.
fn abort_watcher(aborted: Arc<AtomicBool>) -> impl rmx_fetch_router::Handler + Clone {
    move |ctx: Context| {
        let (aborted, signal) = (aborted.clone(), ctx.signal().clone());
        async move {
            tokio::spawn(async move {
                signal.until_aborted().await;
                aborted.store(true, Ordering::SeqCst);
            });
            if ctx.url().pathname() == "/wait" {
                std::future::pending::<()>().await;
            }
            "done"
        }
    }
}

#[tokio::test]
async fn aborts_the_request_signal_when_response_closes_before_finishing() {
    let aborted = Arc::new(AtomicBool::new(false));
    let addr = start(router(abort_watcher(aborted.clone())), ServeOptions::default()).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(b"GET /wait HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!aborted.load(Ordering::SeqCst));
    drop(stream);

    eventually("the signal aborts", || aborted.load(Ordering::SeqCst)).await;
}

#[tokio::test]
async fn does_not_abort_after_finish_even_if_close_occurs_later() {
    let aborted = Arc::new(AtomicBool::new(false));
    let addr = start(router(abort_watcher(aborted.clone())), ServeOptions::default()).await;

    let raw = get(addr, "/done", &["Host: localhost"]).await;
    assert_eq!(body_of(&raw), "done");
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(!aborted.load(Ordering::SeqCst));
}

#[tokio::test]
async fn aborts_the_request_signal_and_rejects_body_reads_when_the_request_aborts_before_ending() {
    let (reads, mut reads_rx) = report();
    let addr = start(router(body_reader(reads, false, true)), ServeOptions::default()).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(PARTIAL_POST).await.unwrap();
    stream.shutdown().await.unwrap();
    let (read, aborted) = next(&mut reads_rx).await;

    assert_eq!(read, Err(AbortError.to_string()));
    assert!(aborted);
}

#[tokio::test]
async fn aborts_the_request_signal_and_rejects_body_reads_when_the_request_errors_before_ending() {
    let (reads, mut reads_rx) = report();
    let addr = start(router(body_reader(reads, false, false)), ServeOptions::default()).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(MALFORMED_POST).await.unwrap();
    let (read, aborted) = next(&mut reads_rx).await;

    assert!(read.is_err_and(|error| error != AbortError.to_string()));
    assert!(aborted);
}

#[tokio::test]
async fn aborts_the_request_signal_and_rejects_body_reads_when_the_request_closes_before_ending() {
    let (reads, mut reads_rx) = report();
    let addr = start(router(body_reader(reads, false, false)), ServeOptions::default()).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(PARTIAL_POST).await.unwrap();
    drop(stream);
    let (read, aborted) = next(&mut reads_rx).await;

    assert_eq!(read, Err(AbortError.to_string()));
    assert!(aborted);
}

#[tokio::test]
async fn ignores_request_errors_after_an_abort_has_already_rejected_the_body() {
    // After the abort, a later failure (here, the socket going away entirely) does not
    // replace the abort error.
    let (reads, mut reads_rx) = report::<(String, String)>();
    let handler = move |mut ctx: Context| {
        let reads = reads.clone();
        async move {
            let mut body = ctx.take_body();
            tokio::spawn(async move {
                let first = loop {
                    match body.chunk().await {
                        Some(Ok(_)) => continue,
                        Some(Err(error)) => break error.to_string(),
                        None => break String::new(),
                    }
                };
                let later = body.chunk().await.and_then(Result::err).map(|e| e.to_string()).unwrap_or_default();
                let _ = reads.send((first, later));
            });
            std::future::pending::<()>().await;
            "never"
        }
    };
    let addr = start(router(handler), ServeOptions::default()).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(PARTIAL_POST).await.unwrap();
    stream.shutdown().await.unwrap();
    let (first, later) = next(&mut reads_rx).await;
    drop(stream);

    assert_eq!(first, AbortError.to_string());
    assert_eq!(later, AbortError.to_string());
}

#[tokio::test]
async fn aborts_the_request_signal_and_rejects_body_reads_when_the_request_already_aborted() {
    let (reads, mut reads_rx) = report();
    let addr = start(router(body_reader(reads, true, false)), ServeOptions::default()).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(PARTIAL_POST).await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    drop(stream);
    let (read, aborted) = next(&mut reads_rx).await;

    assert_eq!(read, Err(AbortError.to_string()));
    assert!(aborted);
}


// Port of `request-listener.http2.test.ts` ("HTTP/2 request authority"). Remix varies the
// handler's arity (no arguments, request, request and client); an rmx handler always gets the
// one `Context`, so those cases differ only in the options they pass.

/// Send a `GET /` over HTTP/2 with `uri` giving `:scheme` and `:authority` and the extra
/// `headers`, like `sendRequest` with `http2.connect`.
async fn h2_get(router: Router, options: ServeOptions, uri: &str, headers: &[(&str, &str)]) -> (StatusCode, String) {
    let addr = start(router, options).await;
    let mut request = http::Request::builder().uri(uri).version(Version::HTTP_2);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let (status, _, body) = send_h2(addr, request.body(Body::empty()).unwrap()).await;
    (status, body)
}

/// A router that answers with the request URL.
fn url_router() -> Router {
    router(|ctx: Context| async move { ctx.url().href().to_string() })
}

/// A router that records whether it was called.
fn called_router() -> (Router, Arc<AtomicBool>) {
    let called = Arc::new(AtomicBool::new(false));
    let flag = called.clone();
    let router = router(move |_: Context| {
        let flag = flag.clone();
        async move {
            flag.store(true, Ordering::SeqCst);
            "ok"
        }
    });
    (router, called)
}

/// `ServeOptions` whose `on_error` records each error and answers 422 "Invalid host".
fn invalid_host() -> (ServeOptions, Arc<Mutex<Vec<String>>>) {
    let errors = Arc::new(Mutex::new(Vec::new()));
    let log = errors.clone();
    let options = ServeOptions::default().on_error(move |error| {
        log.lock().unwrap().push(error.to_string());
        let mut response = http::Response::new(Body::from("Invalid host"));
        *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
        Some(response)
    });
    (options, errors)
}

#[tokio::test]
async fn rejects_conflicting_host_values_before_calling_a_request_handler() {
    let (router, called) = called_router();
    let (options, errors) = recording();
    let (status, body) = h2_get(router, options, "http://example.com/", &[("host", "other.example.com")]).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, "Bad Request");
    assert!(!called.load(Ordering::SeqCst));
    assert_eq!(*errors.lock().unwrap(), ["Host header does not match :authority"]);
}

#[tokio::test]
async fn rejects_conflicting_host_values_before_calling_a_handler_without_arguments() {
    let (router, called) = called_router();
    let (options, errors) = invalid_host();
    let (status, body) = h2_get(router, options, "http://example.com/", &[("host", "other.example.com")]).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body, "Invalid host");
    assert!(!called.load(Ordering::SeqCst));
    assert_eq!(count(&errors), 1);
}

#[tokio::test]
async fn rejects_conflicting_ports_before_calling_a_handler_with_client_information() {
    let (router, called) = called_router();
    let (options, errors) = recording();
    let options = options.host("app.example.com").trust_proxy(true);
    let (status, _) = h2_get(router, options, "http://example.com:8080/", &[("host", "example.com:8081")]).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!called.load(Ordering::SeqCst));
    assert_eq!(count(&errors), 1);
}

#[tokio::test]
async fn uses_a_custom_error_response_for_conflicting_host_values() {
    let (router, called) = called_router();
    let options = ServeOptions::default().on_error(|_| {
        let mut response = http::Response::new(Body::from("Invalid host"));
        *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
        Some(response)
    });
    let (status, body) = h2_get(router, options, "http://example.com/", &[("host", "other.example.com")]).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body, "Invalid host");
    assert!(!called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn returns_400_by_default_for_conflicting_host_values() {
    let (status, body) = h2_get(url_router(), ServeOptions::default(), "http://example.com/", &[("host", "other.example.com")]).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, "Bad Request");
}

#[tokio::test]
async fn uses_authority_when_host_is_equivalent_after_http_normalization() {
    let options = ServeOptions::default().protocol("https:");
    let (status, body) = h2_get(url_router(), options, "http://example.com:80/", &[("host", "EXAMPLE.COM")]).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "https://example.com:80/");
}

#[tokio::test]
async fn normalizes_default_ports_using_the_http_2_scheme() {
    let (status, body) = h2_get(url_router(), ServeOptions::default(), "https://example.com:443/", &[("host", "EXAMPLE.COM")]).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "http://example.com:443/");
}

#[tokio::test]
async fn accepts_equivalent_ipv6_hosts_with_a_nondefault_port() {
    let (status, body) = h2_get(url_router(), ServeOptions::default(), "http://[2001:db8::1]:8080/", &[("host", "[2001:0DB8:0:0:0:0:0:1]:8080")]).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "http://[2001:db8::1]:8080/");
}

#[tokio::test]
async fn uses_authority_when_host_is_absent() {
    let (status, body) = h2_get(url_router(), ServeOptions::default(), "http://example.com/", &[]).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "http://example.com/");
}

/// One HTTP/2 frame header and payload.
fn h2_frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u32;
    let mut frame = vec![(len >> 16) as u8, (len >> 8) as u8, len as u8, kind, flags];
    frame.extend_from_slice(&stream.to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// A `GET` over HTTP/2 with exactly the pseudo-headers and headers in `fields`, written as
/// uncompressed HPACK literals. hyper's and h2's clients build `:scheme` and `:authority` from
/// a `Uri`, which cannot have a scheme without an authority. Returns the `:status` (from the
/// static table index the server uses) and the body.
async fn raw_h2_get(addr: SocketAddr, fields: &[(&str, &str)]) -> (u16, String) {
    let mut block = Vec::new();
    for (name, value) in fields {
        assert!(name.len() < 127 && value.len() < 127);
        block.push(0x00);
        block.push(name.len() as u8);
        block.extend_from_slice(name.as_bytes());
        block.push(value.len() as u8);
        block.extend_from_slice(value.as_bytes());
    }
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let mut out = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n".to_vec();
    out.extend(h2_frame(0x4, 0, 0, &[]));
    // END_STREAM | END_HEADERS on stream 1.
    out.extend(h2_frame(0x1, 0x5, 1, &block));
    stream.write_all(&out).await.unwrap();

    let (mut status, mut body) = (0, String::new());
    loop {
        let mut head = [0u8; 9];
        tokio::time::timeout(TIMEOUT, stream.read_exact(&mut head)).await.expect("a frame").unwrap();
        let len = ((head[0] as usize) << 16) | ((head[1] as usize) << 8) | head[2] as usize;
        let (kind, flags) = (head[3], head[4]);
        let mut payload = vec![0u8; len];
        stream.read_exact(&mut payload).await.unwrap();
        match kind {
            // SETTINGS: acknowledge the server's.
            0x4 if flags & 0x1 == 0 => stream.write_all(&h2_frame(0x4, 0x1, 0, &[])).await.unwrap(),
            0x1 => {
                status = match payload[0] {
                    0x88 => 200,
                    0x8c => 400,
                    other => panic!("an indexed :status, got {other:#x}"),
                };
                if flags & 0x1 != 0 {
                    return (status, body);
                }
            }
            0x0 => {
                body.push_str(&String::from_utf8_lossy(&payload));
                if flags & 0x1 != 0 {
                    return (status, body);
                }
            }
            0x7 => panic!("GOAWAY"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn uses_host_when_authority_is_absent() {
    let addr = start(url_router(), ServeOptions::default()).await;
    let fields = [(":method", "GET"), (":scheme", "http"), (":path", "/"), ("host", "example.com")];
    let (status, body) = raw_h2_get(addr, &fields).await;

    assert_eq!(status, 200);
    assert_eq!(body, "http://example.com/");
}

#[tokio::test]
async fn preserves_explicit_host_and_protocol_overrides() {
    let options = ServeOptions::default().host("app.example.com").protocol("https:").trust_proxy(true);
    let headers = [("host", "example.com"), ("forwarded", "host=proxy.example.com;proto=http")];
    let (status, body) = h2_get(url_router(), options, "http://example.com/", &headers).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "https://app.example.com/");
}

#[tokio::test]
async fn preserves_trusted_proxy_host_and_protocol_overrides() {
    let options = ServeOptions::default().trust_proxy(true);
    let headers = [("host", "example.com"), ("forwarded", "host=proxy.example.com;proto=https")];
    let (status, body) = h2_get(url_router(), options, "http://example.com/", &headers).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "https://proxy.example.com/");
}
