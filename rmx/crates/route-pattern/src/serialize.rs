//! Print parsed patterns back to source (Remix's `route-pattern/serialize.ts`).

use rmx_url::SearchParams;

use crate::{Parts, PartPattern, Protocol, Search, Token};

pub(crate) fn serialize_pattern(parts: &Parts) -> String {
    let protocol = serialize_protocol(parts.protocol);
    let hostname = parts.hostname.as_ref().map(serialize_part).unwrap_or_default();
    let port = parts.port.as_deref().unwrap_or("");
    let mut result = String::new();
    if !protocol.is_empty() || !hostname.is_empty() || !port.is_empty() {
        result.push_str(&format!("{protocol}://{hostname}"));
        if !port.is_empty() {
            result.push_str(&format!(":{port}"));
        }
    }
    result.push('/');
    result.push_str(&serialize_part(&parts.pathname));
    let search = serialize_search(&parts.search);
    if !search.is_empty() {
        result.push('?');
        result.push_str(&search);
    }
    result
}

pub(crate) fn serialize_protocol(protocol: Option<Protocol>) -> &'static str {
    match protocol {
        None => "",
        Some(Protocol::Http) => "http",
        Some(Protocol::Https) => "https",
        Some(Protocol::HttpOrHttps) => "http(s)",
    }
}

pub(crate) fn serialize_search(search: &Search) -> String {
    let mut params = SearchParams::new();
    for (key, values) in search {
        if values.is_empty() {
            params.append(key.clone(), "");
        }
        for value in values {
            params.append(key.clone(), value.clone());
        }
    }
    params.to_string()
}

pub(crate) fn serialize_part(part: &PartPattern) -> String {
    let separator = part.kind.separator();
    let mut result = String::new();
    for token in &part.tokens {
        match token {
            Token::Open => result.push('('),
            Token::Close => result.push(')'),
            Token::Text(text) => {
                for c in text.chars() {
                    if matches!(c, ':' | '*' | '(' | ')' | '\\') {
                        result.push('\\');
                    }
                    result.push(c);
                }
            }
            Token::Var(name) => {
                result.push(':');
                result.push_str(name);
            }
            Token::Wild(name) => {
                result.push('*');
                if name != "*" {
                    result.push_str(name);
                }
            }
            Token::Separator => result.push(separator),
        }
    }
    result
}
