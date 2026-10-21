//! Matcher resource limits (Remix's `match/limits.ts`).

use std::fmt;

/// Limits on compiling and matching patterns, so a hostile pattern set or URL cannot make
/// matching unbounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatcherLimits {
    /// Largest single pattern in UTF-8 bytes (default 65,536).
    pub max_pattern_size: usize,
    /// Largest total of all patterns in UTF-8 bytes (default 16,777,216).
    pub max_matcher_size: usize,
    /// Most work one URL match may do (default 1,000,000).
    pub max_match_work: usize,
}

impl Default for MatcherLimits {
    fn default() -> MatcherLimits {
        MatcherLimits { max_pattern_size: 64 * 1024, max_matcher_size: 16 * 1024 * 1024, max_match_work: 1_000_000 }
    }
}

/// Which limit was exceeded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatcherLimit {
    MaxPatternSize,
    MaxMatcherSize,
    MaxMatchWork,
}

impl MatcherLimit {
    pub fn as_str(self) -> &'static str {
        match self {
            MatcherLimit::MaxPatternSize => "maxPatternSize",
            MatcherLimit::MaxMatcherSize => "maxMatcherSize",
            MatcherLimit::MaxMatchWork => "maxMatchWork",
        }
    }
}

/// A pattern or match went over a [`MatcherLimits`] value (Remix's `MatcherResourceError`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatcherResourceError {
    pub limit: MatcherLimit,
    pub maximum: usize,
    pub actual: usize,
}

impl fmt::Display for MatcherResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} exceeded: {} > {}", self.limit.as_str(), self.actual, self.maximum)
    }
}

impl std::error::Error for MatcherResourceError {}

pub(crate) fn check(limit: MatcherLimit, maximum: usize, actual: usize) -> Result<(), MatcherResourceError> {
    if actual > maximum {
        return Err(MatcherResourceError { limit, maximum, actual });
    }
    Ok(())
}

/// Work done by one match, checked against `max_match_work` as it grows.
pub(crate) struct Budget {
    maximum: usize,
    actual: usize,
}

impl Budget {
    pub fn new(maximum: usize) -> Budget {
        Budget { maximum, actual: 0 }
    }

    pub fn consume(&mut self, count: usize) -> Result<(), MatcherResourceError> {
        let actual = self.actual.saturating_add(count);
        check(MatcherLimit::MaxMatchWork, self.maximum, actual)?;
        self.actual = actual;
        Ok(())
    }
}
