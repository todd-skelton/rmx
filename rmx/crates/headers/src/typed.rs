//! Typed access to an [`http::HeaderMap`], Remix's `SuperHeaders`. Remix subclasses
//! `Headers` with lazy, cached property accessors whose mutations write back through proxies;
//! in Rust the map stays the plain `HeaderMap` every API takes, and this trait reads a header
//! into its typed value, writes one back, or edits one in place with [`SuperHeaders::update`].
//!
//! ```
//! use http::HeaderMap;
//! use rmx_headers::{ContentType, SuperHeaders, Vary};
//!
//! let mut headers = HeaderMap::new();
//! headers.update(|t: &mut ContentType| t.media_type = Some("text/html".into())).unwrap();
//! headers.update(|v: &mut Vary| v.add("Accept-Encoding")).unwrap();
//! assert_eq!(headers.typed::<ContentType>().media_type.as_deref(), Some("text/html"));
//! assert_eq!(headers.get_joined("vary").as_deref(), Some("accept-encoding"));
//! ```

use std::fmt;
use std::time::SystemTime;

use http::header::{AsHeaderName, HeaderMap, HeaderName, HeaderValue, InvalidHeaderValue, COOKIE, SET_COOKIE, VARY};

use crate::{
    format_http_date, parse_http_date, quote_etag, Accept, AcceptEncoding, AcceptLanguage, CacheControl,
    ContentDisposition, ContentRange, ContentType, Cookie, IfMatch, IfNoneMatch, IfRange, Range, SetCookie, Vary,
};

/// A header value with a typed form: parsed from the header's text (`None` when absent)
/// and printed back with `Display`, where empty text means no header.
pub trait TypedHeader: fmt::Display + Sized {
    /// The lowercase header name.
    const NAME: &'static str;

    fn from_header(value: Option<&str>) -> Self;
}

macro_rules! typed_header {
    ($($ty:ty => $name:literal),* $(,)?) => {$(
        impl TypedHeader for $ty {
            const NAME: &'static str = $name;

            fn from_header(value: Option<&str>) -> Self {
                <$ty>::parse(value)
            }
        }
    )*};
}

typed_header! {
    Accept => "accept",
    AcceptEncoding => "accept-encoding",
    AcceptLanguage => "accept-language",
    CacheControl => "cache-control",
    ContentDisposition => "content-disposition",
    ContentType => "content-type",
    Cookie => "cookie",
    IfMatch => "if-match",
    IfNoneMatch => "if-none-match",
    IfRange => "if-range",
    Range => "range",
    Vary => "vary",
}

impl TypedHeader for ContentRange {
    const NAME: &'static str = "content-range";

    /// Text that does not parse reads as an empty `Content-Range`, like Remix's.
    fn from_header(value: Option<&str>) -> Self {
        ContentRange::parse(value).unwrap_or_default()
    }
}

/// Headers that [`SuperHeaders::apply`] appends to instead of replacing: the typed lists
/// Remix appends and every comma-list string header.
const APPEND_ON_APPLY: &[&str] = &[
    "accept",
    "accept-encoding",
    "accept-language",
    "cache-control",
    "if-match",
    "if-none-match",
    "accept-charset",
    "accept-patch",
    "accept-post",
    "accept-ranges",
    "access-control-allow-headers",
    "access-control-allow-methods",
    "access-control-expose-headers",
    "access-control-request-headers",
    "allow",
    "connection",
    "content-encoding",
    "content-language",
    "link",
    "permissions-policy",
    "pragma",
    "prefer",
    "preference-applied",
    "tracestate",
    "via",
    "www-authenticate",
    "x-forwarded-for",
    "x-robots-tag",
];

/// Typed reads and writes on a header map. Every method works on the plain map, so other
/// code sees each change at once and nothing is cached.
pub trait SuperHeaders {
    /// `Headers.get`: every value of `name` joined with `, `, or `None` when absent.
    fn get_joined<K: AsHeaderName>(&self, name: K) -> Option<String>;

    /// The header parsed into its typed value (an empty value when the header is absent).
    fn typed<H: TypedHeader>(&self) -> H;

    /// Replace the header with `value`, or remove it when `value` prints empty.
    fn set_typed<H: TypedHeader>(&mut self, value: &H) -> Result<(), InvalidHeaderValue>;

    /// Read the typed value, change it with `edit` and write it back (removing the header
    /// when it prints empty), like mutating a `SuperHeaders` property.
    fn update<H: TypedHeader, R>(&mut self, edit: impl FnOnce(&mut H) -> R) -> Result<R, InvalidHeaderValue> {
        let mut value = self.typed::<H>();
        let result = edit(&mut value);
        self.set_typed(&value)?;
        Ok(result)
    }

    /// A numeric header (`Content-Length`, `Age`, ...) read like `parseInt`.
    fn number<K: AsHeaderName>(&self, name: K) -> Option<i64>;

    /// A date header (`Date`, `Last-Modified`, ...), `None` when absent or not a date.
    fn date<K: AsHeaderName>(&self, name: K) -> Option<SystemTime>;

    /// Set a date header in HTTP date format.
    fn set_date(&mut self, name: HeaderName, at: SystemTime);

    /// Set a list header (`Allow`, `Vary`-like strings, `X-Forwarded-For`) to its values
    /// joined with `, `.
    fn set_list<I, S>(&mut self, name: HeaderName, values: I) -> Result<(), InvalidHeaderValue>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>;

    /// Set `ETag`, quoting a bare tag.
    fn set_etag(&mut self, tag: &str) -> Result<(), InvalidHeaderValue>;

    /// Every `Set-Cookie` header, parsed, in order.
    fn set_cookies(&self) -> Vec<SetCookie>;

    /// Replace every `Set-Cookie` header with `cookies` (unnamed ones are skipped).
    fn replace_set_cookies(&mut self, cookies: &[SetCookie]) -> Result<(), InvalidHeaderValue>;

    /// Merge `other` into these headers the way `SuperHeaders.apply` does: `Set-Cookie`
    /// values are added, `Cookie` pairs and `Vary` names are merged, list headers are
    /// appended to (empty values skipped), and any other header is replaced.
    fn apply(&mut self, other: &HeaderMap) -> &mut Self;

    /// Merge one typed value with the same rules as [`SuperHeaders::apply`]; a value that
    /// prints empty removes a header that would be replaced.
    fn apply_typed<H: TypedHeader>(&mut self, value: &H) -> Result<&mut Self, InvalidHeaderValue>;
}

fn text(value: &HeaderValue) -> String {
    String::from_utf8_lossy(value.as_bytes()).into_owned()
}

fn header_value(text: &str) -> Result<HeaderValue, InvalidHeaderValue> {
    HeaderValue::from_bytes(text.as_bytes())
}

impl SuperHeaders for HeaderMap {
    fn get_joined<K: AsHeaderName>(&self, name: K) -> Option<String> {
        let values: Vec<String> = self.get_all(name).iter().map(text).collect();
        (!values.is_empty()).then(|| values.join(", "))
    }

    fn typed<H: TypedHeader>(&self) -> H {
        H::from_header(self.get_joined(H::NAME).as_deref())
    }

    fn set_typed<H: TypedHeader>(&mut self, value: &H) -> Result<(), InvalidHeaderValue> {
        let name = HeaderName::from_static(H::NAME);
        let printed = value.to_string();
        if printed.is_empty() {
            self.remove(name);
        } else {
            self.insert(name, header_value(&printed)?);
        }
        Ok(())
    }

    fn number<K: AsHeaderName>(&self, name: K) -> Option<i64> {
        crate::params::parse_int(&self.get_joined(name)?)
    }

    fn date<K: AsHeaderName>(&self, name: K) -> Option<SystemTime> {
        parse_http_date(&self.get_joined(name)?)
    }

    fn set_date(&mut self, name: HeaderName, at: SystemTime) {
        let formatted = format_http_date(at);
        self.insert(name, HeaderValue::from_str(&formatted).expect("HTTP dates are ASCII"));
    }

    fn set_list<I, S>(&mut self, name: HeaderName, values: I) -> Result<(), InvalidHeaderValue>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let joined: Vec<String> = values.into_iter().map(|v| v.as_ref().to_string()).collect();
        self.insert(name, header_value(&joined.join(", "))?);
        Ok(())
    }

    fn set_etag(&mut self, tag: &str) -> Result<(), InvalidHeaderValue> {
        self.insert(http::header::ETAG, header_value(&quote_etag(tag))?);
        Ok(())
    }

    fn set_cookies(&self) -> Vec<SetCookie> {
        self.get_all(SET_COOKIE).iter().filter_map(|v| SetCookie::parse(&text(v))).collect()
    }

    fn replace_set_cookies(&mut self, cookies: &[SetCookie]) -> Result<(), InvalidHeaderValue> {
        let values = cookies
            .iter()
            .map(ToString::to_string)
            .filter(|v| !v.is_empty())
            .map(|v| header_value(&v))
            .collect::<Result<Vec<_>, _>>()?;
        self.remove(SET_COOKIE);
        for value in values {
            self.append(SET_COOKIE, value);
        }
        Ok(())
    }

    fn apply(&mut self, other: &HeaderMap) -> &mut Self {
        for name in other.keys() {
            if name == SET_COOKIE {
                for value in other.get_all(name) {
                    apply_value(self, name, &text(value));
                }
            } else if let Some(joined) = other.get_joined(name) {
                apply_value(self, name, &joined);
            }
        }
        self
    }

    fn apply_typed<H: TypedHeader>(&mut self, value: &H) -> Result<&mut Self, InvalidHeaderValue> {
        let name = HeaderName::from_static(H::NAME);
        let printed = value.to_string();
        if replaces(&name) {
            self.set_typed(value)?;
        } else {
            header_value(&printed)?;
            apply_value(self, &name, &printed);
        }
        Ok(self)
    }
}

/// Whether `apply` replaces this header rather than merging into it.
fn replaces(name: &HeaderName) -> bool {
    name != SET_COOKIE && name != COOKIE && name != VARY && !APPEND_ON_APPLY.contains(&name.as_str())
}

fn apply_value(headers: &mut HeaderMap, name: &HeaderName, value: &str) {
    let Ok(header) = header_value(value) else { return };
    if name == COOKIE {
        let incoming = Cookie::parse(Some(value));
        if incoming.is_empty() {
            return;
        }
        let mut current: Cookie = headers.typed();
        for (cookie, cookie_value) in incoming.iter() {
            current.append(cookie, cookie_value);
        }
        let _ = headers.set_typed(&current);
    } else if name == VARY {
        let incoming = Vary::parse(Some(value));
        if incoming.is_empty() {
            return;
        }
        let mut current: Vary = headers.typed();
        for header_name in incoming.iter() {
            current.add(header_name);
        }
        let _ = headers.set_typed(&current);
    } else if replaces(name) {
        headers.insert(name.clone(), header);
    } else if !value.is_empty() {
        headers.append(name.clone(), header);
    }
}
