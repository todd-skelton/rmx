//! Compiled part programs and the matching algorithm (Remix's `match/program.ts`).
//!
//! Pattern text and URL parts become canonical units: one decoded character each, with
//! `.` (and `/` in URL pathnames) marked structural. Escaped characters in a URL stay data,
//! so `%2F` never acts as a separator and `%2E` never ends a variable.

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::limits::{Budget, MatcherResourceError};
use crate::{CaptureKind, MatchParamMeta, PartPattern, Token, UrlPart};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Unit {
    pub value: char,
    pub structural: bool,
    pub begin: usize,
    pub end: usize,
}

impl Unit {
    pub fn key(&self) -> (bool, char) {
        (self.structural, self.value)
    }

    fn same(&self, other: &Unit) -> bool {
        self.value == other.value && self.structural == other.structural
    }
}

pub(crate) struct CanonicalText {
    pub units: Vec<Unit>,
    pub length: usize,
}

#[derive(Clone, Debug)]
enum Compiled {
    Unit(Unit),
    Var(String),
    Wild(String),
    Open,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MatchKind {
    Static,
    Linear,
    State,
}

#[derive(Clone, Debug)]
pub(crate) struct PartProgram {
    kind: UrlPart,
    tokens: Vec<Compiled>,
    optionals: BTreeMap<usize, usize>,
    match_kind: MatchKind,
    pub static_prefix: Vec<Unit>,
    pub static_suffix: Vec<Unit>,
    pub static_anchor: Vec<Unit>,
}

pub(crate) fn compile_part(part: &PartPattern, ignore_case: bool) -> PartProgram {
    let mut tokens = Vec::new();
    let mut optionals = BTreeMap::new();
    let mut stack = Vec::new();
    for token in &part.tokens {
        match token {
            Token::Text(text) => {
                let canonical = canonicalize_text(text, part.kind, ignore_case, false, true).expect("pattern text always canonicalizes");
                tokens.extend(canonical.units.into_iter().map(Compiled::Unit));
            }
            Token::Separator => {
                tokens.push(Compiled::Unit(Unit { value: part.kind.separator(), structural: true, begin: 0, end: 1 }));
            }
            Token::Open => {
                stack.push(tokens.len());
                tokens.push(Compiled::Open);
            }
            Token::Close => {
                let begin = stack.pop().expect("balanced optionals");
                optionals.insert(begin, tokens.len());
                tokens.push(Compiled::Close);
            }
            Token::Var(name) | Token::Wild(name) => {
                tokens.push(if matches!(token, Token::Var(_)) { Compiled::Var(name.clone()) } else { Compiled::Wild(name.clone()) });
            }
        }
    }

    let unit = |t: &Compiled| match t {
        Compiled::Unit(u) => Some(*u),
        _ => None,
    };
    let static_prefix: Vec<Unit> = tokens.iter().map_while(unit).collect();
    let mut static_suffix: Vec<Unit> = tokens.iter().rev().map_while(unit).collect();
    static_suffix.reverse();

    let mut static_anchor: Vec<Unit> = Vec::new();
    let mut run: Vec<Unit> = Vec::new();
    let mut depth = 0;
    for token in &tokens {
        match token {
            Compiled::Open => {
                if run.len() > static_anchor.len() {
                    static_anchor = std::mem::take(&mut run);
                }
                run.clear();
                depth += 1;
            }
            Compiled::Close => depth -= 1,
            Compiled::Unit(u) if depth == 0 => run.push(*u),
            _ if depth == 0 => {
                if run.len() > static_anchor.len() {
                    static_anchor = std::mem::take(&mut run);
                }
                run.clear();
            }
            _ => {}
        }
    }
    if run.len() > static_anchor.len() {
        static_anchor = run;
    }

    let mut match_kind = MatchKind::Static;
    for token in &tokens {
        match token {
            Compiled::Wild(_) | Compiled::Open | Compiled::Close => {
                match_kind = MatchKind::State;
                break;
            }
            Compiled::Var(_) => match_kind = MatchKind::Linear,
            Compiled::Unit(_) => {}
        }
    }

    PartProgram { kind: part.kind, tokens, optionals, match_kind, static_prefix, static_suffix, static_anchor }
}

fn matches_at(input: &[Unit], position: usize, expected: &[Unit]) -> bool {
    position + expected.len() <= input.len() && expected.iter().zip(&input[position..]).all(|(e, i)| e.same(i))
}

pub(crate) fn has_static_prefix(program: &PartProgram, input: &CanonicalText, budget: &mut Budget) -> Result<bool, MatcherResourceError> {
    if program.static_prefix.len() > input.units.len() {
        return Ok(false);
    }
    budget.consume(program.static_prefix.len())?;
    Ok(matches_at(&input.units, 0, &program.static_prefix))
}

pub(crate) fn has_static_suffix(program: &PartProgram, input: &CanonicalText, budget: &mut Budget) -> Result<bool, MatcherResourceError> {
    let Some(offset) = input.units.len().checked_sub(program.static_suffix.len()) else { return Ok(false) };
    budget.consume(program.static_suffix.len())?;
    Ok(matches_at(&input.units, offset, &program.static_suffix))
}

pub(crate) fn has_static_anchor(program: &PartProgram, input: &CanonicalText, budget: &mut Budget) -> Result<bool, MatcherResourceError> {
    let anchor = &program.static_anchor;
    if anchor.is_empty() {
        return Ok(true);
    }
    let mut position = 0;
    while position + anchor.len() <= input.units.len() {
        budget.consume(anchor.len())?;
        if matches_at(&input.units, position, anchor) {
            return Ok(true);
        }
        position += 1;
    }
    Ok(false)
}

/// Canonical units for a URL part, or `None` when it holds a malformed escape.
pub(crate) fn canonicalize_url_part(text: &str, kind: UrlPart, ignore_case: bool, budget: &mut Budget) -> Result<Option<CanonicalText>, MatcherResourceError> {
    budget.consume(text.encode_utf16().count())?;
    Ok(canonicalize_text(text, kind, ignore_case, true, false))
}

fn canonicalize_text(text: &str, kind: UrlPart, ignore_case: bool, strict: bool, pattern: bool) -> Option<CanonicalText> {
    let mut units = Vec::new();
    let mut offset = 0;
    let mut chunk = String::new();
    let append_chunk = |chunk: &mut String, units: &mut Vec<Unit>, offset: &mut usize| -> bool {
        if chunk.is_empty() {
            return true;
        }
        let decoded = match decode_uri_component(chunk) {
            Some(decoded) => decoded,
            None if strict => return false,
            None => chunk.clone(),
        };
        let decoded = if ignore_case { decoded.to_lowercase() } else { decoded };
        for value in decoded.chars() {
            let width = encoded_width(value, kind);
            units.push(Unit { value, structural: false, begin: *offset, end: *offset + width });
            *offset += width;
        }
        chunk.clear();
        true
    };
    for c in text.chars() {
        let structural = c == '.' || (kind == UrlPart::Pathname && !pattern && c == '/');
        if !structural {
            chunk.push(c);
            continue;
        }
        if !append_chunk(&mut chunk, &mut units, &mut offset) {
            return None;
        }
        units.push(Unit { value: c, structural: true, begin: offset, end: offset + 1 });
        offset += 1;
    }
    if !append_chunk(&mut chunk, &mut units, &mut offset) {
        return None;
    }
    Some(CanonicalText { units, length: offset })
}

/// How long a decoded character is once encoded back into its URL part.
fn encoded_width(value: char, kind: UrlPart) -> usize {
    match kind {
        UrlPart::Pathname if value == '.' || value == '/' => 3,
        UrlPart::Pathname => rmx_url::encode_component(value.encode_utf8(&mut [0; 4])).len(),
        UrlPart::Hostname => value.len_utf16(),
    }
}

/// JavaScript's `decodeURIComponent`: `None` for a malformed escape or invalid UTF-8.
pub(crate) fn decode_uri_component(text: &str) -> Option<String> {
    if !text.contains('%') {
        return Some(text.to_string());
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok().filter(|_| hex.bytes().all(|b| b.is_ascii_hexdigit()))?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

struct CaptureNode {
    kind: CaptureKind,
    name: String,
    begin: usize,
    end: usize,
    next: Option<Rc<CaptureNode>>,
}

#[derive(Clone)]
struct Solution {
    captures: Option<Rc<CaptureNode>>,
    count: usize,
}

fn prepend(solution: Option<&Solution>, kind: CaptureKind, name: &str, begin: usize, end: usize) -> Option<Solution> {
    let solution = solution?;
    Some(Solution {
        captures: Some(Rc::new(CaptureNode { kind, name: name.to_string(), begin, end, next: solution.captures.clone() })),
        count: solution.count + 1,
    })
}

/// The params a part program captures from `input`, or `None` when it does not match.
/// Among several ways to match, the most specific one wins: static characters before
/// variables before wildcards, compared left to right in pathnames and from the last
/// hostname label inward, then fewer captures.
pub(crate) fn match_part(program: &PartProgram, input: &CanonicalText, budget: &mut Budget) -> Result<Option<Vec<MatchParamMeta>>, MatcherResourceError> {
    let state_count = program.tokens.len() + 1;
    let input_len = input.units.len();
    match program.match_kind {
        MatchKind::Static => {
            budget.consume(state_count + input_len)?;
            if program.tokens.len() != input_len {
                return Ok(None);
            }
            let all = program.tokens.iter().zip(&input.units).all(|(t, u)| matches!(t, Compiled::Unit(e) if e.same(u)));
            return Ok(all.then(Vec::new));
        }
        MatchKind::Linear => {
            budget.consume(state_count + input_len)?;
            return Ok(match_linear(program, input));
        }
        MatchKind::State => {}
    }
    budget.consume(state_count.saturating_mul(input_len + 1))?;
    let hostname_order = (program.kind == UrlPart::Hostname).then(|| hostname_positions(&input.units));
    let width = input_len + 1;
    let mut solutions: Vec<Option<Solution>> = vec![None; state_count * width];
    let at = |state: usize, position: usize| state * width + position;
    let token_count = program.tokens.len();
    for position in (0..=input_len).rev() {
        solutions[at(token_count, position)] = (position == input_len).then_some(Solution { captures: None, count: 0 });
        for state in (0..token_count).rev() {
            let value = match &program.tokens[state] {
                Compiled::Close => solutions[at(state + 1, position)].clone(),
                Compiled::Open => {
                    let end = program.optionals[&state];
                    let omitted = solutions[at(end + 1, position)].clone();
                    let included = solutions[at(state + 1, position)].clone();
                    better(input_len, hostname_order.as_deref(), omitted, included, budget)?
                }
                Compiled::Unit(expected) => match input.units.get(position) {
                    Some(unit) if unit.same(expected) => solutions[at(state + 1, position + 1)].clone(),
                    _ => None,
                },
                Compiled::Var(name) => {
                    let mut end = position;
                    while end < input_len && !input.units[end].structural {
                        budget.consume(1)?;
                        end += 1;
                    }
                    let suffix = if end == position { None } else { solutions[at(state + 1, end)].as_ref() };
                    prepend(suffix, CaptureKind::Variable, name, position, end)
                }
                Compiled::Wild(name) => {
                    let consumed = if position < input_len {
                        solutions[at(state, position + 1)].as_ref().map(|next| {
                            let capture = next.captures.as_ref().expect("wildcard continuation");
                            Solution {
                                captures: Some(Rc::new(CaptureNode {
                                    kind: capture.kind,
                                    name: capture.name.clone(),
                                    begin: position,
                                    end: capture.end,
                                    next: capture.next.clone(),
                                })),
                                count: next.count,
                            }
                        })
                    } else {
                        None
                    };
                    let exited = prepend(solutions[at(state + 1, position)].as_ref(), CaptureKind::Wildcard, name, position, position);
                    better(input_len, hostname_order.as_deref(), consumed, exited, budget)?
                }
            };
            solutions[at(state, position)] = value;
        }
    }
    let Some(solution) = solutions[at(0, 0)].take() else { return Ok(None) };
    let mut result = Vec::with_capacity(solution.count);
    let mut capture = solution.captures.as_deref();
    while let Some(node) = capture {
        result.push(to_meta(input, node.kind, &node.name, node.begin, node.end));
        capture = node.next.as_deref();
    }
    Ok(Some(result))
}

fn match_linear(program: &PartProgram, input: &CanonicalText) -> Option<Vec<MatchParamMeta>> {
    let mut captures = Vec::new();
    let mut position = 0;
    for token in &program.tokens {
        match token {
            Compiled::Unit(expected) => {
                if !input.units.get(position).is_some_and(|u| u.same(expected)) {
                    return None;
                }
                position += 1;
            }
            Compiled::Var(name) => {
                let begin = position;
                while position < input.units.len() && !input.units[position].structural {
                    position += 1;
                }
                if position == begin {
                    return None;
                }
                captures.push(to_meta(input, CaptureKind::Variable, name, begin, position));
            }
            _ => unreachable!("linear programs hold only units and variables"),
        }
    }
    (position == input.units.len()).then_some(captures)
}

fn to_meta(input: &CanonicalText, kind: CaptureKind, name: &str, begin: usize, end: usize) -> MatchParamMeta {
    MatchParamMeta {
        kind,
        name: name.to_string(),
        value: input.units[begin..end].iter().map(|u| u.value).collect(),
        begin: offset_at(input, begin),
        end: offset_at(input, end),
    }
}

fn offset_at(input: &CanonicalText, position: usize) -> usize {
    input.units.get(position).map_or(input.length, |u| u.begin)
}

fn better(
    input_len: usize,
    hostname_order: Option<&[usize]>,
    preferred: Option<Solution>,
    alternative: Option<Solution>,
    budget: &mut Budget,
) -> Result<Option<Solution>, MatcherResourceError> {
    match (preferred, alternative) {
        (None, alternative) => Ok(alternative),
        (preferred, None) => Ok(preferred),
        (Some(a), Some(b)) => Ok(Some(if compare_solutions(input_len, hostname_order, &a, &b, budget)? >= 0 { a } else { b })),
    }
}

fn compare_solutions(input_len: usize, hostname_order: Option<&[usize]>, a: &Solution, b: &Solution, budget: &mut Budget) -> Result<i8, MatcherResourceError> {
    match hostname_order {
        None => {
            let mut a_capture = a.captures.as_deref();
            let mut b_capture = b.captures.as_deref();
            for position in 0..input_len {
                budget.consume(1)?;
                while let Some(c) = a_capture.filter(|c| c.end <= position) {
                    budget.consume(1)?;
                    a_capture = c.next.as_deref();
                }
                while let Some(c) = b_capture.filter(|c| c.end <= position) {
                    budget.consume(1)?;
                    b_capture = c.next.as_deref();
                }
                let (sa, sb) = (specificity_at(a_capture, position), specificity_at(b_capture, position));
                if sa < sb {
                    return Ok(1);
                }
                if sa > sb {
                    return Ok(-1);
                }
            }
        }
        Some(order) => {
            let ea = encode_specificity(input_len, a.captures.as_deref(), budget)?;
            let eb = encode_specificity(input_len, b.captures.as_deref(), budget)?;
            for &position in order {
                budget.consume(1)?;
                if ea[position] < eb[position] {
                    return Ok(1);
                }
                if ea[position] > eb[position] {
                    return Ok(-1);
                }
            }
        }
    }
    Ok(match a.count.cmp(&b.count) {
        std::cmp::Ordering::Less => 1,
        std::cmp::Ordering::Greater => -1,
        std::cmp::Ordering::Equal => 0,
    })
}

fn specificity_at(capture: Option<&CaptureNode>, position: usize) -> u8 {
    match capture {
        Some(c) if c.begin <= position => match c.kind {
            CaptureKind::Variable => 1,
            CaptureKind::Wildcard => 2,
        },
        _ => 0,
    }
}

fn encode_specificity(len: usize, captures: Option<&CaptureNode>, budget: &mut Budget) -> Result<Vec<u8>, MatcherResourceError> {
    budget.consume(len)?;
    let mut encoding = vec![0u8; len];
    let mut capture = captures;
    while let Some(c) = capture {
        budget.consume(1 + c.end - c.begin)?;
        let value = if c.kind == CaptureKind::Variable { 1 } else { 2 };
        encoding[c.begin..c.end].fill(value);
        capture = c.next.as_deref();
    }
    Ok(encoding)
}

/// Positions label by label from the last hostname label to the first.
fn hostname_positions(input: &[Unit]) -> Vec<usize> {
    let mut result = Vec::with_capacity(input.len());
    let mut end = input.len();
    for i in (0..input.len()).rev() {
        if input[i].structural && input[i].value == '.' {
            result.extend(i + 1..end);
            end = i;
        }
    }
    result.extend(0..end);
    result
}
