//! `Range` requests.

use std::fmt;

/// `Range: bytes=0-99,200-`. A malformed header parses with no ranges.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub unit: String,
    /// `(start, end)` as written; `(None, Some(n))` is the last `n` bytes.
    pub ranges: Vec<(Option<u64>, Option<u64>)>,
}

impl Range {
    pub fn parse(value: Option<&str>) -> Range {
        let mut h = Range::default();
        let Some((unit, list)) = value.and_then(|v| v.split_once('=')) else { return h };
        if unit.is_empty() || !unit.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') || list.is_empty() {
            return h;
        }
        h.unit = unit.to_string();
        let num = |s: &str| -> Result<Option<u64>, ()> {
            if s.is_empty() {
                Ok(None)
            } else if s.bytes().all(|b| b.is_ascii_digit()) {
                s.parse().map(Some).map_err(|_| ())
            } else {
                Err(())
            }
        };
        for part in list.split(',') {
            let parsed = part.trim().split_once('-').ok_or(()).and_then(|(a, b)| Ok((num(a)?, num(b)?)));
            match parsed {
                Ok((None, None)) | Err(()) => {
                    h.ranges.clear();
                    return h;
                }
                Ok((Some(a), Some(b))) if a > b => {
                    h.ranges.clear();
                    return h;
                }
                Ok(r) => h.ranges.push(r),
            }
        }
        h
    }

    /// Whether at least one range overlaps a resource of `size` bytes.
    pub fn can_satisfy(&self, size: u64) -> bool {
        !self.unit.is_empty() && !self.ranges.is_empty() && self.ranges.iter().any(|r| r.0.is_none_or(|start| start < size))
    }

    /// Inclusive `(start, end)` byte ranges clamped to `size` (empty if unsatisfiable).
    pub fn normalize(&self, size: u64) -> Vec<(u64, u64)> {
        if !self.can_satisfy(size) {
            return Vec::new();
        }
        self.ranges
            .iter()
            .map(|r| match *r {
                (Some(start), Some(end)) => (start, end.min(size.saturating_sub(1))),
                (Some(start), None) => (start, size.saturating_sub(1)),
                (None, Some(suffix)) => (size.saturating_sub(suffix), size.saturating_sub(1)),
                (None, None) => unreachable!("parse rejects empty ranges"),
            })
            .collect()
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.unit.is_empty() || self.ranges.is_empty() {
            return Ok(());
        }
        let parts: Vec<String> = self
            .ranges
            .iter()
            .map(|r| match r {
                (Some(a), Some(b)) => format!("{a}-{b}"),
                (Some(a), None) => format!("{a}-"),
                (None, Some(b)) => format!("-{b}"),
                (None, None) => String::new(),
            })
            .collect();
        write!(f, "{}={}", self.unit, parts.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_normalize() {
        let r = Range::parse(Some("bytes=0-99, 200-, -50"));
        assert_eq!(r.ranges, vec![(Some(0), Some(99)), (Some(200), None), (None, Some(50))]);
        assert_eq!(r.normalize(1000), vec![(0, 99), (200, 999), (950, 999)]);
        assert_eq!(r.to_string(), "bytes=0-99,200-,-50");
        assert!(!Range::parse(Some("bytes=5000-")).can_satisfy(1000));
        assert!(Range::parse(Some("bytes=9-5")).ranges.is_empty());
        assert!(Range::parse(Some("bytes=0-1,x")).ranges.is_empty(), "one bad part spoils the header");
        assert_eq!(Range::parse(Some("bytes=0-5000")).normalize(10), vec![(0, 9)]);
    }
}
