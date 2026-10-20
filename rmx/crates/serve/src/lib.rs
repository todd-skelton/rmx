//! Serve an [`rmx_router::Router`] over TCP (Remix's `node-fetch-server`).
//!
//! ```no_run
//! # async fn run(router: rmx_router::Router) -> std::io::Result<()> {
//! let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
//! rmx_serve::serve(listener, router).await
//! # }
//! ```
//!
//! [`fetch`] is the matching client, for proxies and calls to other servers.
//!
//! This is the only crate that depends on a specific runtime (tokio) and HTTP
//! implementation (hyper); everything else works with plain `Request` / `Response` values.

use std::convert::Infallible;
use std::net::SocketAddr;

use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use rmx_fetch::Body;
use rmx_router::Router;
use tokio::net::TcpListener;

/// The client address, available to handlers via `ctx.get::<RemoteAddr>()`.
#[derive(Clone, Copy, Debug)]
pub struct RemoteAddr(pub SocketAddr);

/// Accept connections forever, handling each request with `router`.
pub async fn serve(listener: TcpListener, router: Router) -> std::io::Result<()> {
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
        let router = router.clone();
        tokio::spawn(async move {
            let service = service_fn(move |req: hyper::Request<Incoming>| {
                let router = router.clone();
                async move {
                    let mut req = req.map(Body::wrap);
                    req.extensions_mut().insert(RemoteAddr(remote));
                    Ok::<_, Infallible>(router.fetch(req).await)
                }
            });
            let _ = auto::Builder::new(TokioExecutor::new())
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
    }
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
    let listener = TcpListener::bind(addr).await?;
    println!("listening on http://{}", listener.local_addr()?);
    serve(listener, router).await
}

#[cfg(test)]
mod tests {
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
}
