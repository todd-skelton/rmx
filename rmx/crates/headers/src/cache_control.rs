//! `Cache-Control`.

use std::fmt;

use crate::params::parse_params;

/// `Cache-Control` directives. Numbers are seconds; `None`/`false` means absent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CacheControl {
    pub max_age: Option<u64>,
    pub max_stale: Option<u64>,
    pub min_fresh: Option<u64>,
    pub s_maxage: Option<u64>,
    pub no_cache: bool,
    pub no_store: bool,
    pub no_transform: bool,
    pub only_if_cached: bool,
    pub must_revalidate: bool,
    pub proxy_revalidate: bool,
    pub must_understand: bool,
    pub private: bool,
    pub public: bool,
    pub immutable: bool,
    pub stale_while_revalidate: Option<u64>,
    pub stale_if_error: Option<u64>,
}

impl CacheControl {
    pub fn parse(value: Option<&str>) -> CacheControl {
        let mut h = CacheControl::default();
        let Some(value) = value else { return h };
        for (name, val) in parse_params(value, b',') {
            let n = || val.as_deref().and_then(|v| v.trim().parse().ok());
            match name.as_str() {
                "max-age" => h.max_age = n(),
                "max-stale" => h.max_stale = n(),
                "min-fresh" => h.min_fresh = n(),
                "s-maxage" => h.s_maxage = n(),
                "no-cache" => h.no_cache = true,
                "no-store" => h.no_store = true,
                "no-transform" => h.no_transform = true,
                "only-if-cached" => h.only_if_cached = true,
                "must-revalidate" => h.must_revalidate = true,
                "proxy-revalidate" => h.proxy_revalidate = true,
                "must-understand" => h.must_understand = true,
                "private" => h.private = true,
                "public" => h.public = true,
                "immutable" => h.immutable = true,
                "stale-while-revalidate" => h.stale_while_revalidate = n(),
                "stale-if-error" => h.stale_if_error = n(),
                _ => {}
            }
        }
        h
    }
}

impl fmt::Display for CacheControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let flag = |on: bool, name: &str| on.then(|| name.to_string());
        let num = |v: Option<u64>, name: &str| v.map(|v| format!("{name}={v}"));
        let parts: Vec<String> = [
            flag(self.public, "public"),
            flag(self.private, "private"),
            num(self.max_age, "max-age"),
            num(self.s_maxage, "s-maxage"),
            flag(self.no_cache, "no-cache"),
            flag(self.no_store, "no-store"),
            flag(self.no_transform, "no-transform"),
            flag(self.only_if_cached, "only-if-cached"),
            flag(self.must_revalidate, "must-revalidate"),
            flag(self.proxy_revalidate, "proxy-revalidate"),
            flag(self.must_understand, "must-understand"),
            flag(self.immutable, "immutable"),
            num(self.stale_while_revalidate, "stale-while-revalidate"),
            num(self.stale_if_error, "stale-if-error"),
            num(self.max_stale, "max-stale"),
            num(self.min_fresh, "min-fresh"),
        ]
        .into_iter()
        .flatten()
        .collect();
        f.write_str(&parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let h = CacheControl::parse(Some("public, max-age=31536000, immutable"));
        assert!(h.public && h.immutable);
        assert_eq!(h.max_age, Some(31536000));
        assert_eq!(h.to_string(), "public, max-age=31536000, immutable");
        let h = CacheControl { no_store: true, must_revalidate: true, ..Default::default() };
        assert_eq!(h.to_string(), "no-store, must-revalidate");
        assert_eq!(CacheControl::parse(Some("s-maxage=60, stale-while-revalidate=30")).to_string(), "s-maxage=60, stale-while-revalidate=30");
    }
}
