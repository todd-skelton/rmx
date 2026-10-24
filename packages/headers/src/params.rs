//! Shared parsing helpers (param-values.ts, utils.ts).

/// `key=value` parameters separated by `delimiter`, the way Remix's `parseParams` reads
/// them: keys run to whitespace, `=` or the delimiter; values may be quoted (with `\\`
/// escapes) or run to the next delimiter; text after a bare key up to the next delimiter
/// is ignored.
pub fn parse_params(input: &str, delimiter: u8) -> Vec<(String, Option<String>)> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut pos = 0;
    let mut at_start = true;
    loop {
        let start = if at_start {
            at_start = false;
            0
        } else {
            match bytes[pos..].iter().position(|&b| b == delimiter) {
                Some(i) => pos + i + 1,
                None => break,
            }
        };
        let mut i = start;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let key_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' && bytes[i] != delimiter {
            i += 1;
        }
        if i == key_start {
            pos = start;
            continue;
        }
        let key = input[key_start..i].to_string();
        let mut j = i;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < bytes.len() && bytes[j] == b'=' {
            j += 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if bytes.get(j) == Some(&b'"') {
                let mut value = String::new();
                let mut chars = input[j + 1..].char_indices();
                let mut end = bytes.len();
                while let Some((k, c)) = chars.next() {
                    match c {
                        '"' => {
                            end = j + 1 + k + 1;
                            break;
                        }
                        '\\' => match chars.next() {
                            Some((_, escaped)) => value.push(escaped),
                            None => value.push('\\'),
                        },
                        c => value.push(c),
                    }
                }
                out.push((key, Some(value)));
                pos = end;
            } else {
                let end = bytes[j..].iter().position(|&b| b == delimiter).map_or(bytes.len(), |e| j + e);
                out.push((key, Some(unescape(&input[j..end]).trim().to_string())));
                pos = end;
            }
        } else {
            out.push((key, None));
            pos = i;
        }
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(next) => out.push(next),
                None => out.push(c),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Quote a parameter value when it contains `"`, a backslash, `;` or a space, escaping
/// quotes and backslashes so the value round-trips.
pub fn quote(value: &str) -> String {
    if value.contains(['"', '\\', ';', ' ']) {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

/// Split a comma list, trimming only beside the commas.
pub fn split_list(value: &str) -> Vec<&str> {
    let pieces: Vec<&str> = value.split(',').collect();
    let last = pieces.len() - 1;
    pieces
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let p = if i > 0 { p.trim_start() } else { p };
            if i < last {
                p.trim_end()
            } else {
                p
            }
        })
        .collect()
}

/// Wrap a bare entity tag in quotes (`*` and already-quoted tags are kept).
pub fn quote_etag(tag: &str) -> String {
    let quoted = {
        let t = tag.strip_prefix("W/").unwrap_or(tag);
        t.len() >= 2 && t.starts_with('"') && t.ends_with('"')
    };
    if tag == "*" || quoted {
        tag.to_string()
    } else {
        format!("\"{tag}\"")
    }
}

/// JavaScript's `parseInt(text, 10)`: leading whitespace, an optional sign, then as many
/// digits as there are (`"60s"` is 60, `"1.5"` is 1); `None` without digits.
pub(crate) fn parse_int(text: &str) -> Option<i64> {
    let text = text.trim_start();
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let end = digits.bytes().position(|b| !b.is_ascii_digit()).unwrap_or(digits.len());
    let magnitude = digits[..end].bytes().try_fold(0i64, |n, b| n.checked_mul(10)?.checked_add(i64::from(b - b'0')));
    if end == 0 {
        return None;
    }
    let magnitude = magnitude.unwrap_or(i64::MAX);
    Some(if negative { -magnitude } else { magnitude })
}

/// JavaScript's `Number(text)` for q-values: blank is 0, junk is NaN.
pub(crate) fn js_number(text: Option<&str>) -> f64 {
    match text.map(str::trim) {
        None => f64::NAN,
        Some("") => 0.0,
        Some(t) => t.parse().unwrap_or(f64::NAN),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(input: &str) -> Vec<(String, Option<String>)> {
        parse_params(input, b';')
    }

    fn kv(k: &str, v: Option<&str>) -> (String, Option<String>) {
        (k.into(), v.map(Into::into))
    }

    #[test]
    fn params() {
        assert_eq!(p("text/html; charset=utf-8"), vec![kv("text/html", None), kv("charset", Some("utf-8"))]);
        assert_eq!(p(r#"form-data; name="a;b"; filename="x \"y\".txt""#), vec![kv("form-data", None), kv("name", Some("a;b")), kv("filename", Some(r#"x "y".txt"#))]);
        assert_eq!(p("a b; c = d ;e"), vec![kv("a", None), kv("c", Some("d")), kv("e", None)]);
        assert_eq!(p(";;x=1"), vec![kv("x", Some("1"))]);
        assert_eq!(p(r#"k="unterminated"#), vec![kv("k", Some("unterminated"))]);
        assert_eq!(parse_params("no-cache, max-age=5", b','), vec![kv("no-cache", None), kv("max-age", Some("5"))]);
    }

    #[test]
    fn helpers() {
        assert_eq!(quote("plain"), "plain");
        assert_eq!(quote("a b"), r#""a b""#);
        assert_eq!(split_list(" a , b ,c "), vec![" a", "b", "c "]);
        assert_eq!(quote_etag("abc"), r#""abc""#);
        assert_eq!(quote_etag(r#"W/"abc""#), r#"W/"abc""#);
        assert_eq!(quote_etag("*"), "*");
    }
}
