//! `Range` requests.

use std::fmt;

/// JavaScript's `Number.MAX_SAFE_INTEGER`: Remix caps larger bounds here, and so do we, so
/// the two serialize oversized ranges the same way.
pub const MAX_BOUND: u64 = (1 << 53) - 1;

/// A decimal bound without leading zeros (`"0"` for all zeros).
fn significant(digits: &str) -> &str {
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() { "0" } else { trimmed }
}

/// Compare two decimal strings by value, without parsing (they can be any length).
fn compare_bounds(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b) = (significant(a), significant(b));
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// A decimal bound, capped at [`MAX_BOUND`] (RFC 9110 asks for large bounds to be handled
/// without overflow).
fn parse_bound(digits: &str) -> u64 {
    let s = significant(digits);
    if compare_bounds(s, &MAX_BOUND.to_string()).is_gt() {
        MAX_BOUND
    } else {
        s.parse().unwrap_or(MAX_BOUND)
    }
}

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
        let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        for part in list.split(',') {
            let Some((a, b)) = part.trim().split_once('-') else {
                h.ranges.clear();
                return h;
            };
            // Reversed bounds are compared as decimals, before oversized ones are capped.
            let reversed = !a.is_empty() && !b.is_empty() && compare_bounds(a, b).is_gt();
            if !digits(a) || !digits(b) || (a.is_empty() && b.is_empty()) || reversed {
                h.ranges.clear();
                return h;
            }
            let bound = |s: &str| (!s.is_empty()).then(|| parse_bound(s));
            h.ranges.push((bound(a), bound(b)));
        }
        h
    }

    /// Whether every range is well formed (a bound, and `start <= end`) and at least one
    /// is satisfiable for a resource of `size` bytes: it starts inside it, or it is a
    /// non-zero suffix (satisfiable even for an empty resource, RFC 9110 14.1.2). One bad
    /// member spoils the set (14.1.1). The fields are public, so this checks ranges built by
    /// hand too. Sizes above [`MAX_BOUND`] are rejected, as in Remix.
    pub fn can_satisfy(&self, size: u64) -> bool {
        let well_formed = |r: &(Option<u64>, Option<u64>)| match *r {
            (None, None) => false,
            (Some(start), Some(end)) => start <= end,
            _ => true,
        };
        let satisfiable = |r: &(Option<u64>, Option<u64>)| match *r {
            (None, end) => end.is_some_and(|n| n > 0),
            (Some(start), _) => start < size,
        };
        size <= MAX_BOUND
            && !self.unit.is_empty()
            && !self.ranges.is_empty()
            && self.ranges.iter().all(well_formed)
            && self.ranges.iter().any(satisfiable)
    }

    /// Inclusive `(start, end)` byte ranges clamped to `size`, leaving out members that
    /// are not satisfiable. Empty when the set cannot be satisfied, and for an empty
    /// resource (it has no byte positions, even when [`Range::can_satisfy`] is true).
    pub fn normalize(&self, size: u64) -> Vec<(u64, u64)> {
        if size == 0 || !self.can_satisfy(size) {
            return Vec::new();
        }
        let last = size - 1;
        self.ranges
            .iter()
            .filter_map(|r| match *r {
                (Some(start), _) if start >= size => None,
                (Some(start), Some(end)) => Some((start, end.min(last))),
                (Some(start), None) => Some((start, last)),
                (None, Some(suffix)) if suffix > 0 => Some((size.saturating_sub(suffix), last)),
                (None, _) => None,
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

    // range.test.ts (remix 3.1): zero-length suffixes, empty resources and partial sets.
    #[test]
    fn zero_length_suffixes_and_empty_resources() {
        assert!(!Range::parse(Some("bytes=-0")).can_satisfy(1000));
        assert_eq!(Range::parse(Some("bytes=-0")).normalize(1000), vec![]);
        let suffix = Range::parse(Some("bytes=-500"));
        assert!(suffix.can_satisfy(0), "a non-zero suffix is satisfiable for an empty resource");
        assert_eq!(suffix.normalize(0), vec![], "but has no byte positions");
        assert_eq!(Range::parse(Some("bytes=1000-,0-99")).normalize(500), vec![(0, 99)]);
        let start_past_end = Range { unit: "bytes".into(), ranges: vec![(Some(600), Some(700)), (None, Some(10))] };
        assert_eq!(start_past_end.normalize(500), vec![(490, 499)]);
    }

    #[test]
    fn rejects_sizes_beyond_the_safe_integer_range() {
        assert!(!Range::parse(Some("bytes=0-")).can_satisfy(MAX_BOUND + 1));
        assert_eq!(Range::parse(Some("bytes=0-")).normalize(MAX_BOUND + 1), vec![]);
    }

    #[test]
    fn caps_decimal_bounds_above_the_maximum_safe_integer() {
        let huge = "9".repeat(309);
        let r = Range::parse(Some(&format!("bytes=9007199254740992-,0-{huge},-{huge}")));
        assert_eq!(r.ranges, vec![(Some(MAX_BOUND), None), (Some(0), Some(MAX_BOUND)), (None, Some(MAX_BOUND))]);
        assert_eq!(r.to_string(), "bytes=9007199254740991-,0-9007199254740991,-9007199254740991");
        assert_eq!(Range::parse(Some(&r.to_string())).ranges, r.ranges);
        assert_eq!(Range::parse(Some(&format!("bytes=0-{huge}"))).normalize(500), vec![(0, 499)]);
        assert_eq!(Range::parse(Some(&format!("bytes=-{huge}"))).normalize(500), vec![(0, 499)]);
    }

    #[test]
    fn leading_zeros_and_reversed_oversized_bounds() {
        assert_eq!(Range::parse(Some(&format!("bytes={}1-", "0".repeat(100)))).ranges, vec![(Some(1), None)]);
        assert!(Range::parse(Some("bytes=9007199254740992-9007199254740991")).ranges.is_empty());
        assert!(Range::parse(Some(&format!("bytes={}-{}", "9".repeat(310), "9".repeat(309)))).ranges.is_empty());
    }
}
