use std::sync::{Arc, Mutex};

use http::{HeaderMap, StatusCode};
use rsmix_fetch::block_on;

use super::*;

#[derive(Clone, Debug, Default)]
struct Seen {
    method: String,
    uri: String,
    headers: HeaderMap,
    body: String,
}

/// Proxy `request` to `target` through a fake upstream that answers with `respond()`.
fn run(request: Request, target: &str, options: FetchProxyOptions, respond: impl Fn() -> Response + Send + Sync + 'static) -> (Seen, Response) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let record = seen.clone();
    let respond = Arc::new(respond);
    let upstream = move |req: Request| {
        let (record, respond) = (record.clone(), respond.clone());
        async move {
            let (parts, body) = req.into_parts();
            let body = body.text().await?;
            *record.lock().unwrap() = Seen { method: parts.method.to_string(), uri: parts.uri.to_string(), headers: parts.headers, body };
            Ok(respond())
        }
    };
    let proxy = FetchProxy::new(target, upstream, options).unwrap();
    let response = block_on(proxy.fetch(request)).unwrap();
    let seen = seen.lock().unwrap().clone();
    (seen, response)
}

fn get(url: &str) -> Request {
    http::Request::builder().uri(url).body(Body::empty()).unwrap()
}

fn ok() -> Response {
    http::Response::new(Body::empty())
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name).map(|v| v.to_str().unwrap().to_string())
}

#[test]
fn appends_the_request_pathname_and_search_to_the_target_url() {
    let cases = [
        ("http://shopify.com/?q=remix", "https://remix.run:3000/rsc", "https://remix.run:3000/rsc?q=remix"),
        ("http://shopify.com/search?q=remix", "https://remix.run:3000/rsc", "https://remix.run:3000/rsc/search?q=remix"),
        ("http://shopify.com/search?q=remix", "https://remix.run:3000/rsc/", "https://remix.run:3000/rsc/search?q=remix"),
        ("http://shopify.com/search?q=remix", "https://remix.run:3000/", "https://remix.run:3000/search?q=remix"),
        ("http://shopify.com/", "https://remix.run:3000", "https://remix.run:3000/"),
    ];
    for (incoming, target, expected) in cases {
        let (seen, _) = run(get(incoming), target, FetchProxyOptions::default(), ok);
        assert_eq!(seen.uri, expected, "{incoming} -> {target}");
    }
}

#[test]
fn forwards_method_headers_and_body_without_connection_specific_headers() {
    let request = http::Request::builder()
        .method("POST")
        .uri("http://shopify.com/search")
        .header("content-type", "text/plain")
        .header("content-length", "5")
        .header("host", "shopify.com")
        .header("accept-encoding", "gzip, br")
        .header("connection", "keep-alive, X-Custom-Hop")
        .header("x-custom-hop", "secret")
        .header("keep-alive", "timeout=5")
        .header("proxy-authorization", "Basic abc")
        .header("upgrade", "websocket")
        .header("te", "trailers")
        .header("x-kept", "yes")
        .body(Body::from("hello"))
        .unwrap();
    let (seen, _) = run(request, "https://remix.run:3000/rsc", FetchProxyOptions::default(), ok);
    assert_eq!((seen.method.as_str(), seen.body.as_str()), ("POST", "hello"));
    assert_eq!(header(&seen.headers, "content-type").as_deref(), Some("text/plain"));
    assert_eq!(header(&seen.headers, "x-kept").as_deref(), Some("yes"));
    for name in ["content-length", "host", "accept-encoding", "connection", "x-custom-hop", "keep-alive", "proxy-authorization", "upgrade", "te"] {
        assert!(!seen.headers.contains_key(name), "{name} was forwarded");
    }
    assert!(!seen.headers.contains_key("x-forwarded-host"), "no X-Forwarded headers by default");
}

#[test]
fn get_and_head_requests_carry_no_body() {
    let request = http::Request::builder().method("GET").uri("http://shopify.com/").body(Body::from("ignored")).unwrap();
    let (seen, _) = run(request, "https://remix.run/", FetchProxyOptions::default(), ok);
    assert_eq!(seen.body, "");
}

#[test]
fn x_forwarded_headers_replace_incoming_forwarding_metadata() {
    let options = FetchProxyOptions { x_forwarded_headers: true, ..Default::default() };
    let request = http::Request::builder()
        .uri("https://shopify.com:8443/search")
        .header("forwarded", "for=1.2.3.4")
        .header("x-forwarded-for", "1.2.3.4")
        .header("x-forwarded-host", "evil.example")
        .body(Body::empty())
        .unwrap();
    let (seen, _) = run(request, "http://127.0.0.1:3000", options.clone(), ok);
    assert_eq!(header(&seen.headers, "x-forwarded-proto").as_deref(), Some("https"));
    assert_eq!(header(&seen.headers, "x-forwarded-host").as_deref(), Some("shopify.com:8443"));
    assert_eq!(header(&seen.headers, "x-forwarded-port").as_deref(), Some("8443"));
    assert!(!seen.headers.contains_key("forwarded") && !seen.headers.contains_key("x-forwarded-for"));
    let (seen, _) = run(get("https://shopify.com/"), "http://127.0.0.1:3000", options.clone(), ok);
    assert_eq!(header(&seen.headers, "x-forwarded-port").as_deref(), Some("443"));
    let (seen, _) = run(get("http://shopify.com/"), "http://127.0.0.1:3000", options, ok);
    assert_eq!(header(&seen.headers, "x-forwarded-port").as_deref(), Some("80"));
}

#[test]
fn forwarding_metadata_passes_through_when_x_forwarded_headers_are_off() {
    let request = http::Request::builder().uri("http://shopify.com/").header("x-forwarded-host", "edge.example").header("forwarded", "for=1.2.3.4").body(Body::empty()).unwrap();
    let (seen, _) = run(request, "http://127.0.0.1:3000", FetchProxyOptions::default(), ok);
    assert_eq!(header(&seen.headers, "x-forwarded-host").as_deref(), Some("edge.example"));
    assert_eq!(header(&seen.headers, "forwarded").as_deref(), Some("for=1.2.3.4"));
}

#[test]
fn redirects_reach_the_client() {
    let (_, response) = run(get("http://shopify.com/login"), "http://127.0.0.1:3000", FetchProxyOptions::default(), || {
        http::Response::builder().status(302).header("location", "/home").body(Body::empty()).unwrap()
    });
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(header(response.headers(), "location").as_deref(), Some("/home"));
}

fn cookies(response: &Response) -> Vec<String> {
    response.headers().get_all("set-cookie").iter().map(|v| v.to_str().unwrap().to_string()).collect()
}

fn with_cookies(list: &'static [&'static str]) -> impl Fn() -> Response + Send + Sync + 'static {
    move || {
        let mut response = http::Response::new(Body::empty());
        for cookie in list {
            response.headers_mut().append("set-cookie", HeaderValue::from_static(cookie));
        }
        response
    }
}

#[test]
fn rewrites_cookie_domain_and_path() {
    let upstream = with_cookies(&["name=value; Domain=remix.run:3000; Path=/dest/search", "name2=value2; Domain=remix.run:3000; Path=/dest"]);
    let (_, response) = run(get("http://shopify.com/search?q=remix"), "https://remix.run:3000/dest", FetchProxyOptions::default(), upstream);
    assert_eq!(cookies(&response), ["name=value; Domain=shopify.com; Path=/search", "name2=value2; Domain=shopify.com; Path=/"]);
}

#[test]
fn cookie_domains_are_dropped_for_localhost_and_ip_hosts_and_kept_for_dns_hosts() {
    let upstream = || with_cookies(&["sid=1; Domain=remix.run; Path=/app/x"]);
    for (incoming, expected) in [
        ("http://localhost:44100/", "sid=1; Path=/x"),
        ("http://127.0.0.1:44100/", "sid=1; Path=/x"),
        ("http://[::1]:44100/", "sid=1; Path=/x"),
        ("http://shop.example:8080/", "sid=1; Domain=shop.example; Path=/x"),
    ] {
        let (_, response) = run(get(incoming), "http://127.0.0.1:3000/app", FetchProxyOptions::default(), upstream());
        assert_eq!(cookies(&response), [expected], "{incoming}");
    }
}

#[test]
fn host_only_cookies_stay_host_only_and_rewriting_can_be_turned_off() {
    let (_, response) = run(get("http://shop.example/"), "http://127.0.0.1:3000/app", FetchProxyOptions::default(), with_cookies(&["sid=1; Path=/app"]));
    assert_eq!(cookies(&response), ["sid=1; Path=/"]);
    let keep_domain = FetchProxyOptions { rewrite_cookie_domain: false, ..Default::default() };
    let (_, response) = run(get("http://shop.example/"), "http://127.0.0.1:3000/app", keep_domain, with_cookies(&["sid=1; Domain=remix.run; Path=/app/x"]));
    assert_eq!(cookies(&response), ["sid=1; Domain=remix.run; Path=/x"]);
    let neither = FetchProxyOptions { rewrite_cookie_domain: false, rewrite_cookie_path: false, ..Default::default() };
    let (_, response) = run(get("http://shop.example/"), "http://127.0.0.1:3000/app", neither, with_cookies(&["sid=1; Domain=remix.run; Path=/app/x"]));
    assert_eq!(cookies(&response), ["sid=1; Domain=remix.run; Path=/app/x"]);
}

fn encoded() -> Response {
    http::Response::builder().header("content-encoding", "gzip").header("content-length", "42").body(Body::from("x")).unwrap()
}

#[test]
fn response_framing_and_encoding_headers() {
    let chunked = || http::Response::builder().header("transfer-encoding", "chunked").header("content-length", "5").body(Body::from("hello")).unwrap();
    let (_, response) = run(get("http://a/"), "http://b", FetchProxyOptions::default(), chunked);
    assert!(!response.headers().contains_key("transfer-encoding") && !response.headers().contains_key("content-length"));

    let plain = || http::Response::builder().header("content-length", "5").body(Body::from("hello")).unwrap();
    let (_, response) = run(get("http://a/"), "http://b", FetchProxyOptions::default(), plain);
    assert_eq!(header(response.headers(), "content-length").as_deref(), Some("5"));

    // A body still encoded keeps describing itself.
    let (_, response) = run(get("http://a/"), "http://b", FetchProxyOptions::default(), encoded);
    assert_eq!((header(response.headers(), "content-encoding").as_deref(), header(response.headers(), "content-length").as_deref()), (Some("gzip"), Some("42")));

    // A fetch that decodes: the headers go, except on HEAD and 304 responses, which have no body.
    let decodes = FetchProxyOptions { fetch_decodes_content: true, ..Default::default() };
    let (_, response) = run(get("http://a/"), "http://b", decodes.clone(), encoded);
    assert!(!response.headers().contains_key("content-encoding") && !response.headers().contains_key("content-length"));
    let head = http::Request::builder().method("HEAD").uri("http://a/").body(Body::empty()).unwrap();
    let (_, response) = run(head, "http://b", decodes.clone(), encoded);
    assert_eq!(header(response.headers(), "content-encoding").as_deref(), Some("gzip"));
    let not_modified = || http::Response::builder().status(304).header("content-encoding", "gzip").body(Body::empty()).unwrap();
    let (_, response) = run(get("http://a/"), "http://b", decodes, not_modified);
    assert_eq!(header(response.headers(), "content-encoding").as_deref(), Some("gzip"));
}
