use super::*;
use rmx_router::Context;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn dechunk(mut body: &str) -> String {
    let mut out = String::new();
    loop {
        let (size, rest) = body.split_once("\r\n").unwrap();
        let size = usize::from_str_radix(size, 16).unwrap();
        if size == 0 {
            return out;
        }
        out.push_str(&rest[..size]);
        body = &rest[size + 2..];
    }
}

#[tokio::test]
async fn serves_requests_and_streams() {
    let mut router = Router::new();
    router.get("/hello/:name", |ctx: Context| async move { format!("hi {}", ctx.param("name").unwrap()) });
    router.get("/stream", |_: Context| async {
        http::Response::new(Body::from_task(|tx| async move {
            tx.send("a");
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            tx.send("b");
        }))
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(serve(listener, router));

    for (path, expect) in [("/hello/ada", "hi ada"), ("/stream", "ab")] {
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        assert!(out.starts_with("HTTP/1.1 200"), "{out}");
        let (head, body) = out.split_once("\r\n\r\n").unwrap();
        let payload = if head.contains("transfer-encoding: chunked") { dechunk(body) } else { body.to_string() };
        assert_eq!(payload, expect, "{out}");
    }
}

#[tokio::test]
async fn fetch_proxies_to_another_server() {
    let mut app = Router::new();
    app.post("/app/echo", |mut ctx: Context| async move {
        let body = ctx.take_body().text().await.unwrap();
        let forwarded = ctx.header("x-forwarded-host").unwrap_or("-").to_string();
        let mut res = rmx_fetch::response::text(format!("{} {} {body}", ctx.url().href(), forwarded));
        res.headers_mut().insert("set-cookie", "sid=1; Domain=internal; Path=/app/echo".parse().unwrap());
        res
    });
    let app_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let app_addr = app_listener.local_addr().unwrap();
    tokio::spawn(serve(app_listener, app));

    let options = rmx_fetch_proxy::FetchProxyOptions { x_forwarded_headers: true, ..Default::default() };
    let proxy = rmx_fetch_proxy::FetchProxy::new(&format!("http://{app_addr}/app"), fetch, options).unwrap();
    let mut front = Router::new();
    front.post("/*path", move |ctx: Context| {
        let proxy = proxy.clone();
        async move { proxy.fetch(ctx.into_request()).await.unwrap_or_else(|e| rmx_fetch::response::text(e.to_string())) }
    });
    let front_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let front_addr = front_listener.local_addr().unwrap();
    tokio::spawn(serve(front_listener, front));

    let request = http::Request::builder().method("POST").uri(format!("http://{front_addr}/echo?x=1")).body(Body::from("ping")).unwrap();
    let response = fetch(request).await.unwrap();
    assert_eq!(response.status(), 200);
    let cookie = response.headers()["set-cookie"].to_str().unwrap().to_string();
    let text = response.into_body().text().await.unwrap();
    assert_eq!(text, format!("http://{app_addr}/app/echo?x=1 {front_addr} ping"));
    assert_eq!(cookie, "sid=1; Path=/echo", "an IP host drops the cookie domain");
    assert!(fetch(http::Request::builder().uri("https://example.com/").body(Body::empty()).unwrap()).await.is_err());
}

// request-listener.test.ts: URL construction and client addresses -----------------------------

const PEER: &str = "127.0.0.1:5555";

fn incoming(path: &str, headers: &[(&str, &str)]) -> Request {
    let mut req = http::Request::get(path);
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    req.body(Body::empty()).unwrap()
}

fn url_of(req: &Request) -> String {
    req.uri().to_string()
}

fn create(path: &str, headers: &[(&str, &str)], options: &ServeOptions) -> Request {
    create_request(incoming(path, headers), PEER.parse().unwrap(), options).unwrap()
}

#[test]
fn builds_the_url_from_the_host_header_or_options() {
    let none = ServeOptions::default();
    assert_eq!(url_of(&create("/a?b=1", &[("host", "example.com:8080")], &none)), "http://example.com:8080/a?b=1");
    assert_eq!(url_of(&create("/", &[], &none)), "http://localhost/");
    assert_eq!(url_of(&create("/", &[("host", "example.com")], &ServeOptions::default().host("remix.run"))), "http://remix.run/");
    assert_eq!(url_of(&create("/", &[("host", "example.com")], &ServeOptions::default().protocol("https:"))), "https://example.com/");
}

#[test]
fn proxy_headers_apply_only_when_trusted() {
    let forwarded = [("host", "internal:3000"), ("forwarded", "for=192.0.2.60;proto=https;host=remix.run"), ("x-forwarded-for", "203.0.113.7")];
    let req = create("/x", &forwarded, &ServeOptions::default());
    assert_eq!(url_of(&req), "http://internal:3000/x", "ignored by default");
    assert_eq!(req.extensions().get::<ClientAddress>().unwrap().address.to_string(), "127.0.0.1");

    let trusted = ServeOptions::default().trust_proxy(true);
    let req = create("/x", &forwarded, &trusted);
    assert_eq!(url_of(&req), "https://remix.run/x");
    let client = *req.extensions().get::<ClientAddress>().unwrap();
    assert_eq!((client.address.to_string().as_str(), client.port, client.family()), ("192.0.2.60", 5555, "IPv4"));

    let quoted = [("forwarded", r#"for="[2001:db8:cafe::17]:4711";proto="HTTPS";host="remix.run:8443""#)];
    let req = create("/", &quoted, &trusted);
    assert_eq!(url_of(&req), "https://remix.run:8443/");
    let client = *req.extensions().get::<ClientAddress>().unwrap();
    assert_eq!((client.address.to_string().as_str(), client.port, client.family()), ("2001:db8:cafe::17", 4711, "IPv6"));

    let x = [("host", "internal"), ("x-forwarded-host", "a.example, b.example"), ("x-forwarded-proto", "https, http"), ("x-forwarded-for", "198.51.100.1:8080, 10.0.0.1")];
    let req = create("/", &x, &trusted);
    assert_eq!(url_of(&req), "https://a.example/", "the first X-Forwarded values win");
    let client = *req.extensions().get::<ClientAddress>().unwrap();
    assert_eq!((client.address.to_string().as_str(), client.port), ("198.51.100.1", 8080));

    let invalid = [("host", "internal"), ("x-forwarded-proto", "ftp"), ("forwarded", "for=unknown"), ("x-forwarded-for", "_hidden")];
    let req = create("/", &invalid, &trusted);
    assert_eq!(url_of(&req), "http://internal/", "invalid protocols are ignored");
    assert_eq!(req.extensions().get::<ClientAddress>().unwrap().address.to_string(), "127.0.0.1", "obfuscated identifiers are ignored");

    let options_first = ServeOptions::default().trust_proxy(true).host("fixed.example").protocol("http");
    assert_eq!(url_of(&create("/", &[("forwarded", "proto=https;host=remix.run")], &options_first)), "http://fixed.example/");
}

fn h2(uri: &str, host: Option<&str>) -> Request {
    let mut req = http::Request::get(uri).version(Version::HTTP_2);
    if let Some(host) = host {
        req = req.header("host", host);
    }
    req.body(Body::empty()).unwrap()
}

#[test]
fn http2_authority_must_match_the_host_header() {
    let peer: SocketAddr = PEER.parse().unwrap();
    let none = ServeOptions::default();
    for (uri, host) in [("http://a.example/x", Some("b.example")), ("http://a.example:8080/", Some("a.example:8081"))] {
        let error = create_request(h2(uri, host), peer, &none).unwrap_err();
        assert_eq!(error, ServeError::ConflictingAuthority);
        let res = none.error_response(&error);
        assert_eq!(res.status(), 400);
    }
    let custom = ServeOptions::default().on_error(|e| Some(rmx_fetch::response::status(StatusCode::MISDIRECTED_REQUEST, e.to_string())));
    assert_eq!(custom.error_response(&ServeError::ConflictingAuthority).status(), 421);

    for (uri, host, expect) in [
        ("http://a.example:80/x", Some("A.EXAMPLE"), "http://a.example:80/x"),
        ("https://a.example/", Some("a.example:443"), "http://a.example/"),
        ("http://[::1]:8080/", Some("[::1]:8080"), "http://[::1]:8080/"),
        ("http://a.example/", None, "http://a.example/"),
    ] {
        assert_eq!(url_of(&create_request(h2(uri, host), peer, &none).unwrap()), expect, "{uri} {host:?}");
    }
    let overrides = ServeOptions::default().host("override.example").protocol("https");
    assert_eq!(url_of(&create_request(h2("http://a.example/p", Some("a.example")), peer, &overrides).unwrap()), "https://override.example/p");
}

async fn raw_get(addr: SocketAddr, path: &str) -> String {
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost:{}\r\nConnection: close\r\n\r\n", addr.port()).as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out
}

fn boom() -> &'static str {
    panic!("boom")
}

#[tokio::test]
async fn panicking_handlers_answer_500_or_the_error_handler_response() {
    let mut router = Router::new();
    router.get("/boom", |_: Context| async { boom() });
    router.get("/whoami", |ctx: Context| async move { format!("{} {}", ctx.url().href(), ctx.get::<ClientAddress>().unwrap().address) });

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(serve(listener, router.clone()));
    let out = raw_get(addr, "/boom").await;
    assert!(out.starts_with("HTTP/1.1 500"), "{out}");
    assert!(out.contains("content-type: text/plain") && out.ends_with("Internal Server Error"), "{out}");
    let out = raw_get(addr, "/whoami").await;
    assert!(out.ends_with(&format!("http://localhost:{}/whoami 127.0.0.1", addr.port())), "the server keeps serving: {out}");

    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    let options = ServeOptions::default().on_error(move |e| {
        log.lock().unwrap().push(e.to_string());
        Some(rmx_fetch::response::status(StatusCode::SERVICE_UNAVAILABLE, "custom"))
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(serve_with(listener, router, options));
    let out = raw_get(addr, "/boom").await;
    assert!(out.starts_with("HTTP/1.1 503") && out.ends_with("custom"), "{out}");
    assert_eq!(*seen.lock().unwrap(), ["request handler panicked: boom"]);
}
