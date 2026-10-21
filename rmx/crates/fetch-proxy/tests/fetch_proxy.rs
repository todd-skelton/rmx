//! Port of Remix's `fetch-proxy.test.ts`, one test per case.
//!
//! Remix's proxy is a `fetch(input, init)` function; rmx's takes an `http::Request`, so the
//! "double-arg style" cases build the request from a URL and parts. Fetch's `cache`,
//! `credentials`, `integrity`, `keepalive`, `mode` and `referrer` have no server-side
//! counterpart on an `http::Request`, so the cases that forward them check the method and the
//! redirect mode, which do. A [Redirect] in a request's extensions plays the part of Remix's
//! per-request and per-call `redirect`.
//!
//! Remix's default `fetch` decodes compressed bodies, and the proxy strips `Content-Encoding`
//! and `Content-Length` to match. rmx's `rmx_serve::fetch` does not decode, so that behavior is
//! opt-in (`fetch_decodes_content`); the cases written for a decoding fetch turn it on.
//!
//! Remix reads responses back through a real server and undici to prove the headers frame the
//! body correctly; here `round_trip` serves them with `rmx_serve` and reads them with
//! `rmx_serve::fetch` (hyper on both ends).

use std::sync::{Arc, Mutex};

use http::{HeaderMap, Method, StatusCode};
use rmx_fetch::{block_on, Body, BoxError, Request, Response};
use rmx_fetch_proxy::{FetchProxy, FetchProxyOptions, Redirect};
use rmx_middleware::compression::{compress_response, CompressOptions, Encoding};
use rmx_router::{Context, Router};
use tokio::net::TcpListener;

/// What the upstream fetch received.
#[derive(Clone, Debug, Default)]
struct Captured {
    method: String,
    url: String,
    headers: HeaderMap,
    body: String,
    body_is_empty: bool,
    redirect: Option<Redirect>,
}

impl Captured {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|v| v.to_str().unwrap())
    }

    fn header_pairs(&self) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> = self.headers.iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap().to_string())).collect();
        pairs.sort();
        pairs
    }
}

type Respond = Arc<dyn Fn(&Captured) -> Response + Send + Sync>;

/// Remix's `testProxy`: proxy `request` through a fetch that records each request it receives.
fn test_proxy(request: Request, target: &str, options: FetchProxyOptions, respond: impl Fn(&Captured) -> Response + Send + Sync + 'static) -> (Vec<Captured>, Result<Response, BoxError>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    let respond: Respond = Arc::new(respond);
    let upstream = move |req: Request| {
        let (record, respond) = (record.clone(), respond.clone());
        async move {
            let (parts, body) = req.into_parts();
            let body_is_empty = http_body::Body::is_end_stream(&body);
            let captured = Captured {
                method: parts.method.to_string(),
                url: parts.uri.to_string(),
                redirect: parts.extensions.get::<Redirect>().copied(),
                headers: parts.headers,
                body: body.text().await?,
                body_is_empty,
            };
            let response = respond(&captured);
            record.lock().unwrap().push(captured);
            Ok(response)
        }
    };
    let proxy = FetchProxy::new(target, upstream, options).unwrap();
    let response = block_on(proxy.fetch(request));
    let seen = seen.lock().unwrap().clone();
    (seen, response)
}

/// The one request the upstream saw, with the default options and an empty response.
fn captured(request: Request, target: &str) -> Captured {
    captured_with(request, target, FetchProxyOptions::default())
}

fn captured_with(request: Request, target: &str, options: FetchProxyOptions) -> Captured {
    let (mut seen, response) = test_proxy(request, target, options, |_| empty());
    response.unwrap();
    assert_eq!(seen.len(), 1);
    seen.remove(0)
}

/// The response the client gets when the upstream answers with `respond()`.
fn proxied(request: Request, target: &str, options: FetchProxyOptions, respond: impl Fn() -> Response + Send + Sync + 'static) -> Response {
    test_proxy(request, target, options, move |_| respond()).1.unwrap()
}

fn get(url: &str) -> Request {
    request(Method::GET, url, &[], Body::empty())
}

fn request(method: Method, url: &str, headers: &[(&str, &str)], body: Body) -> Request {
    let mut builder = http::Request::builder().method(method).uri(url);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(body).unwrap()
}

fn with_redirect(mut request: Request, mode: Redirect) -> Request {
    request.extensions_mut().insert(mode);
    request
}

fn empty() -> Response {
    http::Response::new(Body::empty())
}

fn response(status: u16, headers: &[(&str, &str)], body: Body) -> Response {
    let mut builder = http::Response::builder().status(status);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(body).unwrap()
}

fn header<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
    response.headers().get(name).map(|v| v.to_str().unwrap())
}

fn set_cookies(response: &Response) -> Vec<String> {
    response.headers().get_all("set-cookie").iter().map(|v| v.to_str().unwrap().to_string()).collect()
}

fn cookie_response(cookies: &'static [&'static str]) -> impl Fn() -> Response + Send + Sync + 'static {
    move || {
        let mut response = empty();
        for cookie in cookies {
            response.headers_mut().append("set-cookie", cookie.parse().unwrap());
        }
        response
    }
}

fn decoding() -> FetchProxyOptions {
    FetchProxyOptions { fetch_decodes_content: true, ..Default::default() }
}

fn x_forwarded() -> FetchProxyOptions {
    FetchProxyOptions { x_forwarded_headers: true, ..Default::default() }
}

/// Bind a router on a loopback port and serve it in the background.
async fn listen(router: Router) -> (std::net::SocketAddr, tokio::task::JoinHandle<std::io::Result<()>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    (addr, tokio::spawn(rmx_serve::serve(listener, router)))
}

/// Remix's `readResponseWithUndici`: serve `response` over HTTP and read it back as a client.
async fn round_trip(response: Response, method: Method) -> Result<(StatusCode, HeaderMap, Vec<u8>), BoxError> {
    let slot = Arc::new(Mutex::new(Some(response)));
    let mut router = Router::new();
    router.any("/", move |_: Context| {
        let slot = slot.clone();
        async move { slot.lock().unwrap().take().unwrap() }
    });
    let (addr, server) = listen(router).await;
    let result = async {
        let response = rmx_serve::fetch(request(method, &format!("http://{addr}/"), &[], Body::empty())).await?;
        let (parts, body) = response.into_parts();
        Ok::<_, BoxError>((parts.status, parts.headers, body.bytes().await?.to_vec()))
    }
    .await;
    server.abort();
    result
}

/// Remix's `readBodyWithUndici`.
async fn read_body(response: Response) -> Result<String, BoxError> {
    let body = round_trip(response, Method::GET).await?.2;
    Ok(String::from_utf8(body)?)
}

/// A gzip-encoded copy of `text`, made the way Remix's test makes it (`compressResponse`).
async fn gzip(text: &'static str) -> (String, Vec<u8>) {
    let request = request(Method::GET, "https://remix.run", &[("accept-encoding", "gzip")], Body::empty());
    let options = CompressOptions { encodings: vec![Encoding::Gzip], threshold: 0, ..Default::default() };
    let compressed = compress_response(http::Response::new(Body::from(text)), &request, &options);
    let encoding = header(&compressed, "content-encoding").unwrap().to_string();
    (encoding, compressed.into_body().bytes().await.unwrap().to_vec())
}

// fetch proxy

#[test]
fn appends_the_request_url_pathname_and_search_to_the_target_url() {
    let cases = [
        ("http://shopify.com", "https://remix.run:3000/dest", "https://remix.run:3000/dest"),
        ("http://shopify.com/?q=remix", "https://remix.run:3000/dest", "https://remix.run:3000/dest?q=remix"),
        ("http://shopify.com/search?q=remix", "https://remix.run:3000/", "https://remix.run:3000/search?q=remix"),
        ("http://shopify.com/search?q=remix", "https://remix.run:3000/dest", "https://remix.run:3000/dest/search?q=remix"),
    ];
    for (incoming, target, expected) in cases {
        assert_eq!(captured(get(incoming), target).url, expected, "{incoming} -> {target}");
    }
}

#[test]
fn forwards_request_method_headers_and_body() {
    let request = request(Method::POST, "http://shopify.com/search?q=remix", &[("content-type", "text/plain")], Body::from("hello"));
    let seen = captured(request, "https://remix.run:3000/dest");
    assert_eq!(seen.method, "POST");
    assert_eq!(seen.header("content-type"), Some("text/plain"));
    assert_eq!(seen.body, "hello");
}

#[test]
fn forwards_an_empty_request_body() {
    let seen = captured(request(Method::POST, "http://shopify.com/search?q=remix", &[], Body::empty()), "https://remix.run:3000/dest");
    assert_eq!(seen.method, "POST");
    assert_eq!(seen.header("content-type"), None);
    assert_eq!(seen.body, "");
}

#[test]
fn forwards_various_http_methods_correctly() {
    for method in ["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "HEAD"] {
        let seen = captured(request(method.parse().unwrap(), "http://shopify.com/api/resource", &[], Body::empty()), "https://remix.run:3000/backend");
        assert_eq!(seen.method, method, "Method {method} should be forwarded correctly");
    }
}

#[test]
fn does_not_append_x_forwarded_headers_by_default() {
    let seen = captured(get("http://shopify.com:8080/search?q=remix"), "https://remix.run:3000/dest");
    assert_eq!(seen.header("x-forwarded-proto"), None);
    assert_eq!(seen.header("x-forwarded-host"), None);
    assert_eq!(seen.header("x-forwarded-port"), None);
}

#[test]
fn does_not_forward_the_incoming_host_header() {
    let seen = captured(request(Method::GET, "http://shopify.com/search?q=remix", &[("host", "shopify.com")], Body::empty()), "https://remix.run:3000/dest");
    assert_eq!(seen.header("host"), None);
}

#[test]
fn does_not_forward_the_incoming_accept_encoding_header() {
    let seen = captured(request(Method::GET, "http://shopify.com/search?q=remix", &[("accept-encoding", "gzip, br")], Body::empty()), "https://remix.run:3000/dest");
    assert_eq!(seen.header("accept-encoding"), None);
}

#[test]
fn sets_x_forwarded_headers_when_desired() {
    let seen = captured_with(get("http://shopify.com:8080/search?q=remix"), "https://remix.run:3000/dest", x_forwarded());
    assert_eq!(seen.header("x-forwarded-proto"), Some("http"));
    assert_eq!(seen.header("x-forwarded-host"), Some("shopify.com:8080"));
    assert_eq!(seen.header("x-forwarded-port"), Some("8080"));
}

#[test]
fn does_not_forward_connection_specific_request_headers() {
    let headers = [
        ("connection", "keep-alive"),
        ("keep-alive", "timeout=5"),
        ("proxy-connection", "keep-alive"),
        ("proxy-authenticate", "Basic"),
        ("proxy-authorization", "Basic example"),
        ("te", "trailers"),
        ("trailer", "X-Checksum"),
        ("transfer-encoding", "chunked"),
        ("upgrade", "websocket"),
        ("authorization", "Bearer example"),
        ("cookie", "session=example"),
        ("accept", "application/json"),
    ];
    let seen = captured(request(Method::GET, "http://shopify.com/", &headers, Body::empty()), "https://remix.run/");
    let expected = [("accept", "application/json"), ("authorization", "Bearer example"), ("cookie", "session=example")];
    assert_eq!(seen.header_pairs(), expected.map(|(k, v)| (k.to_string(), v.to_string())));
}

#[test]
fn removes_request_headers_nominated_by_connection_case_insensitively() {
    // Remix also checks that the caller's request is unchanged; here the proxy takes the
    // request by value, so the caller has nothing left to observe.
    let headers = [
        ("connection", "keep-alive, X-First-Hop, ,"),
        ("x-first-hop", "first"),
        ("x-second-hop", "second"),
        ("x-end-to-end", "preserved"),
        ("connection", "x-second-hop, CONNECTION"),
    ];
    let seen = captured(request(Method::GET, "http://shopify.com/", &headers, Body::empty()), "https://remix.run/");
    assert_eq!(seen.header_pairs(), [("x-end-to-end".to_string(), "preserved".to_string())]);
}

#[test]
fn leaves_streamed_request_framing_to_fetch() {
    let request = request(Method::POST, "http://shopify.com/", &[("content-type", "text/plain"), ("content-length", "999")], Body::from("hello"));
    let seen = captured(request, "https://remix.run/");
    assert_eq!(seen.header("content-length"), None);
    assert_eq!(seen.header("content-type"), Some("text/plain"));
    assert_eq!(seen.body, "hello");
}

#[test]
fn removes_caller_supplied_framing_headers_from_bodyless_requests() {
    let seen = captured(request(Method::GET, "http://shopify.com/", &[("content-length", "42")], Body::from("ignored")), "https://remix.run/");
    assert_eq!(seen.header("content-length"), None);
    assert!(seen.body_is_empty);
}

#[test]
fn sets_forwarding_metadata_after_removing_connection_nominated_headers() {
    let headers = [("connection", "X-First-Hop, X-Forwarded-Host, X-Forwarded-Proto, X-Forwarded-Port"), ("x-first-hop", "local-only"), ("x-forwarded-host", "example.com")];
    let (seen, response) = test_proxy(request(Method::GET, "http://shopify.com:8080/", &headers, Body::empty()), "https://remix.run/", x_forwarded(), |_| http::Response::new(Body::from("ok")));
    let seen = &seen[0];
    assert_eq!(seen.header("connection"), None);
    assert_eq!(seen.header("x-first-hop"), None);
    assert_eq!(seen.header("x-forwarded-host"), Some("shopify.com:8080"));
    assert_eq!(seen.header("x-forwarded-proto"), Some("http"));
    assert_eq!(seen.header("x-forwarded-port"), Some("8080"));
    assert_eq!(block_on(response.unwrap().into_body().text()).unwrap(), "ok");
}

#[tokio::test]
async fn forwards_a_streamed_body_through_default_fetch_with_fresh_framing() {
    let received = Arc::new(Mutex::new(None::<HeaderMap>));
    let record = received.clone();
    let mut router = Router::new();
    router.any("/", move |mut ctx: Context| {
        let record = record.clone();
        async move {
            *record.lock().unwrap() = Some(ctx.request().headers().clone());
            ctx.take_body().text().await.unwrap()
        }
    });
    let (addr, server) = listen(router).await;

    let proxy = FetchProxy::new(&format!("http://{addr}"), rmx_serve::fetch, FetchProxyOptions::default()).unwrap();
    let headers = [
        ("connection", "keep-alive, X-First-Hop"),
        ("x-first-hop", "local-only"),
        ("proxy-authorization", "Basic example"),
        ("content-length", "999"),
        ("transfer-encoding", "chunked"),
        ("upgrade", "websocket"),
        ("content-type", "text/plain"),
        ("authorization", "Bearer example"),
    ];
    let response = proxy.fetch(request(Method::POST, "http://shopify.com/", &headers, Body::from("hello"))).await.unwrap();
    assert_eq!(response.into_body().text().await.unwrap(), "hello");
    let received = received.lock().unwrap().clone().unwrap();
    let get = |name: &str| received.get(name).map(|v| v.to_str().unwrap().to_string());
    assert_eq!(get("x-first-hop"), None);
    assert_eq!(get("proxy-authorization"), None);
    assert_eq!(get("upgrade"), None);
    assert_ne!(get("content-length").as_deref(), Some("999"));
    assert_eq!(get("content-type").as_deref(), Some("text/plain"));
    assert_eq!(get("authorization").as_deref(), Some("Bearer example"));
    server.abort();
}

#[test]
fn replaces_incoming_forwarding_metadata_when_x_forwarded_headers_are_enabled() {
    let headers = [
        ("forwarded", "for=203.0.113.43; proto=https; host=example.com"),
        ("x-forwarded-for", "203.0.113.43"),
        ("x-forwarded-host", "example.com"),
        ("x-forwarded-port", "443"),
        ("x-forwarded-proto", "https"),
    ];
    let seen = captured_with(request(Method::GET, "http://shopify.com:8080/search?q=remix", &headers, Body::empty()), "https://remix.run:3000/dest", x_forwarded());
    assert_eq!(seen.header("forwarded"), None);
    assert_eq!(seen.header("x-forwarded-for"), None);
    assert_eq!(seen.header("x-forwarded-proto"), Some("http"));
    assert_eq!(seen.header("x-forwarded-host"), Some("shopify.com:8080"));
    assert_eq!(seen.header("x-forwarded-port"), Some("8080"));
}

#[test]
fn sets_default_x_forwarded_port_values_when_request_urls_omit_ports() {
    let http = captured_with(get("http://shopify.com/search?q=remix"), "https://remix.run:3000/dest", x_forwarded());
    let https = captured_with(get("https://shopify.com/search?q=remix"), "https://remix.run:3000/dest", x_forwarded());
    assert_eq!(http.header("x-forwarded-port"), Some("80"));
    assert_eq!(https.header("x-forwarded-port"), Some("443"));
}

#[test]
fn preserves_forwarding_metadata_when_x_forwarded_headers_are_disabled() {
    let headers = [
        ("forwarded", "for=203.0.113.43; proto=https; host=example.com"),
        ("x-forwarded-for", "203.0.113.43, 198.51.100.7"),
        ("x-forwarded-host", "example.com"),
        ("x-forwarded-port", "443"),
        ("x-forwarded-proto", "https"),
    ];
    let seen = captured(request(Method::GET, "http://shopify.com:8080/search?q=remix", &headers, Body::empty()), "https://remix.run:3000/dest");
    let mut expected: Vec<(String, String)> = headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    expected.sort();
    assert_eq!(seen.header_pairs(), expected);
}

#[test]
fn forwards_additional_request_init_options() {
    let request = with_redirect(request(Method::DELETE, "http://shopify.com/search?q=remix", &[], Body::empty()), Redirect::Manual);
    let (seen, response) = test_proxy(request, "https://remix.run:3000/dest", FetchProxyOptions::default(), |_| response(302, &[("location", "/elsewhere")], Body::empty()));
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "DELETE");
    assert_eq!(seen[0].redirect, Some(Redirect::Manual));
    assert_eq!(response.unwrap().status(), 302, "manual mode returns the redirect");
}

#[tokio::test]
async fn returns_redirects_to_the_client_by_default_and_follows_them_when_configured() {
    let mut router = Router::new();
    router.get("/login", |_: Context| async { response(303, &[("location", "/account"), ("set-cookie", "session=abc; Path=/; HttpOnly")], Body::empty()) });
    router.get("/account", |_: Context| async { "account page" });
    let (addr, server) = listen(router).await;
    let target = format!("http://{addr}");

    let proxy = FetchProxy::new(&target, rmx_serve::fetch, FetchProxyOptions::default()).unwrap();
    let response = proxy.fetch(get("http://shopify.com/login")).await.unwrap();
    assert_eq!(response.status(), 303);
    assert_eq!(header(&response, "location"), Some("/account"));
    assert_eq!(set_cookies(&response), ["session=abc; HttpOnly; Path=/"]);
    response.into_body().text().await.unwrap();

    let following = FetchProxy::new(&target, rmx_serve::fetch, FetchProxyOptions { redirect: Redirect::Follow, ..Default::default() }).unwrap();
    let followed = following.fetch(get("http://shopify.com/login")).await.unwrap();
    assert_eq!(followed.status(), 200);
    assert_eq!(followed.into_body().text().await.unwrap(), "account page");
    server.abort();
}

#[tokio::test]
async fn preserves_browser_origins_and_auth_redirects_through_a_trusted_development_proxy() {
    let mut app = Router::new();
    app.any("/*path", |ctx: Context| async move {
        if ctx.url().pathname() == "/account" {
            return http::Response::new(Body::from("account page"));
        }
        if ctx.header("origin") != Some(ctx.url().origin().as_str()) {
            return response(403, &[], Body::from("Forbidden"));
        }
        response(303, &[("location", "/account"), ("set-cookie", "session=abc; Path=/; HttpOnly")], Body::empty())
    });
    let app_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let app_addr = app_listener.local_addr().unwrap();
    let app_server = tokio::spawn(rmx_serve::serve_with(app_listener, app, rmx_serve::ServeOptions::default().trust_proxy(true)));

    let proxy = FetchProxy::new(&format!("http://{app_addr}"), rmx_serve::fetch, x_forwarded()).unwrap();
    let mut front = Router::new();
    front.any("/*path", move |ctx: Context| {
        let proxy = proxy.clone();
        async move { proxy.fetch(ctx.into_request()).await.unwrap() }
    });
    let (front_addr, front_server) = listen(front).await;

    let public_origin = format!("http://{front_addr}");
    let response = rmx_serve::fetch(request(Method::POST, &format!("{public_origin}/signup"), &[("origin", &public_origin)], Body::empty())).await.unwrap();
    assert_eq!(response.status(), 303);
    assert_eq!(header(&response, "location"), Some("/account"));
    assert_eq!(set_cookies(&response), ["session=abc; HttpOnly; Path=/"]);
    response.into_body().text().await.unwrap();
    front_server.abort();
    app_server.abort();
}

#[test]
fn rewrites_cookie_domain_and_path() {
    let respond = cookie_response(&["name=value; Domain=remix.run:3000; Path=/dest/search", "name2=value2; Domain=remix.run:3000; Path=/dest"]);
    let response = proxied(get("http://shopify.com/search?q=remix"), "https://remix.run:3000/dest", FetchProxyOptions::default(), respond);
    assert_eq!(set_cookies(&response), ["name=value; Domain=shopify.com; Path=/search", "name2=value2; Domain=shopify.com; Path=/"]);
}

#[tokio::test]
async fn strips_gzip_response_headers_when_fetch_returns_a_decoded_body() {
    const DECODED: &str = "decoded response body";
    let (encoding, compressed) = gzip(DECODED).await;
    assert_eq!(encoding, "gzip");
    assert_ne!(compressed.len(), DECODED.len());
    // The gzip body itself is well framed (hyper's client hands it back undecoded).
    let gzipped = response(200, &[("content-encoding", "gzip")], Body::from(compressed.clone()));
    assert_eq!(round_trip(gzipped, Method::GET).await.unwrap().2, compressed);

    let length = compressed.len().to_string();
    let stale = move || response(200, &[("content-encoding", "gzip"), ("content-length", &length), ("content-type", "text/plain")], Body::from(DECODED));
    assert!(read_body(stale()).await.is_err(), "a decoded body under the encoded framing cannot be read");

    let response = tokio::task::spawn_blocking(move || proxied(get("http://shopify.com/search?q=remix"), "https://remix.run:3000/dest", decoding(), stale)).await.unwrap();
    assert_eq!(header(&response, "content-encoding"), None);
    assert_eq!(header(&response, "content-length"), None);
    assert_eq!(header(&response, "transfer-encoding"), None);
    assert_eq!(header(&response, "content-type"), Some("text/plain"));
    assert_eq!(read_body(response).await.unwrap(), DECODED);
}

#[tokio::test]
async fn preserves_content_length_for_unencoded_responses() {
    const BODY: &str = "unencoded response body";
    let length = BODY.len().to_string();
    let expected = length.clone();
    let response = tokio::task::spawn_blocking(move || {
        proxied(get("http://shopify.com/search?q=remix"), "https://remix.run:3000/dest", decoding(), move || response(200, &[("content-length", &length), ("content-type", "text/plain")], Body::from(BODY)))
    })
    .await
    .unwrap();
    assert_eq!(header(&response, "content-encoding"), None);
    assert_eq!(header(&response, "content-length"), Some(expected.as_str()));
    assert_eq!(header(&response, "transfer-encoding"), None);
    assert_eq!(header(&response, "content-type"), Some("text/plain"));
    assert_eq!(read_body(response).await.unwrap(), BODY);
}

/// The HEAD and 304 cases: encoding metadata describes a body that is not sent, so it stays.
async fn assert_metadata_kept(method: Method, status: u16, extra: (&'static str, &'static str)) {
    let (encoding, compressed) = gzip("bodyless response body").await;
    assert_eq!(encoding, "gzip");
    let length = compressed.len().to_string();
    let (expected_length, sent_length) = (length.clone(), length.clone());
    let request = request(method.clone(), "http://shopify.com/search?q=remix", &[], Body::empty());
    let response = tokio::task::spawn_blocking(move || {
        proxied(request, "https://remix.run:3000/dest", decoding(), move || response(status, &[("content-encoding", "gzip"), ("content-length", &sent_length), extra], Body::empty()))
    })
    .await
    .unwrap();
    assert_eq!(header(&response, "content-encoding"), Some("gzip"));
    assert_eq!(header(&response, "content-length"), Some(expected_length.as_str()));
    assert_eq!(header(&response, "transfer-encoding"), None);
    let (client_status, client_headers, body) = round_trip(response, method).await.unwrap();
    assert_eq!(client_status.as_u16(), status);
    assert_eq!(client_headers.get("content-encoding").unwrap(), "gzip");
    if status != 304 {
        // hyper's server omits Content-Length on a 304, which RFC 9110 permits.
        assert_eq!(client_headers.get("content-length").unwrap().to_str().unwrap(), expected_length);
    }
    assert!(body.is_empty());
}

#[tokio::test]
async fn preserves_content_encoding_metadata_for_head_responses() {
    assert_metadata_kept(Method::HEAD, 200, ("content-type", "text/plain")).await;
}

#[tokio::test]
async fn preserves_content_encoding_metadata_for_304_responses() {
    assert_metadata_kept(Method::GET, 304, ("etag", "\"compressed-response\"")).await;
}

#[tokio::test]
async fn strips_ambiguous_content_encoding_headers_for_unsupported_content_encodings() {
    const BODY: &str = "unsupported encoded response body";
    let length = BODY.len().to_string();
    let passthrough = response(200, &[("content-encoding", "foobar"), ("content-length", &length)], Body::from(BODY));
    assert_eq!(read_body(passthrough).await.unwrap(), BODY);

    let response = tokio::task::spawn_blocking(move || {
        proxied(get("http://shopify.com/search?q=remix"), "https://remix.run:3000/dest", decoding(), move || {
            response(200, &[("content-encoding", "foobar"), ("content-length", &length), ("content-type", "text/plain")], Body::from(BODY))
        })
    })
    .await
    .unwrap();
    assert_eq!(header(&response, "content-encoding"), None);
    assert_eq!(header(&response, "content-length"), None);
    assert_eq!(header(&response, "transfer-encoding"), None);
    assert_eq!(header(&response, "content-type"), Some("text/plain"));
    assert_eq!(read_body(response).await.unwrap(), BODY);
}

#[tokio::test]
async fn strips_transfer_framing_headers_from_unencoded_responses() {
    const BODY: &str = "chunked response body";
    let stale = || response(200, &[("content-length", "999"), ("content-type", "text/plain"), ("transfer-encoding", "chunked")], Body::from(BODY));
    assert!(read_body(stale()).await.is_err(), "conflicting framing cannot be read");

    let response = tokio::task::spawn_blocking(move || proxied(get("http://shopify.com/search?q=remix"), "https://remix.run:3000/dest", decoding(), stale)).await.unwrap();
    assert_eq!(header(&response, "content-encoding"), None);
    assert_eq!(header(&response, "content-length"), None);
    assert_eq!(header(&response, "transfer-encoding"), None);
    assert_eq!(header(&response, "content-type"), Some("text/plain"));
    assert_eq!(read_body(response).await.unwrap(), BODY);
}

#[test]
fn omits_cookie_domain_for_localhost_requests_with_a_port_and_rewrites_path() {
    let response = proxied(get("http://localhost:5173/search?q=remix"), "https://remix.run:3000/dest", FetchProxyOptions::default(), cookie_response(&["name=value; Domain=remix.run; Path=/dest/search"]));
    assert_eq!(set_cookies(&response), ["name=value; Path=/search"]);
}

#[test]
fn rewrites_cookie_domain_to_hostname_for_dns_requests_with_a_port_and_rewrites_path() {
    let response = proxied(get("https://preview.example.com:8443/search?q=remix"), "https://remix.run:3000/dest", FetchProxyOptions::default(), cookie_response(&["name=value; Domain=remix.run; Path=/dest/search"]));
    assert_eq!(set_cookies(&response), ["name=value; Domain=preview.example.com; Path=/search"]);
}

#[test]
fn omits_cookie_domains_for_ipv4_request_hosts_with_ports() {
    let respond = cookie_response(&["name=value; Domain=remix.run; Path=/dest/search", "name2=value2; Domain=remix.run:3000; Path=/dest"]);
    let response = proxied(get("http://127.0.0.1:5173/search?q=remix"), "https://remix.run:3000/dest", FetchProxyOptions::default(), respond);
    assert_eq!(set_cookies(&response), ["name=value; Path=/search", "name2=value2; Path=/"]);
}

#[test]
fn omits_cookie_domains_for_ipv6_request_hosts_with_ports() {
    let response = proxied(get("http://[2001:db8::1]:5173/search?q=remix"), "https://remix.run:3000/dest", FetchProxyOptions::default(), cookie_response(&["name=value; Domain=remix.run; Path=/dest/search"]));
    assert_eq!(set_cookies(&response), ["name=value; Path=/search"]);
}

#[test]
fn rewrites_cookie_domains_to_punycode_hostnames_without_ports() {
    let response = proxied(get("https://xn--mnich-kva.example:8443/search?q=remix"), "https://remix.run:3000/dest", FetchProxyOptions::default(), cookie_response(&["name=value; Domain=remix.run:3000; Path=/dest/search"]));
    assert_eq!(set_cookies(&response), ["name=value; Domain=xn--mnich-kva.example; Path=/search"]);
}

#[test]
fn preserves_host_only_upstream_cookies_while_rewriting_paths() {
    let response = proxied(get("https://preview.example.com:8443/search?q=remix"), "https://remix.run:3000/dest", FetchProxyOptions::default(), cookie_response(&["name=value; Path=/dest/search; HttpOnly"]));
    assert_eq!(set_cookies(&response), ["name=value; HttpOnly; Path=/search"]);
}

#[test]
fn preserves_cookie_domains_when_only_domain_rewriting_is_disabled() {
    let options = FetchProxyOptions { rewrite_cookie_domain: false, ..Default::default() };
    let response = proxied(get("https://preview.example.com:8443/search?q=remix"), "https://remix.run:3000/dest", options, cookie_response(&["name=value; Domain=remix.run:3000; Path=/dest/search"]));
    assert_eq!(set_cookies(&response), ["name=value; Domain=remix.run:3000; Path=/search"]);
}

#[test]
fn does_not_rewrite_cookie_domain_and_path_when_opting_out() {
    let options = FetchProxyOptions { rewrite_cookie_domain: false, rewrite_cookie_path: false, ..Default::default() };
    let respond = cookie_response(&["name=value; Domain=remix.run:3000; Path=/dest/search", "name2=value2; Domain=remix.run:3000; Path=/dest"]);
    let response = proxied(get("http://shopify.com/?q=remix"), "https://remix.run:3000/dest", options, respond);
    assert_eq!(set_cookies(&response), ["name=value; Domain=remix.run:3000; Path=/dest/search", "name2=value2; Domain=remix.run:3000; Path=/dest"]);
}

#[test]
fn preserves_all_request_properties_when_using_proxy_request() {
    let request = with_redirect(request(Method::PUT, "http://shopify.com/api/resource", &[], Body::empty()), Redirect::Error);
    let (seen, result) = test_proxy(request, "https://remix.run:3000/dest", FetchProxyOptions::default(), |_| response(302, &[("location", "/elsewhere")], Body::empty()));
    assert_eq!(seen[0].method, "PUT");
    assert_eq!(seen[0].redirect, Some(Redirect::Error));
    assert!(result.unwrap_err().to_string().starts_with("Unexpected redirect (302)"), "error mode fails on a redirect");
}

#[test]
fn allows_init_to_override_request_properties() {
    // The proxy is configured with Redirect::Error; the request's own mode wins.
    let options = FetchProxyOptions { redirect: Redirect::Error, ..Default::default() };
    let request = with_redirect(request(Method::POST, "http://shopify.com/api/resource", &[], Body::empty()), Redirect::Follow);
    let (seen, result) = test_proxy(request, "https://remix.run:3000/dest", options, |seen| {
        if seen.url.ends_with("/api/resource") {
            response(307, &[("location", "/dest/final")], Body::empty())
        } else {
            http::Response::new(Body::from("final"))
        }
    });
    let methods: Vec<&str> = seen.iter().map(|s| s.method.as_str()).collect();
    assert_eq!(methods, ["POST", "POST"], "307 keeps the method");
    assert_eq!(seen[1].url, "https://remix.run:3000/dest/final");
    let response = result.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(block_on(response.into_body().text()).unwrap(), "final");
}

// fetch proxy (double-arg style)

#[test]
fn works_with_proxy_url_init_style() {
    let seen = captured(request(Method::PATCH, "http://shopify.com/api/resource", &[("x-custom", "value")], Body::empty()), "https://remix.run:3000/dest");
    assert_eq!(seen.method, "PATCH");
    assert_eq!(seen.header("x-custom"), Some("value"));
    assert_eq!(seen.url, "https://remix.run:3000/dest/api/resource");
}

#[test]
fn keeps_x_forwarded_headers_when_proxy_url_init_provides_custom_headers() {
    let headers = [("x-custom", "present"), ("forwarded", "for=198.51.100.7"), ("x-forwarded-for", "203.0.113.43, 198.51.100.7")];
    let seen = captured_with(request(Method::PATCH, "http://shop.example:8080/orders", &headers, Body::empty()), "https://upstream.example/base", x_forwarded());
    assert_eq!(seen.method, "PATCH");
    assert_eq!(seen.header("x-custom"), Some("present"));
    assert_eq!(seen.header("forwarded"), None);
    assert_eq!(seen.header("x-forwarded-for"), None);
    assert_eq!(seen.header("x-forwarded-proto"), Some("http"));
    assert_eq!(seen.header("x-forwarded-host"), Some("shop.example:8080"));
    assert_eq!(seen.header("x-forwarded-port"), Some("8080"));
}

#[test]
fn handles_proxy_url_with_defaults() {
    let seen = captured(get("http://shopify.com/api/resource"), "https://remix.run:3000/dest");
    assert_eq!(seen.method, "GET");
    assert_eq!(seen.url, "https://remix.run:3000/dest/api/resource");
}

#[test]
fn forwards_headers_correctly_with_proxy_url_init() {
    let headers = [("authorization", "Bearer token123"), ("content-type", "application/json")];
    let seen = captured(request(Method::GET, "http://shopify.com/api/resource", &headers, Body::empty()), "https://remix.run:3000/dest");
    assert_eq!(seen.header("authorization"), Some("Bearer token123"));
    assert_eq!(seen.header("content-type"), Some("application/json"));
}

#[test]
fn handles_body_with_proxy_url_init() {
    let body = r#"{"name":"test","value":123}"#;
    let seen = captured(request(Method::POST, "http://shopify.com/api/resource", &[("content-type", "application/json")], Body::from(body)), "https://remix.run:3000/dest");
    assert_eq!(seen.method, "POST");
    assert_eq!(seen.body, body);
}

// Not in Remix: the redirect following that Remix leaves to fetch.

fn follow() -> FetchProxyOptions {
    FetchProxyOptions { redirect: Redirect::Follow, ..Default::default() }
}

#[test]
fn a_303_after_a_post_follows_with_a_bodyless_get() {
    let request = request(Method::POST, "http://shopify.com/form", &[("content-type", "text/plain"), ("authorization", "Bearer t")], Body::from("data"));
    let (seen, result) = test_proxy(request, "https://remix.run/app", follow(), |seen| {
        if seen.method == "POST" { response(303, &[("location", "done")], Body::empty()) } else { empty() }
    });
    assert_eq!(result.unwrap().status(), 200);
    assert_eq!((seen[1].method.as_str(), seen[1].url.as_str()), ("GET", "https://remix.run/app/done"));
    assert_eq!(seen[1].header("content-type"), None);
    assert_eq!(seen[1].header("authorization"), Some("Bearer t"), "same origin keeps credentials");
    assert!(seen[1].body_is_empty);
}

#[test]
fn a_cross_origin_redirect_drops_credentials() {
    let request = request(Method::GET, "http://shopify.com/", &[("authorization", "Bearer t"), ("cookie", "a=1"), ("x-kept", "yes")], Body::empty());
    let (seen, _) = test_proxy(request, "https://remix.run/", follow(), |seen| {
        if seen.url.starts_with("https://remix.run") { response(302, &[("location", "https://other.example/x")], Body::empty()) } else { empty() }
    });
    assert_eq!(seen[1].url, "https://other.example/x");
    assert_eq!((seen[1].header("authorization"), seen[1].header("cookie"), seen[1].header("x-kept")), (None, None, Some("yes")));
}

#[test]
fn a_redirect_without_a_location_is_returned_as_is() {
    let (seen, result) = test_proxy(get("http://shopify.com/"), "https://remix.run/", follow(), |_| response(302, &[], Body::empty()));
    assert_eq!(seen.len(), 1);
    assert_eq!(result.unwrap().status(), 302);
}

#[test]
fn following_stops_after_20_redirects() {
    let (seen, result) = test_proxy(get("http://shopify.com/"), "https://remix.run/", follow(), |_| response(302, &[("location", "/again")], Body::empty()));
    assert_eq!(result.unwrap_err().to_string(), "Too many redirects");
    assert_eq!(seen.len(), 21);
    let (seen, result) = test_proxy(get("http://shopify.com/"), "https://remix.run/", follow(), |seen| {
        if seen.url.ends_with("/20") { empty() } else {
            let next = seen.url.rsplit('/').next().unwrap().parse::<u32>().map_or(1, |n| n + 1);
            response(302, &[("location", &format!("/{next}"))], Body::empty())
        }
    });
    assert_eq!(result.unwrap().status(), 200, "exactly 20 redirects are allowed");
    assert_eq!(seen.len(), 21);
}

#[test]
fn a_307_cannot_replay_a_streamed_body() {
    let request = request(Method::PUT, "http://shopify.com/", &[], Body::from("data"));
    let (seen, result) = test_proxy(request, "https://remix.run/", follow(), |_| response(307, &[("location", "/again")], Body::empty()));
    assert_eq!(seen.len(), 1);
    assert_eq!(result.unwrap_err().to_string(), "Cannot follow a redirect with a streamed request body");
}
