//! Forward requests to another server (Remix's `fetch-proxy`).
//!
//! ```
//! use rmx_fetch::{response, Body, BoxError, Request, Response};
//! use rmx_fetch_proxy::{FetchProxy, FetchProxyOptions};
//!
//! // Any async function from Request to Response can do the forwarding; servers usually pass
//! // `rmx_serve::fetch`, an HTTP/1 client for `http://` targets.
//! async fn upstream(req: Request) -> Result<Response, BoxError> {
//!     Ok(response::text(format!("{} {}", req.method(), req.uri())))
//! }
//!
//! let proxy = FetchProxy::new("http://127.0.0.1:3000/app", upstream, FetchProxyOptions::default()).unwrap();
//! let req = http::Request::builder().uri("https://example.com/users?page=2").body(Body::empty()).unwrap();
//! let res = rmx_fetch::block_on(proxy.fetch(req)).unwrap();
//! assert_eq!(rmx_fetch::block_on(res.into_body().text()).unwrap(), "GET http://127.0.0.1:3000/app/users?page=2");
//! ```
//!
//! The request path and query are appended to the target URL. Connection-specific headers,
//! `Host`, `Accept-Encoding` and `Content-Length` are not forwarded, and `X-Forwarded-*`
//! headers can be set from the incoming URL. On the way back, `Set-Cookie` domains and paths are
//! rewritten for the client.
//!
//! Redirects reach the client unless the fetch function follows them. Unlike browser and
//! Node `fetch`, a Rust HTTP client usually hands back the body as sent, so
//! `Content-Encoding` is kept unless [`FetchProxyOptions::fetch_decodes_content`] says the
//! fetch function decoded it.

use std::sync::Arc;

use http::header::{self, HeaderName, HeaderValue};
use http::Method;
use rmx_fetch::{request_url, Body, BoxError, Request, Response};
use rmx_headers::SetCookie;
use rmx_url::Url;

/// Request headers that only apply to one connection (RFC 9110 section 7.6.1, plus the legacy
/// `Proxy-Connection`).
const HOP_BY_HOP: [&str; 9] = ["connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "proxy-connection", "te", "trailer", "transfer-encoding", "upgrade"];

/// What performs the forwarded request (Remix's `fetch` option); the request URI is absolute.
pub use rmx_fetch::{BoxFuture, Fetch};


/// Options for [`FetchProxy`] (Remix's `FetchProxyOptions`).
#[derive(Clone, Debug)]
pub struct FetchProxyOptions {
    /// Rewrite a `Set-Cookie` `Domain` to the incoming request's host (dropped for
    /// `localhost` and IP addresses). Default `true`.
    pub rewrite_cookie_domain: bool,
    /// Remove the target's path prefix from a `Set-Cookie` `Path`. Default `true`.
    pub rewrite_cookie_path: bool,
    /// Set `X-Forwarded-Proto`, `-Host` and `-Port` from the incoming URL, replacing any
    /// existing values and removing `Forwarded` and `X-Forwarded-For`. Default `false`, which
    /// passes existing forwarding headers through.
    pub x_forwarded_headers: bool,
    /// The fetch function returns decoded bodies, so `Content-Encoding` and `Content-Length`
    /// no longer describe them and are removed (what Remix does for the Fetch API's `fetch`).
    /// Default `false`.
    pub fetch_decodes_content: bool,
}

impl Default for FetchProxyOptions {
    fn default() -> Self {
        FetchProxyOptions { rewrite_cookie_domain: true, rewrite_cookie_path: true, x_forwarded_headers: false, fetch_decodes_content: false }
    }
}

/// Forwards requests to a target server (Remix's `createFetchProxy`). Cheap to clone.
#[derive(Clone)]
pub struct FetchProxy {
    target: Url,
    fetch: Arc<dyn Fetch>,
    options: FetchProxyOptions,
}

impl FetchProxy {
    /// A proxy to `target` (an absolute URL; a trailing slash is ignored).
    pub fn new(target: &str, fetch: impl Fetch, options: FetchProxyOptions) -> Result<FetchProxy, rmx_url::ParseError> {
        let mut target = Url::parse(target)?;
        let trimmed = target.pathname().trim_end_matches('/').to_string();
        target.set_pathname(if trimmed.is_empty() { "/" } else { &trimmed });
        Ok(FetchProxy { target, fetch: Arc::new(fetch), options })
    }

    /// Forward `request` and return the upstream response, rewritten for the client.
    pub async fn fetch(&self, request: Request) -> Result<Response, BoxError> {
        let (parts, body) = request.into_parts();
        let incoming = request_url(&parts);
        let method = parts.method.clone();
        let outgoing = self.proxy_request(Request::from_parts(parts, body));
        let response = self.fetch.fetch(outgoing).await?;
        Ok(self.proxy_response(&incoming, &method, response))
    }

    /// The target URL for an incoming URL: the target path followed by the request path,
    /// with the request's query.
    pub fn proxy_url(&self, incoming: &Url) -> Url {
        let mut url = self.target.clone();
        url.set_query(incoming.query());
        url.set_fragment(None);
        if incoming.pathname() != "/" {
            let path = if self.target.pathname() == "/" { incoming.pathname().to_string() } else { format!("{}{}", self.target.pathname(), incoming.pathname()) };
            url.set_pathname(&path);
        }
        url
    }

    /// The request sent upstream.
    pub fn proxy_request(&self, request: Request) -> Request {
        let (mut parts, body) = request.into_parts();
        let incoming = request_url(&parts);
        let headers = &mut parts.headers;
        let nominated: Vec<String> = headers
            .get_all(header::CONNECTION)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(',').map(|n| n.trim().to_ascii_lowercase()).collect::<Vec<_>>())
            .filter(|n| !n.is_empty())
            .collect();
        for name in nominated.iter().map(String::as_str).chain(HOP_BY_HOP).chain(["host", "accept-encoding", "content-length"]) {
            if let Ok(name) = HeaderName::from_bytes(name.as_bytes()) {
                headers.remove(name);
            }
        }
        if self.options.x_forwarded_headers {
            headers.remove(header::FORWARDED);
            headers.remove("x-forwarded-for");
            let port = incoming.port().map_or_else(|| if incoming.scheme() == "https" { "443".into() } else { "80".into() }, |p| p.to_string());
            for (name, value) in [("x-forwarded-proto", incoming.scheme().to_string()), ("x-forwarded-host", incoming.host()), ("x-forwarded-port", port)] {
                if let Ok(value) = HeaderValue::from_str(&value) {
                    headers.insert(name, value);
                }
            }
        }
        if let Ok(uri) = self.proxy_url(&incoming).href().parse() {
            parts.uri = uri;
        }
        let body = if parts.method == Method::GET || parts.method == Method::HEAD { Body::empty() } else { body };
        Request::from_parts(parts, body)
    }

    /// The response returned to the client for `response` to a `method` request made to
    /// `incoming`.
    pub fn proxy_response(&self, incoming: &Url, method: &Method, response: Response) -> Response {
        let (mut parts, body) = response.into_parts();
        let headers = &mut parts.headers;
        let has_transfer_encoding = headers.remove(header::TRANSFER_ENCODING).is_some();
        let has_body = *method != Method::HEAD && parts.status != http::StatusCode::NOT_MODIFIED;
        let decoded = self.options.fetch_decodes_content && has_body && headers.contains_key(header::CONTENT_ENCODING);
        if has_transfer_encoding || decoded {
            headers.remove(header::CONTENT_LENGTH);
        }
        if decoded {
            headers.remove(header::CONTENT_ENCODING);
        }
        let cookies: Vec<String> = headers.get_all(header::SET_COOKIE).iter().filter_map(|v| v.to_str().ok().map(str::to_string)).collect();
        if !cookies.is_empty() {
            headers.remove(header::SET_COOKIE);
            for cookie in cookies {
                let rewritten = match SetCookie::parse(&cookie) {
                    Some(set_cookie) => self.rewrite_cookie(set_cookie, incoming).to_string(),
                    None => cookie,
                };
                if let Ok(value) = HeaderValue::from_str(&rewritten) {
                    headers.append(header::SET_COOKIE, value);
                }
            }
        }
        Response::from_parts(parts, body)
    }

    fn rewrite_cookie(&self, mut cookie: SetCookie, incoming: &Url) -> SetCookie {
        if self.options.rewrite_cookie_domain && cookie.domain.is_some() {
            cookie.domain = cookie_domain(incoming.hostname());
        }
        if self.options.rewrite_cookie_path {
            if let Some(path) = &cookie.path {
                let prefix = self.target.pathname();
                if let Some(rest) = path.strip_prefix(prefix).filter(|rest| rest.starts_with('/')) {
                    cookie.path = Some(rest.to_string());
                } else if path == prefix {
                    cookie.path = Some("/".into());
                }
            }
        }
        cookie
    }
}

/// The cookie domain for a request host: none for `localhost` and IP addresses.
fn cookie_domain(hostname: &str) -> Option<String> {
    let ipv4 = hostname.split('.').count() == 4 && hostname.split('.').all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()));
    if hostname.eq_ignore_ascii_case("localhost") || ipv4 || hostname.contains(':') {
        return None;
    }
    Some(hostname.to_string())
}

#[cfg(test)]
mod tests;
