//! URL patterns for routing: parse, match, rank and generate hrefs.
//!
//! A Rust port of the core of Remix's `route-pattern`:
//!
//! - `:name` variables capture a non-empty run up to the next `/` (or `.` in pathnames and
//!   hostnames), possessively.
//! - `*name` (or bare `*`) wildcards capture across segments.
//! - `( ... )` makes part of a pattern optional; groups nest.
//! - `\` escapes the next character.
//! - `?key` / `?key=value` add search constraints.
//! - `http(s)://host:port/path` patterns also constrain protocol, hostname and port.
//!
//! When several patterns match, [`Matcher`] picks the most specific one: compared left to
//! right, static text beats variables, and variables beat wildcards.
//!
//! ```
//! use rmx_route_pattern::RoutePattern;
//! use rmx_url::Url;
//!
//! let p = RoutePattern::parse("blog/:slug(.html)").unwrap();
//! let m = p.match_url(&Url::parse("https://x.test/blog/hello.html").unwrap()).unwrap();
//! assert_eq!(m.params.get("slug"), Some("hello"));
//! assert_eq!(p.href(&[("slug", "hi")]).unwrap(), "/blog/hi.html");
//! ```

use std::fmt;

use rmx_url::{decode, encode_path, encode_path_segment, SearchParams, Url};

mod matcher;

pub use matcher::{Match, Matcher};

/// A parsed route pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePattern {
    protocol: Protocol,
    hostname: Option<Vec<Token>>,
    port: Option<String>,
    pathname: Vec<Token>,
    search: Vec<SearchConstraint>,
}

/// `?key` (any value) or `?key=value`.
pub(crate) type SearchConstraint = (String, Option<String>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Protocol {
    Any,
    Http,
    Https,
    HttpOrHttps,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    Static(String),
    Var(String),
    Wild(Option<String>),
    Optional(Vec<Token>),
}

/// A malformed pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub source: String,
    pub index: usize,
    pub message: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid route pattern {:?} at {}: {}", self.source, self.index, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Why an href could not be generated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HrefError {
    MissingParam(String),
    NamelessWildcard,
    InvalidWildcard(String),
    EmptyVariable(String),
}

impl fmt::Display for HrefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HrefError::MissingParam(n) => write!(f, "missing required param {n:?}"),
            HrefError::NamelessWildcard => f.write_str("cannot generate an href through a nameless wildcard"),
            HrefError::InvalidWildcard(n) => write!(f, "wildcard param {n:?} contains a '.' or '..' segment"),
            HrefError::EmptyVariable(n) => write!(f, "variable param {n:?} is empty"),
        }
    }
}

impl std::error::Error for HrefError {}

/// Captured params: the last capture with a given name wins, like Remix.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Params(Vec<(String, String)>);

impl Params {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.0.push((name.into(), value.into()));
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Anything usable as href params: `&[("id", "1")]`, `[("id", id.as_str())]`, `&Params`, `()`.
pub trait HrefParams {
    fn param(&self, name: &str) -> Option<String>;
}

impl HrefParams for () {
    fn param(&self, _: &str) -> Option<String> {
        None
    }
}

impl<K: AsRef<str>, V: AsRef<str>> HrefParams for [(K, V)] {
    fn param(&self, name: &str) -> Option<String> {
        self.iter().rev().find(|(k, _)| k.as_ref() == name).map(|(_, v)| v.as_ref().to_string())
    }
}

impl<K: AsRef<str>, V: AsRef<str>, const N: usize> HrefParams for [(K, V); N] {
    fn param(&self, name: &str) -> Option<String> {
        self.as_slice().param(name)
    }
}

impl<K: AsRef<str>, V: AsRef<str>> HrefParams for Vec<(K, V)> {
    fn param(&self, name: &str) -> Option<String> {
        self.as_slice().param(name)
    }
}

impl HrefParams for Params {
    fn param(&self, name: &str) -> Option<String> {
        self.get(name).map(str::to_string)
    }
}

impl<T: HrefParams + ?Sized> HrefParams for &T {
    fn param(&self, name: &str) -> Option<String> {
        (**self).param(name)
    }
}

impl RoutePattern {
    /// Parse a pattern. A leading `/` on pathname-only patterns is optional.
    pub fn parse(source: &str) -> Result<RoutePattern, ParseError> {
        let err = |index: usize, message: &'static str| ParseError { source: source.to_string(), index, message };
        let mut protocol = Protocol::Any;
        let mut hostname = None;
        let mut port = None;
        let mut rest = source;
        let mut offset = 0;

        if let Some(i) = find_unescaped(source, "://") {
            protocol = match &source[..i] {
                "" => Protocol::Any,
                "http" => Protocol::Http,
                "https" => Protocol::Https,
                "http(s)" => Protocol::HttpOrHttps,
                _ => return Err(err(0, "protocol must be http, https or http(s)")),
            };
            let after = &source[i + 3..];
            let end = after.find(['/', '?']).unwrap_or(after.len());
            let mut authority = &after[..end];
            // A trailing `:digits` is a port; other colons are hostname variables.
            if let Some(colon) = authority.rfind(':') {
                let digits = &authority[colon + 1..];
                if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                    port = Some(digits.to_string());
                    authority = &authority[..colon];
                }
            }
            if !authority.is_empty() {
                let lower = authority.to_ascii_lowercase();
                hostname = Some(tokenize(&lower, Part::Hostname).map_err(|(i, m)| err(i + offset, m))?);
            }
            offset = i + 3 + end;
            rest = &after[end..];
        }

        let (path, search) = match find_unescaped(rest, "?") {
            Some(i) => (&rest[..i], Some(&rest[i + 1..])),
            None => (rest, None),
        };
        let path = path.strip_prefix('/').unwrap_or(path);
        let pathname = tokenize(path, Part::Pathname).map_err(|(i, m)| err(i + offset, m))?;
        let search = search
            .map(|s| {
                s.split('&')
                    .filter(|p| !p.is_empty())
                    .map(|p| match p.split_once('=') {
                        Some((k, v)) => (decode(k), Some(decode(v))),
                        None => (decode(p), None),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(RoutePattern { protocol, hostname, port, pathname, search })
    }

    /// Match a URL against this pattern.
    pub fn match_url(&self, url: &Url) -> Option<Match<()>> {
        matcher::match_one(self, url).map(|(params, _)| Match { params, data: () })
    }

    /// Generate an href. Origin-less patterns produce a root-relative path.
    pub fn href(&self, params: impl HrefParams) -> Result<String, HrefError> {
        self.href_with_search(params, &SearchParams::new())
    }

    /// Generate an href with extra search params (placed before the pattern's own constraints).
    pub fn href_with_search(&self, params: impl HrefParams, search: &SearchParams) -> Result<String, HrefError> {
        let mut out = String::new();
        if let Some(host) = &self.hostname {
            let scheme = match self.protocol {
                Protocol::Http => "http://",
                Protocol::Https | Protocol::HttpOrHttps => "https://",
                Protocol::Any => "//",
            };
            out.push_str(scheme);
            out.push_str(&build(host, &params, Part::Hostname)?);
            if let Some(port) = &self.port {
                out.push(':');
                out.push_str(port);
            }
        }
        let path = build(&self.pathname, &params, Part::Pathname)?;
        out.push('/');
        out.push_str(path.trim_start_matches('/'));
        let mut all = search.clone();
        for (k, v) in &self.search {
            all.append(k.clone(), v.clone().unwrap_or_default());
        }
        if !all.is_empty() {
            out.push('?');
            out.push_str(&all.to_string());
        }
        Ok(out)
    }

    /// Combine `self` (a base) with `other`: pathnames are joined with `/`, and any protocol,
    /// hostname, port or search constraint in `other` overrides or extends the base.
    pub fn join(&self, other: &RoutePattern) -> RoutePattern {
        let mut pathname = self.pathname.clone();
        let base_ends_slash = matches!(pathname.last(), Some(Token::Static(s)) if s.ends_with('/'));
        let other_starts_slash = matches!(other.pathname.first(), Some(Token::Static(s)) if s.starts_with('/'));
        if !pathname.is_empty() && !other.pathname.is_empty() && !base_ends_slash && !other_starts_slash {
            push_static(&mut pathname, "/");
        }
        for t in &other.pathname {
            match t {
                Token::Static(s) => push_static(&mut pathname, s),
                other => pathname.push(other.clone()),
            }
        }
        let mut search = self.search.clone();
        for (k, v) in &other.search {
            search.retain(|(key, _)| key != k);
            search.push((k.clone(), v.clone()));
        }
        RoutePattern {
            protocol: if other.protocol != Protocol::Any { other.protocol } else { self.protocol },
            hostname: other.hostname.clone().or_else(|| self.hostname.clone()),
            port: other.port.clone().or_else(|| self.port.clone()),
            pathname,
            search,
        }
    }

    /// Names of every variable and named wildcard, in pattern order.
    pub fn param_names(&self) -> Vec<String> {
        fn walk(tokens: &[Token], out: &mut Vec<String>) {
            for t in tokens {
                match t {
                    Token::Var(n) | Token::Wild(Some(n)) => out.push(n.clone()),
                    Token::Optional(inner) => walk(inner, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        if let Some(h) = &self.hostname {
            walk(h, &mut out);
        }
        walk(&self.pathname, &mut out);
        out
    }

    pub(crate) fn tokens(&self) -> (&[Token], Option<&[Token]>) {
        (&self.pathname, self.hostname.as_deref())
    }

    /// Whether a protocol is set, the port constraint, and search constraints.
    pub(crate) fn origin_constraints(&self) -> (bool, Option<&str>, &[SearchConstraint]) {
        (self.protocol != Protocol::Any, self.port.as_deref(), &self.search)
    }

    pub(crate) fn protocol_matches(&self, scheme: &str) -> bool {
        match self.protocol {
            Protocol::Any => true,
            Protocol::Http => scheme == "http",
            Protocol::Https => scheme == "https",
            Protocol::HttpOrHttps => scheme == "http" || scheme == "https",
        }
    }
}

impl fmt::Display for RoutePattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.hostname.is_some() || self.port.is_some() || self.protocol != Protocol::Any {
            match self.protocol {
                Protocol::Any => f.write_str("://")?,
                Protocol::Http => f.write_str("http://")?,
                Protocol::Https => f.write_str("https://")?,
                Protocol::HttpOrHttps => f.write_str("http(s)://")?,
            }
            if let Some(h) = &self.hostname {
                f.write_str(&stringify(h))?;
            }
            if let Some(p) = &self.port {
                write!(f, ":{p}")?;
            }
        }
        write!(f, "/{}", stringify(&self.pathname))?;
        for (i, (k, v)) in self.search.iter().enumerate() {
            f.write_str(if i == 0 { "?" } else { "&" })?;
            f.write_str(k)?;
            if let Some(v) = v {
                write!(f, "={v}")?;
            }
        }
        Ok(())
    }
}

impl std::str::FromStr for RoutePattern {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, ParseError> {
        RoutePattern::parse(s)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    Hostname,
    Pathname,
}

fn find_unescaped(s: &str, needle: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i..].starts_with(needle.as_bytes()) {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn push_static(tokens: &mut Vec<Token>, s: &str) {
    if let Some(Token::Static(last)) = tokens.last_mut() {
        last.push_str(s);
    } else {
        tokens.push(Token::Static(s.to_string()));
    }
}

fn tokenize(src: &str, part: Part) -> Result<Vec<Token>, (usize, &'static str)> {
    let mut stack: Vec<Vec<Token>> = vec![Vec::new()];
    let mut chars = src.char_indices().peekable();
    let mut text = String::new();
    let flush = |text: &mut String, stack: &mut Vec<Vec<Token>>| {
        if !text.is_empty() {
            let s = if part == Part::Pathname { encode_path(text) } else { text.clone() };
            push_static(stack.last_mut().expect("stack"), &s);
            text.clear();
        }
    };
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some((_, '/')) if part == Part::Pathname => text.push_str("%2F"),
                Some((_, '.')) => text.push_str("%2E"),
                Some((_, e)) => text.push(e),
                None => return Err((i, "dangling escape")),
            },
            ':' | '*' => {
                flush(&mut text, &mut stack);
                let mut name = String::new();
                while let Some(&(_, n)) = chars.peek() {
                    if n.is_ascii_alphanumeric() || n == '_' || n == '$' {
                        name.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let top = stack.last_mut().expect("stack");
                if c == ':' {
                    if name.is_empty() {
                        return Err((i, "variable needs a name"));
                    }
                    if matches!(top.last(), Some(Token::Var(_) | Token::Wild(_))) {
                        return Err((i, "adjacent captures are ambiguous"));
                    }
                    top.push(Token::Var(name));
                } else {
                    if matches!(top.last(), Some(Token::Var(_) | Token::Wild(_))) {
                        return Err((i, "adjacent captures are ambiguous"));
                    }
                    top.push(Token::Wild((!name.is_empty()).then_some(name)));
                }
            }
            '(' => {
                flush(&mut text, &mut stack);
                stack.push(Vec::new());
            }
            ')' => {
                flush(&mut text, &mut stack);
                if stack.len() < 2 {
                    return Err((i, "unmatched )"));
                }
                let inner = stack.pop().expect("group");
                if inner.is_empty() {
                    return Err((i, "empty optional"));
                }
                stack.last_mut().expect("stack").push(Token::Optional(inner));
            }
            c => text.push(c),
        }
    }
    flush(&mut text, &mut stack);
    if stack.len() != 1 {
        return Err((src.len(), "unclosed ("));
    }
    Ok(stack.pop().expect("root"))
}

fn stringify(tokens: &[Token]) -> String {
    let mut s = String::new();
    for t in tokens {
        match t {
            Token::Static(text) => {
                for c in text.chars() {
                    if matches!(c, ':' | '*' | '(' | ')' | '\\') {
                        s.push('\\');
                    }
                    s.push(c);
                }
            }
            Token::Var(n) => {
                s.push(':');
                s.push_str(n);
            }
            Token::Wild(n) => {
                s.push('*');
                if let Some(n) = n {
                    s.push_str(n);
                }
            }
            Token::Optional(inner) => {
                s.push('(');
                s.push_str(&stringify(inner));
                s.push(')');
            }
        }
    }
    s
}

fn build(tokens: &[Token], params: &impl HrefParams, part: Part) -> Result<String, HrefError> {
    let mut out = String::new();
    for t in tokens {
        match t {
            Token::Static(s) => out.push_str(s),
            Token::Var(name) => {
                let v = params.param(name).ok_or_else(|| HrefError::MissingParam(name.clone()))?;
                if v.is_empty() {
                    return Err(HrefError::EmptyVariable(name.clone()));
                }
                match part {
                    Part::Pathname => out.push_str(&encode_path_segment(&v).replace('.', "%2E")),
                    Part::Hostname => out.push_str(&v),
                }
            }
            Token::Wild(None) => return Err(HrefError::NamelessWildcard),
            Token::Wild(Some(name)) => {
                let v = params.param(name).ok_or_else(|| HrefError::MissingParam(name.clone()))?;
                if part == Part::Pathname {
                    if v.split('/').any(|seg| seg == "." || seg == "..") {
                        return Err(HrefError::InvalidWildcard(name.clone()));
                    }
                    let encoded: Vec<String> = v.split('/').map(encode_path_segment).collect();
                    out.push_str(&encoded.join("/"));
                } else {
                    out.push_str(&v);
                }
            }
            Token::Optional(inner) => {
                // Include the group only when every param inside it is available.
                if let Ok(s) = build(inner, params, part) {
                    out.push_str(&s);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
