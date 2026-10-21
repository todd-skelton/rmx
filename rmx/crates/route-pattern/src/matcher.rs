//! Matching and ranking.

use rmx_url::{decode, Url};

use crate::{Params, Part, RoutePattern, Token};

/// A successful match: captured params plus the data attached to the pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match<T> {
    pub params: Params,
    pub data: T,
}

/// How specific a match is; greater is more specific (Remix's `Specificity` comparisons).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity(Vec<u8>);

/// Per-character specificity: higher is more specific.
const STATIC: u8 = 3;
const VAR: u8 = 2;
const WILD: u8 = 1;

/// Matches a URL against many patterns and returns the most specific match.
pub struct Matcher<T> {
    entries: Vec<(RoutePattern, T)>,
}

impl<T> Default for Matcher<T> {
    fn default() -> Self {
        Matcher { entries: Vec::new() }
    }
}

impl<T> Matcher<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, pattern: RoutePattern, data: T) {
        self.entries.push((pattern, data));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The most specific match. Ties go to the pattern added first.
    pub fn match_url(&self, url: &Url) -> Option<Match<&T>> {
        let mut best: Option<(Vec<u8>, Params, &T)> = None;
        for (pattern, data) in &self.entries {
            if let Some((params, rank)) = match_one(pattern, url) {
                if best.as_ref().is_none_or(|(r, _, _)| rank > *r) {
                    best = Some((rank, params, data));
                }
            }
        }
        best.map(|(_, params, data)| Match { params, data })
    }

    /// Every match, most specific first.
    pub fn match_all(&self, url: &Url) -> Vec<Match<&T>> {
        self.match_all_ranked(url).into_iter().map(|(_, m)| m).collect()
    }

    /// Every match with its specificity, most specific first (ties in insertion order).
    pub fn match_all_ranked(&self, url: &Url) -> Vec<(Specificity, Match<&T>)> {
        let mut all: Vec<(Vec<u8>, usize, Params, &T)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, (p, d))| match_one(p, url).map(|(params, rank)| (rank, i, params, d)))
            .collect();
        all.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        all.into_iter().map(|(rank, _, params, data)| (Specificity(rank), Match { params, data })).collect()
    }

    /// Patterns and data in insertion order.
    pub fn entries(&self) -> impl Iterator<Item = (&RoutePattern, &T)> {
        self.entries.iter().map(|(p, d)| (p, d))
    }
}

/// Match one pattern; returns params and a rank vector for specificity comparison.
pub(crate) fn match_one(pattern: &RoutePattern, url: &Url) -> Option<(Params, Vec<u8>)> {
    if !pattern.protocol_matches(url.scheme()) {
        return None;
    }
    let (explicit_protocol, port, search) = pattern.origin_constraints();
    if let Some(port) = port {
        let actual = url.port().map(|p| p.to_string()).unwrap_or_else(|| match url.scheme() {
            "https" => "443".into(),
            _ => "80".into(),
        });
        if actual != port {
            return None;
        }
    }
    if !search.is_empty() {
        let params = url.search_params();
        for (k, v) in search {
            match v {
                None if !params.has(k) => return None,
                Some(v) if !params.get_all(k).contains(&v.as_str()) => return None,
                _ => {}
            }
        }
    }

    let mut st = State::default();
    let mut rank = vec![explicit_protocol as u8, port.is_some() as u8];
    let (path_tokens, host_tokens) = pattern.tokens();
    match host_tokens {
        Some(tokens) => {
            let host = url.hostname();
            if !run(tokens, host, 0, Part::Hostname, &mut st, &mut |p, _| p == host.len()) {
                return None;
            }
            let dynamic = st.kinds.iter().any(|&k| k != STATIC);
            rank.push(if dynamic { 1 } else { 2 });
            st.kinds.clear();
        }
        None => rank.push(0),
    }
    rank.push(search.len().min(255) as u8);

    let path = url.pathname().strip_prefix('/').unwrap_or(url.pathname());
    if !run(path_tokens, path, 0, Part::Pathname, &mut st, &mut |p, _| p == path.len()) {
        return None;
    }
    rank.extend(st.kinds);
    let mut params = Params::default();
    for (k, v) in st.caps {
        params.insert(k, v);
    }
    Some((params, rank))
}

#[derive(Default)]
struct State {
    caps: Vec<(String, String)>,
    kinds: Vec<u8>,
}

/// Backtracking matcher in continuation-passing style: when `tokens` are exhausted, `done`
/// decides whether the whole match succeeds. Optionals chain the tokens after the group
/// into the continuation, so a group can end anywhere the rest still matches.
fn run(
    tokens: &[Token],
    input: &str,
    pos: usize,
    part: Part,
    st: &mut State,
    done: &mut dyn FnMut(usize, &mut State) -> bool,
) -> bool {
    let Some((first, rest)) = tokens.split_first() else {
        return done(pos, st);
    };
    let (caps_len, kinds_len) = (st.caps.len(), st.kinds.len());
    let reset = |st: &mut State| {
        st.caps.truncate(caps_len);
        st.kinds.truncate(kinds_len);
    };
    match first {
        Token::Static(s) => {
            let tail = &input[pos..];
            let matches = match part {
                Part::Hostname => tail.len() >= s.len() && tail.is_char_boundary(s.len()) && tail[..s.len()].eq_ignore_ascii_case(s),
                Part::Pathname => tail.starts_with(s.as_str()),
            };
            if !matches {
                return false;
            }
            st.kinds.extend(std::iter::repeat_n(STATIC, s.len()));
            if run(rest, input, pos + s.len(), part, st, done) {
                return true;
            }
            reset(st);
            false
        }
        Token::Var(name) => {
            // Possessive: take the whole run up to the next delimiter.
            let end = input[pos..].find(['/', '.']).map_or(input.len(), |i| pos + i);
            if end == pos {
                return false;
            }
            st.caps.push((name.clone(), decode(&input[pos..end])));
            st.kinds.extend(std::iter::repeat_n(VAR, end - pos));
            if run(rest, input, end, part, st, done) {
                return true;
            }
            reset(st);
            false
        }
        Token::Wild(name) => {
            // Greedy: try the longest capture first.
            for end in (pos..=input.len()).rev().filter(|&e| input.is_char_boundary(e)) {
                if let Some(n) = name {
                    st.caps.push((n.clone(), decode(&input[pos..end])));
                }
                st.kinds.extend(std::iter::repeat_n(WILD, end - pos));
                if run(rest, input, end, part, st, done) {
                    return true;
                }
                reset(st);
            }
            false
        }
        Token::Optional(inner) => {
            let with_group = run(inner, input, pos, part, st, &mut |p, st| run(rest, input, p, part, st, done));
            if with_group {
                return true;
            }
            reset(st);
            run(rest, input, pos, part, st, done)
        }
    }
}