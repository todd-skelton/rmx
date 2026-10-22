//! Ports of Remix's `raw-headers.test.ts` and `header-names.test.ts`. `parse` returns a
//! `HeaderMap`; [`SuperHeaders::get_joined`] reads it like `Headers.get`.

use http::header::{HeaderMap, HeaderValue};
use rmx_headers::raw::{canonical_header_name, parse, stringify};
use rmx_headers::SuperHeaders;

fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.append(*name, HeaderValue::from_static(value));
    }
    map
}

#[test]
fn parses() {
    assert_eq!(parse("Content-Type: text/html").unwrap().get_joined("content-type").as_deref(), Some("text/html"));
    let two = parse("Content-Type: text/html\r\nCache-Control: no-cache").unwrap();
    assert_eq!(two.get_joined("Content-Type").as_deref(), Some("text/html"));
    assert_eq!(two.get_joined("Cache-Control").as_deref(), Some("no-cache"));
    let spaced = parse("  Content-Type  :  text/html  ").unwrap();
    assert_eq!(spaced.get_joined("Content-Type").as_deref(), Some("text/html"));
    let cookies = parse("Set-Cookie: a=1\r\nSet-Cookie: b=2").unwrap();
    assert_eq!(cookies.get_joined("Set-Cookie").as_deref(), Some("a=1, b=2"));
    let malformed = parse("Content-Type: text/html\r\nmalformed line\r\nCache-Control: no-cache").unwrap();
    assert_eq!(malformed.len(), 2);
    assert!(parse("").unwrap().is_empty());
    let colons = parse("Location: https://example.com:8080/path").unwrap();
    assert_eq!(colons.get_joined("Location").as_deref(), Some("https://example.com:8080/path"));
    // A name Headers would reject is an error where Remix throws.
    assert!(parse("Bad Name: x").is_err());
    assert!(parse(": no name").unwrap().is_empty());
}

#[test]
fn stringifies() {
    assert_eq!(stringify(&headers(&[("content-type", "text/html")])), "Content-Type: text/html");
    let two = stringify(&headers(&[("content-type", "text/html"), ("cache-control", "no-cache")]));
    assert_eq!(two, "Cache-Control: no-cache\r\nContent-Type: text/html", "sorted like Headers iteration");
    assert_eq!(stringify(&HeaderMap::new()), "");
    let colons = headers(&[("location", "https://example.com:8080/path")]);
    assert_eq!(stringify(&colons), "Location: https://example.com:8080/path");
    let cased = stringify(&headers(&[("etag", "\"abc\""), ("www-authenticate", "Basic"), ("x-custom-header", "value")]));
    assert_eq!(cased, "ETag: \"abc\"\r\nWWW-Authenticate: Basic\r\nX-Custom-Header: value");
    let repeated = headers(&[("set-cookie", "a=1"), ("vary", "accept"), ("set-cookie", "b=2"), ("vary", "origin")]);
    assert_eq!(stringify(&repeated), "Set-Cookie: a=1\r\nSet-Cookie: b=2\r\nVary: accept, origin");
}

#[test]
fn round_trips() {
    let original = headers(&[("content-type", "text/html"), ("cache-control", "no-cache")]);
    let parsed = parse(&stringify(&original)).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn canonical_names() {
    let cases = [
        ("content-type", "Content-Type"),
        ("content-length", "Content-Length"),
        ("user-agent", "User-Agent"),
        ("accept", "Accept"),
        ("etag", "ETag"),
        ("www-authenticate", "WWW-Authenticate"),
        ("x-forwarded-for", "X-Forwarded-For"),
        ("x-xss-protection", "X-XSS-Protection"),
        ("te", "TE"),
        ("expect-ct", "Expect-CT"),
        ("CoNtEnT-TyPe", "Content-Type"),
        ("x-FoRwArDeD-fOr", "X-Forwarded-For"),
        ("authorization", "Authorization"),
        ("host", "Host"),
        ("x-custom-header", "X-Custom-Header"),
        ("x-requested-with", "X-Requested-With"),
        ("x-csrf-token", "X-Csrf-Token"),
        ("x-api-key", "X-Api-Key"),
    ];
    for (name, expected) in cases {
        assert_eq!(canonical_header_name(name), expected);
    }
    let common = [
        "Accept-Charset", "Accept-Encoding", "Accept-Language", "Cache-Control", "Connection", "Cookie", "Date",
        "Expect", "Forwarded", "From", "If-Match", "If-Modified-Since", "If-None-Match", "If-Range",
        "If-Unmodified-Since", "Max-Forwards", "Origin", "Pragma", "Proxy-Authorization", "Range", "Referer",
        "Server", "Transfer-Encoding", "Upgrade", "Via", "Warning", "Alt-Svc", "Content-Disposition",
        "Content-Encoding", "Content-Language", "Content-Location", "Content-Range", "Link", "Location",
        "Retry-After", "Strict-Transport-Security", "Vary",
    ];
    for expected in common {
        assert_eq!(canonical_header_name(&expected.to_lowercase()), expected);
    }
}
