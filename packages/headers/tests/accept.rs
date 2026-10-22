//! Ports of Remix's `accept.test.ts`, `accept-encoding.test.ts` and `accept-language.test.ts`.
//! `new X(string)` and `X.from` are `parse`; array and object inits are `collect()` over
//! `(value, weight)` pairs; `size` is `len`; `getWeight` is `weight`; `getPreferred` is
//! `preferred`; `set(key)` without a weight is `set(key, 1.0)`; `entries`, `forEach` and
//! direct iteration are `iter` and `for` over a reference.

use rmx_headers::{Accept, AcceptEncoding, AcceptLanguage};

fn pairs<'a>(entries: impl IntoIterator<Item = (&'a str, f64)>) -> Vec<(String, f64)> {
    entries.into_iter().map(|(k, w)| (k.to_string(), w)).collect()
}

fn p(list: &[(&str, f64)]) -> Vec<(String, f64)> {
    list.iter().map(|(k, w)| (k.to_string(), *w)).collect()
}

#[test]
fn accept_initializes() {
    assert_eq!(Accept::parse(Some("")).len(), 0);
    assert_eq!(Accept::parse(Some("text/html,application/json;q=0.9")).len(), 2);
    let built: Accept = [("text/html", 1.0), ("application/json", 0.9)].into_iter().collect();
    assert_eq!(built, Accept::parse(Some("text/html,application/json;q=0.9")));
    assert_eq!(built.clone().len(), 2);
    assert_eq!(Accept::parse(Some(" text/html ,  application/json;q=  0.9  ")).len(), 2);
    assert_eq!(Accept::parse(None).len(), 0);
}

#[test]
fn quality_lists_keep_weights_and_order_around_empty_entries() {
    let accept = Accept::parse(Some(" , TEXT/HTML ;q=0.5,\t,application/json;q=0.9, text/html;q=0.7, "));
    assert_eq!(pairs(&accept), p(&[("application/json", 0.9), ("text/html", 0.7)]));
    assert_eq!(Accept::parse(Some(" \t ")).len(), 0);
    let long = format!("text/html,application/json{};q=0.5", " ".repeat(100));
    assert_eq!(pairs(&Accept::parse(Some(&long))), p(&[("text/html", 1.0), ("application/json", 0.5)]));

    let encoding = AcceptEncoding::parse(Some(" , GZIP ;q=0.5,\t,br;q=0.9, gzip;q=0.7, "));
    assert_eq!(pairs(&encoding), p(&[("br", 0.9), ("gzip", 0.7)]));
    assert_eq!(AcceptEncoding::parse(Some(" \t ")).len(), 0);
    let long = format!("gzip,br{};q=0.5", " ".repeat(100));
    assert_eq!(pairs(&AcceptEncoding::parse(Some(&long))), p(&[("gzip", 1.0), ("br", 0.5)]));

    let language = AcceptLanguage::parse(Some(" , EN-US ;q=0.5,\t,fr;q=0.9, en-us;q=0.7, "));
    assert_eq!(pairs(&language), p(&[("fr", 0.9), ("en-us", 0.7)]));
    assert_eq!(AcceptLanguage::parse(Some(" \t ")).len(), 0);
    let long = format!("en-US,fr{};q=0.5", " ".repeat(100));
    assert_eq!(pairs(&AcceptLanguage::parse(Some(&long))), p(&[("en-us", 1.0), ("fr", 0.5)]));
}

#[test]
fn accept_lists_weights_and_negotiates() {
    let h = Accept::parse(Some("text/html,application/json;q=0.9"));
    assert_eq!(h.media_types().collect::<Vec<_>>(), ["text/html", "application/json"]);
    assert_eq!(h.weights().collect::<Vec<_>>(), [1.0, 0.9]);
    let h = Accept::parse(Some("text/html,text/*;q=0.9,application/json;q=0.8"));
    for (ty, ok) in [("text/html", true), ("text/*", true), ("text/plain", true), ("application/json", true), ("image/jpeg", false)] {
        assert_eq!(h.accepts(ty), ok, "{ty}");
    }
    for (ty, w) in [("text/html", 1.0), ("*/*", 1.0), ("text/*", 1.0), ("text/plain", 0.9), ("application/json", 0.8), ("image/jpeg", 0.0)] {
        assert_eq!(h.weight(ty), w, "{ty}");
    }
    assert_eq!(h.preferred(&["text/html", "application/json"]), Some("text/html"));
    assert_eq!(h.preferred(&["text/plain", "text/html"]), Some("text/html"));
    assert_eq!(h.preferred(&["image/jpeg"]), None);
}

#[test]
fn accept_edits_and_sorts() {
    let mut h = Accept::default();
    h.set("application/json", 0.9);
    assert_eq!(h.get("application/json"), Some(0.9));
    let mut one = Accept::parse(Some("text/html"));
    assert!(one.has("text/html"));
    one.delete("text/html");
    assert!(!one.has("text/html"));
    let mut two = Accept::parse(Some("text/html,application/json;q=0.9"));
    assert_eq!(pairs(two.iter()), p(&[("text/html", 1.0), ("application/json", 0.9)]));
    let mut seen = Vec::new();
    for (ty, w) in &two {
        seen.push((ty.to_string(), w));
    }
    assert_eq!(seen, p(&[("text/html", 1.0), ("application/json", 0.9)]));
    assert_eq!(two.to_string(), "text/html,application/json;q=0.9");
    two.set("application/json", 0.8);
    assert_eq!((two.get("application/json"), two.to_string()), (Some(0.8), "text/html,application/json;q=0.8".to_string()));
    two.clear();
    assert!(two.is_empty());
    let mut any = Accept::default();
    any.set("*/*", 1.0);
    assert_eq!(any.get("*/*"), Some(1.0));
    let sorted = Accept::parse(Some("application/json;q=0.9,text/html"));
    assert_eq!((sorted.to_string(), sorted.media_types().collect::<Vec<_>>()), ("text/html,application/json;q=0.9".to_string(), vec!["text/html", "application/json"]));
    let one: Accept = [("text/html", 1.0)].into_iter().collect();
    assert_eq!(one.len(), 1);
}

#[test]
fn accept_encoding_negotiates() {
    assert_eq!(AcceptEncoding::parse(Some("")).len(), 0);
    let built: AcceptEncoding = [("gzip", 1.0), ("deflate", 0.9)].into_iter().collect();
    assert_eq!(built, AcceptEncoding::parse(Some("gzip, deflate;q=0.9")));
    assert_eq!(AcceptEncoding::parse(Some(" gzip ,  deflate;q=  0.9  ")).len(), 2);
    let h = AcceptEncoding::parse(Some("gzip, deflate;q=0.9"));
    assert_eq!((h.encodings().collect::<Vec<_>>(), h.weights().collect::<Vec<_>>()), (vec!["gzip", "deflate"], vec![1.0, 0.9]));
    let h = AcceptEncoding::parse(Some("gzip, deflate;q=0.9,br;q=0.8"));
    for (enc, ok) in [("gzip", true), ("deflate", true), ("br", true), ("compress", false), ("identity", true)] {
        assert_eq!(h.accepts(enc), ok, "{enc}");
    }
    let h = AcceptEncoding::parse(Some("gzip, deflate;q=0.9,*;q=0.8"));
    assert_eq!((h.weight("gzip"), h.weight("deflate"), h.weight("br")), (1.0, 0.9, 0.8));
    assert_eq!(h.preferred(&["gzip", "deflate"]), Some("gzip"));
    assert_eq!(h.preferred(&["deflate", "br"]), Some("deflate"));
}

#[test]
fn accept_encoding_edits_and_sorts() {
    let mut h = AcceptEncoding::default();
    h.set("gzip", 1.0);
    assert_eq!(h.get("gzip"), Some(1.0));
    h.set("*", 0.8);
    assert_eq!(h.get("*"), Some(0.8));
    let mut h = AcceptEncoding::parse(Some("gzip, deflate;q=0.9"));
    assert_eq!(pairs(&h), p(&[("gzip", 1.0), ("deflate", 0.9)]));
    assert_eq!(h.to_string(), "gzip,deflate;q=0.9");
    assert!(h.has("gzip"));
    h.delete("gzip");
    assert!(!h.has("gzip"));
    h.clear();
    assert_eq!(h.len(), 0);
    assert_eq!(AcceptEncoding::parse(Some("deflate;q=0.9,gzip")).to_string(), "gzip,deflate;q=0.9");
    let mut h = AcceptEncoding::parse(Some("gzip;q=0.8,deflate"));
    h.set("br", 1.0);
    assert_eq!(h.to_string(), "deflate,br,gzip;q=0.8");
    h.set("deflate", 0.9);
    assert_eq!(h.to_string(), "br,deflate;q=0.9,gzip;q=0.8");
}

#[test]
fn accept_language_negotiates() {
    assert_eq!(AcceptLanguage::parse(Some("")).len(), 0);
    let built: AcceptLanguage = [("en-US", 1.0), ("en", 0.9)].into_iter().collect();
    assert_eq!(built, AcceptLanguage::parse(Some("en-US,en;q=0.9")));
    assert_eq!(AcceptLanguage::parse(Some(" en-US ,  en;q=  0.9  ")).len(), 2);
    let h = AcceptLanguage::parse(Some("en-US,en;q=0.9"));
    assert_eq!((h.languages().collect::<Vec<_>>(), h.weights().collect::<Vec<_>>()), (vec!["en-us", "en"], vec![1.0, 0.9]));
    let h = AcceptLanguage::parse(Some("en-US,en;q=0.9,fr;q=0.8"));
    for (lang, ok) in [("en-US", true), ("en", true), ("en-GB", true), ("fr", true), ("fi", false)] {
        assert_eq!(h.accepts(lang), ok, "{lang}");
    }
    for (lang, w) in [("en-US", 1.0), ("*", 1.0), ("en", 1.0), ("en-GB", 0.9), ("fr", 0.8), ("fi", 0.0)] {
        assert_eq!(h.weight(lang), w, "{lang}");
    }
    let h = AcceptLanguage::parse(Some("en-US,en;q=0.9"));
    assert_eq!(h.preferred(&["en-GB", "en-US"]), Some("en-US"));
    assert_eq!(h.preferred(&["en-GB", "en"]), Some("en"));
    assert_eq!(h.preferred(&["fr", "en-GB"]), Some("en-GB"));
    assert_eq!(h.preferred(&["fi", "ja"]), None);
}

#[test]
fn accept_language_edits_and_sorts() {
    let mut h = AcceptLanguage::default();
    h.set("en", 0.9);
    assert_eq!(h.get("en"), Some(0.9));
    h.set("en-US", 1.0);
    assert_eq!(h.get("en-US"), Some(1.0));
    h.set("*", 1.0);
    assert_eq!(h.get("*"), Some(1.0));
    let mut over = AcceptLanguage::parse(Some("en;q=0.9"));
    over.set("en", 1.0);
    assert_eq!(over.get("en"), Some(1.0));
    let mut one = AcceptLanguage::parse(Some("en-US"));
    assert!(one.has("en-US"));
    one.delete("en-US");
    assert!(!one.has("en-US"));
    let mut two = AcceptLanguage::parse(Some("en-US,en;q=0.9"));
    assert_eq!(pairs(&two), p(&[("en-us", 1.0), ("en", 0.9)]));
    assert_eq!(two.to_string(), "en-us,en;q=0.9");
    two.clear();
    assert_eq!(two.len(), 0);
    assert_eq!(AcceptLanguage::parse(Some("en;q=0.9,en-US")).to_string(), "en-us,en;q=0.9");
    let mut h = AcceptLanguage::parse(Some("en-US,en;q=0.9"));
    h.set("fi", 1.0);
    assert_eq!(h.to_string(), "en-us,fi,en;q=0.9");
    h.set("en-US", 0.8);
    assert_eq!(h.to_string(), "fi,en;q=0.9,en-us;q=0.8");
}
