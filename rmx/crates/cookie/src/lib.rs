//! Cookies (Remix's `cookie` package): read a value from a `Cookie` header and write
//! `Set-Cookie` headers.
//!
//! ```
//! use rmx_cookie::{Cookie, SameSite};
//!
//! let auth = Cookie::new("auth").http_only(true).same_site(SameSite::Lax).path("/");
//! assert_eq!(auth.parse(Some("theme=dark; auth=1")), Some("1".to_string()));
//! assert_eq!(auth.serialize("1"), "auth=1; Path=/; HttpOnly; SameSite=Lax");
//! assert_eq!(auth.clear(), "auth=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax");
//! ```
//!
//! Values are percent-encoded, so any string round-trips.

use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SameSite {
    Strict,
    Lax,
    None,
}

/// A named cookie and the attributes it is written with.
#[derive(Clone, Debug)]
pub struct Cookie {
    name: String,
    path: Option<String>,
    domain: Option<String>,
    max_age: Option<i64>,
    http_only: bool,
    secure: bool,
    same_site: Option<SameSite>,
}

impl Cookie {
    pub fn new(name: impl Into<String>) -> Cookie {
        Cookie { name: name.into(), path: None, domain: None, max_age: None, http_only: false, secure: false, same_site: None }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(mut self, path: impl Into<String>) -> Cookie {
        self.path = Some(path.into());
        self
    }

    pub fn domain(mut self, domain: impl Into<String>) -> Cookie {
        self.domain = Some(domain.into());
        self
    }

    /// Lifetime in seconds.
    pub fn max_age(mut self, seconds: i64) -> Cookie {
        self.max_age = Some(seconds);
        self
    }

    pub fn http_only(mut self, on: bool) -> Cookie {
        self.http_only = on;
        self
    }

    pub fn secure(mut self, on: bool) -> Cookie {
        self.secure = on;
        self
    }

    pub fn same_site(mut self, same_site: SameSite) -> Cookie {
        self.same_site = Some(same_site);
        self
    }

    /// This cookie's value in a `Cookie` request header.
    pub fn parse(&self, header: Option<&str>) -> Option<String> {
        parse(header?).into_iter().find(|(k, _)| *k == self.name).map(|(_, v)| v)
    }

    /// A `Set-Cookie` header value.
    pub fn serialize(&self, value: &str) -> String {
        self.write(value, self.max_age)
    }

    /// A `Set-Cookie` header value that deletes the cookie.
    pub fn clear(&self) -> String {
        self.write("", Some(0))
    }

    fn write(&self, value: &str, max_age: Option<i64>) -> String {
        let mut out = format!("{}={}", self.name, rmx_url::encode_component(value));
        if let Some(p) = &self.path {
            let _ = write!(out, "; Path={p}");
        }
        if let Some(d) = &self.domain {
            let _ = write!(out, "; Domain={d}");
        }
        if let Some(m) = max_age {
            let _ = write!(out, "; Max-Age={m}");
        }
        if self.http_only {
            out.push_str("; HttpOnly");
        }
        if self.secure {
            out.push_str("; Secure");
        }
        match self.same_site {
            Some(SameSite::Strict) => out.push_str("; SameSite=Strict"),
            Some(SameSite::Lax) => out.push_str("; SameSite=Lax"),
            Some(SameSite::None) => out.push_str("; SameSite=None"),
            None => {}
        }
        out
    }
}

/// Every `name=value` pair in a `Cookie` header, values decoded.
pub fn parse(header: &str) -> Vec<(&str, String)> {
    header
        .split(';')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            let v = v.trim().trim_matches('"');
            Some((k.trim(), rmx_url::decode(v)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_encoded_values() {
        let c = Cookie::new("prefs").max_age(60).secure(true);
        let set = c.serialize("a b;c=d");
        assert_eq!(set, "prefs=a%20b%3Bc%3Dd; Max-Age=60; Secure");
        let pair = set.split(';').next().unwrap();
        assert_eq!(c.parse(Some(&format!("x=1; {pair}"))), Some("a b;c=d".into()));
        assert_eq!(c.parse(Some("x=1")), None);
        assert_eq!(c.parse(None), None);
    }
}
