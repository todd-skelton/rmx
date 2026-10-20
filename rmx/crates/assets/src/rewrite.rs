//! Rewriting the references between assets: CSS `url()` / `@import`, and relative module
//! specifiers in JavaScript.

/// Called with a reference found in a file and whether it is a module import; returns the
/// replacement URL, or `None` to leave the reference alone.
pub(crate) type Link<'a> = dyn FnMut(&str, bool) -> Option<String> + 'a;

/// Rewrite `url(...)` and `@import "..."` references that are relative paths.
pub(crate) fn css(text: &str, link: &mut Link<'_>) -> String {
    let mut out = String::with_capacity(text.len());
    let lower = text.to_ascii_lowercase();
    let mut i = 0;
    while i < text.len() {
        if lower[i..].starts_with("url(") {
            let start = i + 4;
            if let Some((value, quote, end)) = css_url_value(text, start) {
                if let Some(href) = is_relative(&value).then(|| link(&value, false)).flatten() {
                    out.push_str("url(");
                    out.push_str(quote);
                    out.push_str(&href);
                    out.push_str(quote);
                    out.push(')');
                    i = end;
                    continue;
                }
            }
        } else if lower[i..].starts_with("@import") {
            let rest = &text[i + 7..];
            let trimmed = rest.trim_start();
            let skipped = rest.len() - trimmed.len();
            if let Some((value, len)) = string_literal(trimmed) {
                if let Some(href) = is_relative(&value).then(|| link(&value, false)).flatten() {
                    let q = &trimmed[..1];
                    out.push_str("@import");
                    out.push_str(&rest[..skipped]);
                    out.push_str(q);
                    out.push_str(&href);
                    out.push_str(q);
                    i += 7 + skipped + len;
                    continue;
                }
            }
        }
        let ch = text[i..].chars().next().expect("in bounds");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// The value inside `url(...)` starting at `start`: (value, quote, index after `)`).
fn css_url_value(text: &str, start: usize) -> Option<(String, &'static str, usize)> {
    let rest = &text[start..];
    let trimmed = rest.trim_start();
    let lead = rest.len() - trimmed.len();
    if let Some((value, len)) = string_literal(trimmed) {
        let quote = if trimmed.starts_with('"') { "\"" } else { "'" };
        let after = &trimmed[len..];
        let close = after.find(')')?;
        return Some((value, quote, start + lead + len + close + 1));
    }
    let close = rest.find(')')?;
    Some((rest[..close].trim().to_string(), "", start + close + 1))
}

/// A quoted string at the start of `s`: (contents, length including quotes).
fn string_literal(s: &str) -> Option<(String, usize)> {
    let quote = s.chars().next().filter(|c| matches!(c, '"' | '\'' | '`'))?;
    let mut escaped = false;
    for (i, c) in s.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            return Some((s[1..i].to_string(), i + 1));
        } else if c == '\n' {
            return None;
        }
    }
    None
}

/// Not a URL with a scheme, a protocol-relative or absolute path, a fragment or a data URI.
fn is_relative(value: &str) -> bool {
    let v = value.trim();
    !(v.is_empty()
        || v.starts_with('/')
        || v.starts_with('#')
        || v.contains("${")
        || v.split_once(':').is_some_and(|(scheme, _)| scheme.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))))
}

/// Rewrite relative module specifiers (`./x.js`, `../y.js`) after `from`, `import` and
/// `import(`, and relative paths in `new URL('...', import.meta.url)`.
pub(crate) fn script(text: &str, link: &mut Link<'_>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < text.len() {
        let c = bytes[i];
        if matches!(c, b'"' | b'\'') {
            if let Some((value, len)) = string_literal(&text[i..]) {
                let before = text[..i].trim_end();
                let module = (before.ends_with("from") || before.ends_with("import") || before.ends_with("import("))
                    && (value.starts_with("./") || value.starts_with("../"));
                let url_ref = before.ends_with("new URL(") && text[i + len..].trim_start().starts_with(",") && {
                    let after = text[i + len..].trim_start()[1..].trim_start();
                    after.starts_with("import.meta.url")
                } && is_relative(&value);
                if module || url_ref {
                    if let Some(href) = link(&value, module) {
                        let q = c as char;
                        out.push(q);
                        out.push_str(&href);
                        out.push(q);
                        i += len;
                        continue;
                    }
                }
                out.push_str(&text[i..i + len]);
                i += len;
                continue;
            }
        }
        let ch = text[i..].chars().next().expect("in bounds");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upper(found: &mut Vec<(String, bool)>) -> impl FnMut(&str, bool) -> Option<String> + '_ {
        move |spec, import| {
            found.push((spec.to_string(), import));
            Some(format!("/assets/{}", spec.trim_start_matches("./").trim_start_matches("../")))
        }
    }

    #[test]
    fn css_references() {
        let mut found = Vec::new();
        let css = super::css(
            "a { background: url('../images/a.svg') center, url(b.png), url(\"data:image/png;base64,x\"), url(/abs.png), url(#f); }\n@import \"./theme.css\";",
            &mut upper(&mut found),
        );
        assert_eq!(
            css,
            "a { background: url('/assets/images/a.svg') center, url(/assets/b.png), url(\"data:image/png;base64,x\"), url(/abs.png), url(#f); }\n@import \"/assets/theme.css\";"
        );
        assert_eq!(found.len(), 3);
    }

    #[test]
    fn script_references() {
        let mut found = Vec::new();
        let js = script(
            "import init from './pkg/app.js';\nimport 'react';\nexport { x } from \"../x.js\";\nconst m = await import('./lazy.js');\nconst w = new URL('app_bg.wasm', import.meta.url);\nconst s = './not-a-module.js';",
            &mut upper(&mut found),
        );
        assert_eq!(
            js,
            "import init from '/assets/pkg/app.js';\nimport 'react';\nexport { x } from \"/assets/x.js\";\nconst m = await import('/assets/lazy.js');\nconst w = new URL('/assets/app_bg.wasm', import.meta.url);\nconst s = './not-a-module.js';"
        );
        assert_eq!(
            found.iter().map(|(_, import)| *import).collect::<Vec<_>>(),
            vec![true, true, true, false],
            "wasm URLs are linked but are not module imports"
        );
    }
}
