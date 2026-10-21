//! Compare matches by specificity (Remix's `specificity.ts`).
//!
//! An explicit protocol beats none, then a port beats none, then the hostname (static
//! before variable before wildcard, from the last label inward), then the pathname
//! (left to right), then search constraints with values, then constraints on keys alone.

use std::cmp::Ordering;

use crate::{CaptureKind, Match, MatchParamMeta, Search};

/// How `a` compares to `b`: `Greater` when `a` is more specific. Both must match the
/// same URL.
///
/// # Panics
///
/// When the matches are for different URLs, as Remix throws.
pub fn compare<A, B>(a: &Match<A>, b: &Match<B>) -> Ordering {
    let (href_a, href_b) = (a.url.href(), b.url.href());
    assert!(href_a == href_b, "Cannot compare matches for different URLs: {href_a} vs {href_b}");
    let (pa, pb) = (a.pattern.parts(), b.pattern.parts());
    let explicit = |p: Option<crate::Protocol>| matches!(p, Some(crate::Protocol::Http | crate::Protocol::Https));
    explicit(pa.protocol)
        .cmp(&explicit(pb.protocol))
        .then(pa.port.is_some().cmp(&pb.port.is_some()))
        .then_with(|| {
            let hostname = crate::punycode::decode_hostname(a.url.hostname());
            compare_hostname(&hostname, &a.params_meta.hostname, &b.params_meta.hostname)
        })
        .then_with(|| compare_pathname(&a.params_meta.pathname, &b.params_meta.pathname))
        .then_with(|| compare_search(&pa.search, &pb.search))
}

/// `a` is less specific than `b`.
pub fn less_than<A, B>(a: &Match<A>, b: &Match<B>) -> bool {
    compare(a, b) == Ordering::Less
}

/// `a` is more specific than `b`.
pub fn greater_than<A, B>(a: &Match<A>, b: &Match<B>) -> bool {
    compare(a, b) == Ordering::Greater
}

/// `a` and `b` are equally specific.
pub fn equal<A, B>(a: &Match<A>, b: &Match<B>) -> bool {
    compare(a, b) == Ordering::Equal
}

/// Sort key order from least to most specific (`matches.sort_by(ascending)`).
pub fn ascending<T>(a: &Match<T>, b: &Match<T>) -> Ordering {
    compare(a, b)
}

/// Sort key order from most to least specific.
pub fn descending<T>(a: &Match<T>, b: &Match<T>) -> Ordering {
    compare(b, a)
}

fn level(kind: CaptureKind) -> i8 {
    match kind {
        CaptureKind::Variable => 1,
        CaptureKind::Wildcard => 2,
    }
}

fn compare_hostname(hostname: &str, a: &[MatchParamMeta], b: &[MatchParamMeta]) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    let units: Vec<u16> = hostname.encode_utf16().collect();
    let encode = |ranges: &[MatchParamMeta]| {
        let mut encoding = vec![0i8; units.len()];
        for range in ranges {
            let end = range.end.min(units.len());
            let begin = range.begin.min(end);
            encoding[begin..end].fill(level(range.kind));
        }
        encoding
    };
    let (ea, eb) = (encode(a), encode(b));
    let mut segments = Vec::new();
    let mut end = units.len();
    for i in (0..units.len()).rev() {
        if units[i] == u16::from(b'.') {
            segments.push((i + 1, end));
            end = i;
        }
    }
    segments.push((0, end));
    for (begin, end) in segments {
        for j in begin..end {
            match ea[j].cmp(&eb[j]) {
                Ordering::Less => return Ordering::Greater,
                Ordering::Greater => return Ordering::Less,
                Ordering::Equal => {}
            }
        }
    }
    Ordering::Equal
}

fn compare_pathname(a: &[MatchParamMeta], b: &[MatchParamMeta]) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    let (mut ai, mut bi) = (0, 0);
    while ai < a.len() || bi < b.len() {
        let (Some(ar), Some(br)) = (a.get(ai), b.get(bi)) else {
            return if ai >= a.len() { Ordering::Greater } else { Ordering::Less };
        };
        let i = ar.begin.min(br.begin);
        if i < ar.begin {
            return Ordering::Greater;
        }
        if i < br.begin {
            return Ordering::Less;
        }
        match (ar.kind, br.kind) {
            (CaptureKind::Variable, CaptureKind::Wildcard) => return Ordering::Greater,
            (CaptureKind::Wildcard, CaptureKind::Variable) => return Ordering::Less,
            _ => {}
        }
        let min_end = ar.end.min(br.end);
        if min_end >= ar.end {
            ai += 1;
        }
        if min_end >= br.end {
            bi += 1;
        }
    }
    Ordering::Equal
}

fn compare_search(a: &Search, b: &Search) -> Ordering {
    let specificity = |search: &Search| {
        let key_value: usize = search.iter().map(|(_, values)| values.len()).sum();
        let key = search.iter().filter(|(_, values)| values.is_empty()).count();
        (key_value, key)
    };
    specificity(a).cmp(&specificity(b))
}
