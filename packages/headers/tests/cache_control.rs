//! Ports of Remix's `headers/src/lib/cache-control.test.ts`. Directives are struct fields;
//! an object init is a struct literal.

use rsmix_headers::CacheControl;

fn parse(value: &str) -> CacheControl {
    CacheControl::parse(Some(value))
}

type Case = (&'static str, &'static str, fn(&CacheControl) -> String, fn(&mut CacheControl));

/// Remix's `paramTestCases`: each directive, its value, how to read it and how to set it.
const CASES: [Case; 16] = [
    ("max-age", "3600", |h| format!("{:?}", h.max_age), |h| h.max_age = Some(3600)),
    ("max-stale", "7200", |h| format!("{:?}", h.max_stale), |h| h.max_stale = Some(7200)),
    ("min-fresh", "1800", |h| format!("{:?}", h.min_fresh), |h| h.min_fresh = Some(1800)),
    ("s-maxage", "3600", |h| format!("{:?}", h.s_maxage), |h| h.s_maxage = Some(3600)),
    ("no-cache", "", |h| h.no_cache.to_string(), |h| h.no_cache = true),
    ("no-store", "", |h| h.no_store.to_string(), |h| h.no_store = true),
    ("no-transform", "", |h| h.no_transform.to_string(), |h| h.no_transform = true),
    ("only-if-cached", "", |h| h.only_if_cached.to_string(), |h| h.only_if_cached = true),
    ("must-revalidate", "", |h| h.must_revalidate.to_string(), |h| h.must_revalidate = true),
    ("proxy-revalidate", "", |h| h.proxy_revalidate.to_string(), |h| h.proxy_revalidate = true),
    ("must-understand", "", |h| h.must_understand.to_string(), |h| h.must_understand = true),
    ("private", "", |h| h.private.to_string(), |h| h.private = true),
    ("public", "", |h| h.public.to_string(), |h| h.public = true),
    ("immutable", "", |h| h.immutable.to_string(), |h| h.immutable = true),
    ("stale-while-revalidate", "60", |h| format!("{:?}", h.stale_while_revalidate), |h| h.stale_while_revalidate = Some(60)),
    ("stale-if-error", "120", |h| format!("{:?}", h.stale_if_error), |h| h.stale_if_error = Some(120)),
];

#[test]
fn initializes_empty_and_from_strings() {
    let empty = parse("");
    assert_eq!((empty.max_age, empty.public, empty.to_string()), (None, false, String::new()));
    let h = parse("public, max-age=3600, s-maxage=3600");
    assert_eq!((h.max_age, h.s_maxage, h.public), (Some(3600), Some(3600), true));
    let h = parse("max-age=3600, public");
    assert_eq!((h.max_age, h.public), (Some(3600), true));
}

#[test]
fn every_directive_parses_and_prints() {
    for (name, value, read, set) in CASES {
        let parsed = parse(&format!("{name}={value}"));
        let expected = if value.is_empty() { "true".to_string() } else { format!("Some({value})") };
        assert_eq!(read(&parsed), expected, "{name}");
        let mut built = CacheControl::default();
        set(&mut built);
        let printed = if value.is_empty() { name.to_string() } else { format!("{name}={value}") };
        assert_eq!(built.to_string(), printed, "{name}");
    }
}

#[test]
fn builds_copies_and_edits() {
    let built = CacheControl { public: true, max_age: Some(3600), s_maxage: Some(3600), ..Default::default() };
    assert_eq!(built, parse("public, max-age=3600, s-maxage=3600"));
    assert_eq!(built.clone(), built);
    let spaced = parse(" public , max-age = 3600, s-maxage=3600 ");
    assert_eq!((spaced.max_age, spaced.s_maxage, spaced.public), (Some(3600), Some(3600), true), "whitespace");
    let mut edited = parse("");
    edited.max_age = Some(3600);
    edited.s_maxage = Some(3600);
    edited.public = true;
    assert_eq!(edited, built);
    assert_eq!(parse("public, max-age=3600, s-maxage=3600").to_string(), "public, max-age=3600, s-maxage=3600");
    assert_eq!(CacheControl { max_age: Some(0), ..Default::default() }.to_string(), "max-age=0", "zero is kept");
}
