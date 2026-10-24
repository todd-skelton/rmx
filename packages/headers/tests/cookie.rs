//! Ports of Remix's `headers/src/lib/cookie.test.ts`. `new Cookie(string)` and `Cookie.from` are
//! [`Cookie::parse`]; array, object and `Cookie` inits collect from pairs. `get` returns
//! `None` where Remix returns `null`, `size` is `len`, and `entries`, `forEach` and direct
//! iteration are `iter`.

use rsmix_headers::Cookie;

fn parse(value: &str) -> Cookie {
    Cookie::parse(Some(value))
}

fn pairs(header: &Cookie) -> Vec<(&str, &str)> {
    header.iter().collect()
}

#[test]
fn initializes() {
    assert_eq!(parse("").len(), 0);
    assert_eq!(Cookie::parse(None), Cookie::default());
    let h = parse("name1=value1; name2=value2");
    assert_eq!((h.get("name1"), h.get("name2")), (Some("value1"), Some("value2")));
    let from_pairs: Cookie = [("name1", "value1"), ("name2", "value2")].into_iter().collect();
    assert_eq!(from_pairs, h);
    let copied: Cookie = h.clone().into_iter().collect();
    assert_eq!(copied, h);
    let spaced = parse(" name1 = value1 ;  name2  =  value2 ");
    assert_eq!((spaced.get("name1"), spaced.get("name2")), (Some("value1"), Some("value2")));
    let parsed = parse("session=abc123; user=john");
    assert_eq!((parsed.get("session"), parsed.get("user")), (Some("abc123"), Some("john")));
}

#[test]
fn duplicate_names_keep_their_order() {
    let h = parse("session=child; session=parent; theme=dark");
    assert_eq!(h.get("session"), Some("child"));
    assert_eq!(h.get_all("session"), ["child", "parent"]);
    assert!(h.get_all("missing").is_empty());
    assert_eq!(h.names(), ["session", "session", "theme"]);
    assert_eq!(h.values(), ["child", "parent", "dark"]);
    assert_eq!(h.len(), 3);
    assert_eq!(pairs(&h), [("session", "child"), ("session", "parent"), ("theme", "dark")]);
    assert_eq!(h.to_string(), "session=child; session=parent; theme=dark");
}

#[test]
fn semicolons_always_split_pairs() {
    let h = parse(r#"first="one; session=child; session=parent; last=two""#);
    assert_eq!(pairs(&h), [("first", "\"one"), ("session", "child"), ("session", "parent"), ("last", "two\"")]);
    assert_eq!(h.get("session"), Some("child"));
    let escaped = parse(r#"first="one\; second=two""#);
    assert_eq!(pairs(&escaped), [("first", r#""one\"#), ("second", "two\"")]);
}

#[test]
fn decodes_values() {
    assert_eq!(pairs(&parse(r#"unquoted=a\b; quoted="c\d""#)), [("unquoted", "ab"), ("quoted", "cd")]);
    let trimmed = parse("first=\" \tvalue \t\"; second=\" \tvalue \t");
    assert_eq!(pairs(&trimmed), [("first", "value"), ("second", "\" \tvalue")]);
    let mixed = parse(r#"quoted="abc=="; encoded=a%3Bb; empty=""; bare; =ignored; ;"#);
    assert_eq!(pairs(&mixed), [("quoted", "abc=="), ("encoded", "a%3Bb"), ("empty", ""), ("bare", "")]);
    let h = parse("name1=value1; name2");
    assert_eq!((h.get("name1"), h.get("name2")), (Some("value1"), Some("")));
}

#[test]
fn round_trips() {
    let quoted: Cookie = [("name", "a\"b")].into_iter().collect();
    assert_eq!(quoted.to_string(), r#"name="a\"b""#);
    assert_eq!(parse(&quoted.to_string()).get("name"), Some("a\"b"));
    let semis: Cookie = [("name", "a;b;c")].into_iter().collect();
    assert_eq!(semis.to_string(), "name=a%3Bb%3Bc");
    assert_eq!(pairs(&parse(&semis.to_string())), [("name", "a%3Bb%3Bc")]);
    assert_eq!(parse("name1=value1; name2=value2").to_string(), "name1=value1; name2=value2");
}

#[test]
fn edits() {
    let mut h = Cookie::default();
    assert_eq!(h.get("name"), None);
    h.set("name", "value");
    assert_eq!(h.get("name"), Some("value"));

    let mut appended = parse("name=value1");
    appended.append("name", "value2");
    assert_eq!(appended.get_all("name"), ["value1", "value2"]);
    assert_eq!(appended.to_string(), "name=value1; name=value2");

    let mut deleted = parse("name=value1; other=value; name=value2");
    assert!(deleted.has("name"));
    deleted.delete("name");
    assert!(!deleted.has("name") && !deleted.has("nonexistent"));
    assert_eq!(pairs(&deleted), [("other", "value")]);

    let mut cleared = parse("name1=value1; name2=value2");
    assert_eq!(cleared.len(), 2);
    cleared.clear();
    assert!(cleared.is_empty());

    let mut empty = parse("");
    empty.set("name", "");
    assert_eq!(empty.get("name"), Some(""));
    assert_eq!(empty.to_string(), "name=");

    let mut overwritten = parse("name=value1; other=value; name=value2");
    overwritten.set("name", "value2");
    assert_eq!(overwritten.get_all("name"), ["value2"]);
    assert_eq!(pairs(&overwritten), [("name", "value2"), ("other", "value")]);
}

#[test]
fn iterates() {
    let h = parse("name1=value1; name2=value2");
    assert_eq!(h.names(), ["name1", "name2"]);
    assert_eq!(h.values(), ["value1", "value2"]);
    let mut seen = Vec::new();
    for (name, value) in h.iter() {
        seen.push((name, value));
    }
    assert_eq!(seen, [("name1", "value1"), ("name2", "value2")]);
    let owned: Vec<(String, String)> = h.into_iter().collect();
    assert_eq!(owned, [("name1".to_string(), "value1".to_string()), ("name2".to_string(), "value2".to_string())]);
}
