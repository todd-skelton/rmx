//! Join two patterns (Remix's `join.ts`).

use std::collections::BTreeMap;

use crate::{Parts, PartPattern, Search, Token, UrlPart};

pub(crate) fn join_patterns(base: &Parts, next: &Parts) -> Parts {
    Parts {
        protocol: next.protocol.or(base.protocol),
        hostname: next.hostname.clone().or_else(|| base.hostname.clone()),
        port: next.port.clone().or_else(|| base.port.clone()),
        pathname: join_pathname(&base.pathname, &next.pathname),
        search: join_search(&base.search, &next.search),
    }
}

/// `a` + `b` is `a/b`: a trailing separator on `base` is dropped and one is added
/// before `next` unless it starts with one (optionals around them are kept in place).
fn join_pathname(base: &PartPattern, next: &PartPattern) -> PartPattern {
    if base.tokens.is_empty() {
        return next.clone();
    }
    if next.tokens.is_empty() {
        return base.clone();
    }
    let is_group = |t: &Token| matches!(t, Token::Open | Token::Close);
    let base_last = base.tokens.iter().rposition(|t| !is_group(t));
    let base_has_trailing_separator = base_last.is_some_and(|i| base.tokens[i] == Token::Separator);

    let mut tokens = Vec::new();
    for (index, token) in base.tokens.iter().enumerate() {
        if Some(index) == base_last && *token == Token::Separator {
            continue;
        }
        tokens.push(token.clone());
    }
    let next_first = next.tokens.iter().find(|t| !is_group(t));
    if next_first != Some(&Token::Separator) {
        tokens.push(Token::Separator);
    }
    let offset = tokens.len();
    tokens.extend(next.tokens.iter().cloned());

    let mut optionals = BTreeMap::new();
    for (&begin, &end) in &base.optionals {
        let shift = |i: usize| match base_last {
            Some(last) if base_has_trailing_separator && i > last => i - 1,
            _ => i,
        };
        optionals.insert(shift(begin), shift(end));
    }
    for (&begin, &end) in &next.optionals {
        optionals.insert(offset + begin, offset + end);
    }
    PartPattern { tokens, optionals, kind: UrlPart::Pathname }
}

/// Constraints from both patterns, values merged per key.
fn join_search(base: &Search, next: &Search) -> Search {
    let mut result = base.clone();
    for (name, values) in next {
        match result.iter_mut().find(|(n, _)| n == name) {
            Some((_, current)) => {
                for value in values {
                    if !current.contains(value) {
                        current.push(value.clone());
                    }
                }
            }
            None => result.push((name.clone(), values.clone())),
        }
    }
    result
}
