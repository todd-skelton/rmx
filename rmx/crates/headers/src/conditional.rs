//! `If-Match`, `If-None-Match` and `If-Range`.

use std::fmt;
use std::time::SystemTime;

use crate::date::{parse_http_date, unix_seconds};
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

    pub fn matches(&self, tag: &str) -> bool {
        self.has(tag) || self.tags.iter().any(|t| t == "*")
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

impl IfRange {
    pub fn parse(value: Option<&str>) -> IfRange {
        IfRange { value: value.unwrap_or_default().to_string() }
    }

    /// Whether a range request may be served for a resource with this ETag and
    /// modification time (an empty header always matches).
    pub fn matches(&self, etag: Option<&str>, last_modified: Option<SystemTime>) -> bool {
        if self.value.is_empty() {
            return true;
        }
        if let (Some(date), Some(modified)) = (parse_http_date(&self.value), last_modified) {
            return unix_seconds(date) == unix_seconds(modified);
        }
        let Some(etag) = etag else { return false };
        let (mine, theirs) = (quote_etag(&self.value), quote_etag(etag));
        !mine.starts_with("W/") && !theirs.starts_with("W/") && mine == theirs
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    #[test]
    fn if_none_match_is_weak() {
        let h = IfNoneMatch::parse(Some(r#"W/"a", "b""#));
        assert!(h.matches(r#"W/"a""#) && h.matches("b") && !h.matches("c"));
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
        assert!(IfRange::parse(Some(r#""v1""#)).matches(Some("v1"), None));
        assert!(!IfRange::parse(Some(r#"W/"v1""#)).matches(Some(r#"W/"v1""#), None));
        assert!(IfRange::parse(None).matches(None, None));
    }
}
