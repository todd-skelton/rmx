//! `rsmix_node_fetch_server::test`, Remix's `node-fetch-server/test` (`createTestServer`).

use rsmix_fetch::Body;
use rsmix_fetch_router::{Context, Router};
use rsmix_node_fetch_server::test::{create_test_server, create_test_server_with};
use rsmix_node_fetch_server::{fetch, ClientAddress, ServeOptions};

fn router() -> Router {
    let mut router = Router::new();
    router.get("/hello/:name", |ctx: Context| async move {
        let client = ctx.get::<ClientAddress>().map(|c| c.address.to_string()).unwrap_or_default();
        format!("hi {} from {client}", ctx.param("name").unwrap())
    });
    router
}

async fn get(url: &str) -> Result<(u16, String), rsmix_fetch::BoxError> {
    let response = fetch(http::Request::builder().uri(url).body(Body::empty())?).await?;
    let status = response.status().as_u16();
    Ok((status, response.into_body().text().await?))
}

/// `server.close()`, failing instead of hanging when it does not return.
async fn close(server: rsmix_node_fetch_server::test::TestServer) {
    let closed = tokio::time::timeout(std::time::Duration::from_secs(5), server.close()).await;
    assert!(closed.is_ok(), "close returns once the server stopped accepting");
}

fn port(base_url: &str) -> u16 {
    base_url.rsplit(':').next().unwrap().parse().unwrap()
}

#[tokio::test]
async fn serves_the_router_on_a_free_loopback_port() {
    let server = create_test_server(router()).await.unwrap();
    assert!(server.base_url.starts_with("http://127.0.0.1:"), "{}", server.base_url);
    assert_ne!(port(&server.base_url), 0);
    let (status, body) = get(&format!("{}/hello/remix", server.base_url)).await.unwrap();
    assert_eq!((status, body.as_str()), (200, "hi remix from 127.0.0.1"));
    assert_eq!(get(&format!("{}/nowhere", server.base_url)).await.unwrap().0, 404);

    let other = create_test_server(router()).await.unwrap();
    assert_ne!(other.base_url, server.base_url, "each server gets its own port");
    close(server).await;
    close(other).await;
}

#[tokio::test]
async fn close_stops_accepting_connections() {
    let server = create_test_server(router()).await.unwrap();
    let url = format!("{}/hello/a", server.base_url);
    assert_eq!(get(&url).await.unwrap().0, 200);
    close(server).await;
    assert!(get(&url).await.is_err(), "nothing listens after close");
}

#[tokio::test]
async fn dropping_the_server_closes_it() {
    let server = create_test_server_with(router(), ServeOptions::default().host("app.test")).await.unwrap();
    let url = format!("{}/hello/b", server.base_url);
    assert_eq!(get(&url).await.unwrap().0, 200);
    drop(server);
    // The accept loop ends on its next turn of the runtime.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while get(&url).await.is_ok() {
        assert!(std::time::Instant::now() < deadline, "still listening after drop");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}
