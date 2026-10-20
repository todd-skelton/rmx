//! Serve an [`rmx_router::Router`] over TCP (Remix's `node-fetch-server`).
//!
//! ```no_run
//! # async fn run(router: rmx_router::Router) -> std::io::Result<()> {
//! let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
//! rmx_serve::serve(listener, router).await
//! # }
//! ```
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
}
