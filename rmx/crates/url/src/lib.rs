//! Minimal URL handling for rmx: parsing absolute URLs, resolving relative references,
//! percent-encoding, and an ordered `SearchParams` (the `URLSearchParams` web API).
//!
//! Scope is deliberately small: http(s) URLs as seen by a web server and browser runtime.
//! It does not implement IDNA, IPv6 zone ids, or every WHATWG edge case.

use std::fmt;

mod encode;
mod search;

pub use encode::{decode, decode_form, encode_component, encode_form, encode_path, encode_path_segment};
pub use search::SearchParams;

/// A parsed absolute URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    scheme: String,
    host: String,
    port: Option<u16>,
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

/// Why a URL could not be parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid URL: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

impl Url {
    /// Parse an absolute URL such as `https://example.com:8080/a/b?q=1#top`.
    pub fn parse(input: &str) -> Result<Url, ParseError> {
        let input = input.trim();
        let Some(colon) = input.find(':') else {
            return Err(ParseError(format!("{input:?} has no scheme")));
        };
        let scheme = &input[..colon];
        if scheme.is_empty() || !scheme.bytes().all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b)) {
            return Err(ParseError(format!("{input:?} has an invalid scheme")));
        }
        let rest = &input[colon + 1..];
        let Some(rest) = rest.strip_prefix("//") else {
            return Err(ParseError(format!("{input:?} has no authority")));
        };
        let auth_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..auth_end];
        // Drop userinfo; rmx never needs it.
        let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let (host, port) = split_host_port(authority)?;
        let mut url = Url {
            scheme: scheme.to_ascii_lowercase(),
            host: host.to_ascii_lowercase(),
            port,
            path: String::new(),
            query: None,
            fragment: None,
        };
        if url.port == default_port(&url.scheme) {
            url.port = None;
        }
        url.set_path_query_fragment(&rest[auth_end..]);
        if url.path.is_empty() {
            url.path.push('/');
        }
        Ok(url)
    }

    /// Resolve `reference` against this URL, like `new URL(reference, base)`.
    pub fn join(&self, reference: &str) -> Result<Url, ParseError> {
        let reference = reference.trim();
        if has_scheme(reference) {
            return Url::parse(reference);
        }
        let mut url = self.clone();
        if let Some(rest) = reference.strip_prefix("//") {
            return Url::parse(&format!("{}://{}", self.scheme, rest));
        }
        if reference.is_empty() {
            url.fragment = None;
            return Ok(url);
        }
        if let Some(fragment) = reference.strip_prefix('#') {
            url.fragment = Some(fragment.to_string());
            return Ok(url);
        }
        if reference.starts_with('?') {
            url.query = None;
            url.fragment = None;
            let path = url.path.clone();
            url.set_path_query_fragment(&format!("{path}{reference}"));
            return Ok(url);
        }
        let joined = if reference.starts_with('/') {
            reference.to_string()
        } else {
            let dir = &self.path[..self.path.rfind('/').map_or(0, |i| i + 1)];
            format!("{dir}{reference}")
        };
        url.query = None;
        url.fragment = None;
        url.set_path_query_fragment(&joined);
        Ok(url)
    }

    fn set_path_query_fragment(&mut self, s: &str) {
        let (before_fragment, fragment) = match s.find('#') {
            Some(i) => (&s[..i], Some(s[i + 1..].to_string())),
            None => (s, None),
        };
        let (path, query) = match before_fragment.find('?') {
            Some(i) => (&before_fragment[..i], Some(before_fragment[i + 1..].to_string())),
            None => (before_fragment, None),
        };
        self.path = normalize_path(path);
        self.query = query;
        self.fragment = fragment;
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// The hostname without the port.
    pub fn hostname(&self) -> &str {
        &self.host
    }

    /// The explicit port, or `None` when it is the scheme's default.
    pub fn port(&self) -> Option<u16> {
        self.port
    }

    /// `host:port` when a non-default port is set, otherwise the hostname.
    pub fn host(&self) -> String {
        match self.port {
            Some(p) => format!("{}:{p}", self.host),
            None => self.host.clone(),
        }
    }

    /// `scheme://host[:port]`
    pub fn origin(&self) -> String {
        format!("{}://{}", self.scheme, self.host())
    }

    /// The percent-encoded path, always starting with `/`.
    pub fn pathname(&self) -> &str {
        &self.path
    }

    pub fn set_pathname(&mut self, path: &str) {
        self.path = normalize_path(path);
        if self.path.is_empty() {
            self.path.push('/');
        }
    }

    /// The raw query string without `?`, if any.
    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    /// `?query`, or an empty string when there is no query (like `URL.search`).
    pub fn search(&self) -> String {
        match &self.query {
            Some(q) if !q.is_empty() => format!("?{q}"),
            _ => String::new(),
        }
    }

    pub fn set_query(&mut self, query: Option<&str>) {
        self.query = query.filter(|q| !q.is_empty()).map(str::to_string);
    }

    pub fn search_params(&self) -> SearchParams {
        SearchParams::parse(self.query.as_deref().unwrap_or(""))
    }

    pub fn set_search_params(&mut self, params: &SearchParams) {
        let s = params.to_string();
        self.set_query(Some(&s));
    }

    pub fn fragment(&self) -> Option<&str> {
        self.fragment.as_deref()
    }

    pub fn set_fragment(&mut self, fragment: Option<&str>) {
        self.fragment = fragment.map(str::to_string);
    }

    /// Path plus `?query`, the request target sent to a server.
    pub fn path_and_query(&self) -> String {
        format!("{}{}", self.path, self.search())
    }

    pub fn href(&self) -> String {
        let mut s = format!("{}{}", self.origin(), self.path_and_query());
        if let Some(f) = &self.fragment {
            s.push('#');
            s.push_str(f);
        }
        s
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.href())
    }
}

impl std::str::FromStr for Url {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Url, ParseError> {
        Url::parse(s)
    }
}

fn has_scheme(s: &str) -> bool {
    match s.find(':') {
        Some(i) if i > 0 => {
            let scheme = &s[..i];
            scheme.as_bytes()[0].is_ascii_alphabetic()
                && scheme.bytes().all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
                && !s[..i].contains('/')
        }
        _ => false,
    }
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        _ => None,
    }
}

fn split_host_port(authority: &str) -> Result<(&str, Option<u16>), ParseError> {
    if authority.starts_with('[') {
        // IPv6 literal
        let end = authority.find(']').ok_or_else(|| ParseError(format!("bad IPv6 host {authority:?}")))?;
        let host = &authority[..=end];
        let port = match &authority[end + 1..] {
            "" => None,
            p => Some(parse_port(p.trim_start_matches(':'))?),
        };
        return Ok((host, port));
    }
    match authority.rsplit_once(':') {
        Some((host, "")) => Ok((host, None)),
        Some((host, port)) => Ok((host, Some(parse_port(port)?))),
        None => Ok((authority, None)),
    }
}

fn parse_port(p: &str) -> Result<u16, ParseError> {
    p.parse().map_err(|_| ParseError(format!("bad port {p:?}")))
}

/// Resolve `.` and `..` segments and percent-encode characters that are not allowed raw.
fn normalize_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    let mut out: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').collect();
    let last = segments.len().saturating_sub(1);
    for (i, seg) in segments.iter().enumerate() {
        match *seg {
            "." | "%2e" | "%2E" => {
                if i == last {
                    out.push("");
                }
            }
            ".." | ".%2e" | "%2e." | "%2e%2e" | ".%2E" | "%2E." | "%2E%2E" => {
                if out.len() > 1 {
                    out.pop();
                }
                if i == last {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    let joined = if joined.starts_with('/') || joined.is_empty() { joined } else { format!("/{joined}") };
    encode::encode_path(&joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_parts() {
        let u = Url::parse("HTTPS://Example.com:8443/a/b?x=1&y=2#top").unwrap();
        assert_eq!(u.scheme(), "https");
        assert_eq!(u.hostname(), "example.com");
        assert_eq!(u.port(), Some(8443));
        assert_eq!(u.pathname(), "/a/b");
        assert_eq!(u.search(), "?x=1&y=2");
        assert_eq!(u.fragment(), Some("top"));
        assert_eq!(u.origin(), "https://example.com:8443");
        assert_eq!(Url::parse("http://x:80").unwrap().href(), "http://x/");
    }

    #[test]
    fn joins_references() {
        let base = Url::parse("https://example.com/admin/settings?tab=1").unwrap();
        assert_eq!(base.join("../users/1").unwrap().href(), "https://example.com/users/1");
        assert_eq!(base.join("profile").unwrap().href(), "https://example.com/admin/profile");
        assert_eq!(base.join("/x?y=1").unwrap().href(), "https://example.com/x?y=1");
        assert_eq!(base.join("?q=2").unwrap().href(), "https://example.com/admin/settings?q=2");
        assert_eq!(base.join("#h").unwrap().href(), "https://example.com/admin/settings?tab=1#h");
        assert_eq!(base.join("//cdn.test/a").unwrap().href(), "https://cdn.test/a");
        assert_eq!(base.join("http://other/").unwrap().origin(), "http://other");
    }

    #[test]
    fn normalizes_paths() {
        assert_eq!(Url::parse("http://x/a/./b/../c").unwrap().pathname(), "/a/c");
        assert_eq!(Url::parse("http://x/a b").unwrap().pathname(), "/a%20b");
        assert_eq!(Url::parse("http://x/caf%C3%A9").unwrap().pathname(), "/caf%C3%A9");
        assert_eq!(Url::parse("http://x/café").unwrap().pathname(), "/caf%C3%A9");
    }
}
