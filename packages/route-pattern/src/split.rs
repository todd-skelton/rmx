//! Split a pattern source into protocol, hostname, port, pathname and search spans
//! (Remix's `route-pattern/split.ts`).

/// A byte range `[begin, end)` of the source.
pub(crate) type Span = (usize, usize);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SplitResult {
    pub protocol: Option<Span>,
    pub hostname: Option<Span>,
    pub port: Option<Span>,
    pub pathname: Option<Span>,
    pub search: Option<Span>,
}

/// Spans for `protocol://hostname:port/pathname?search`. Delimiters are left out, except
/// that a pathname span never includes its leading `/`. Empty parts have no span.
pub(crate) fn split(source: &str) -> SplitResult {
    let mut result = SplitResult::default();
    let mut source = source;
    if let Some(question) = source.find('?') {
        result.search = span(question + 1, source.len());
        source = &source[..question];
    }

    let Some(solidus) = source.find("://") else {
        result.pathname = pathname_span(source, 0, source.len());
        return result;
    };

    let mut slash = source.find('/');
    if slash == Some(solidus + 1) {
        slash = source[solidus + 3..].find('/').map(|i| i + solidus + 3);
    }

    let Some(slash) = slash else {
        result.protocol = span(0, solidus);
        if let Some(host) = span(solidus + 3, source.len()) {
            (result.hostname, result.port) = host_spans(source, host);
        }
        return result;
    };

    if slash < solidus {
        result.pathname = pathname_span(source, 0, source.len());
        return result;
    }

    result.protocol = span(0, solidus);
    if let Some(host) = span(solidus + 3, slash) {
        (result.hostname, result.port) = host_spans(source, host);
    }
    result.pathname = pathname_span(source, slash, source.len());
    result
}

fn span(start: usize, end: usize) -> Option<Span> {
    (start != end).then_some((start, end))
}

fn host_spans(source: &str, host: Span) -> (Option<Span>, Option<Span>) {
    match source[..host.1].rfind(':') {
        Some(colon) if colon >= host.0 => {
            let digits = &source[colon + 1..host.1];
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                (span(host.0, colon), span(colon + 1, host.1))
            } else {
                (Some(host), None)
            }
        }
        _ => (Some(host), None),
    }
}

fn pathname_span(source: &str, begin: usize, end: usize) -> Option<Span> {
    let begin = if source.as_bytes().get(begin) == Some(&b'/') { begin + 1 } else { begin };
    span(begin, end)
}
