//! `Vary`.

use std::fmt;

/// A case-insensitive set of header names, kept lowercase in insertion order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vary(Vec<String>);

impl Vary {
    /// Parse one header value; `None` (no header) is empty.
    pub fn parse(value: Option<&str>) -> Vary {
        let mut v = Vary::default();
        for name in value.unwrap_or_default().split(',') {
            v.add(name);
        }
        v
    }

    /// Merge every `Vary` header in `headers`.
    pub fn from_headers<'a>(values: impl IntoIterator<Item = &'a str>) -> Vary {
        let mut v = Vary::default();
        for value in values {
            for name in value.split(',') {
                v.add(name);
            }
        }
        v
    }

    pub fn add(&mut self, name: &str) {
        let name = name.trim().to_lowercase();
        if !name.is_empty() && !self.0.contains(&name) {
            self.0.push(name);
        }
    }

    pub fn has(&self, name: &str) -> bool {
        self.0.contains(&name.trim().to_lowercase())
    }

    pub fn delete(&mut self, name: &str) {
        let name = name.trim().to_lowercase();
        self.0.retain(|n| *n != name);
    }

    pub fn header_names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for Vary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_case_insensitively() {
        let mut v = Vary::parse(Some("Accept-Encoding, Origin"));
        v.add("origin");
        v.add("Cookie");
        assert_eq!(v.to_string(), "accept-encoding, origin, cookie");
        assert!(v.has("ORIGIN"));
        assert_eq!(Vary::from_headers(["a", "B, a"]).to_string(), "a, b");
    }
}
