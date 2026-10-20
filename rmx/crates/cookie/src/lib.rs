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
//! Values are percent-encoded, so any string round-trips. With [`Cookie::secrets`] the
//! value is signed (HMAC-SHA-256, `value.signature`) and [`Cookie::parse`] only returns
//! values signed by one of the secrets, which allows rotating them.

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
    http_only: Option<bool>,
    secure: Option<bool>,
    same_site: Option<SameSite>,
    secrets: Vec<String>,
}

impl Cookie {
    pub fn new(name: impl Into<String>) -> Cookie {
        Cookie {
            name: name.into(),
            path: None,
            domain: None,
            max_age: None,
            http_only: None,
            secure: None,
            same_site: None,
            secrets: Vec::new(),
        }
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
        self.http_only = Some(on);
        self
    }

    pub fn secure(mut self, on: bool) -> Cookie {
        self.secure = Some(on);
        self
    }

    pub fn same_site(mut self, same_site: SameSite) -> Cookie {
        self.same_site = Some(same_site);
        self
    }

    /// Sign values with the first secret and accept values signed with any of them (put a
    /// new secret first to rotate).
    pub fn secrets(mut self, secrets: Vec<String>) -> Cookie {
        self.secrets = secrets;
        self
    }

    pub fn is_signed(&self) -> bool {
        !self.secrets.is_empty()
    }

    /// `HttpOnly` as configured; `None` when never set, so middleware can pick a default.
    pub fn get_http_only(&self) -> Option<bool> {
        self.http_only
    }

    /// `Secure` as configured; `None` when never set.
    pub fn get_secure(&self) -> Option<bool> {
        self.secure
    }

    pub fn get_max_age(&self) -> Option<i64> {
        self.max_age
    }

    /// This cookie's value in a `Cookie` request header. A signed cookie whose signature
    /// does not verify reads as `None`.
    pub fn parse(&self, header: Option<&str>) -> Option<String> {
        let raw = parse_raw(header?).into_iter().find(|(k, _)| *k == self.name).map(|(_, v)| v)?;
        if raw.is_empty() {
            return Some(String::new());
        }
        if self.secrets.is_empty() {
            return Some(rmx_url::decode(raw));
        }
        self.secrets.iter().find_map(|secret| unsign(raw, secret)).map(rmx_url::decode)
    }

    /// A `Set-Cookie` header value.
    pub fn serialize(&self, value: &str) -> String {
        self.write(value, self.max_age)
    }

    /// A `Set-Cookie` header value with `HttpOnly` and `Secure` overridden.
    pub fn serialize_with(&self, value: &str, http_only: bool, secure: bool) -> String {
        Cookie { http_only: Some(http_only), secure: Some(secure), ..self.clone() }.serialize(value)
    }

    /// A `Set-Cookie` header value that deletes the cookie.
    pub fn clear(&self) -> String {
        self.write("", Some(0))
    }

    fn write(&self, value: &str, max_age: Option<i64>) -> String {
        let mut encoded = rmx_url::encode_component(value);
        if let (Some(secret), false) = (self.secrets.first(), value.is_empty()) {
            encoded = sign(&encoded, secret);
        }
        let mut out = format!("{}={}", self.name, encoded);
        if let Some(p) = &self.path {
            let _ = write!(out, "; Path={p}");
        }
        if let Some(d) = &self.domain {
            let _ = write!(out, "; Domain={d}");
        }
        if let Some(m) = max_age {
            let _ = write!(out, "; Max-Age={m}");
        }
        if self.http_only == Some(true) {
            out.push_str("; HttpOnly");
        }
        if self.secure == Some(true) {
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
    parse_raw(header).into_iter().map(|(k, v)| (k, rmx_url::decode(v))).collect()
}

fn parse_raw(header: &str) -> Vec<(&str, &str)> {
    header
        .split(';')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((k.trim(), v.trim().trim_matches('"')))
        })
        .collect()
}

/// `value.signature`, the signature being base64 HMAC-SHA-256 of the value.
pub fn sign(value: &str, secret: &str) -> String {
    let mac = rmx_crypto::hmac_sha256(secret.as_bytes(), value.as_bytes());
    format!("{value}.{}", rmx_crypto::base64_encode(&mac))
}

/// The value of a signed `value.signature` string, if the signature matches `secret`.
pub fn unsign<'a>(signed: &'a str, secret: &str) -> Option<&'a str> {
    let (value, signature) = signed.rsplit_once('.')?;
    let given = rmx_crypto::base64_decode(signature)?;
    let expected = rmx_crypto::hmac_sha256(secret.as_bytes(), value.as_bytes());
    rmx_crypto::constant_time_eq(&given, &expected).then_some(value)
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

    #[test]
    fn signed_cookies_verify_and_rotate() {
        let old = Cookie::new("s").secrets(vec!["old".into()]);
        let set = old.serialize("hello world");
        let pair = set.split(';').next().unwrap();
        assert!(pair.starts_with("s=hello%20world."), "{pair}");
        assert_eq!(old.parse(Some(pair)), Some("hello world".into()));

        let rotated = Cookie::new("s").secrets(vec!["new".into(), "old".into()]);
        assert_eq!(rotated.parse(Some(pair)), Some("hello world".into()), "old signatures still verify");
        assert!(rotated.serialize("x").starts_with(&format!("s={}", sign("x", "new"))));

        let other = Cookie::new("s").secrets(vec!["other".into()]);
        assert_eq!(other.parse(Some(pair)), None, "wrong secret");
        let tampered = pair.replace("hello", "jello");
        assert_eq!(old.parse(Some(&tampered)), None, "tampered value");
        assert_eq!(old.parse(Some("s=unsigned")), None);
        assert_eq!(old.parse(Some("s=")), Some(String::new()));
    }
}
