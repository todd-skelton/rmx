//! `If-Match`, `If-None-Match` and `If-Range`.

use std::fmt;
use std::time::SystemTime;

use crate::date::{format_http_date, parse_http_date, unix_seconds};
use crate::params::{quote_etag, split_list};

/// `If-None-Match`: weak comparison, `*` matches anything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IfNoneMatch {
    pub tags: Vec<String>,
}

impl IfNoneMatch {
    pub fn parse(value: Option<&str>) -> IfNoneMatch {
        IfNoneMatch { tags: value.map(|v| split_list(v).into_iter().map(quote_etag).collect()).unwrap_or_default() }
    }

    pub fn has(&self, tag: &str) -> bool {
        self.tags.contains(&quote_etag(tag))
    }

    /// Weak comparison (RFC 9110 13.1.2): `W/"a"` and `"a"` are the same tag here.
    pub fn matches(&self, tag: &str) -> bool {
        let tag = quote_etag(tag);
        let opaque = tag.strip_prefix("W/").unwrap_or(&tag);
        self.tags.iter().any(|t| t == "*" || t.strip_prefix("W/").unwrap_or(t) == opaque)
    }
}

/// Tags from a list (Remix's array init), quoting bare ones.
impl<S: AsRef<str>> FromIterator<S> for IfNoneMatch {
    fn from_iter<I: IntoIterator<Item = S>>(tags: I) -> IfNoneMatch {
        IfNoneMatch { tags: tags.into_iter().map(|t| quote_etag(t.as_ref())).collect() }
    }
}

impl fmt::Display for IfNoneMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.tags.join(", "))
    }
}

/// `If-Match`: strong comparison only; an empty header matches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IfMatch {
    pub tags: Vec<String>,
}

impl IfMatch {
    pub fn parse(value: Option<&str>) -> IfMatch {
        IfMatch { tags: value.map(|v| split_list(v).into_iter().map(quote_etag).collect()).unwrap_or_default() }
    }

    pub fn has(&self, tag: &str) -> bool {
        self.tags.contains(&quote_etag(tag))
    }

    pub fn matches(&self, tag: &str) -> bool {
        if self.tags.is_empty() || self.tags.iter().any(|t| t == "*") {
            return true;
        }
        let tag = quote_etag(tag);
        !tag.starts_with("W/") && self.tags.iter().any(|t| !t.starts_with("W/") && *t == tag)
    }
}

/// Tags from a list (Remix's array init), quoting bare ones.
impl<S: AsRef<str>> FromIterator<S> for IfMatch {
    fn from_iter<I: IntoIterator<Item = S>>(tags: I) -> IfMatch {
        IfMatch { tags: tags.into_iter().map(|t| quote_etag(t.as_ref())).collect() }
    }
}

impl fmt::Display for IfMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.tags.join(", "))
    }
}

/// `If-Range`: an HTTP date or a strong entity tag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IfRange {
    pub value: String,
}

/// An HTTP date (Remix's `Date` init).
impl From<SystemTime> for IfRange {
    fn from(date: SystemTime) -> IfRange {
        IfRange { value: format_http_date(date) }
    }
}

impl fmt::Display for IfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

impl IfRange {
    pub fn parse(value: Option<&str>) -> IfRange {
        IfRange { value: value.unwrap_or_default().to_string() }
    }

    /// Whether a range request may be served for a resource with this ETag and
    /// modification time (RFC 9110 13.1.5). An absent or empty header always matches. The
    /// value is classified by its syntax: a quoted strong entity tag matches the identical
    /// strong ETag, an HTTP date matches the same modification second, and anything else
    /// (weak or bare tags, malformed values) never matches.
    pub fn matches(&self, etag: Option<&str>, last_modified: Option<SystemTime>) -> bool {
        if self.value.is_empty() {
            return true;
        }
        if is_strong_entity_tag(&self.value) {
            return etag.is_some_and(|etag| is_strong_entity_tag(etag) && etag == self.value);
        }
        match parse_http_date(&self.value) {
            Some(date) => last_modified.is_some_and(|modified| unix_seconds(date) == unix_seconds(modified)),
            None => false,
        }
    }
}

/// A quoted opaque tag without `W/` (RFC 9110 8.8.3): `"` then etagc characters then `"`.
fn is_strong_entity_tag(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2
        && bytes[0] == b'"'
        && bytes[bytes.len() - 1] == b'"'
        && bytes[1..bytes.len() - 1].iter().all(|&b| b == 0x21 || (0x23..=0x7e).contains(&b) || b >= 0x80)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    #[test]
    fn if_none_match_is_weak() {
        let h = IfNoneMatch::parse(Some(r#"W/"a", "b""#));
        assert!(h.matches(r#"W/"a""#) && h.matches("b") && !h.matches("c"));
        // if-none-match.test.ts (remix 3.1): strong and weak forms of a tag are equivalent.
        let h2 = IfNoneMatch::parse(Some(r#"W/"67ab43", "54ed21""#));
        assert!(h2.matches(r#"W/"67ab43""#) && h2.matches(r#""67ab43""#));
        assert!(h2.matches(r#""54ed21""#) && h2.matches(r#"W/"54ed21""#) && !h2.matches(r#""7892dd""#));
        assert!(IfNoneMatch::parse(Some("*")).matches("anything"));
        assert_eq!(h.to_string(), r#"W/"a", "b""#);
    }

    #[test]
    fn if_match_is_strong() {
        let h = IfMatch::parse(Some(r#"W/"a", "b""#));
        assert!(h.matches("b"));
        assert!(!h.matches(r#"W/"a""#) && !h.matches("a"));
        assert!(IfMatch::parse(None).matches("x"));
    }

    #[test]
    fn if_range_by_date_or_strong_etag() {
        let modified = UNIX_EPOCH + Duration::from_millis(784_111_777_500);
        assert!(IfRange::parse(Some("Sun, 06 Nov 1994 08:49:37 GMT")).matches(None, Some(modified)));
        assert!(!IfRange::parse(Some("Sun, 06 Nov 1994 08:49:38 GMT")).matches(None, Some(modified)));
        assert!(IfRange::parse(Some(r#""v1""#)).matches(Some(r#""v1""#), None));
        assert!(!IfRange::parse(Some(r#"W/"v1""#)).matches(Some(r#"W/"v1""#), None));
        assert!(IfRange::parse(None).matches(None, None));
    }

    // if-range.test.ts (remix 3.1): validators are classified by syntax.
    #[test]
    fn if_range_classifies_validators_by_syntax() {
        let date = "Sun, 06 Nov 1994 08:49:37 GMT";
        assert!(!IfRange::parse(Some(date)).matches(Some(date), None), "a date is not an ETag");
        assert!(!IfRange::parse(Some(date)).matches(Some(&format!("\"{date}\"")), None));
        assert!(!IfRange::parse(Some("not-a-date")).matches(Some(r#""not-a-date""#), None));
        assert!(!IfRange::parse(Some("67ab43")).matches(Some("67ab43"), None), "bare ETags do not match");
        assert!(!IfRange::parse(Some("67ab43")).matches(Some(r#""67ab43""#), None));
        assert!(!IfRange::parse(Some(r#""not valid""#)).matches(Some(r#""not valid""#), None), "space is not an etagc");
        assert!(!IfRange::parse(Some(r#""67ab43""#)).matches(Some(r#""7892dd""#), None));
        assert!(IfRange::parse(Some("")).matches(None, None), "an empty header matches");
        let h = IfRange { value: r#""abc""#.into() };
        assert_eq!(h.to_string(), r#""abc""#);
        assert!(h.matches(Some(r#""abc""#), None));
    }
}
