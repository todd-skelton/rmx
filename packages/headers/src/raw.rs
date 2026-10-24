//! Raw header blocks (Remix's `raw-headers.ts` and `header-names.ts`): `Name: value` lines
//! separated by CRLF, read into and written from an [`http::HeaderMap`].

use http::header::{HeaderMap, HeaderName, HeaderValue, SET_COOKIE};

const CRLF: &str = "\r\n";

/// Read `Name: value` lines separated by CRLF. Names and values are trimmed, repeated names
/// are appended, and lines without a name before a colon are skipped, as are lines whose
/// name or value a `Headers` object would reject.
pub fn parse(raw: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for line in raw.split(CRLF) {
        let Some((name, value)) = line.split_once(':') else { continue };
        if name.is_empty() {
            continue;
        }
        // Lines a `Headers` object would reject (a bad name or value) are skipped, as in Remix.
        let (Ok(name), Ok(value)) = (HeaderName::from_bytes(name.trim().as_bytes()), HeaderValue::from_bytes(value.trim().as_bytes())) else {
            continue;
        };
        headers.append(name, value);
    }
    headers
}

/// Write headers the way iterating a `Headers` object reads them: names sorted, values of a
/// repeated name joined with `, ` (each `Set-Cookie` on its own line), and names in
/// [`canonical_header_name`] casing.
pub fn stringify(headers: &HeaderMap) -> String {
    let mut names: Vec<&HeaderName> = headers.keys().collect();
    names.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    let mut lines = Vec::new();
    for name in names {
        let canonical = canonical_header_name(name.as_str());
        let values = headers.get_all(name).iter().map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned());
        if name == SET_COOKIE {
            lines.extend(values.map(|v| format!("{canonical}: {v}")));
        } else {
            lines.push(format!("{canonical}: {}", values.collect::<Vec<_>>().join(", ")));
        }
    }
    lines.join(CRLF)
}

/// `content-type` as `Content-Type`, with the casing exceptions Remix knows (`ETag`,
/// `WWW-Authenticate`, `X-XSS-Protection`, `TE`, `Expect-CT`).
pub fn canonical_header_name(name: &str) -> String {
    let words: Vec<String> = name
        .to_lowercase()
        .split('-')
        .map(|word| match word {
            "ct" => "CT".to_string(),
            "etag" => "ETag".to_string(),
            "te" => "TE".to_string(),
            "www" => "WWW".to_string(),
            "x" => "X".to_string(),
            "xss" => "XSS".to_string(),
            _ => {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                }
            }
        })
        .collect();
    words.join("-")
}
