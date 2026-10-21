//! URL patterns for routing: parse, match, rank and generate hrefs.
//!
//! A Rust port of Remix's `route-pattern`:
//!
//! - `:name` variables capture a non-empty run inside one segment (one label in hostnames).
//! - `*name` (or bare `*`) wildcards capture across segments.
//! - `( ... )` makes part of a pattern optional; groups nest.
//! - `\` escapes the next character.
//! - `?key` / `?key=value` add search constraints.
//! - `http(s)://host:port/path` patterns also constrain protocol, hostname and port.
//!
//! When several patterns match, [`Matcher`] picks the most specific one (see [`specificity`]).
//!
//! ```
//! use rmx_route_pattern::RoutePattern;
//! use rmx_url::Url;
//!
//! let p = RoutePattern::parse("blog/:slug(.html)").unwrap();
//! let m = p.match_url(&Url::parse("https://x.test/blog/hello.html").unwrap()).unwrap().unwrap();
//! assert_eq!(m.params.get("slug"), Some("hello"));
//! assert_eq!(p.href(&[("slug", "hi")]).unwrap(), "/blog/hi.html");
//! ```

use std::collections::BTreeMap;
use std::fmt;

use rmx_url::{SearchParams, Url};

mod href;
mod join;
mod limits;
mod matcher;
mod parse;
mod program;
mod punycode;
mod serialize;
pub mod specificity;
mod split;
#[cfg(test)]
mod tests;

pub use href::{encode_pathname_variable, encode_pathname_wildcard, HrefError, HrefOptions, HrefParams};
pub use limits::{MatcherLimit, MatcherLimits, MatcherResourceError};
pub use matcher::{AddError, Match, MatchParamMeta, Matcher, MatcherOptions, MultiMatcher, ParamsMeta};
pub use parse::{ParseError, ParseErrorType};

/// The protocol constraint of a pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Protocol {
    Http,
    Https,
    HttpOrHttps,
}

/// The URL part a pattern part or capture belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UrlPart {
    Hostname,
    Pathname,
}

impl UrlPart {
    pub(crate) fn separator(self) -> char {
        match self {
            UrlPart::Hostname => '.',
            UrlPart::Pathname => '/',
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            UrlPart::Hostname => "hostname",
            UrlPart::Pathname => "pathname",
        }
    }
}

/// A variable (`:`) or a wildcard (`*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaptureKind {
    Variable,
    Wildcard,
}

/// A token of a parsed hostname or pathname (Remix's `PartPatternToken`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    Text(String),
    Separator,
    Open,
    Close,
    Var(String),
    /// A wildcard; the name is `*` when it has none.
    Wild(String),
}

/// A parsed hostname or pathname (Remix's `PartPattern`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PartPattern {
    pub(crate) tokens: Vec<Token>,
    /// Maps each `(` token index to the index of its `)`.
    pub(crate) optionals: BTreeMap<usize, usize>,
    pub(crate) kind: UrlPart,
}

impl PartPattern {
    pub(crate) fn empty(kind: UrlPart) -> PartPattern {
        PartPattern { tokens: Vec::new(), optionals: BTreeMap::new(), kind }
    }
}

/// Required search values by key, in source order. No values means the key must appear.
pub(crate) type Search = Vec<(String, Vec<String>)>;

/// The parsed parts of a pattern (Remix's `RoutePatternParts`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Parts {
    pub(crate) protocol: Option<Protocol>,
    pub(crate) hostname: Option<PartPattern>,
    pub(crate) port: Option<String>,
    pub(crate) pathname: PartPattern,
    pub(crate) search: Search,
}

/// Captured params by name. A name captured twice keeps the last value, like Remix.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Params(Vec<(String, String)>);

impl Params {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Set a param, replacing an earlier value with the same name in place.
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let (name, value) = (name.into(), value.into());
        match self.0.iter_mut().find(|(k, _)| *k == name) {
            Some(entry) => entry.1 = value,
            None => self.0.push((name, value)),
        }
    }

    /// Same as [`Params::set`].
    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.set(name, value);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<K: Into<String>, V: Into<String>> FromIterator<(K, V)> for Params {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Params {
        let mut params = Params::default();
        for (k, v) in iter {
            params.set(k, v);
        }
        params
    }
}

/// Each serialized part of a pattern (Remix's `RoutePatternJSON`). Missing parts are empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoutePatternJson {
    pub protocol: String,
    pub hostname: String,
    pub port: String,
    /// Without the leading `/`.
    pub pathname: String,
    /// Without the leading `?`.
    pub search: String,
}

/// A variable or wildcard declared in a pattern (Remix's `RoutePatternCapture`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePatternCapture {
    pub part: UrlPart,
    pub kind: CaptureKind,
    /// The name, or `*` for a nameless wildcard.
    pub name: String,
    /// Whether the capture sits inside an optional group.
    pub optional: bool,
}

/// A parsed route pattern (Remix's `RoutePattern`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePattern {
    parts: Parts,
}

impl RoutePattern {
    /// Parse a pattern source.
    pub fn parse(source: &str) -> Result<RoutePattern, ParseError> {
        parse::parse_pattern(source).map(|parts| RoutePattern { parts })
    }

    pub(crate) fn parts(&self) -> &Parts {
        &self.parts
    }

    /// The normalized source: same as `to_string()`.
    pub fn source(&self) -> String {
        serialize::serialize_pattern(&self.parts)
    }

    /// Each serialized part (Remix's `toJSON`).
    pub fn to_json(&self) -> RoutePatternJson {
        let parts = &self.parts;
        RoutePatternJson {
            protocol: serialize::serialize_protocol(parts.protocol).to_string(),
            hostname: parts.hostname.as_ref().map(serialize::serialize_part).unwrap_or_default(),
            port: parts.port.clone().unwrap_or_default(),
            pathname: serialize::serialize_part(&parts.pathname),
            search: serialize::serialize_search(&parts.search),
        }
    }

    /// Generate an href (Remix's `createHref`). Origin-less patterns produce a root-relative path.
    pub fn href(&self, params: impl HrefParams) -> Result<String, HrefError> {
        self.href_with(params, &HrefOptions::default())
    }

    /// Generate an href with search params and an optional base URL.
    pub fn href_with(&self, params: impl HrefParams, options: &HrefOptions) -> Result<String, HrefError> {
        href::create_href(self, &params, options)
    }

    /// Generate an href with extra search params, placed before the pattern's own constraints.
    pub fn href_with_search(&self, params: impl HrefParams, search: &SearchParams) -> Result<String, HrefError> {
        self.href_with(params, &HrefOptions { search_params: search.clone(), base_url: None })
    }

    /// Join `next` onto this base pattern (Remix's `joinPatterns`): pathnames are joined with
    /// `/`, the protocol, hostname and port of `next` win, and search constraints merge.
    pub fn join(&self, next: &RoutePattern) -> RoutePattern {
        RoutePattern { parts: join::join_patterns(&self.parts, &next.parts) }
    }

    /// Match one URL against this pattern (Remix's `createMatcher(pattern).match(url)`).
    pub fn match_url(&self, url: &Url) -> Result<Option<Match<()>>, MatcherResourceError> {
        self.match_url_with(url, MatcherOptions::default())
    }

    /// [`RoutePattern::match_url`] with matcher options.
    pub fn match_url_with(&self, url: &Url, options: MatcherOptions) -> Result<Option<Match<()>>, MatcherResourceError> {
        let mut matcher = Matcher::with_options(options);
        matcher.add(self.clone(), ())?;
        Ok(matcher.match_url(url)?.map(|m| Match { url: m.url, pattern: m.pattern, data: (), params: m.params, params_meta: m.params_meta }))
    }

    /// Hostname then pathname captures in source order (Remix's `getRoutePatternCaptures`).
    pub fn captures(&self) -> Vec<RoutePatternCapture> {
        let mut out = Vec::new();
        for part in self.parts.hostname.iter().chain([&self.parts.pathname]) {
            let mut depth = 0usize;
            for token in &part.tokens {
                let (kind, name) = match token {
                    Token::Open => {
                        depth += 1;
                        continue;
                    }
                    Token::Close => {
                        depth -= 1;
                        continue;
                    }
                    Token::Var(name) => (CaptureKind::Variable, name),
                    Token::Wild(name) => (CaptureKind::Wildcard, name),
                    Token::Text(_) | Token::Separator => continue,
                };
                out.push(RoutePatternCapture { part: part.kind, kind, name: name.clone(), optional: depth > 0 });
            }
        }
        out
    }

    /// Names of every variable and named wildcard, in source order, without repeats.
    pub fn param_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for capture in self.captures() {
            if capture.name != "*" && !names.contains(&capture.name) {
                names.push(capture.name);
            }
        }
        names
    }
}

impl fmt::Display for RoutePattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source())
    }
}

impl std::str::FromStr for RoutePattern {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, ParseError> {
        RoutePattern::parse(s)
    }
}
