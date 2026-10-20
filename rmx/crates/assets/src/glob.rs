//! Path globs for `allow_files` / `deny_files`: `**` spans any number of directories, `*`
//! and `?` match within one segment, `{a,b}` lists alternatives. A pattern without glob
//! characters also matches everything below it, so a directory can be listed by name.

/// Whether `path` (`/`-separated, relative) matches `pattern`.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    expand_braces(pattern).iter().any(|p| {
        let pattern: Vec<&str> = p.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
        let path: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let literal = !p.contains(['*', '?']);
        segments(&pattern, &path) || (literal && path.len() > pattern.len() && segments(&pattern, &path[..pattern.len()]))
    })
}

fn segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|i| segments(rest, &path[i..])),
        Some((first, rest)) => path.split_first().is_some_and(|(seg, path_rest)| segment(first.as_bytes(), seg.as_bytes()) && segments(rest, path_rest)),
    }
}

fn segment(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|i| segment(rest, &text[i..])),
        Some((b'?', rest)) => !text.is_empty() && segment(rest, &text[1..]),
        Some((c, rest)) => text.first() == Some(c) && segment(rest, &text[1..]),
    }
}

/// `a/{b,c}/*.{ts,tsx}` -> four patterns.
fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else { return vec![pattern.to_string()] };
    let Some(close) = pattern[open..].find('}').map(|i| open + i) else { return vec![pattern.to_string()] };
    let (head, tail) = (&pattern[..open], &pattern[close + 1..]);
    pattern[open + 1..close]
        .split(',')
        .flat_map(|alt| expand_braces(&format!("{head}{alt}{tail}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    #[test]
    fn globs() {
        assert!(glob_match("app/**/public/**", "app/actions/public/styles/app.css"));
        assert!(glob_match("app/**/public/**", "app/public/entry.js"));
        assert!(!glob_match("app/**/public/**", "app/actions/controller.rs"));
        assert!(glob_match("app/**/*.test.*", "app/public/a.test.js"));
        assert!(!glob_match("app/**/*.test.*", "app/public/a.js"));
        assert!(glob_match("app/routes.rs", "app/routes.rs"));
        assert!(glob_match("app/public", "app/public/x/y.css"), "a literal directory covers its contents");
        assert!(!glob_match("app/pub", "app/public/x.css"));
        assert!(glob_match("img/*.{svg,png}", "img/logo.png"));
        assert!(glob_match("img/?.svg", "img/a.svg") && !glob_match("img/?.svg", "img/ab.svg"));
    }
}
