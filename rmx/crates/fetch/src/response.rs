//! Response constructors (Remix's `remix/response/*`).

use http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};

use bytes::Bytes;

use crate::{Body, File, Response};

mod file;

pub use file::{file, Digest, DigestFn, ETag, FileLike, FileResponseOptions};

const DOCTYPE: &str = "<!DOCTYPE html>";
/// How much of a streamed body's first chunk is decoded to look for a doctype (Remix's
/// `DOCTYPE_SNIFF_BYTES`); more is read only while the prefix is leading whitespace.
const DOCTYPE_SNIFF_BYTES: usize = 64;

/// Status and headers for a response (the Web `ResponseInit`). An unset status keeps the
/// response's own (`200`, or `302` for [`redirect`]); headers replace the ones a
/// constructor sets, so an init can override `Content-Type` or `Location`.
#[derive(Clone, Debug, Default)]
pub struct ResponseInit {
    pub status: Option<StatusCode>,
    pub headers: HeaderMap,
}

impl ResponseInit {
    pub fn status(status: StatusCode) -> ResponseInit {
        ResponseInit { status: Some(status), ..ResponseInit::default() }
    }

    /// Adds a header. Names are case-insensitive; an invalid name or value is skipped.
    pub fn header(mut self, name: &str, value: &str) -> ResponseInit {
        if let (Ok(name), Ok(value)) = (HeaderName::from_bytes(name.as_bytes()), HeaderValue::from_str(value)) {
            self.headers.append(name, value);
        }
        self
    }

    /// Sets this status on `res` (when set) and adds these headers, replacing ones it
    /// already has.
    pub fn apply(self, mut res: Response) -> Response {
        if let Some(status) = self.status {
            *res.status_mut() = status;
        }
        let mut last: Option<HeaderName> = None;
        for (name, value) in self.headers {
            let name = name.or(last.clone()).expect("the first header has a name");
            if last.as_ref() != Some(&name) {
                res.headers_mut().remove(&name);
            }
            res.headers_mut().append(name.clone(), value);
            last = Some(name);
        }
        res
    }
}

/// A status alone, so `redirect_with(url, StatusCode::SEE_OTHER)` reads like Remix's
/// `redirect(url, 303)`.
impl From<StatusCode> for ResponseInit {
    fn from(status: StatusCode) -> ResponseInit {
        ResponseInit::status(status)
    }
}

/// What [html] accepts (Remix's `string | SafeHtml | Blob | BufferSource | ReadableStream`):
/// text, escaped markup, bytes (decoded as UTF-8, invalid sequences replaced), an in-memory
/// [File] or a streamed [Body].
pub enum HtmlBody {
    Text(String),
    Bytes(Bytes),
    Stream(Body),
}

impl From<String> for HtmlBody {
    fn from(text: String) -> HtmlBody {
        HtmlBody::Text(text)
    }
}

impl From<&str> for HtmlBody {
    fn from(text: &str) -> HtmlBody {
        HtmlBody::Text(text.to_string())
    }
}

impl From<&String> for HtmlBody {
    fn from(text: &String) -> HtmlBody {
        HtmlBody::Text(text.clone())
    }
}

impl From<std::borrow::Cow<'_, str>> for HtmlBody {
    fn from(text: std::borrow::Cow<'_, str>) -> HtmlBody {
        HtmlBody::Text(text.into_owned())
    }
}

impl From<rmx_html_template::SafeHtml> for HtmlBody {
    fn from(html: rmx_html_template::SafeHtml) -> HtmlBody {
        HtmlBody::Text(html.into_string())
    }
}

impl From<Bytes> for HtmlBody {
    fn from(bytes: Bytes) -> HtmlBody {
        HtmlBody::Bytes(bytes)
    }
}

impl From<Vec<u8>> for HtmlBody {
    fn from(bytes: Vec<u8>) -> HtmlBody {
        HtmlBody::Bytes(bytes.into())
    }
}

impl From<&[u8]> for HtmlBody {
    fn from(bytes: &[u8]) -> HtmlBody {
        HtmlBody::Bytes(Bytes::copy_from_slice(bytes))
    }
}

/// A file's bytes, streamed like a `Blob` body.
impl From<File> for HtmlBody {
    fn from(file: File) -> HtmlBody {
        HtmlBody::Stream(Body::from(file.bytes))
    }
}

impl From<Body> for HtmlBody {
    fn from(body: Body) -> HtmlBody {
        HtmlBody::Stream(body)
    }
}

/// A `text/html` response (Remix's `createHtmlResponse`). Adds `<!DOCTYPE html>` when the
/// body does not start with one (case-insensitive, after leading whitespace). A stream is
/// checked by its first chunk, and an empty stream becomes just the doctype.
pub fn html(body: impl Into<HtmlBody>) -> Response {
    let body = match body.into() {
        HtmlBody::Text(text) => Body::from(with_doctype(text)),
        HtmlBody::Bytes(bytes) => Body::from(with_doctype(String::from_utf8_lossy(&bytes).into_owned())),
        HtmlBody::Stream(body) => prepend_doctype(body),
    };
    with_type(body, "text/html; charset=UTF-8")
}

fn with_doctype(mut text: String) -> String {
    if !starts_with_doctype(&text) {
        text.insert_str(0, DOCTYPE);
    }
    text
}

/// [`html`] with a status and headers (Remix's `createHtmlResponse(body, init)`). A
/// `Content-Type` in `init` replaces the HTML one.
pub fn html_with(body: impl Into<HtmlBody>, init: impl Into<ResponseInit>) -> Response {
    init.into().apply(html(body))
}

/// [`html`] for a streamed body.
pub fn html_stream(body: Body) -> Response {
    html(body)
}

/// `body` with `<!DOCTYPE html>` in front unless its first chunk starts with one. A stream
/// error is passed on.
fn prepend_doctype(body: Body) -> Body {
    Body::from_task(move |tx| async move {
        let mut body = body;
        let first = match body.chunk().await {
            Some(Ok(chunk)) => chunk,
            Some(Err(error)) => return tx.abort(error),
            None => {
                tx.send(DOCTYPE);
                return;
            }
        };
        if !starts_with_doctype_bytes(&first) {
            tx.send(DOCTYPE);
        }
        if !tx.send(first) {
            return;
        }
        while let Some(chunk) = body.chunk().await {
            match chunk {
                Ok(chunk) => {
                    if !tx.send(chunk) {
                        return;
                    }
                }
                Err(error) => return tx.abort(error),
            }
        }
    })
}

/// A `text/plain` response.
pub fn text(body: impl Into<String>) -> Response {
    with_type(Body::from(body.into()), "text/plain; charset=UTF-8")
}

/// An `application/json` response.
#[cfg(feature = "json")]
pub fn json<T: serde::Serialize + ?Sized>(value: &T) -> Response {
    match serde_json::to_vec(value) {
        Ok(bytes) => with_type(Body::from(bytes), "application/json; charset=UTF-8"),
        Err(e) => status(StatusCode::INTERNAL_SERVER_ERROR, format!("JSON serialization failed: {e}")),
    }
}

/// A `302 Found` redirect.
pub fn redirect(location: &str) -> Response {
    redirect_with(location, ResponseInit::default())
}

/// A redirect with a status (use `303` after a POST) or a whole [`ResponseInit`], like
/// Remix's `redirect(location, init)`. A `Location` header in `init` wins over
/// `location`; a URL is passed as `&url.href()`.
pub fn redirect_with(location: &str, init: impl Into<ResponseInit>) -> Response {
    let init = init.into();
    let mut res = Response::new(Body::empty());
    *res.status_mut() = StatusCode::FOUND;
    if let Ok(v) = HeaderValue::from_str(location) {
        res.headers_mut().insert(header::LOCATION, v);
    }
    init.apply(res)
}

/// A plain-text response with a status, such as `status(404, "Not Found")`.
pub fn status(status: StatusCode, body: impl Into<String>) -> Response {
    let mut res = text(body);
    *res.status_mut() = status;
    res
}

pub fn not_found() -> Response {
    status(StatusCode::NOT_FOUND, "Not Found")
}

fn with_type(body: Body, content_type: &'static str) -> Response {
    let mut res = Response::new(body);
    res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    res
}

fn starts_with_doctype(s: &str) -> bool {
    let t = s.trim_start();
    t.as_bytes().get(..14).is_some_and(|b| b.eq_ignore_ascii_case(b"<!doctype html"))
}

/// [`starts_with_doctype`] on the start of a chunk, decoding only a short prefix unless the
/// prefix is all leading whitespace. A character split at the prefix end is left for the
/// next, longer prefix.
fn starts_with_doctype_bytes(bytes: &[u8]) -> bool {
    let mut len = DOCTYPE_SNIFF_BYTES.min(bytes.len());
    loop {
        let prefix = match std::str::from_utf8(&bytes[..len]) {
            Ok(s) => s,
            Err(e) => std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or_default(),
        };
        if starts_with_doctype(prefix) || len == bytes.len() || prefix.trim_start().len() >= 14 {
            return starts_with_doctype(prefix);
        }
        len = (len * 2).min(bytes.len());
    }
}
