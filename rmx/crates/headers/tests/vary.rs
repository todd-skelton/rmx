//! Ports of Remix's `headers/src/lib/vary.test.ts`. `new Vary(string)` and `Vary.from` are
//! [`Vary::parse`]; an array is `collect()`; `size` is `len`; `forEach` is a `for` loop.

use rmx_headers::Vary;

fn names(v: &Vary) -> Vec<&str> {
    v.header_names().collect()
}

#[test]
fn initializes_from_strings_lists_and_copies() {
    let empty = Vary::parse(Some(""));
    assert_eq!((empty.len(), names(&empty)), (0, vec![]));
    let parsed = Vary::parse(Some("Accept-Encoding, Accept-Language"));
    assert_eq!((names(&parsed), parsed.len()), (vec!["accept-encoding", "accept-language"], 2));
    let listed: Vary = ["Accept-Encoding", "Accept-Language"].into_iter().collect();
    assert_eq!(listed, parsed);
    assert_eq!(parsed.clone(), parsed);
    assert_eq!(names(&Vary::parse(Some("  Accept-Encoding  ,   Accept-Language  "))), ["accept-encoding", "accept-language"]);
    let mixed: Vary = ["Accept-Encoding", "ACCEPT-LANGUAGE", "user-agent"].into_iter().collect();
    assert_eq!(names(&mixed), ["accept-encoding", "accept-language", "user-agent"]);
    assert_eq!(Vary::parse(Some("Accept-Encoding, Accept-Language, User-Agent")).len(), 3);
}

#[test]
fn has_is_case_insensitive() {
    let v = Vary::parse(Some("Accept-Encoding, Accept-Language"));
    assert!(v.has("accept-encoding") && v.has("Accept-Encoding") && v.has("ACCEPT-ENCODING"));
    assert!(!v.has("user-agent"));
}

#[test]
fn adds_without_duplicates_or_blanks() {
    let mut v = Vary::default();
    v.add("Accept-Encoding");
    assert!(v.has("Accept-Encoding") && v.len() == 1);
    v.add("Accept-Language");
    v.add("User-Agent");
    assert_eq!(names(&v), ["accept-encoding", "accept-language", "user-agent"]);
    let mut one = Vary::parse(Some("Accept-Encoding"));
    for name in ["accept-encoding", "ACCEPT-ENCODING", "Accept-Encoding"] {
        one.add(name);
    }
    assert_eq!(names(&one), ["accept-encoding"]);
    let mut blank = Vary::default();
    blank.add("");
    blank.add("  ");
    assert!(blank.is_empty());
}

#[test]
fn deletes_and_clears() {
    let mut v: Vary = ["Accept-Encoding", "Accept-Language"].into_iter().collect();
    v.delete("Accept-Encoding");
    assert_eq!((v.has("Accept-Encoding"), names(&v)), (false, vec!["accept-language"]));
    let mut lower: Vary = ["Accept-Encoding", "Accept-Language"].into_iter().collect();
    lower.delete("accept-encoding");
    assert_eq!(lower.len(), 1);
    let mut all: Vary = ["Accept-Encoding", "Accept-Language", "User-Agent"].into_iter().collect();
    all.clear();
    assert!(all.is_empty() && names(&all).is_empty());
}

#[test]
fn to_string_and_iteration() {
    let v: Vary = ["Accept-Encoding", "Accept-Language", "User-Agent"].into_iter().collect();
    assert_eq!(v.to_string(), "accept-encoding, accept-language, user-agent");
    assert_eq!(Vary::default().to_string(), "");
    let mut seen = Vec::new();
    for name in &v {
        seen.push(name);
    }
    assert_eq!(seen, ["accept-encoding", "accept-language", "user-agent"]);
    assert_eq!(v.iter().count(), 3);
}
