//! Response constructors (Remix's `remix/response/*`).

use http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};

use crate::{Body, Response};

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

/// A `text/html` response. Adds `<!DOCTYPE html>` when the body does not start with one.
pub fn html(body: impl Into<String>) -> Response {
    let mut body = body.into();
    if !starts_with_doctype(&body) {
        body.insert_str(0, DOCTYPE);
    }
    with_type(Body::from(body), "text/html; charset=UTF-8")
}

/// [`html`] with a status and headers (Remix's `createHtmlResponse(body, init)`). A
/// `Content-Type` in `init` replaces the HTML one.
pub fn html_with(body: impl Into<String>, init: impl Into<ResponseInit>) -> Response {
    init.into().apply(html(body))
}

/// A `text/html` response streamed from `body`. Adds `<!DOCTYPE html>` when the first chunk
/// does not start with one (an empty stream becomes just the doctype).
pub fn html_stream(body: Body) -> Response {
    let body = Body::from_task(move |tx| async move {
        let mut body = body;
        let first = match body.chunk().await {
            Some(Ok(chunk)) => chunk,
            Some(Err(_)) => return,
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
        while let Some(Ok(chunk)) = body.chunk().await {
            if !tx.send(chunk) {
                return;
            }
        }
    });
    with_type(body, "text/html; charset=UTF-8")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_on;

    #[test]
    fn html_adds_doctype_once() {
        let res = html("<p>x</p>");
        assert_eq!(res.headers()["content-type"], "text/html; charset=UTF-8");
        assert_eq!(block_on(res.into_body().text()).unwrap(), "<!DOCTYPE html><p>x</p>");
        assert_eq!(block_on(html("<!doctype html><p/>").into_body().text()).unwrap(), "<!doctype html><p/>");
    }

    fn chunked(parts: &'static [&'static str]) -> Body {
        Body::from_task(move |tx| async move {
            for part in parts {
                tx.send(*part);
            }
        })
    }

    #[test]
    fn html_stream_adds_doctype_once() {
        let res = html_stream(chunked(&["<p>", "x</p>"]));
        assert_eq!(res.headers()["content-type"], "text/html; charset=UTF-8");
        assert_eq!(block_on(res.into_body().text()).unwrap(), "<!DOCTYPE html><p>x</p>");
        let res = html_stream(chunked(&["<!doctype html>", "<p/>"]));
        assert_eq!(block_on(res.into_body().text()).unwrap(), "<!doctype html><p/>");
        let res = html_stream(chunked(&[]));
        assert_eq!(block_on(res.into_body().text()).unwrap(), "<!DOCTYPE html>");
    }

    #[test]
    fn redirects() {
        let res = redirect_with("/x", StatusCode::SEE_OTHER);
        assert_eq!(res.status(), 303);
        assert_eq!(res.headers()["location"], "/x");
    }

    // Ports of response/src/lib/redirect.test.ts.

    #[test]
    fn redirect_defaults_to_302() {
        let res = redirect("/home");
        assert_eq!((res.status(), res.headers()["location"].to_str().unwrap()), (StatusCode::FOUND, "/home"));
    }

    #[test]
    fn redirect_keeps_relative_urls_and_takes_a_url() {
        let res = redirect_with("../parent", StatusCode::SEE_OTHER);
        assert_eq!((res.status().as_u16(), res.headers()["location"].to_str().unwrap()), (303, "../parent"));
        let url = crate::Url::parse("https://example.com/login").unwrap();
        let res = redirect_with(&url.href(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!((res.status().as_u16(), res.headers()["location"].to_str().unwrap()), (301, "https://example.com/login"));
    }

    #[test]
    fn redirect_accepts_a_response_init() {
        let res = redirect_with("/dashboard", ResponseInit::status(StatusCode::TEMPORARY_REDIRECT).header("X-Redirect-Reason", "authentication"));
        assert_eq!(res.status(), 307);
        assert_eq!(res.headers()["location"], "/dashboard");
        assert_eq!(res.headers()["x-redirect-reason"], "authentication");
    }

    #[test]
    fn redirect_init_location_overrides_the_argument() {
        let res = redirect_with("/default", ResponseInit::default().header("Location", "/override"));
        assert_eq!((res.status(), res.headers()["location"].to_str().unwrap()), (StatusCode::FOUND, "/override"));
        let mut headers = HeaderMap::new();
        headers.insert(header::LOCATION, HeaderValue::from_static("/custom-location"));
        headers.insert("x-custom", HeaderValue::from_static("value"));
        let res = redirect_with("/default", ResponseInit { status: None, headers });
        assert_eq!(res.headers().get_all(header::LOCATION).iter().count(), 1);
        assert_eq!(res.headers()["location"], "/custom-location");
        assert_eq!(res.headers()["x-custom"], "value");
    }

    // Ports of response/src/lib/html.test.ts (init and stream sniffing).

    #[test]
    fn html_keeps_init_headers_and_status() {
        let res = html_with("<h1>Hello</h1>", ResponseInit::status(StatusCode::CREATED).header("X-Custom", "a"));
        assert_eq!(res.status(), 201);
        assert_eq!(res.headers()["content-type"], "text/html; charset=UTF-8");
        assert_eq!(res.headers()["x-custom"], "a");
        assert_eq!(block_on(res.into_body().text()).unwrap(), "<!DOCTYPE html><h1>Hello</h1>");
    }

    #[test]
    fn html_body_starting_with_multibyte_text_gets_a_doctype() {
        let res = html("日本語🙂<p>x</p>");
        assert_eq!(block_on(res.into_body().text()).unwrap(), "<!DOCTYPE html>日本語🙂<p>x</p>");
    }

    #[test]
    fn html_init_overrides_content_type() {
        let res = html_with("<h1>Hello</h1>", ResponseInit::default().header("Content-Type", "text/plain"));
        assert_eq!(res.headers().get_all(header::CONTENT_TYPE).iter().count(), 1);
        assert_eq!(res.headers()["content-type"], "text/plain");
    }

    fn one_chunk(content: String) -> Body {
        Body::from_task(move |tx| async move {
            tx.send(content);
        })
    }

    #[test]
    fn html_stream_sniffs_a_large_first_chunk_unchanged() {
        let content = format!("<!DOCTYPE html><p>{}</p>", "日本語🙂".repeat(5000));
        assert!(starts_with_doctype_bytes(content.as_bytes()));
        assert_eq!(block_on(html_stream(one_chunk(content.clone())).into_body().text()).unwrap(), content);
    }

    #[test]
    fn html_stream_finds_a_doctype_across_the_sniff_prefix() {
        let content = format!("{}<!doctype html><p>日本語🙂</p>", " ".repeat(60));
        assert_eq!(block_on(html_stream(one_chunk(content.clone())).into_body().text()).unwrap(), content);
        let content = format!("{}\u{2003}<!DOCTYPE html><p>日本語🙂</p>", " ".repeat(63));
        assert_eq!(block_on(html_stream(one_chunk(content.clone())).into_body().text()).unwrap(), content);
    }

    #[test]
    fn html_stream_adds_a_doctype_after_long_whitespace() {
        let content = format!("{}<p>日本語🙂</p>", "\u{2003}".repeat(50));
        assert_eq!(block_on(html_stream(one_chunk(content.clone())).into_body().text()).unwrap(), format!("<!DOCTYPE html>{content}"));
    }
}
