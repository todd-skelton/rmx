//! `SearchParams`: an ordered list of name/value pairs, like the web's `URLSearchParams`.

use std::fmt;

use crate::encode::{decode_form, encode_form};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchParams {
    pairs: Vec<(String, String)>,
}

impl SearchParams {
    pub fn new() -> SearchParams {
        SearchParams::default()
    }

    /// Parse a query string or form body. A leading `?` is ignored.
    pub fn parse(input: &str) -> SearchParams {
        let input = input.strip_prefix('?').unwrap_or(input);
        let pairs = input
            .split('&')
            .filter(|s| !s.is_empty())
            .map(|pair| match pair.split_once('=') {
                Some((k, v)) => (decode_form(k), decode_form(v)),
                None => (decode_form(pair), String::new()),
            })
            .collect();
        SearchParams { pairs }
    }

    /// The first value for `name`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn get_all(&self, name: &str) -> Vec<&str> {
        self.pairs.iter().filter(|(k, _)| k == name).map(|(_, v)| v.as_str()).collect()
    }

    pub fn has(&self, name: &str) -> bool {
        self.pairs.iter().any(|(k, _)| k == name)
    }

    pub fn append(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.pairs.push((name.into(), value.into()));
    }

    /// Replace every value for `name` with one value, keeping the first position.
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        match self.pairs.iter().position(|(k, _)| *k == name) {
            Some(i) => {
                self.pairs[i].1 = value;
                let mut idx = 0;
                self.pairs.retain(|(k, _)| {
                    let keep = idx <= i || *k != name;
                    idx += 1;
                    keep
                });
            }
            None => self.pairs.push((name, value)),
        }
    }

    pub fn delete(&mut self, name: &str) {
        self.pairs.retain(|(k, _)| k != name);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.pairs.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

impl fmt::Display for SearchParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, (k, v)) in self.pairs.iter().enumerate() {
            if i > 0 {
                f.write_str("&")?;
            }
            write!(f, "{}={}", encode_form(k), encode_form(v))?;
        }
        Ok(())
    }
}

impl<K: Into<String>, V: Into<String>> FromIterator<(K, V)> for SearchParams {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        SearchParams { pairs: iter.into_iter().map(|(k, v)| (k.into(), v.into())).collect() }
    }
}

impl<'a> IntoIterator for &'a SearchParams {
    type Item = (&'a str, &'a str);
    type IntoIter = Box<dyn Iterator<Item = (&'a str, &'a str)> + 'a>;
    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_set_and_serialize() {
        let mut p = SearchParams::parse("?a=1&b=two+words&a=3&c");
        assert_eq!(p.get("a"), Some("1"));
        assert_eq!(p.get_all("a"), vec!["1", "3"]);
        assert_eq!(p.get("b"), Some("two words"));
        assert_eq!(p.get("c"), Some(""));
        p.set("a", "x");
        assert_eq!(p.to_string(), "a=x&b=two+words&c=");
        p.delete("b");
        p.append("d", "é&");
        assert_eq!(p.to_string(), "a=x&c=&d=%C3%A9%26");
    }
}
