//! Ports of Remix's `headers/src/lib/content-type.test.ts`. `new ContentType(string)` and
//! `ContentType.from` are [`ContentType::parse`]; an object init is a struct literal.

use rmx_headers::ContentType;

fn parse(value: &str) -> ContentType {
    ContentType::parse(Some(value))
}

fn some(value: &str) -> Option<String> {
    Some(value.to_string())
}

#[test]
fn initializes() {
    let empty = parse("");
    assert_eq!((empty.media_type, empty.charset), (None, None));
    let h = parse("text/plain; charset=utf-8");
    assert_eq!((h.media_type.clone(), h.charset.clone()), (some("text/plain"), some("utf-8")));
    let built = ContentType { media_type: some("text/plain"), charset: some("utf-8"), boundary: None };
    assert_eq!(built, h);
    assert_eq!(h.clone(), h);
    let spaced = parse(" text/html ;  charset = iso-8859-1 ");
    assert_eq!((spaced.media_type, spaced.charset), (some("text/html"), some("iso-8859-1")));
}

#[test]
fn sets_fields() {
    let mut h = parse("text/plain");
    h.media_type = some("application/json");
    h.charset = some("utf-8");
    assert_eq!((h.media_type.as_deref(), h.charset.as_deref()), (Some("application/json"), Some("utf-8")));
    let mut form = parse("multipart/form-data");
    form.boundary = some("abc123");
    assert_eq!(form.boundary.as_deref(), Some("abc123"));
}

#[test]
fn reads_parameters() {
    assert_eq!(parse("text/plain; charset=\"us-ascii\"").charset, some("us-ascii"));
    let h = parse("multipart/form-data; boundary=\"abc123\"; charset=utf-8");
    assert_eq!((h.media_type, h.boundary, h.charset), (some("multipart/form-data"), some("abc123"), some("utf-8")));
    assert_eq!(parse("Text/HTML").media_type, some("Text/HTML"), "media type case is kept");
    assert_eq!(parse("multipart/form-data; boundary=\"---=_Part_0_1234567.89\"").boundary, some("---=_Part_0_1234567.89"));
    assert_eq!(parse("text/plain; charset=").charset, some(""));
    assert_eq!(parse("text/plain; charset").charset, None, "no value");
}

#[test]
fn to_string() {
    assert_eq!(parse("text/plain; charset=utf-8").to_string(), "text/plain; charset=utf-8");
    assert_eq!(ContentType { charset: some("utf-8"), ..Default::default() }.to_string(), "", "no media type");
    let mut form = parse("multipart/form-data");
    form.boundary = some("abc 123");
    assert_eq!(form.to_string(), "multipart/form-data; boundary=\"abc 123\"");
    assert_eq!(parse("multipart/form-data; charset=utf-8; boundary=abc123").to_string(), "multipart/form-data; charset=utf-8; boundary=abc123");
    assert_eq!(parse("text/plain; charset=").to_string(), "text/plain", "empty values are left out");
}

#[test]
fn parameter_edge_cases() {
    assert_eq!(parse(r"multipart/form-data; note=foo\; boundary=abc").boundary, some("abc"), "a backslash does not escape outside quotes");
    assert_eq!(parse("multipart/form-data; boundary=\"abc").boundary, some("abc"), "unterminated quote");
    let inside = parse("multipart/form-data; note=\"value; boundary=abc; charset=utf-8");
    assert_eq!((inside.boundary, inside.charset), (None, None), "nothing is read inside an unterminated quote");
    assert_eq!(parse("multipart/form-data; boundary=abc; note=\"value").boundary, some("abc"));
    assert_eq!(parse("multipart/form-data; boundary=\" abc \" ").boundary, some(" abc "));
    assert_eq!(parse("multipart/form-data; boundary=\" abc ").boundary, some(" abc"));
    let mixed = parse("Multipart/Form-Data; BOUNDARY=\"MixedCaseBoundary\"");
    assert_eq!((mixed.media_type, mixed.boundary), (some("Multipart/Form-Data"), some("MixedCaseBoundary")));
    let html = parse("Text/HTML; CharSet=\"UTF-8\"");
    assert_eq!((html.media_type, html.charset), (some("Text/HTML"), some("UTF-8")));
}

#[test]
fn the_first_value_wins() {
    assert_eq!(parse("multipart/form-data; boundary=first; boundary=second").boundary, some("first"));
    assert_eq!(parse("text/html; charset=utf-8; charset=iso-8859-1").charset, some("utf-8"));
    let cased = parse("multipart/form-data; BOUNDARY=First; boundary=Second; CharSet=UTF-8; CHARSET=ISO-8859-1");
    assert_eq!((cased.boundary, cased.charset), (some("First"), some("UTF-8")));
    let empty = parse("multipart/form-data; boundary=\"\"; boundary=second; charset=\"\"; charset=utf-8");
    assert_eq!((empty.boundary, empty.charset), (some(""), some("")), "an empty quoted value still counts");
    let bare = parse("multipart/form-data; boundary; boundary=first; boundary; charset; charset=utf-8; charset");
    assert_eq!((bare.boundary, bare.charset), (some("first"), some("utf-8")), "valueless parameters are skipped");
    let h = parse("text/html; charset=utf-8");
    assert_eq!((h.media_type, h.charset), (some("text/html"), some("utf-8")));
}
