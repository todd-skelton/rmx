//! Serve an [`rmx_router::Router`] over TCP (Remix's `node-fetch-server`).
//!
//! ```no_run
//! # async fn run(router: rmx_router::Router) -> std::io::Result<()> {
//! let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
//! let options = rmx_serve::ServeOptions::default().trust_proxy(true);
//! rmx_serve::serve_with(listener, router, options).await
//! # }
//! ```
//!
//! Each request reaches the router with an absolute URL built like Remix's
//! `createRequestListener` builds it ([`create_request`]): the `protocol` and `host` options
//! first, then trusted proxy headers (`Forwarded`, `X-Forwarded-Proto`, `X-Forwarded-Host`),
//! then the HTTP/2 `:authority` or the `Host` header. Handlers read the client with
//! `ctx.get::<ClientAddress>()`. A handler that panics answers 500 "Internal Server Error"
//! (Remix's response for a thrown error) unless [`ServeOptions::on_error`] supplies a response.
//!
//! Each request also carries an [`AbortSignal`](rmx_fetch::AbortSignal) (`ctx.signal()`)
//! that aborts when the response can no longer be written before it finished: the client
//! disconnected while the handler ran or while the body streamed.
//!
//! [`fetch`] is the matching client, for proxies and calls to other servers.
//!
//! This is the only crate that depends on a specific runtime (tokio) and HTTP
//! implementation (hyper); everything else works with plain `Request` / `Response` values.

use std::any::Any;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::fmt;
use std::future::{poll_fn, Future};
use std::net::{IpAddr, SocketAddr};
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll, Waker};

use http::{header, StatusCode, Version};
use http_body::Body as _;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use rmx_fetch::{AbortController, AbortError, Body, Request, RequestExt, Response};
use rmx_router::Router;
use rmx_url::Url;
use tokio::net::TcpListener;

/// The socket peer, available to handlers via `ctx.get::<RemoteAddr>()`.
#[derive(Clone, Copy, Debug)]
pub struct RemoteAddr(pub SocketAddr);

/// The client that sent a request (`ClientAddress`): the socket peer, or with
/// [`ServeOptions::trust_proxy`] the address a trusted proxy reported. Read it with
/// `ctx.get::<ClientAddress>()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientAddress {
    pub address: IpAddr,
    pub port: u16,
}

impl ClientAddress {
    /// `IPv4` or `IPv6`.
    pub fn family(&self) -> &'static str {
        if self.address.is_ipv4() {
            "IPv4"
        } else {
            "IPv6"
        }
    }
}

/// Why a request failed before or while it was handled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServeError {
    /// An HTTP/2 request whose `Host` header names a different host than `:authority`.
    /// Answered with 400 by default.
    ConflictingAuthority,
    /// The URL built for the request is not valid.
    InvalidUrl(String),
    /// The handler panicked, with the panic message.
    Panic(String),
    /// The response body failed after the status and headers were sent. The connection is
    /// closed; a response [`ServeOptions::on_error`] returns is not used.
    ResponseBody(String),
}

impl fmt::Display for ServeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServeError::ConflictingAuthority => f.write_str("Host header does not match :authority"),
            ServeError::InvalidUrl(url) => write!(f, "Invalid URL: {url}"),
            ServeError::Panic(message) => write!(f, "request handler panicked: {message}"),
            ServeError::ResponseBody(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ServeError {}

type ErrorHandler = Arc<dyn Fn(&ServeError) -> Option<Response> + Send + Sync>;

/// Options for [`serve_with`] and [`create_request`] (`RequestListenerOptions`).
#[derive(Clone, Default)]
pub struct ServeOptions {
    /// Use this host for every request URL instead of `:authority` or `Host`.
    pub host: Option<String>,
    /// Use this protocol (`https` or `https:`) for every request URL. The default is `http`,
    /// since this server speaks plain TCP.
    pub protocol: Option<String>,
    /// Read `Forwarded`, `X-Forwarded-Proto`, `X-Forwarded-Host` and `X-Forwarded-For`.
    /// Only enable this behind a proxy that overwrites them.
    pub trust_proxy: bool,
    on_error: Option<ErrorHandler>,
}

impl fmt::Debug for ServeOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServeOptions")
            .field("host", &self.host)
            .field("protocol", &self.protocol)
            .field("trust_proxy", &self.trust_proxy)
            .field("on_error", &self.on_error.is_some())
            .finish()
    }
}

impl ServeOptions {
    pub fn host(mut self, host: impl Into<String>) -> ServeOptions {
        self.host = Some(host.into());
        self
    }

    pub fn protocol(mut self, protocol: impl Into<String>) -> ServeOptions {
        self.protocol = Some(protocol.into());
        self
    }

    pub fn trust_proxy(mut self, trust: bool) -> ServeOptions {
        self.trust_proxy = trust;
        self
    }

    /// Decide the response for a failed request (`onError`). Returning `None` keeps the
    /// default: 400 for [`ServeError::ConflictingAuthority`], else 500. Without a handler,
    /// errors are logged to stderr.
    pub fn on_error(mut self, handler: impl Fn(&ServeError) -> Option<Response> + Send + Sync + 'static) -> ServeOptions {
        self.on_error = Some(Arc::new(handler));
        self
    }

    /// The response for `error`.
    pub fn error_response(&self, error: &ServeError) -> Response {
        let custom = match &self.on_error {
            Some(handler) => match std::panic::catch_unwind(AssertUnwindSafe(|| handler(error))) {
                Ok(response) => response,
                Err(_) => {
                    eprintln!("There was an error in the error handler: {error}");
                    return internal_server_error();
                }
            },
            None => {
                eprintln!("{error}");
                None
            }
        };
        custom.unwrap_or_else(|| match error {
            ServeError::ConflictingAuthority => rmx_fetch::response::status(StatusCode::BAD_REQUEST, "Bad Request"),
            _ => internal_server_error(),
        })
    }
}

fn internal_server_error() -> Response {
    let mut res = http::Response::new(Body::from("Internal Server Error"));
    *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    res.headers_mut().insert(header::CONTENT_TYPE, header::HeaderValue::from_static("text/plain"));
    res
}

/// Accept connections forever, handling each request with `router`.
pub async fn serve(listener: TcpListener, router: Router) -> std::io::Result<()> {
    serve_with(listener, router, ServeOptions::default()).await
}

/// [`serve`] with options (`createRequestListener(handler, options)`).
pub async fn serve_with(listener: TcpListener, router: Router, options: ServeOptions) -> std::io::Result<()> {
    let options = Arc::new(options);
    loop {
        let (stream, remote) = match listener.accept().await {
            Ok(conn) => conn,
            // Transient accept errors (too many open files, aborted handshakes) should not
            // take the server down.
            Err(e) => {
                eprintln!("rmx-serve: accept failed: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
        };
        let _ = stream.set_nodelay(true);
        let (router, options) = (router.clone(), options.clone());
        tokio::spawn(async move {
            let service = service_fn(move |req: hyper::Request<Incoming>| {
                let (router, options) = (router.clone(), options.clone());
                async move {
                    let lifecycle = Lifecycle::new();
                    let controller = lifecycle.controller.clone();
                    let mut req = req.map(|body| Body::wrap(RequestBody::new(body, controller)));
                    req.set_signal(lifecycle.controller.signal());
                    let res = handle(&router, &options, req, remote).await;
                    Ok::<_, Infallible>(res.map(|body| ServedBody::new(body, lifecycle, options)))
                }
            });
            let _ = auto::Builder::new(TokioExecutor::new())
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
    }
}

/// A request's lifecycle (Remix's `createRequestLifecycle`): aborts the request signal when
/// dropped before the response finished, that is when the connection closed first.
struct Lifecycle {
    controller: Arc<AbortController>,
    finished: bool,
}

impl Lifecycle {
    fn new() -> Lifecycle {
        Lifecycle { controller: Arc::new(AbortController::new()), finished: false }
    }
}

impl Drop for Lifecycle {
    fn drop(&mut self) {
        if !self.finished {
            self.controller.abort();
        }
    }
}

/// How much of a request body the server reads ahead of the handler (Node's default
/// `highWaterMark` for a request stream). Reading ahead lets the server notice a client that
/// goes away mid-upload while the handler is not reading; past it the upload waits for the
/// handler, so an unread body is not buffered whole.
const REQUEST_BODY_HIGH_WATER_MARK: usize = 16 * 1024;

/// A request body that aborts the request signal when the upload fails or the client goes
/// away before it ended (Remix's `createRequestBodyStream`). A task reads the connection up
/// to [`REQUEST_BODY_HIGH_WATER_MARK`] ahead of the handler. Reads fail with [`AbortError`]
/// once the signal aborted, or once with the body's own error when it is not a disconnect.
struct RequestBody {
    queue: Arc<Mutex<BodyQueue>>,
    controller: Arc<AbortController>,
    remaining: Option<u64>,
}

#[derive(Default)]
struct BodyQueue {
    frames: VecDeque<http_body::Frame<rmx_fetch::Bytes>>,
    queued: usize,
    ended: bool,
    error: Option<rmx_fetch::BoxError>,
    reader: Option<Waker>,
    pump: Option<Waker>,
    reader_gone: bool,
}

impl BodyQueue {
    fn wake_reader(&mut self) {
        if let Some(waker) = self.reader.take() {
            waker.wake();
        }
    }
}

impl RequestBody {
    fn new<B>(body: B, controller: Arc<AbortController>) -> RequestBody
    where
        B: http_body::Body<Data = rmx_fetch::Bytes> + Send + Unpin + 'static,
        B::Error: Into<rmx_fetch::BoxError>,
    {
        let hint = body.size_hint();
        let remaining = hint.exact();
        let queue = Arc::new(Mutex::new(BodyQueue::default()));
        if body.is_end_stream() {
            queue.lock().expect("request body lock").ended = true;
        } else {
            tokio::spawn(read_ahead(body, queue.clone(), controller.clone()));
        }
        RequestBody { queue, controller, remaining }
    }
}

/// Read `body` into `queue` until it ends, fails or the reader is dropped, staying at most
/// [`REQUEST_BODY_HIGH_WATER_MARK`] bytes ahead of the reader.
async fn read_ahead<B>(mut body: B, queue: Arc<Mutex<BodyQueue>>, controller: Arc<AbortController>)
where
    B: http_body::Body<Data = rmx_fetch::Bytes> + Unpin,
    B::Error: Into<rmx_fetch::BoxError>,
{
    loop {
        let polled = poll_fn(|cx| {
            {
                let mut queue = queue.lock().expect("request body lock");
                if queue.reader_gone {
                    return Poll::Ready(None);
                }
                queue.pump = Some(cx.waker().clone());
                if queue.queued >= REQUEST_BODY_HIGH_WATER_MARK {
                    return Poll::Pending;
                }
            }
            Pin::new(&mut body).poll_frame(cx).map(Some)
        })
        .await;
        let Some(frame) = polled else { return };
        let error = match frame {
            Some(Ok(frame)) => {
                let mut queue = queue.lock().expect("request body lock");
                queue.queued += frame.data_ref().map_or(0, |data| data.len());
                queue.frames.push_back(frame);
                queue.wake_reader();
                continue;
            }
            Some(Err(error)) => error,
            None => {
                let mut queue = queue.lock().expect("request body lock");
                queue.ended = true;
                queue.wake_reader();
                return;
            }
        };
        let error: rmx_fetch::BoxError = error.into();
        let error = if error.downcast_ref::<hyper::Error>().is_some_and(is_disconnect) { AbortError.into() } else { error };
        controller.abort();
        let mut queue = queue.lock().expect("request body lock");
        queue.error = Some(error);
        queue.ended = true;
        queue.wake_reader();
        return;
    }
}

/// Whether a request body error means the client went away (closed, reset or half-closed the
/// connection before the body ended) rather than sent a malformed body.
fn is_disconnect(error: &hyper::Error) -> bool {
    if error.is_incomplete_message() || error.is_canceled() || error.is_closed() {
        return true;
    }
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            use std::io::ErrorKind::*;
            return matches!(io.kind(), UnexpectedEof | ConnectionReset | ConnectionAborted | BrokenPipe);
        }
        source = cause.source();
    }
    false
}

impl http_body::Body for RequestBody {
    type Data = rmx_fetch::Bytes;
    type Error = rmx_fetch::BoxError;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = &mut *self;
        let mut queue = this.queue.lock().expect("request body lock");
        if let Some(error) = queue.error.take() {
            return Poll::Ready(Some(Err(error)));
        }
        if this.controller.signal().aborted() {
            return Poll::Ready(Some(Err(AbortError.into())));
        }
        if let Some(frame) = queue.frames.pop_front() {
            let len = frame.data_ref().map_or(0, |data| data.len());
            queue.queued -= len;
            if let Some(waker) = queue.pump.take() {
                waker.wake();
            }
            drop(queue);
            this.remaining = this.remaining.map(|remaining| remaining.saturating_sub(len as u64));
            return Poll::Ready(Some(Ok(frame)));
        }
        if queue.ended {
            return Poll::Ready(None);
        }
        queue.reader = Some(cx.waker().clone());
        Poll::Pending
    }

    fn is_end_stream(&self) -> bool {
        let queue = self.queue.lock().expect("request body lock");
        queue.ended && queue.frames.is_empty() && queue.error.is_none()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.remaining.map_or_else(http_body::SizeHint::default, http_body::SizeHint::with_exact)
    }
}

impl Drop for RequestBody {
    fn drop(&mut self) {
        let mut queue = self.queue.lock().expect("request body lock");
        queue.reader_gone = true;
        if let Some(waker) = queue.pump.take() {
            waker.wake();
        }
    }
}

/// A response body that holds its request's [`Lifecycle`] until the last frame is written.
/// hyper drops it early when the client goes away, which aborts the request signal. A body
/// that fails after the response started closes the connection and is reported to
/// [`ServeOptions::on_error`] on a blocking task, so a slow handler does not hold the
/// connection open; aborts are not reported.
struct ServedBody {
    body: Body,
    lifecycle: Option<Lifecycle>,
    signal: rmx_fetch::AbortSignal,
    options: Arc<ServeOptions>,
}

impl ServedBody {
    fn new(body: Body, lifecycle: Lifecycle, options: Arc<ServeOptions>) -> ServedBody {
        let signal = lifecycle.controller.signal();
        let mut served = ServedBody { body, lifecycle: Some(lifecycle), signal, options };
        served.finish_if_ended();
        served
    }

    fn finish_if_ended(&mut self) {
        if self.body.is_end_stream() {
            if let Some(mut lifecycle) = self.lifecycle.take() {
                lifecycle.finished = true;
            }
        }
    }
}

impl http_body::Body for ServedBody {
    type Data = rmx_fetch::Bytes;
    type Error = rmx_fetch::BoxError;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = &mut *self;
        let polled = Pin::new(&mut this.body).poll_frame(cx);
        match &polled {
            Poll::Ready(None) => {
                if let Some(mut lifecycle) = this.lifecycle.take() {
                    lifecycle.finished = true;
                }
            }
            Poll::Ready(Some(Ok(_))) => this.finish_if_ended(),
            Poll::Ready(Some(Err(error))) if !this.signal.aborted() && !error.is::<AbortError>() => {
                let (options, error) = (this.options.clone(), ServeError::ResponseBody(error.to_string()));
                tokio::task::spawn_blocking(move || options.error_response(&error));
            }
            _ => {}
        }
        polled
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.body.size_hint()
    }
}

async fn handle(router: &Router, options: &ServeOptions, req: Request, remote: SocketAddr) -> Response {
    let req = match create_request(req, remote, options) {
        Ok(req) => req,
        Err(error) => return options.error_response(&error),
    };
    let signal = req.signal();
    match CatchUnwind(Box::pin(router.fetch(req))).await {
        Ok(res) => res,
        // A handler that fails because the client went away is not a server error, and no
        // one is left to read the response (Remix's `isRequestAbortError`).
        Err(_) if signal.aborted() => {
            let mut res = Response::new(Body::empty());
            *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            res
        }
        Err(payload) => options.error_response(&ServeError::Panic(panic_message(&*payload))),
    }
}

/// Polls a future and turns a panic into an error, so one failing handler answers 500
/// instead of dropping the connection.
struct CatchUnwind<'a>(Pin<Box<dyn Future<Output = Response> + Send + 'a>>);

impl Future for CatchUnwind<'_> {
    type Output = Result<Response, Box<dyn Any + Send>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
        match std::panic::catch_unwind(AssertUnwindSafe(|| self.0.as_mut().poll(cx))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(res)) => Poll::Ready(Ok(res)),
            Err(payload) => Poll::Ready(Err(payload)),
        }
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}

/// Prepare an incoming request for the router (`createRequest`): give it an absolute URL
/// and attach [`RemoteAddr`] and [`ClientAddress`]. Fails for HTTP/2 requests whose `Host`
/// and `:authority` disagree and for URLs that do not parse.
pub fn create_request(req: Request, remote: SocketAddr, options: &ServeOptions) -> Result<Request, ServeError> {
    let (mut parts, body) = req.into_parts();
    let authority = request_authority(&parts)?;
    let protocol = options
        .protocol
        .as_deref()
        .map(|p| p.trim_end_matches(':').to_ascii_lowercase())
        .or_else(|| options.trust_proxy.then(|| forwarded_protocol(&parts.headers)).flatten())
        .unwrap_or_else(|| "http".into());
    let host = options
        .host
        .clone()
        .or_else(|| options.trust_proxy.then(|| forwarded_host(&parts.headers)).flatten())
        .or(authority)
        .or_else(|| parts.headers.get(header::HOST).and_then(|h| h.to_str().ok()).map(str::to_string))
        .unwrap_or_else(|| "localhost".into());
    let url = format!("{protocol}://{host}{}", parts.uri.path_and_query().map_or("/", |p| p.as_str()));
    parts.uri = url.parse().map_err(|_| ServeError::InvalidUrl(url.clone()))?;

    let forwarded = if options.trust_proxy { forwarded_client(&parts.headers) } else { None };
    let client = ClientAddress {
        address: forwarded.map_or(remote.ip(), |(address, _)| address),
        port: forwarded.and_then(|(_, port)| port).unwrap_or(remote.port()),
    };
    parts.extensions.insert(RemoteAddr(remote));
    parts.extensions.insert(client);
    Ok(Request::from_parts(parts, body))
}

/// The HTTP/2 `:authority`, checked against the `Host` header.
fn request_authority(parts: &http::request::Parts) -> Result<Option<String>, ServeError> {
    if parts.version != Version::HTTP_2 {
        return Ok(None);
    }
    let Some(authority) = parts.uri.authority().map(|a| a.as_str().to_string()) else { return Ok(None) };
    if let Some(host) = parts.headers.get(header::HOST) {
        let scheme = parts.uri.scheme_str().unwrap_or("http");
        let normalize = |value: &str| Url::parse(&format!("{scheme}://{value}/")).ok().map(|u| u.host());
        let host = host.to_str().ok().and_then(normalize);
        if host.is_none() || host != normalize(&authority) {
            return Err(ServeError::ConflictingAuthority);
        }
    }
    Ok(Some(authority))
}

fn header_str<'a>(headers: &'a http::HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn forwarded_protocol(headers: &http::HeaderMap) -> Option<String> {
    let normalize = |value: String| {
        let protocol = value.trim().to_ascii_lowercase();
        let protocol = protocol.trim_end_matches(':');
        matches!(protocol, "http" | "https").then(|| protocol.to_string())
    };
    forwarded_parameter(header_str(headers, "forwarded"), "proto")
        .and_then(normalize)
        .or_else(|| first_value(header_str(headers, "x-forwarded-proto")).and_then(normalize))
}

fn forwarded_host(headers: &http::HeaderMap) -> Option<String> {
    let normalize = |value: String| Some(value.trim().to_string()).filter(|h| !h.is_empty());
    forwarded_parameter(header_str(headers, "forwarded"), "host")
        .and_then(normalize)
        .or_else(|| first_value(header_str(headers, "x-forwarded-host")).and_then(normalize))
}

fn forwarded_client(headers: &http::HeaderMap) -> Option<(IpAddr, Option<u16>)> {
    forwarded_parameter(header_str(headers, "forwarded"), "for")
        .and_then(|v| client_address(&v))
        .or_else(|| first_value(header_str(headers, "x-forwarded-for")).and_then(|v| client_address(&v)))
}

/// An IP with an optional port from a `Forwarded: for=` or `X-Forwarded-For` value
/// (`192.0.2.1`, `192.0.2.1:4711`, `[2001:db8::1]:4711`). Obfuscated and `unknown` values
/// are ignored.
fn client_address(value: &str) -> Option<(IpAddr, Option<u16>)> {
    let input = value.trim();
    if input.is_empty() || input.eq_ignore_ascii_case("unknown") || input.starts_with('_') {
        return None;
    }
    let (address, port) = if let Some(rest) = input.strip_prefix('[') {
        let (address, after) = rest.split_once(']')?;
        (address, parse_port(after))
    } else {
        match input.rsplit_once(':') {
            // Exactly one colon: IPv4 with a port. More colons: a bare IPv6 address.
            Some((address, port)) if !address.contains(':') => match parse_port(&format!(":{port}")) {
                Some(port) => (address, Some(port)),
                None => (input, None),
            },
            _ => (input, None),
        }
    };
    address.parse::<IpAddr>().ok().map(|ip| (ip, port))
}

fn parse_port(value: &str) -> Option<u16> {
    value.strip_prefix(':')?.parse::<u16>().ok().filter(|p| *p > 0)
}

/// The first value of a comma-separated header, unquoted.
fn first_value(value: Option<&str>) -> Option<String> {
    split_header(value?, ',').first().map(|v| unquote(v.trim()))
}

/// The named parameter of the first `Forwarded` element that has it.
fn forwarded_parameter(value: Option<&str>, name: &str) -> Option<String> {
    for element in split_header(value?, ',') {
        for parameter in split_header(element, ';') {
            let Some((key, value)) = parameter.split_once('=') else { continue };
            if key.trim().eq_ignore_ascii_case(name) {
                return Some(unquote(value.trim()));
            }
        }
    }
    None
}

/// Split on `delimiter` outside quoted strings.
fn split_header(value: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut quoted, mut escaped) = (0, false, false);
    for (i, c) in value.char_indices() {
        if escaped {
            escaped = false;
        } else if quoted && c == '\\' {
            escaped = true;
        } else if c == '"' {
            quoted = !quoted;
        } else if !quoted && c == delimiter {
            parts.push(&value[start..i]);
            start = i + 1;
        }
    }
    parts.push(&value[start..]);
    parts
}

fn unquote(value: &str) -> String {
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else { return value.to_string() };
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            c => out.push(c),
        }
    }
    out
}

/// Send `request` (with an absolute `http://` URI) over a new HTTP/1.1 connection and return the
/// response as it streams in: the Fetch API's `fetch` for servers, enough for
/// `rmx-fetch-proxy` and calls to local services. Redirects are returned, not followed, and
/// bodies are not decoded. There is no TLS; use an HTTP client library for `https://` URLs.
pub async fn fetch(request: rmx_fetch::Request) -> Result<rmx_fetch::Response, rmx_fetch::BoxError> {
    let (mut parts, body) = request.into_parts();
    if parts.uri.scheme_str() != Some("http") {
        return Err(format!("rmx_serve::fetch only supports http:// URLs, got {}", parts.uri).into());
    }
    let authority = parts.uri.authority().cloned().ok_or("request URI has no host")?;
    let host = authority.host().trim_start_matches('[').trim_end_matches(']');
    let stream = tokio::net::TcpStream::connect((host, authority.port_u16().unwrap_or(80))).await?;
    let _ = stream.set_nodelay(true);
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    tokio::spawn(connection);
    parts.uri = parts.uri.path_and_query().map_or("/", |p| p.as_str()).parse()?;
    if !parts.headers.contains_key(http::header::HOST) {
        parts.headers.insert(http::header::HOST, authority.as_str().parse()?);
    }
    let response = sender.send_request(http::Request::from_parts(parts, body)).await?;
    Ok(response.map(Body::wrap))
}

/// Bind `addr` and serve. Prints the address once listening.
pub async fn listen(addr: &str, router: Router) -> std::io::Result<()> {
    listen_with(addr, router, ServeOptions::default()).await
}

/// [`listen`] with options.
pub async fn listen_with(addr: &str, router: Router, options: ServeOptions) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    println!("listening on http://{}", listener.local_addr()?);
    serve_with(listener, router, options).await
}

#[cfg(test)]
mod tests;
