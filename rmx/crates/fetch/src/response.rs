//! Response constructors (Remix's `remix/response/*`).

use http::{header, HeaderValue, StatusCode};

use crate::{Body, Response};

const DOCTYPE: &str = "<!DOCTYPE html>";

/// A `text/html` response. Adds `<!DOCTYPE html>` when the body does not start with one.
pub fn html(body: impl Into<String>) -> Response {
    let mut body = body.into();
    if !starts_with_doctype(&body) {
        body.insert_str(0, DOCTYPE);
    }
    with_type(Body::from(body), "text/html; charset=UTF-8")
}

/// A `text/html` response streamed from `body`. The caller writes the doctype.
pub fn html_stream(body: Body) -> Response {
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

/// A redirect. `302 Found` unless another 3xx status is given (use 303 after a POST).
pub fn redirect(location: &str) -> Response {
    redirect_with(location, StatusCode::FOUND)
}

pub fn redirect_with(location: &str, status: StatusCode) -> Response {
    let mut res = Response::new(Body::empty());
    *res.status_mut() = status;
    if let Ok(v) = HeaderValue::from_str(location) {
        res.headers_mut().insert(header::LOCATION, v);
    }
    res
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
    t.len() >= 14 && t[..14].eq_ignore_ascii_case("<!doctype html")
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

    #[test]
    fn redirects() {
        let res = redirect_with("/x", StatusCode::SEE_OTHER);
        assert_eq!(res.status(), 303);
        assert_eq!(res.headers()["location"], "/x");
    }
}
