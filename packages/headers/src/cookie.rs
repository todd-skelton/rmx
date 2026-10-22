//! The `Cookie` request header and the `Set-Cookie` response header.

use std::fmt;
use std::time::SystemTime;

use crate::date::{format_http_date, parse_http_date};
use crate::params::{parse_int, parse_params, quote};

/// `Cookie: a=1; b=2` as ordered name/value pairs (values are not decoded).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cookie(Vec<(String, String)>);

impl Cookie {
    pub fn parse(value: Option<&str>) -> Cookie {
        let mut pairs = Vec::new();
        for piece in value.unwrap_or_default().split(';') {
            let trimmed = piece.trim_start();
            let name_end = trimmed.find(|c: char| c == '=' || c.is_whitespace()).unwrap_or(trimmed.len());
            if name_end == 0 {
                continue;
            }
            let name = &trimmed[..name_end];
            let rest = trimmed[name_end..].trim_start();
            let raw = rest.strip_prefix('=').map(str::trim_start).unwrap_or("");
            let value = match raw.strip_prefix('"').and_then(|r| quoted_prefix(r)) {
                Some(inner) => inner,
                None => raw,
            };
            pairs.push((name.to_string(), unescape(value).trim().to_string()));
        }
        Cookie(pairs)
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    pub fn get_all(&self, name: &str) -> Vec<&str> {
        self.0.iter().filter(|(n, _)| n == name).map(|(_, v)| v.as_str()).collect()
    }

    pub fn has(&self, name: &str) -> bool {
        self.0.iter().any(|(n, _)| n == name)
    }

    /// Replace every pair named `name` with one, at the first one's position.
    pub fn set(&mut self, name: &str, value: &str) {
        match self.0.iter().position(|(n, _)| n == name) {
            Some(i) => {
                self.0[i].1 = value.to_string();
                let mut seen = false;
                self.0.retain(|(n, _)| n != name || !std::mem::replace(&mut seen, true));
            }
            None => self.0.push((name.to_string(), value.to_string())),
        }
    }

    pub fn append(&mut self, name: &str, value: &str) {
        self.0.push((name.to_string(), value.to_string()));
    }

    pub fn delete(&mut self, name: &str) {
        self.0.retain(|(n, _)| n != name);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(n, v)| (n.as_str(), v.as_str()))
    }

    /// Every cookie name in order, repeats included.
    pub fn names(&self) -> Vec<&str> {
        self.0.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// Every cookie value in order.
    pub fn values(&self) -> Vec<&str> {
        self.0.iter().map(|(_, v)| v.as_str()).collect()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The text inside a quoted value that starts right after the opening quote.
fn quoted_prefix(rest: &str) -> Option<&str> {
    let mut escaped = false;
    for (i, c) in rest.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => return Some(&rest[..i]),
            _ => {}
        }
    }
    None
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (c, c == '\\') {
            (_, true) => out.push(chars.next().unwrap_or(c)),
            (c, false) => out.push(c),
        }
    }
    out
}

impl<K: Into<String>, V: Into<String>> FromIterator<(K, V)> for Cookie {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Cookie {
        Cookie(iter.into_iter().map(|(k, v)| (k.into(), v.into())).collect())
    }
}

impl IntoIterator for Cookie {
    type Item = (String, String);
    type IntoIter = std::vec::IntoIter<(String, String)>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl fmt::Display for Cookie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pairs: Vec<String> = self.0.iter().map(|(n, v)| format!("{n}={}", quote(&v.replace(';', "%3B")))).collect();
        f.write_str(&pairs.join("; "))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SameSite {
    Strict,
    Lax,
    None,
}

/// A parsed `Set-Cookie` header.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetCookie {
    pub name: String,
    pub value: String,
    pub domain: Option<String>,
    pub expires: Option<SystemTime>,
    pub http_only: bool,
    pub max_age: Option<i64>,
    pub partitioned: bool,
    pub path: Option<String>,
    pub same_site: Option<SameSite>,
    pub secure: bool,
}

impl SetCookie {
    pub fn parse(value: &str) -> Option<SetCookie> {
        let mut params = parse_params(value, b';').into_iter();
        let (name, value) = params.next()?;
        let mut h = SetCookie { name, value: value.unwrap_or_default(), ..Default::default() };
        for (key, val) in params {
            match key.to_lowercase().as_str() {
                "domain" => h.domain = val,
                "expires" => h.expires = val.as_deref().and_then(parse_http_date),
                "httponly" => h.http_only = true,
                "max-age" => h.max_age = val.as_deref().and_then(parse_int).or(h.max_age),
                "partitioned" => h.partitioned = true,
                "path" => h.path = val,
                "samesite" => {
                    h.same_site = match val.as_deref().map(str::to_lowercase).as_deref() {
                        Some("strict") => Some(SameSite::Strict),
                        Some("lax") => Some(SameSite::Lax),
                        Some("none") => Some(SameSite::None),
                        _ => h.same_site,
                    }
                }
                "secure" => h.secure = true,
                _ => {}
            }
        }
        Some(h)
    }
}

impl fmt::Display for SetCookie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.name.is_empty() {
            return Ok(());
        }
        write!(f, "{}={}", self.name, quote(&self.value))?;
        if let Some(d) = self.domain.as_deref().filter(|d| !d.is_empty()) {
            write!(f, "; Domain={d}")?;
        }
        if let Some(e) = self.expires {
            write!(f, "; Expires={}", format_http_date(e))?;
        }
        if self.http_only {
            f.write_str("; HttpOnly")?;
        }
        if let Some(m) = self.max_age {
            write!(f, "; Max-Age={m}")?;
        }
        if self.partitioned {
            f.write_str("; Partitioned")?;
        }
        if let Some(p) = self.path.as_deref().filter(|p| !p.is_empty()) {
            write!(f, "; Path={p}")?;
        }
        if let Some(s) = self.same_site {
            write!(f, "; SameSite={s:?}")?;
        }
        if self.secure {
            f.write_str("; Secure")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    #[test]
    fn cookie_header() {
        let mut c = Cookie::parse(Some(r#"theme=dark; session="a\"b"; x; theme=light"#));
        assert_eq!(c.get("theme"), Some("dark"));
        assert_eq!(c.get_all("theme"), vec!["dark", "light"]);
        assert_eq!(c.get("session"), Some(r#"a"b"#));
        assert_eq!(c.get("x"), Some(""));
        c.set("theme", "blue");
        assert_eq!(c.get_all("theme"), vec!["blue"]);
        c.delete("x");
        assert_eq!(c.to_string(), r#"theme=blue; session="a\"b""#);
        let built: Cookie = [("sid", "a;b")].into_iter().collect();
        assert_eq!(built.to_string(), "sid=a%3Bb");
    }

    #[test]
    fn set_cookie_round_trip() {
        let h = SetCookie::parse("sid=abc; Path=/; Max-Age=60; HttpOnly; SameSite=lax; Secure; Expires=Sun, 06 Nov 1994 08:49:37 GMT").unwrap();
        assert_eq!((h.name.as_str(), h.value.as_str()), ("sid", "abc"));
        assert_eq!(h.max_age, Some(60));
        assert_eq!(h.same_site, Some(SameSite::Lax));
        assert!(h.http_only && h.secure);
        assert_eq!(h.expires, Some(UNIX_EPOCH + Duration::from_secs(784_111_777)));
        assert_eq!(h.to_string(), "sid=abc; Expires=Sun, 06 Nov 1994 08:49:37 GMT; HttpOnly; Max-Age=60; Path=/; SameSite=Lax; Secure");
    }
}
