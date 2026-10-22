//! Cookies (Remix's `cookie` package): read this cookie's value from a `Cookie` header and
//! write `Set-Cookie` headers, with optional signing and secret rotation.
//!
//! ```
//! use rmx_cookie::{Cookie, CookieProperties};
//!
//! let auth = Cookie::new("auth").http_only(true);
//! let set = auth.serialize("1");
//! assert_eq!(set, "auth=MQ==; HttpOnly; Path=/; SameSite=Lax");
//! assert_eq!(auth.parse(Some("theme=dark; auth=MQ==")), Some("1".to_string()));
//! let cleared = auth.serialize_with("", &CookieProperties { max_age: Some(0), ..Default::default() });
//! assert_eq!(cleared, "auth=; HttpOnly; Max-Age=0; Path=/; SameSite=Lax");
//! ```
//!
//! Like Remix, the default codec is base64 of the UTF-8 bytes, so any string round-trips;
//! [`Cookie::encode`] and [`Cookie::decode`] replace it. With [`Cookie::secrets`] the encoded
//! value is signed (`value.signature`, HMAC-SHA-256 in unpadded base64) with the first secret,
//! and [`Cookie::parse`] accepts a signature from any of them.

use std::fmt;
use std::sync::Arc;
use std::time::SystemTime;

pub use rmx_headers::SameSite;
use rmx_headers::SetCookie;

type Encoder = Arc<dyn Fn(&str) -> String + Send + Sync>;
type Decoder = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// `Set-Cookie` attributes (Remix's `CookieProperties`). Unset fields keep the cookie's own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CookieProperties {
    pub domain: Option<String>,
    pub expires: Option<SystemTime>,
    pub http_only: Option<bool>,
    /// Lifetime in seconds.
    pub max_age: Option<i64>,
    pub partitioned: Option<bool>,
    pub path: Option<String>,
    pub same_site: Option<SameSite>,
    pub secure: Option<bool>,
}

/// A named cookie: its attributes, value codec and signing secrets (Remix's `Cookie`).
#[derive(Clone)]
pub struct Cookie {
    name: String,
    encode: Encoder,
    decode: Decoder,
    secrets: Vec<String>,
    domain: Option<String>,
    expires: Option<SystemTime>,
    http_only: Option<bool>,
    max_age: Option<i64>,
    partitioned: Option<bool>,
    path: String,
    same_site: SameSite,
    secure: Option<bool>,
}

impl fmt::Debug for Cookie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cookie")
            .field("name", &self.name)
            .field("signed", &self.is_signed())
            .field("domain", &self.domain)
            .field("expires", &self.expires)
            .field("http_only", &self.http_only)
            .field("max_age", &self.max_age)
            .field("partitioned", &self.partitioned)
            .field("path", &self.path)
            .field("same_site", &self.same_site)
            .field("secure", &self.secure)
            .finish_non_exhaustive()
    }
}

impl Cookie {
    /// A cookie with Remix's defaults: `Path=/`, `SameSite=Lax` and the base64 codec.
    pub fn new(name: impl Into<String>) -> Cookie {
        Cookie {
            name: name.into(),
            encode: Arc::new(default_encode),
            decode: Arc::new(default_decode),
            secrets: Vec::new(),
            domain: None,
            expires: None,
            http_only: None,
            max_age: None,
            partitioned: None,
            path: "/".into(),
            same_site: SameSite::Lax,
            secure: None,
        }
    }

    pub fn path(mut self, path: impl Into<String>) -> Cookie {
        self.path = path.into();
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

    pub fn expires(mut self, at: SystemTime) -> Cookie {
        self.expires = Some(at);
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
        self.same_site = same_site;
        self
    }

    /// A partitioned cookie is always `Secure`.
    pub fn partitioned(mut self, on: bool) -> Cookie {
        self.partitioned = Some(on);
        self
    }

    /// Sign values with the first secret and accept values signed with any of them (put a
    /// new secret first to rotate).
    pub fn secrets(mut self, secrets: Vec<String>) -> Cookie {
        self.secrets = secrets;
        self
    }

    /// Turn a value into the cookie value (before signing). It must return text that is
    /// safe in a `Set-Cookie` header.
    pub fn encode(mut self, encode: impl Fn(&str) -> String + Send + Sync + 'static) -> Cookie {
        self.encode = Arc::new(encode);
        self
    }

    /// Turn a cookie value (after verifying) back into the value; `None` makes
    /// [`Cookie::parse`] return `None`, like a throwing decoder in Remix.
    pub fn decode(mut self, decode: impl Fn(&str) -> Option<String> + Send + Sync + 'static) -> Cookie {
        self.decode = Arc::new(decode);
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Remix's `signed`: whether values are signed.
    pub fn is_signed(&self) -> bool {
        !self.secrets.is_empty()
    }

    pub fn get_domain(&self) -> Option<&str> {
        self.domain.as_deref()
    }

    pub fn get_expires(&self) -> Option<SystemTime> {
        self.expires
    }

    /// `HttpOnly` as configured; `None` when never set, so middleware can pick a default.
    pub fn get_http_only(&self) -> Option<bool> {
        self.http_only
    }

    pub fn get_max_age(&self) -> Option<i64> {
        self.max_age
    }

    pub fn get_partitioned(&self) -> bool {
        self.partitioned.unwrap_or(false)
    }

    pub fn get_path(&self) -> &str {
        &self.path
    }

    pub fn get_same_site(&self) -> SameSite {
        self.same_site
    }

    /// `Secure` as configured (always `Some(true)` when partitioned); `None` when never set.
    pub fn get_secure(&self) -> Option<bool> {
        if self.partitioned == Some(true) {
            return Some(true);
        }
        self.secure
    }

    /// This cookie's value in a `Cookie` request header: `None` when the header or the
    /// cookie is missing, the signature does not verify, or the value does not decode.
    pub fn parse(&self, header: Option<&str>) -> Option<String> {
        let header = header.filter(|h| !h.is_empty())?;
        let cookies = rmx_headers::Cookie::parse(Some(header));
        let value = cookies.get(&self.name)?;
        if value.is_empty() {
            return Some(String::new());
        }
        if self.secrets.is_empty() {
            return (self.decode)(value);
        }
        let unsigned = self.secrets.iter().find_map(|secret| unsign(value, secret))?;
        (self.decode)(unsigned)
    }

    /// A `Set-Cookie` header value.
    pub fn serialize(&self, value: &str) -> String {
        self.serialize_with(value, &CookieProperties::default())
    }

    /// A `Set-Cookie` header value with some attributes replaced.
    pub fn serialize_with(&self, value: &str, props: &CookieProperties) -> String {
        let value = if value.is_empty() {
            String::new()
        } else {
            let encoded = (self.encode)(value);
            match self.secrets.first() {
                Some(secret) => sign(&encoded, secret),
                None => encoded,
            }
        };
        let header = SetCookie {
            name: self.name.clone(),
            value,
            domain: props.domain.clone().or_else(|| self.domain.clone()),
            expires: props.expires.or(self.expires),
            http_only: props.http_only.or(self.http_only).unwrap_or(false),
            max_age: props.max_age.or(self.max_age),
            partitioned: props.partitioned.or(self.partitioned).unwrap_or(false),
            path: Some(props.path.clone().unwrap_or_else(|| self.path.clone())),
            same_site: Some(props.same_site.unwrap_or(self.same_site)),
            secure: props.secure.or(self.get_secure()).unwrap_or(false),
        };
        header.to_string()
    }
}

/// Remix's default encoder: base64 of the UTF-8 bytes, padded like `btoa`.
fn default_encode(value: &str) -> String {
    let mut encoded = rmx_crypto::base64_encode(value.as_bytes());
    while !encoded.len().is_multiple_of(4) {
        encoded.push('=');
    }
    encoded
}

/// Remix's default decoder: base64 to UTF-8, `None` when either step fails.
fn default_decode(value: &str) -> Option<String> {
    String::from_utf8(rmx_crypto::base64_decode(value)?).ok()
}

/// `value.signature`, the signature being HMAC-SHA-256 of the value in unpadded base64.
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
