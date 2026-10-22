//! `Accept`, `Accept-Encoding` and `Accept-Language`: weighted lists sorted by q-value.

use std::fmt;

use crate::params::{js_number, parse_params, split_list};

/// Entries in preference order. Re-adding a key keeps its position, like a JS `Map`.
#[derive(Clone, Debug, Default, PartialEq)]
struct QualityList(Vec<(String, f64)>);

impl QualityList {
    fn parse(value: &str) -> QualityList {
        let mut list = QualityList::default();
        for piece in split_list(value) {
            let params = parse_params(piece, b';');
            let Some((first, _)) = params.first() else { continue };
            let weight = params[1..].iter().find(|(k, _)| k == "q").map_or(1.0, |(_, v)| js_number(v.as_deref()));
            list.insert(first.to_lowercase(), weight);
        }
        list.sort();
        list
    }

    fn insert(&mut self, key: String, weight: f64) {
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = weight,
            None => self.0.push((key, weight)),
        }
    }

    fn sort(&mut self) {
        self.0.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    }

    fn set(&mut self, key: &str, weight: f64) {
        self.insert(key.to_lowercase(), weight);
        self.sort();
    }

    fn get(&self, key: &str) -> Option<f64> {
        let key = key.to_lowercase();
        self.0.iter().find(|(k, _)| *k == key).map(|(_, w)| *w)
    }

    fn delete(&mut self, key: &str) {
        let key = key.to_lowercase();
        self.0.retain(|(k, _)| *k != key);
    }

    /// The first entry (in preference order) that `matches`, else 0.
    fn weight(&self, matches: impl Fn(&str) -> bool) -> f64 {
        self.0.iter().find(|(k, _)| matches(k)).map_or(0.0, |(_, w)| *w)
    }

    fn preferred<'a, S: AsRef<str> + ?Sized>(candidates: &'a [&'a S], weight: impl Fn(&str) -> f64) -> Option<&'a S> {
        let mut ranked: Vec<(&S, f64)> = candidates.iter().map(|c| (*c, weight(c.as_ref()))).collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.first().filter(|(_, w)| *w > 0.0).map(|(c, _)| *c)
    }
}

impl fmt::Display for QualityList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, (key, weight)) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            f.write_str(key)?;
            if *weight != 1.0 {
                write!(f, ";q={weight}")?;
            }
        }
        Ok(())
    }
}

macro_rules! quality_header {
    ($name:ident, $what:literal, $items:ident, $always:expr) => {
        impl $name {
            /// Parse a header value; `None` (no header) is empty.
            pub fn parse(value: Option<&str>) -> $name {
                $name(value.map(QualityList::parse).unwrap_or_default())
            }

            #[doc = concat!("Every ", $what, ", most preferred first.")]
            pub fn $items(&self) -> impl Iterator<Item = &str> {
                self.0 .0.iter().map(|(k, _)| k.as_str())
            }

            /// `(value, weight)` pairs, most preferred first.
            pub fn iter(&self) -> impl Iterator<Item = (&str, f64)> {
                self.0 .0.iter().map(|(k, w)| (k.as_str(), *w))
            }

            /// The weights, most preferred first.
            pub fn weights(&self) -> impl Iterator<Item = f64> + '_ {
                self.0 .0.iter().map(|(_, w)| *w)
            }

            pub fn clear(&mut self) {
                self.0 .0.clear();
            }

            pub fn len(&self) -> usize {
                self.0 .0.len()
            }

            pub fn is_empty(&self) -> bool {
                self.0 .0.is_empty()
            }

            /// The weight listed for exactly this value (no wildcard matching).
            pub fn get(&self, key: &str) -> Option<f64> {
                self.0.get(key)
            }

            pub fn has(&self, key: &str) -> bool {
                self.0.get(key).is_some()
            }

            pub fn set(&mut self, key: &str, weight: f64) {
                self.0.set(key, weight)
            }

            pub fn delete(&mut self, key: &str) {
                self.0.delete(key)
            }

            /// Whether `key` has a weight above zero (wildcards count).
            pub fn accepts(&self, key: &str) -> bool {
                let always: fn(&str) -> bool = $always;
                always(key) || self.weight(key) > 0.0
            }

            /// The candidate with the highest weight above zero (ties keep candidate order).
            pub fn preferred<'a, S: AsRef<str> + ?Sized>(&self, candidates: &'a [&'a S]) -> Option<&'a S> {
                QualityList::preferred(candidates, |c| self.weight(c))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        /// `(value, weight)` pairs (Remix's array and object inits), sorted by weight.
        impl<S: AsRef<str>> FromIterator<(S, f64)> for $name {
            fn from_iter<I: IntoIterator<Item = (S, f64)>>(entries: I) -> $name {
                let mut list = QualityList::default();
                for (key, weight) in entries {
                    list.insert(key.as_ref().to_lowercase(), weight);
                }
                list.sort();
                $name(list)
            }
        }

        impl<'a> IntoIterator for &'a $name {
            type Item = (&'a str, f64);
            type IntoIter = std::iter::Map<std::slice::Iter<'a, (String, f64)>, fn(&'a (String, f64)) -> (&'a str, f64)>;

            fn into_iter(self) -> Self::IntoIter {
                self.0 .0.iter().map(|(k, w)| (k.as_str(), *w))
            }
        }
    };
}

/// `Accept`: media types with wildcards (`text/*`, `*/*`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Accept(QualityList);
quality_header!(Accept, "media type", media_types, |_| false);

impl Accept {
    /// The weight of the first entry matching `media_type`, wildcards on either side.
    pub fn weight(&self, media_type: &str) -> f64 {
        let lower = media_type.to_lowercase();
        let (ty, sub) = lower.split_once('/').unwrap_or((&lower, ""));
        self.0.weight(|key| {
            let (t, s) = key.split_once('/').unwrap_or((key, ""));
            (t == ty || t == "*" || ty == "*") && (s == sub || s == "*" || sub == "*")
        })
    }
}

/// `Accept-Encoding`. [`AcceptEncoding::accepts`] always passes `identity`, like Remix.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AcceptEncoding(QualityList);
quality_header!(AcceptEncoding, "encoding", encodings, |key| key.eq_ignore_ascii_case("identity"));

impl AcceptEncoding {
    pub fn weight(&self, encoding: &str) -> f64 {
        let lower = encoding.to_lowercase();
        self.0.weight(|key| key == lower || key == "*" || lower == "*")
    }
}

/// `Accept-Language`: `en` matches `en-US` and the reverse, `*` matches all.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AcceptLanguage(QualityList);
quality_header!(AcceptLanguage, "language tag", languages, |_| false);

impl AcceptLanguage {
    pub fn weight(&self, language: &str) -> f64 {
        let lower = language.to_lowercase();
        let mut parts = lower.split('-');
        let (base, sub) = (parts.next().unwrap_or(""), parts.next());
        self.0.weight(|key| {
            let mut kp = key.split('-');
            let (b, s) = (kp.next().unwrap_or(""), kp.next());
            (b == base || b == "*" || base == "*") && (s == sub || s.is_none() || sub.is_none())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_language_negotiation() {
        let h = AcceptLanguage::parse(Some("fr-CA, fr;q=0.9, en;q=0.8, *;q=0.5"));
        assert_eq!(h.languages().collect::<Vec<_>>(), ["fr-ca", "fr", "en", "*"]);
        assert_eq!(h.weight("fr"), 1.0, "fr matches fr-ca first");
        assert_eq!(h.weight("en-US"), 0.8);
        assert_eq!(h.weight("de"), 0.5);
        assert_eq!(h.preferred(&["en", "fr"]), Some("fr"));
        assert_eq!(h.preferred(&["de", "en"]), Some("en"));
        assert_eq!(h.to_string(), "fr-ca,fr;q=0.9,en;q=0.8,*;q=0.5");

        let ja = AcceptLanguage::parse(Some("ja-JP,ja;q=0.9"));
        assert_eq!(ja.preferred(&["en", "es", "fr", "ja", "ar"]), Some("ja"));
        assert_eq!(AcceptLanguage::parse(Some("de")).preferred(&["en", "es"]), None);
        assert_eq!(AcceptLanguage::parse(None).preferred(&["en"]), None);
        assert_eq!(AcceptLanguage::parse(Some("en;q=0, es")).preferred(&["en", "es"]), Some("es"));
    }

    #[test]
    fn accept_media_types() {
        let mut h = Accept::parse(Some("text/html, application/xhtml+xml, application/xml;q=0.9, */*;q=0.8"));
        assert_eq!(h.weight("text/html"), 1.0);
        assert_eq!(h.weight("image/png"), 0.8);
        assert_eq!(h.preferred(&["application/json", "text/html"]), Some("text/html"));
        assert!(h.accepts("text/*"));
        h.set("application/json", 0.95);
        assert_eq!(h.media_types().nth(2), Some("application/json"));
        h.delete("*/*");
        assert!(!h.accepts("image/png"));
    }

    #[test]
    fn accept_encoding() {
        let h = AcceptEncoding::parse(Some("gzip;q=0.5, br, identity;q=0"));
        assert_eq!(h.preferred(&["gzip", "br"]), Some("br"));
        assert!(h.accepts("identity"), "identity always passes, even with q=0");
        assert!(!h.accepts("deflate"));
        assert!(AcceptEncoding::parse(Some("*")).accepts("zstd"));
    }
}
