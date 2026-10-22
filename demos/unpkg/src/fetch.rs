//! `fetch()` for the npm client. Remix calls the runtime's global `fetch` and its tests swap
//! it for a mock; here the client takes a [Fetch] so tests can pass fixtures instead.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;

pub type FetchResult = Result<http::Response<Bytes>, String>;

type FetchFn = dyn Fn(http::Request<()>) -> Pin<Box<dyn Future<Output = FetchResult> + Send>> + Send + Sync;

/// A request-to-response function, like the Fetch API's `fetch`.
#[derive(Clone)]
pub struct Fetch(Arc<FetchFn>);

impl Fetch {
    pub fn new<F, Fut>(fetch: F) -> Fetch
    where
        F: Fn(http::Request<()>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = FetchResult> + Send + 'static,
    {
        Fetch(Arc::new(move |req| Box::pin(fetch(req))))
    }

    pub async fn fetch(&self, req: http::Request<()>) -> FetchResult {
        (self.0)(req).await
    }

    /// The real network: HTTPS through `ureq` on Tokio's blocking pool.
    pub fn network() -> Fetch {
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
        Fetch::new(move |req: http::Request<()>| {
            let agent = agent.clone();
            async move {
                tokio::task::spawn_blocking(move || {
                    let res = agent.run(req).map_err(|e| e.to_string())?;
                    let (parts, mut body) = res.into_parts();
                    let bytes = body.with_config().limit(512 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())?;
                    Ok(http::Response::from_parts(parts, Bytes::from(bytes)))
                })
                .await
                .map_err(|e| e.to_string())?
            }
        })
    }
}
