//! Parse pattern sources (Remix's `route-pattern/parse.ts`).

use std::collections::BTreeMap;
use std::fmt;

use rmx_url::SearchParams;

use crate::split::{split, Span};
use crate::{Parts, PartPattern, Protocol, Search, Token, UrlPart};

/// Why a pattern could not be parsed (Remix's `ParseError` types).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseErrorType {
    UnmatchedOpen,
    UnmatchedClose,
    MissingVariableName,
    DanglingEscape,
    InvalidProtocol,
    MissingHostname,
    AdjacentWildcards,
    InvalidParamDelimiter,
    EmptyOptional,
    AmbiguousOptionalCaptures,
}

impl ParseErrorType {
    /// Remix's name for the error type, such as `unmatched (`.
    pub fn as_str(self) -> &'static str {
        match self {
            ParseErrorType::UnmatchedOpen => "unmatched (",
            ParseErrorType::UnmatchedClose => "unmatched )",
            ParseErrorType::MissingVariableName => "missing variable name",
            ParseErrorType::DanglingEscape => "dangling escape",
            ParseErrorType::InvalidProtocol => "invalid protocol",
            ParseErrorType::MissingHostname => "missing hostname",
            ParseErrorType::AdjacentWildcards => "adjacent wildcards",
            ParseErrorType::InvalidParamDelimiter => "invalid param delimiter",
            ParseErrorType::EmptyOptional => "empty optional",
            ParseErrorType::AmbiguousOptionalCaptures => "ambiguous optional captures",
        }
    }
}

impl fmt::Display for ParseErrorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A malformed pattern: the error type, the source and where in it parsing failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ParseErrorType,
    pub source: String,
    /// Character index into `source` (UTF-16 units, as in Remix).
    pub index: usize,
}

impl ParseError {
    pub fn new(kind: ParseErrorType, source: &str, index: usize) -> ParseError {
        ParseError { kind, source: source.to_string(), index }
    }

    fn at(kind: ParseErrorType, source: &str, byte: usize) -> ParseError {
        ParseError::new(kind, source, source[..byte].encode_utf16().count())
    }
}

impl fmt::Display for ParseError {
    /// The type, the source and a caret under the failing index, like Remix's message.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\n\n{}\n{}^", self.kind, self.source, " ".repeat(self.index))
    }
}

impl std::error::Error for ParseError {}

pub(crate) fn parse_pattern(source: &str) -> Result<Parts, ParseError> {
    let spans = split(source);
    if let (Some(port), None) = (spans.port, spans.hostname) {
        return Err(ParseError::at(ParseErrorType::MissingHostname, source, port.0 - 1));
    }
    let protocol = parse_protocol(source, spans.protocol)?;
    let hostname = match spans.hostname {
        Some(span) => {
            let part = parse_part(source, span, UrlPart::Hostname)?;
            (!is_nameless_wildcard(&part)).then_some(part)
        }
        None => None,
    };
    let port = spans.port.map(|(a, b)| source[a..b].to_string());
    let pathname = match spans.pathname {
        Some(span) => parse_part(source, span, UrlPart::Pathname)?,
        None => PartPattern::empty(UrlPart::Pathname),
    };
    let search = spans.search.map(|(a, b)| parse_search(&source[a..b])).unwrap_or_default();
    Ok(Parts { protocol, hostname, port, pathname, search })
}

fn is_identifier_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '$'
}

fn is_identifier_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

fn identifier(text: &str) -> Option<&str> {
    let first = text.chars().next().filter(|c| is_identifier_start(*c))?;
    let end = text[first.len_utf8()..].find(|c: char| !is_identifier_char(c)).map_or(text.len(), |i| i + first.len_utf8());
    Some(&text[..end])
}

/// Parse one hostname or pathname part of `source`.
pub(crate) fn parse_part(source: &str, span: Span, kind: UrlPart) -> Result<PartPattern, ParseError> {
    let separator = kind.separator();
    let mut tokens: Vec<Token> = Vec::new();
    // Byte index in `source` where each token starts, for error positions.
    let mut token_indices: Vec<usize> = Vec::new();
    let mut optionals = BTreeMap::new();
    // Optionals in the order they close, the order Remix checks them in.
    let mut closed: Vec<(usize, usize)> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut empty_optionals: Vec<usize> = Vec::new();

    let append_text = |tokens: &mut Vec<Token>, indices: &mut Vec<usize>, text: &str, at: usize| {
        if let Some(Token::Text(current)) = tokens.last_mut() {
            current.push_str(text);
        } else {
            tokens.push(Token::Text(text.to_string()));
            indices.push(at);
        }
    };

    let mut i = span.0;
    while i < span.1 {
        let c = source[i..].chars().next().expect("in bounds");
        match c {
            '(' => {
                stack.push((tokens.len(), i));
                tokens.push(Token::Open);
                token_indices.push(i);
                i += 1;
            }
            ')' => {
                let Some((begin, at)) = stack.pop() else {
                    return Err(ParseError::at(ParseErrorType::UnmatchedClose, source, i));
                };
                if begin == tokens.len() - 1 {
                    empty_optionals.push(at);
                }
                optionals.insert(begin, tokens.len());
                closed.push((begin, tokens.len()));
                tokens.push(Token::Close);
                token_indices.push(i);
                i += 1;
            }
            ':' => {
                let Some(name) = identifier(&source[i + 1..span.1]) else {
                    return Err(ParseError::at(ParseErrorType::MissingVariableName, source, i));
                };
                tokens.push(Token::Var(name.to_string()));
                token_indices.push(i);
                i += 1 + name.len();
            }
            '*' => {
                let name = identifier(&source[i + 1..span.1]);
                tokens.push(Token::Wild(name.unwrap_or("*").to_string()));
                token_indices.push(i);
                i += 1 + name.map_or(0, str::len);
            }
            c if c == separator => {
                tokens.push(Token::Separator);
                token_indices.push(i);
                i += 1;
            }
            '\\' => {
                let Some(escaped) = source[i + 1..span.1].chars().next() else {
                    return Err(ParseError::at(ParseErrorType::DanglingEscape, source, i));
                };
                append_text(&mut tokens, &mut token_indices, &source[i + 1..i + 1 + escaped.len_utf8()], i);
                i += 1 + escaped.len_utf8();
            }
            c => {
                append_text(&mut tokens, &mut token_indices, &source[i..i + c.len_utf8()], i);
                i += c.len_utf8();
            }
        }
    }
    if let Some((_, at)) = stack.last() {
        return Err(ParseError::at(ParseErrorType::UnmatchedOpen, source, *at));
    }
    if let Some(at) = empty_optionals.first() {
        return Err(ParseError::at(ParseErrorType::EmptyOptional, source, *at));
    }
    let part = PartPattern { tokens, optionals, kind };
    validate_token_grammar(source, &part, &token_indices)?;
    validate_optional_capture_schemas(source, &part, &closed, &token_indices)?;
    Ok(part)
}

/// A variable must be followed by a separator, a wildcard, text starting with `.` or the
/// end, and a wildcard must not be followed by another wildcard, whichever optionals are
/// left out.
fn validate_token_grammar(source: &str, part: &PartPattern, indices: &[usize]) -> Result<(), ParseError> {
    for (index, token) in part.tokens.iter().enumerate() {
        let is_var = matches!(token, Token::Var(_));
        if !is_var && !matches!(token, Token::Wild(_)) {
            continue;
        }
        for next in next_consuming_token_indices(part, index + 1) {
            let Some(next_token) = part.tokens.get(next) else { continue };
            if !is_var {
                if matches!(next_token, Token::Wild(_)) {
                    return Err(ParseError::at(ParseErrorType::AdjacentWildcards, source, indices[next]));
                }
                continue;
            }
            let delimits = match next_token {
                Token::Separator | Token::Wild(_) => true,
                Token::Text(text) => text.starts_with('.'),
                _ => false,
            };
            if !delimits {
                return Err(ParseError::at(ParseErrorType::InvalidParamDelimiter, source, indices[next]));
            }
        }
    }
    Ok(())
}

/// Indices of the tokens that can come next after `start`, stepping into and around
/// optionals (`tokens.len()` for the end), in Remix's order.
fn next_consuming_token_indices(part: &PartPattern, start: usize) -> Vec<usize> {
    let mut result = Vec::new();
    let mut pending = vec![start];
    let mut visited = Vec::new();
    while let Some(index) = pending.pop() {
        if visited.contains(&index) {
            continue;
        }
        visited.push(index);
        let found = match part.tokens.get(index) {
            None => part.tokens.len(),
            Some(Token::Open) => {
                let end = part.optionals[&index];
                pending.push(index + 1);
                pending.push(end + 1);
                continue;
            }
            Some(Token::Close) => {
                pending.push(index + 1);
                continue;
            }
            Some(_) => index,
        };
        if !result.contains(&found) {
            result.push(found);
        }
    }
    result
}

/// Neighbouring optionals with the same shape must capture the same names, or a URL could
/// fill either one.
fn validate_optional_capture_schemas(
    source: &str,
    part: &PartPattern,
    closed: &[(usize, usize)],
    indices: &[usize],
) -> Result<(), ParseError> {
    for &(begin, end) in closed {
        let next_begin = end + 1;
        if part.tokens.get(next_begin) != Some(&Token::Open) {
            continue;
        }
        let Some(&next_end) = part.optionals.get(&next_begin) else { continue };
        let first = optional_structure(&part.tokens[begin + 1..end]);
        let second = optional_structure(&part.tokens[next_begin + 1..next_end]);
        if first.0 == second.0 && first.1 != second.1 {
            return Err(ParseError::at(ParseErrorType::AmbiguousOptionalCaptures, source, indices[next_begin]));
        }
    }
    Ok(())
}

fn optional_structure(tokens: &[Token]) -> (String, String) {
    let mut structure = String::new();
    let mut captures = String::new();
    for token in tokens {
        match token {
            Token::Var(name) => {
                structure.push(':');
                captures.push_str(&format!(":{name}|"));
            }
            Token::Wild(name) => {
                structure.push('*');
                captures.push_str(&format!("*{name}|"));
            }
            Token::Text(text) => structure.push_str(&format!("t:{text}|")),
            Token::Separator => structure.push_str("separator|"),
            Token::Open => structure.push_str("(|"),
            Token::Close => structure.push_str(")|"),
        }
    }
    (structure, captures)
}

fn parse_protocol(source: &str, span: Option<Span>) -> Result<Option<Protocol>, ParseError> {
    let Some((a, b)) = span else { return Ok(None) };
    match &source[a..b] {
        "http" => Ok(Some(Protocol::Http)),
        "https" => Ok(Some(Protocol::Https)),
        "http(s)" => Ok(Some(Protocol::HttpOrHttps)),
        _ => Err(ParseError::at(ParseErrorType::InvalidProtocol, source, a)),
    }
}

fn is_nameless_wildcard(part: &PartPattern) -> bool {
    matches!(part.tokens.as_slice(), [Token::Wild(name)] if name == "*")
}

/// Search constraints grouped by name, decoded like `URLSearchParams`: an empty value only
/// requires the key.
fn parse_search(source: &str) -> Search {
    let mut constraints: Search = Vec::new();
    for (key, value) in SearchParams::parse(source).iter() {
        let index = match constraints.iter().position(|(k, _)| k == key) {
            Some(index) => index,
            None => {
                constraints.push((key.to_string(), Vec::new()));
                constraints.len() - 1
            }
        };
        let values = &mut constraints[index].1;
        if !value.is_empty() && !values.iter().any(|v| v == value) {
            values.push(value.to_string());
        }
    }
    constraints
}
