//! Generate hrefs from patterns (Remix's `href.ts`).

use std::fmt;

use rsmix_url::{encode_component, SearchParams, Url};

use crate::{PartPattern, Protocol, RoutePattern, Search, Token, UrlPart};

/// Why an href could not be generated (Remix's `CreateHrefError` details).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HrefError {
    /// The pattern has a protocol or port but no hostname.
    MissingHostname { pattern: String },
    /// Required params with no value, in pattern order.
    MissingParams { pattern: String, missing: Vec<String> },
    /// A required nameless wildcard (`*`), which no param can fill.
    NamelessWildcard { pattern: String },
    /// A hostname variable value containing `.` or a character that would change the URL.
    InvalidHostnameVariable { value: String, char: char },
    /// A hostname wildcard value containing a character that would change the URL.
    InvalidHostnameWildcard { value: String, char: char },
    /// An empty pathname variable value.
    InvalidPathnameVariable { pattern: String, param: String, value: String },
    /// A pathname wildcard value with a `.` or `..` segment.
    InvalidPathnameWildcard { value: String, segment: String },
}

impl HrefError {
    /// Remix's `details.type`, such as `missing-params`.
    pub fn kind(&self) -> &'static str {
        match self {
            HrefError::MissingHostname { .. } => "missing-hostname",
            HrefError::MissingParams { .. } => "missing-params",
            HrefError::NamelessWildcard { .. } => "nameless-wildcard",
            HrefError::InvalidHostnameVariable { .. } => "invalid-hostname-variable",
            HrefError::InvalidHostnameWildcard { .. } => "invalid-hostname-wildcard",
            HrefError::InvalidPathnameVariable { .. } => "invalid-pathname-variable",
            HrefError::InvalidPathnameWildcard { .. } => "invalid-pathname-wildcard",
        }
    }
}

impl fmt::Display for HrefError {
    /// Remix's `CreateHrefError` messages.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HrefError::MissingHostname { pattern } => write!(f, "pattern requires hostname\n\nPattern: {pattern}"),
            HrefError::NamelessWildcard { pattern } => write!(f, "pattern contains nameless wildcard\n\nPattern: {pattern}"),
            HrefError::MissingParams { pattern, missing } => {
                let names: Vec<String> = missing.iter().map(|p| format!("'{p}'")).collect();
                write!(f, "missing param(s): {}\n\nPattern: {pattern}", names.join(", "))
            }
            HrefError::InvalidHostnameVariable { value, char } => {
                write!(f, "invalid hostname variable param: {value:?} contains {:?}", char.to_string())
            }
            HrefError::InvalidHostnameWildcard { value, char } => {
                write!(f, "invalid hostname wildcard param: {value:?} contains {:?}", char.to_string())
            }
            HrefError::InvalidPathnameVariable { pattern, param, value } => {
                write!(f, "invalid pathname variable param: '{param}' cannot be empty\n\nPattern: {pattern}\nValue: {value:?}")
            }
            HrefError::InvalidPathnameWildcard { value, segment } => {
                write!(f, "invalid pathname wildcard param: {value:?} contains dot segment {segment:?}")
            }
        }
    }
}

impl std::error::Error for HrefError {}

/// Anything usable as href params: `&[("id", "1")]`, `[("id", id.as_str())]`, `&Params`, `()`.
/// A param that is `None` is missing, which leaves out the optional around it.
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

impl HrefParams for crate::Params {
    fn param(&self, name: &str) -> Option<String> {
        self.get(name).map(str::to_string)
    }
}

impl<T: HrefParams + ?Sized> HrefParams for &T {
    fn param(&self, name: &str) -> Option<String> {
        (**self).param(name)
    }
}

/// Options for [`RoutePattern::href_with`] (Remix's `CreateHrefOptions`).
#[derive(Clone, Debug, Default)]
pub struct HrefOptions {
    /// Search params placed before the pattern's own constraints.
    pub search_params: SearchParams,
    /// With a base URL, a same-origin target becomes a path-relative href.
    pub base_url: Option<Url>,
}

pub(crate) fn create_href(pattern: &RoutePattern, params: &dyn HrefParams, options: &HrefOptions) -> Result<String, HrefError> {
    let parts = pattern.parts();
    let has_origin = parts.protocol.is_some() || parts.hostname.is_some() || parts.port.is_some();
    let mut result = String::new();
    let mut target_origin = None;
    if has_origin {
        let protocol = match parts.protocol {
            Some(Protocol::Http) => "http",
            _ => "https",
        };
        let Some(hostname) = &parts.hostname else {
            return Err(HrefError::MissingHostname { pattern: pattern.source() });
        };
        let hostname = href_part(pattern, hostname, params)?;
        let port = parts.port.as_ref().map(|p| format!(":{p}")).unwrap_or_default();
        let origin = format!("{protocol}://{hostname}{port}");
        result.push_str(&origin);
        target_origin = Some(origin);
    }
    let pathname = href_part(pattern, &parts.pathname, params)?;
    let pathname = format!("/{}", pathname.trim_start_matches('/'));
    result.push_str(&pathname);
    let search = href_search(&parts.search, &options.search_params);
    let suffix = if search.is_empty() { String::new() } else { format!("?{search}") };
    result.push_str(&suffix);

    if let Some(base) = &options.base_url {
        let target = match &target_origin {
            Some(origin) => Url::parse(origin).map(|u| u.origin()).unwrap_or_else(|_| origin.clone()),
            None => base.origin(),
        };
        if target == base.origin() {
            return Ok(relative_href(&pathname, &suffix, base));
        }
    }
    Ok(result)
}

fn relative_href(pathname: &str, search: &str, base: &Url) -> String {
    let mut base_segments = pathname_segments(base.pathname());
    if !base.pathname().ends_with('/') {
        base_segments.pop();
    }
    let mut target_segments = pathname_segments(pathname);
    let target_is_directory = pathname.ends_with('/');
    let target_file = if target_is_directory { None } else { target_segments.pop() };
    let common = base_segments.iter().zip(&target_segments).take_while(|(a, b)| a == b).count();
    let mut relative: Vec<&str> = vec![".."; base_segments.len() - common];
    relative.extend(target_segments[common..].iter().copied());
    if let Some(file) = target_file {
        relative.push(file);
    }
    let mut result = relative.join("/");
    if target_is_directory {
        result = if result.is_empty() { "./".into() } else { format!("{result}/") };
    }
    if result.is_empty() {
        result = "./".into();
    }
    let first_segment = result.split('/').next().unwrap_or("");
    if result.starts_with(['/', '\\']) || first_segment.contains(':') {
        result = format!("./{result}");
    }
    result + search
}

fn pathname_segments(pathname: &str) -> Vec<&str> {
    let mut segments: Vec<&str> = pathname.get(1..).unwrap_or("").split('/').collect();
    if pathname.ends_with('/') {
        segments.pop();
    }
    segments
}

struct Frame {
    begin: Option<usize>,
    href: String,
}

fn href_part(pattern: &RoutePattern, part: &PartPattern, params: &dyn HrefParams) -> Result<String, HrefError> {
    let separator = part.kind.separator();
    let mut missing: Vec<String> = Vec::new();
    let mut stack = vec![Frame { begin: None, href: String::new() }];
    let mut i = 0;
    while i < part.tokens.len() {
        let top = stack.len() - 1;
        match &part.tokens[i] {
            Token::Text(text) => stack[top].href.push_str(text),
            Token::Separator => stack[top].href.push(separator),
            Token::Open => stack.push(Frame { begin: Some(i), href: String::new() }),
            Token::Close => {
                let frame = stack.pop().expect("open frame");
                stack.last_mut().expect("root frame").href.push_str(&frame.href);
            }
            token @ (Token::Var(name) | Token::Wild(name)) => {
                let is_var = matches!(token, Token::Var(_));
                let Some(value) = params.param(name) else {
                    if stack.len() <= 1 {
                        if name == "*" {
                            return Err(HrefError::NamelessWildcard { pattern: pattern.source() });
                        }
                        if !missing.contains(name) {
                            missing.push(name.clone());
                        }
                        i += 1;
                        continue;
                    }
                    let frame = stack.pop().expect("optional frame");
                    i = part.optionals[&frame.begin.expect("optional begin")] + 1;
                    let parent = stack.last().expect("root frame");
                    if parent.href.ends_with(separator) && part.tokens.get(i) == Some(&Token::Separator) {
                        i += 1;
                    }
                    continue;
                };
                let encoded = match (part.kind, is_var) {
                    (UrlPart::Pathname, true) => {
                        if value.is_empty() {
                            return Err(HrefError::InvalidPathnameVariable {
                                pattern: pattern.source(),
                                param: name.clone(),
                                value,
                            });
                        }
                        encode_pathname_variable(&value)
                    }
                    (UrlPart::Pathname, false) => encode_pathname_wildcard(&value)?,
                    (UrlPart::Hostname, true) => validate_hostname_variable(value)?,
                    (UrlPart::Hostname, false) => validate_hostname_wildcard(value)?,
                };
                stack[top].href.push_str(&encoded);
            }
        }
        i += 1;
    }
    if !missing.is_empty() {
        return Err(HrefError::MissingParams { pattern: pattern.source(), missing });
    }
    Ok(stack.pop().expect("root frame").href)
}

fn href_search(constraints: &Search, search_params: &SearchParams) -> String {
    let mut params = search_params.clone();
    for (key, required) in constraints {
        if required.is_empty() {
            if !params.has(key) {
                params.append(key.clone(), "");
            }
            continue;
        }
        for value in required {
            if !params.get_all(key).contains(&value.as_str()) {
                params.append(key.clone(), value.clone());
            }
        }
    }
    params.to_string()
}

/// A pathname variable: `encodeURIComponent` with `.` escaped too, so a value can never
/// be read back as a dot segment or an extension.
pub fn encode_pathname_variable(value: &str) -> String {
    encode_component(value).replace('.', "%2E")
}

/// A pathname wildcard: each `/`-separated segment encoded; `.` and `..` segments are
/// rejected since URL normalization would remove them.
pub fn encode_pathname_wildcard(value: &str) -> Result<String, HrefError> {
    let mut segments = Vec::new();
    for segment in value.split('/') {
        if segment == "." || segment == ".." {
            return Err(HrefError::InvalidPathnameWildcard { value: value.to_string(), segment: segment.to_string() });
        }
        segments.push(encode_component(segment));
    }
    Ok(segments.join("/"))
}

/// Characters that would change a URL's authority (`@` userinfo, `:` port, `/`, `\\`,
/// `?`, `#` path, query and fragment, `%` escapes) and control characters.
fn is_invalid_hostname_param_char(c: char) -> bool {
    let code = u32::from(c);
    code <= 0x1f || code == 0x7f || matches!(c, '@' | ':' | '/' | '\\' | '?' | '#' | '%')
}

fn validate_hostname_variable(value: String) -> Result<String, HrefError> {
    match value.chars().find(|&c| c == '.' || is_invalid_hostname_param_char(c)) {
        Some(char) => Err(HrefError::InvalidHostnameVariable { value, char }),
        None => Ok(value),
    }
}

fn validate_hostname_wildcard(value: String) -> Result<String, HrefError> {
    match value.chars().find(|&c| is_invalid_hostname_param_char(c)) {
        Some(char) => Err(HrefError::InvalidHostnameWildcard { value, char }),
        None => Ok(value),
    }
}
