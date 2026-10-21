//! The Fetch API, Rust edition: `Request`, `Response`, and a streaming [`Body`].
//!
//! rmx builds on the `http` crate's request/response types the way Remix builds on the
//! web's `Request`/`Response`. This crate adds what `http` leaves out: a body type that is
//! either buffered or streamed, helpers to read it (`bytes`, `text`, `form`), and a few
//! constructors for common responses.

use std::collections::VecDeque;
use std::fmt;
use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

pub use bytes::Bytes;
pub use http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use http_body::Frame;
pub use rmx_url::{SearchParams, Url};

pub mod abort;
pub mod form_data;
pub mod response;

pub use abort::{AbortController, AbortError, AbortSignal, RequestExt};
pub use form_data::{File, FileUpload, FormData, FormDataError, FormLimits, FormValue, UploadResult};

/// An HTTP request with an rmx [`Body`].
pub type Request = http::Request<Body>;
/// An HTTP response with an rmx [`Body`].
pub type Response = http::Response<Body>;
/// A boxed, thread-safe error.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
/// A boxed, sendable future.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// Something that performs HTTP requests, like the web's `fetch()`. Implemented for async
/// functions and closures from [`Request`] to `Result<Response, BoxError>`. Libraries that
/// call out over HTTP (`rmx-fetch-proxy`, `rmx-auth`) take one, so the app chooses the client
/// (`rmx_serve::fetch` for `http://`, or any HTTPS client) and tests can answer in memory.
pub trait Fetch: Send + Sync + 'static {
    fn fetch(&self, request: Request) -> BoxFuture<Result<Response, BoxError>>;
}

impl<F, Fut> Fetch for F
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, BoxError>> + Send + 'static,
{
    fn fetch(&self, request: Request) -> BoxFuture<Result<Response, BoxError>> {
        Box::pin(self(request))
    }
}

/// A request or response body: empty, buffered, or a stream of chunks.
pub struct Body {
    inner: Inner,
}

enum Inner {
    Empty,
    Full(Option<Bytes>),
    Channel(Arc<Shared>),
    /// A channel fed by a future that the body polls itself (no spawned task needed).
    Task(Arc<Shared>, Option<Pin<Box<dyn Future<Output = ()> + Send>>>),
    Boxed(Pin<Box<dyn http_body::Body<Data = Bytes, Error = BoxError> + Send>>),
}

impl Body {
    pub fn empty() -> Body {
        Body { inner: Inner::Empty }
    }

    /// A body backed by any `http_body::Body` (for example hyper's incoming body).
    pub fn wrap<B>(body: B) -> Body
    where
        B: http_body::Body<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        Body { inner: Inner::Boxed(Box::pin(MapErr(body))) }
    }

    /// A body read on demand from a blocking reader, up to 64 KiB per chunk (Remix's lazy
    /// files). Each chunk is read when the body is polled, so only one chunk is held at a time.
    pub fn from_reader<R: std::io::Read + Send + Unpin + 'static>(reader: R) -> Body {
        Body::wrap(ReaderBody(Some(reader)))
    }

    /// A streaming body and the [`BodySender`] that feeds it. The body ends when every
    /// sender is dropped.
    pub fn channel() -> (BodySender, Body) {
        let shared = Arc::new(Shared { state: Mutex::new(ChannelState::default()) });
        shared.state.lock().expect("body lock").senders = 1;
        (BodySender { shared: shared.clone() }, Body { inner: Inner::Channel(shared) })
    }

    /// A streaming body produced by `task`, which writes chunks to the sender it is given.
    /// The body polls the task as it is read, so no executor or spawned task is required;
    /// dropping the body drops the task.
    pub fn from_task<F, Fut>(task: F) -> Body
    where
        F: FnOnce(BodySender) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (tx, body) = Body::channel();
        let Inner::Channel(shared) = &body.inner else { unreachable!("channel body") };
        let shared = shared.clone();
        drop(body);
        shared.state.lock().expect("body lock").closed_by_reader = false;
        Body { inner: Inner::Task(shared, Some(Box::pin(task(tx)))) }
    }

    /// Whether the body is known to be empty without reading it.
    pub fn is_empty_hint(&self) -> bool {
        match &self.inner {
            Inner::Empty => true,
            Inner::Full(b) => b.as_ref().is_none_or(Bytes::is_empty),
            _ => false,
        }
    }

    /// Read the next chunk; `None` at the end.
    pub async fn chunk(&mut self) -> Option<Result<Bytes, BoxError>> {
        poll_fn(|cx| self.poll_chunk(cx)).await
    }

    fn poll_chunk(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Bytes, BoxError>>> {
        match &mut self.inner {
            Inner::Empty => Poll::Ready(None),
            Inner::Full(b) => Poll::Ready(b.take().map(Ok)),
            Inner::Channel(shared) => poll_shared(shared, cx),
            Inner::Task(shared, driver) => {
                if let Some(fut) = driver {
                    if fut.as_mut().poll(cx).is_ready() {
                        *driver = None;
                    }
                }
                poll_shared(shared, cx)
            }
            Inner::Boxed(body) => loop {
                match body.as_mut().poll_frame(cx) {
                    Poll::Ready(Some(Ok(frame))) => {
                        if let Ok(data) = frame.into_data() {
                            return Poll::Ready(Some(Ok(data)));
                        }
                        // Skip trailers.
                    }
                    Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
                    Poll::Ready(None) => return Poll::Ready(None),
                    Poll::Pending => return Poll::Pending,
                }
            },
        }
    }

    /// Read the whole body.
    pub async fn bytes(mut self) -> Result<Bytes, BoxError> {
        if let Inner::Full(b) = &mut self.inner {
            return Ok(b.take().unwrap_or_default());
        }
        let mut buf = Vec::new();
        while let Some(chunk) = self.chunk().await {
            buf.extend_from_slice(&chunk?);
        }
        Ok(Bytes::from(buf))
    }

    /// Read the whole body as UTF-8 (lossy).
    pub async fn text(self) -> Result<String, BoxError> {
        let bytes = self.bytes().await?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Read a `application/x-www-form-urlencoded` body.
    pub async fn form(self) -> Result<SearchParams, BoxError> {
        Ok(SearchParams::parse(&self.text().await?))
    }

    /// Read a JSON body.
    #[cfg(feature = "json")]
    pub async fn json<T: serde::de::DeserializeOwned>(self) -> Result<T, BoxError> {
        Ok(serde_json::from_slice(&self.bytes().await?)?)
    }
}

impl Default for Body {
    fn default() -> Body {
        Body::empty()
    }
}

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match &self.inner {
            Inner::Empty => "empty",
            Inner::Full(_) => "full",
            Inner::Channel(_) => "channel",
            Inner::Task(..) => "task",
            Inner::Boxed(_) => "stream",
        };
        write!(f, "Body({kind})")
    }
}

impl From<Bytes> for Body {
    fn from(b: Bytes) -> Body {
        Body { inner: Inner::Full(Some(b)) }
    }
}

impl From<String> for Body {
    fn from(s: String) -> Body {
        Bytes::from(s).into()
    }
}

impl From<&'static str> for Body {
    fn from(s: &'static str) -> Body {
        Bytes::from_static(s.as_bytes()).into()
    }
}

impl From<Vec<u8>> for Body {
    fn from(v: Vec<u8>) -> Body {
        Bytes::from(v).into()
    }
}

impl From<()> for Body {
    fn from(_: ()) -> Body {
        Body::empty()
    }
}

impl http_body::Body for Body {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        self.get_mut().poll_chunk(cx).map(|o| o.map(|r| r.map(Frame::data)))
    }

    fn is_end_stream(&self) -> bool {
        match &self.inner {
            Inner::Empty => true,
            Inner::Full(b) => b.is_none(),
            Inner::Boxed(b) => b.is_end_stream(),
            Inner::Channel(_) | Inner::Task(..) => false,
        }
    }

    fn size_hint(&self) -> http_body::SizeHint {
        match &self.inner {
            Inner::Empty => http_body::SizeHint::with_exact(0),
            Inner::Full(b) => http_body::SizeHint::with_exact(b.as_ref().map_or(0, |b| b.len() as u64)),
            Inner::Boxed(b) => b.size_hint(),
            Inner::Channel(_) | Inner::Task(..) => http_body::SizeHint::default(),
        }
    }
}

fn poll_shared(shared: &Shared, cx: &mut Context<'_>) -> Poll<Option<Result<Bytes, BoxError>>> {
    let mut st = shared.state.lock().expect("body lock");
    if let Some(chunk) = st.queue.pop_front() {
        return Poll::Ready(Some(Ok(chunk)));
    }
    if st.senders == 0 {
        return Poll::Ready(st.error.take().map(Err));
    }
    st.waker = Some(cx.waker().clone());
    Poll::Pending
}

struct ReaderBody<R>(Option<R>);

impl<R: std::io::Read + Unpin> http_body::Body for ReaderBody<R> {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, std::io::Error>>> {
        let this = self.get_mut();
        let Some(reader) = this.0.as_mut() else { return Poll::Ready(None) };
        let mut buf = vec![0; 64 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    this.0 = None;
                    return Poll::Ready(None);
                }
                Ok(n) => {
                    buf.truncate(n);
                    return Poll::Ready(Some(Ok(Frame::data(Bytes::from(buf)))));
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    this.0 = None;
                    return Poll::Ready(Some(Err(e)));
                }
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_none()
    }
}

struct MapErr<B>(B);

impl<B> http_body::Body for MapErr<B>
where
    B: http_body::Body<Data = Bytes>,
    B::Error: Into<BoxError>,
{
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        // SAFETY: structural pinning of the only field.
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.0) };
        inner.poll_frame(cx).map(|o| o.map(|r| r.map_err(Into::into)))
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.0.size_hint()
    }
}

#[derive(Default)]
struct ChannelState {
    queue: VecDeque<Bytes>,
    senders: usize,
    closed_by_reader: bool,
    error: Option<BoxError>,
    waker: Option<Waker>,
}

struct Shared {
    state: Mutex<ChannelState>,
}

/// Feeds a streaming [`Body`]. Cloneable; the body ends when every sender is dropped.
pub struct BodySender {
    shared: Arc<Shared>,
}

impl BodySender {
    /// Queue a chunk. Returns `false` if the body was dropped (the client went away).
    pub fn send(&self, chunk: impl Into<Bytes>) -> bool {
        let mut st = self.shared.state.lock().expect("body lock");
        if st.closed_by_reader {
            return false;
        }
        st.queue.push_back(chunk.into());
        if let Some(w) = st.waker.take() {
            w.wake();
        }
        true
    }

    /// End the stream with an error.
    pub fn abort(self, error: impl Into<BoxError>) {
        self.shared.state.lock().expect("body lock").error = Some(error.into());
    }

    /// Whether the reading side has been dropped.
    pub fn is_closed(&self) -> bool {
        self.shared.state.lock().expect("body lock").closed_by_reader
    }
}

impl Clone for BodySender {
    fn clone(&self) -> Self {
        self.shared.state.lock().expect("body lock").senders += 1;
        BodySender { shared: self.shared.clone() }
    }
}

impl Drop for BodySender {
    fn drop(&mut self) {
        let mut st = self.shared.state.lock().expect("body lock");
        st.senders -= 1;
        if st.senders == 0 {
            if let Some(w) = st.waker.take() {
                w.wake();
            }
        }
    }
}

impl Drop for Body {
    fn drop(&mut self) {
        if let Inner::Channel(shared) | Inner::Task(shared, _) = &self.inner {
            shared.state.lock().expect("body lock").closed_by_reader = true;
        }
    }
}

/// The request URL as an absolute [`Url`]. Uses the `Host` header (or `localhost`) when the
/// request target is origin-relative, as it is for requests arriving from a server.
pub fn request_url(req: &http::request::Parts) -> Url {
    let uri = &req.uri;
    if let (Some(scheme), Some(authority)) = (uri.scheme_str(), uri.authority()) {
        if let Ok(url) = Url::parse(&format!("{scheme}://{authority}{}", uri.path_and_query().map_or("/", |p| p.as_str()))) {
            return url;
        }
    }
    let host = req.headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("localhost");
    let path = uri.path_and_query().map_or("/", |p| p.as_str());
    Url::parse(&format!("http://{host}{path}")).unwrap_or_else(|_| Url::parse("http://localhost/").expect("static url"))
}

/// Await a future to completion on the current thread. For tests and simple tools only.
pub fn block_on<F: Future>(fut: F) -> F::Output {
    use std::task::Wake;
    struct ThreadWaker(std::thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut fut = std::pin::pin!(fut);
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_full_and_channel_bodies() {
        assert_eq!(block_on(Body::from("hi").text()).unwrap(), "hi");
        let (tx, body) = Body::channel();
        let t = std::thread::spawn(move || {
            tx.send("a");
            std::thread::sleep(std::time::Duration::from_millis(10));
            tx.send("b");
        });
        assert_eq!(block_on(body.text()).unwrap(), "ab");
        t.join().unwrap();
        assert_eq!(block_on(Body::from("a=1&b=x+y").form()).unwrap().get("b"), Some("x y"));
    }

    #[test]
    fn reader_body_streams_in_chunks() {
        let data: Vec<u8> = (0..200_000u32).map(|i| i as u8).collect();
        let mut body = Body::from_reader(std::io::Cursor::new(data.clone()));
        let first = block_on(body.chunk()).unwrap().unwrap();
        assert_eq!(first.len(), 64 * 1024);
        let rest = block_on(body.bytes()).unwrap();
        assert_eq!([&first[..], &rest[..]].concat(), data);
    }

    #[test]
    fn task_body_streams_without_a_runtime() {
        let body = Body::from_task(|tx| async move {
            tx.send("one ");
            Yield(false).await;
            tx.send("two");
        });
        assert_eq!(block_on(body.text()).unwrap(), "one two");
    }

    /// Returns Pending once (waking itself), then Ready.
    struct Yield(bool);
    impl Future for Yield {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if self.0 {
                return Poll::Ready(());
            }
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }

    #[test]
    fn sender_sees_dropped_body() {
        let (tx, body) = Body::channel();
        drop(body);
        assert!(!tx.send("x"));
        assert!(tx.is_closed());
    }
}
