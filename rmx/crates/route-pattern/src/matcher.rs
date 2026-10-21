//! Matching URLs against many patterns (Remix's `match.ts` and `match/trie.ts`).

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use rmx_url::Url;

use crate::limits::{check, Budget, MatcherLimit, MatcherLimits, MatcherResourceError};
use crate::program::{canonicalize_url_part, compile_part, has_static_anchor, has_static_prefix, has_static_suffix, match_part, CanonicalText, PartProgram, Unit};
use crate::punycode::decode_hostname;
use crate::{specificity, CaptureKind, ParseError, Params, Parts, Protocol, RoutePattern, UrlPart};

/// A matched param and where it sits in the normalized URL part (Remix's `MatchParamMeta`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchParamMeta {
    pub kind: CaptureKind,
    /// The param name, or `*` for a nameless wildcard.
    pub name: String,
    /// The decoded value.
    pub value: String,
    pub begin: usize,
    pub end: usize,
}

/// Params captured from the hostname and the pathname.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParamsMeta {
    pub hostname: Vec<MatchParamMeta>,
    pub pathname: Vec<MatchParamMeta>,
}

/// A successful match (Remix's `Match`).
#[derive(Clone, Debug)]
pub struct Match<T> {
    /// The URL that matched.
    pub url: Url,
    /// The pattern that matched.
    pub pattern: Arc<RoutePattern>,
    /// The data added with the pattern.
    pub data: T,
    /// Captured params by name; a nameless wildcard is left out.
    pub params: Params,
    pub params_meta: ParamsMeta,
}

/// Options for a [`Matcher`] (Remix's `MatcherOptions`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MatcherOptions {
    /// Match pathnames case-insensitively. Hostnames always are; search never is.
    pub ignore_case: bool,
    pub limits: MatcherLimits,
}

/// Why a pattern source could not be added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddError {
    Parse(ParseError),
    Resource(MatcherResourceError),
}

impl fmt::Display for AddError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AddError::Parse(e) => e.fmt(f),
            AddError::Resource(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for AddError {}

impl From<ParseError> for AddError {
    fn from(e: ParseError) -> AddError {
        AddError::Parse(e)
    }
}

impl From<MatcherResourceError> for AddError {
    fn from(e: MatcherResourceError) -> AddError {
        AddError::Resource(e)
    }
}

struct Entry<T> {
    pattern: Arc<RoutePattern>,
    data: T,
    hostname: Option<PartProgram>,
    pathname: PartProgram,
}

#[derive(Default)]
struct IndexNode {
    children: HashMap<(bool, char), IndexNode>,
    entries: Vec<usize>,
}

impl IndexNode {
    fn insert(&mut self, units: impl Iterator<Item = Unit>, entry: usize) {
        let mut node = self;
        for unit in units {
            node = node.children.entry(unit.key()).or_default();
        }
        node.entries.push(entry);
    }
}

/// Above this many prefix candidates, the suffix and anchor indexes are tried too.
const MAX_CANDIDATES_BEFORE_SECONDARY_INDEX: usize = 32;

/// Matches URLs against patterns and picks the most specific match (Remix's
/// `createMultiMatcher`). Patterns are indexed by their static prefix, suffix and longest
/// static run, so only likely candidates are run.
pub struct Matcher<T> {
    options: MatcherOptions,
    entries: Vec<Entry<T>>,
    prefix_index: IndexNode,
    suffix_index: IndexNode,
    anchor_index: IndexNode,
    size: usize,
}

impl<T> Default for Matcher<T> {
    fn default() -> Self {
        Matcher::with_options(MatcherOptions::default())
    }
}

impl<T> Matcher<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_options(options: MatcherOptions) -> Self {
        Matcher {
            options,
            entries: Vec::new(),
            prefix_index: IndexNode::default(),
            suffix_index: IndexNode::default(),
            anchor_index: IndexNode::default(),
            size: 0,
        }
    }

    /// Whether pathnames match case-insensitively.
    pub fn ignore_case(&self) -> bool {
        self.options.ignore_case
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn check_size(&self, source: &str) -> Result<usize, MatcherResourceError> {
        let size = source.len();
        let limits = self.options.limits;
        check(MatcherLimit::MaxPatternSize, limits.max_pattern_size, size)?;
        check(MatcherLimit::MaxMatcherSize, limits.max_matcher_size, self.size.saturating_add(size))?;
        Ok(size)
    }

    /// Add a pattern source; its size is checked before it is parsed.
    pub fn add_str(&mut self, source: &str, data: T) -> Result<(), AddError> {
        let mut size = self.check_size(source)?;
        let pattern = RoutePattern::parse(source)?;
        let normalized = pattern.source();
        if normalized != source {
            size = self.check_size(&normalized)?;
        }
        self.insert(pattern, data, size);
        Ok(())
    }

    /// Add a parsed pattern with the data to return when it matches.
    pub fn add(&mut self, pattern: RoutePattern, data: T) -> Result<(), MatcherResourceError> {
        let size = self.check_size(&pattern.source())?;
        self.insert(pattern, data, size);
        Ok(())
    }

    fn insert(&mut self, pattern: RoutePattern, data: T, size: usize) {
        let parts = pattern.parts();
        let hostname = parts.hostname.as_ref().map(|h| compile_part(h, true));
        let pathname = compile_part(&parts.pathname, self.options.ignore_case);
        let index = self.entries.len();
        self.prefix_index.insert(pathname.static_prefix.iter().copied(), index);
        self.anchor_index.insert(pathname.static_anchor.iter().copied(), index);
        self.suffix_index.insert(pathname.static_suffix.iter().rev().copied(), index);
        self.entries.push(Entry { pattern: Arc::new(pattern), data, hostname, pathname });
        self.size += size;
    }

    /// Patterns and data in the order they were added.
    pub fn entries(&self) -> impl Iterator<Item = (&RoutePattern, &T)> {
        self.entries.iter().map(|e| (&*e.pattern, &e.data))
    }

    /// The most specific match; ties go to the pattern added first.
    pub fn match_url(&self, url: &Url) -> Result<Option<Match<&T>>, MatcherResourceError> {
        let mut best: Option<Match<&T>> = None;
        for m in self.search(url)? {
            if best.as_ref().is_none_or(|b| specificity::greater_than(&m, b)) {
                best = Some(m);
            }
        }
        Ok(best)
    }

    /// Every match, most specific first (ties in the order patterns were added).
    pub fn match_all(&self, url: &Url) -> Result<Vec<Match<&T>>, MatcherResourceError> {
        let mut matches = self.search(url)?;
        matches.sort_by(specificity::descending);
        Ok(matches)
    }

    fn search(&self, url: &Url) -> Result<Vec<Match<&T>>, MatcherResourceError> {
        let protocol = url.scheme();
        if (protocol != "http" && protocol != "https") || self.entries.is_empty() {
            return Ok(Vec::new());
        }
        let mut budget = Budget::new(self.options.limits.max_match_work);
        let path = url.pathname().strip_prefix('/').unwrap_or(url.pathname());
        let Some(pathname) = canonicalize_url_part(path, UrlPart::Pathname, self.options.ignore_case, &mut budget)? else {
            return Ok(Vec::new());
        };

        let port = url.port().map(|p| p.to_string()).unwrap_or_default();
        let search = url.search_params();
        let mut candidates = Vec::new();
        let mut needs_hostname = false;
        for index in self.find_candidates(&pathname, &mut budget)? {
            budget.consume(1)?;
            let entry = &self.entries[index];
            let parts = entry.pattern.parts();
            if !matches_protocol(parts.protocol, protocol) || !matches_port(parts, protocol, &port) {
                continue;
            }
            if !matches_search(&search, &parts.search, &mut budget)? {
                continue;
            }
            needs_hostname |= entry.hostname.is_some();
            candidates.push(index);
        }
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        candidates.sort_unstable();

        let decoded_hostname = decode_hostname(url.hostname());
        let hostname = if needs_hostname {
            match canonicalize_url_part(&decoded_hostname, UrlPart::Hostname, true, &mut budget)? {
                Some(h) => Some(h),
                None => return Ok(Vec::new()),
            }
        } else {
            None
        };

        let mut results = Vec::new();
        for index in candidates {
            let entry = &self.entries[index];
            let hostname_match = match (&entry.hostname, &hostname) {
                (None, _) => vec![MatchParamMeta {
                    kind: CaptureKind::Wildcard,
                    name: "*".into(),
                    value: decoded_hostname.clone(),
                    begin: 0,
                    end: decoded_hostname.encode_utf16().count(),
                }],
                (Some(program), Some(text)) => match match_part(program, text, &mut budget)? {
                    Some(m) => m,
                    None => continue,
                },
                (Some(_), None) => unreachable!("hostname canonicalized when a candidate needs it"),
            };
            let Some(pathname_match) = match_part(&entry.pathname, &pathname, &mut budget)? else { continue };
            let mut params = Params::default();
            for capture in hostname_match.iter().chain(&pathname_match) {
                if capture.name != "*" {
                    params.set(&capture.name, &capture.value);
                }
            }
            results.push(Match {
                url: url.clone(),
                pattern: entry.pattern.clone(),
                data: &entry.data,
                params,
                params_meta: ParamsMeta { hostname: hostname_match, pathname: pathname_match },
            });
        }
        Ok(results)
    }

    fn find_candidates(&self, pathname: &CanonicalText, budget: &mut Budget) -> Result<Vec<usize>, MatcherResourceError> {
        let mut result = collect(&self.prefix_index, pathname.units.iter(), budget)?;
        if result.len() > MAX_CANDIDATES_BEFORE_SECONDARY_INDEX {
            let suffix = collect(&self.suffix_index, pathname.units.iter().rev(), budget)?;
            if suffix.len() < result.len() {
                result = suffix;
            }
        }
        if result.len() > MAX_CANDIDATES_BEFORE_SECONDARY_INDEX {
            let anchor = collect_anchor(&self.anchor_index, &pathname.units, self.entries.len(), budget)?;
            if anchor.len() < result.len() {
                result = anchor;
            }
        }
        let mut filtered = Vec::new();
        for index in result {
            let program = &self.entries[index].pathname;
            if has_static_prefix(program, pathname, budget)? && has_static_suffix(program, pathname, budget)? && has_static_anchor(program, pathname, budget)? {
                filtered.push(index);
            }
        }
        Ok(filtered)
    }
}

fn collect<'a>(index: &IndexNode, units: impl Iterator<Item = &'a Unit>, budget: &mut Budget) -> Result<Vec<usize>, MatcherResourceError> {
    let mut result = Vec::new();
    budget.consume(index.entries.len())?;
    result.extend_from_slice(&index.entries);
    let mut node = index;
    for unit in units {
        budget.consume(1)?;
        let Some(next) = node.children.get(&unit.key()) else { break };
        node = next;
        budget.consume(node.entries.len())?;
        result.extend_from_slice(&node.entries);
    }
    Ok(result)
}

fn collect_anchor(index: &IndexNode, units: &[Unit], count: usize, budget: &mut Budget) -> Result<Vec<usize>, MatcherResourceError> {
    let mut seen = vec![false; count];
    let mut result = Vec::new();
    let mut add = |entries: &[usize], budget: &mut Budget| -> Result<(), MatcherResourceError> {
        budget.consume(entries.len())?;
        for &entry in entries {
            if !std::mem::replace(&mut seen[entry], true) {
                result.push(entry);
            }
        }
        Ok(())
    };
    add(&index.entries, budget)?;
    for position in 0..units.len() {
        let mut node = index;
        for unit in &units[position..] {
            budget.consume(1)?;
            let Some(next) = node.children.get(&unit.key()) else { break };
            node = next;
            add(&node.entries, budget)?;
        }
    }
    Ok(result)
}

fn matches_protocol(expected: Option<Protocol>, actual: &str) -> bool {
    match expected {
        None | Some(Protocol::HttpOrHttps) => true,
        Some(Protocol::Http) => actual == "http",
        Some(Protocol::Https) => actual == "https",
    }
}

/// A pattern without a port matches only default ports when it names a hostname; a
/// pattern's default port (`80` for http, `443` for https) matches a URL without one.
fn matches_port(parts: &Parts, protocol: &str, actual: &str) -> bool {
    let Some(expected) = &parts.port else {
        return parts.hostname.is_none() || actual.is_empty();
    };
    let default = (protocol == "http" && expected == "80") || (protocol == "https" && expected == "443");
    if default {
        actual.is_empty()
    } else {
        expected == actual
    }
}

fn matches_search(params: &rmx_url::SearchParams, constraints: &crate::Search, budget: &mut Budget) -> Result<bool, MatcherResourceError> {
    for (name, required) in constraints {
        budget.consume(params.len() + required.len() + 1)?;
        if required.is_empty() {
            if !params.has(name) {
                return Ok(false);
            }
            continue;
        }
        let values = params.get_all(name);
        if !required.iter().all(|r| values.contains(&r.as_str())) {
            return Ok(false);
        }
    }
    Ok(true)
}
