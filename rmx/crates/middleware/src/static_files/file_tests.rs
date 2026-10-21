//! Ports of Remix's `response/src/lib/file.test.ts` (`createFileResponse`), case by case,
//! against [`file_response_for`]. Remix's `File` is rmx's in-memory [`File`]; a `LazyFile`
//! is a [`file_response`] path, covered by `tests.rs`.

use std::time::{Duration, UNIX_EPOCH};

use http::{header, HeaderMap, HeaderValue, Method};
use rmx_crypto::DigestAlgorithm;
use rmx_fetch::{block_on, File, Response};
use rmx_headers::format_http_date;

use super::*;

const JAN_2024: u64 = 1_704_067_200_000;
const JAN_2025: u64 = 1_735_689_600_000;
const JAN_2026: u64 = 1_767_225_600_000;

fn file(content: &str, name: &str, content_type: &str, last_modified: u64) -> File {
    File { name: name.into(), content_type: content_type.into(), bytes: content.as_bytes().to_vec().into(), last_modified }
}

fn text_file() -> File {
    file("Hello, World!", "test.txt", "text/plain", 1_000_000)
}

fn video() -> File {
    file("0123456789", "video.mp4", "video/mp4", 1_000_000)
}

fn date(ms: u64) -> String {
    format_http_date(UNIX_EPOCH + Duration::from_millis(ms))
}

fn send(file: &File, method: Method, headers: &[(&str, &str)], options: &StaticOptions) -> Response {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        map.append(header::HeaderName::from_bytes(name.as_bytes()).unwrap(), HeaderValue::from_str(value).unwrap());
    }
    file_response_for(file, &method, &map, options)
}

fn get(file: &File, headers: &[(&str, &str)]) -> Response {
    send(file, Method::GET, headers, &StaticOptions::default())
}

fn with(file: &File, headers: &[(&str, &str)], options: StaticOptions) -> Response {
    send(file, Method::GET, headers, &options)
}

fn header_of<'a>(res: &'a Response, name: &str) -> Option<&'a str> {
    res.headers().get(name).map(|v| v.to_str().unwrap())
}

fn body(res: Response) -> String {
    block_on(res.into_body().text()).unwrap()
}

fn strong() -> StaticOptions {
    StaticOptions { etag: ETag::Strong, ..Default::default() }
}

fn ranges(on: bool) -> StaticOptions {
    StaticOptions { accept_ranges: if on { AcceptRanges::Always } else { AcceptRanges::Never }, ..Default::default() }
}

#[test]
fn preserves_html_content_types_for_inline_responses() {
    let res = get(&file("<p>Example</p>", "example.html", "text/html", 0), &[]);
    assert_eq!(header_of(&res, "content-type"), Some("text/html; charset=utf-8"));
    assert_eq!(header_of(&res, "content-disposition"), None);
    assert_eq!(body(res), "<p>Example</p>");
}

#[test]
fn serves_a_file_and_a_head_request() {
    let res = get(&text_file(), &[]);
    assert_eq!(res.status(), 200);
    assert_eq!((header_of(&res, "content-type"), header_of(&res, "content-length")), (Some("text/plain; charset=utf-8"), Some("13")));
    assert_eq!(body(res), "Hello, World!");
    let res = send(&text_file(), Method::HEAD, &[], &StaticOptions::default());
    assert_eq!((res.status().as_u16(), header_of(&res, "content-length")), (200, Some("13")));
    assert_eq!(header_of(&res, "content-type"), Some("text/plain; charset=utf-8"));
    assert_eq!(body(res), "");
}

// ETag support

#[test]
fn weak_etag_by_default_and_none_when_off() {
    let res = get(&text_file(), &[]);
    assert_eq!(header_of(&res, "etag"), Some("W/\"13-1000000\""));
    let res = with(&text_file(), &[], StaticOptions { etag: ETag::Off, ..Default::default() });
    assert_eq!((res.status().as_u16(), header_of(&res, "etag")), (200, None));
}

#[test]
fn strong_etags_use_sha256_by_default() {
    let res = with(&text_file(), &[], strong());
    let etag = header_of(&res, "etag").unwrap();
    assert!(!etag.starts_with("W/"));
    assert_eq!(etag.len(), 64 + 2, "{etag}");
    assert!(etag.trim_matches('"').bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
}

#[test]
fn strong_etags_with_other_algorithms_and_a_function() {
    for (name, hex_len) in [("SHA-1", 40), ("SHA-384", 96), ("SHA-512", 128)] {
        let digest = Digest::from(name.parse::<DigestAlgorithm>().unwrap());
        let res = with(&text_file(), &[], StaticOptions { digest, ..strong() });
        assert_eq!(header_of(&res, "etag").unwrap().len(), hex_len + 2, "{name}");
    }
    let digest = Digest::With(Arc::new(|_: &[u8]| "custom-hash-12345".into()));
    let res = with(&text_file(), &[], StaticOptions { digest, ..strong() });
    assert_eq!(header_of(&res, "etag"), Some("\"custom-hash-12345\""));
    // Remix rejects with NotSupportedError when the response is created; here the name
    // fails to parse before any options exist.
    assert!("MD5".parse::<DigestAlgorithm>().unwrap_err().starts_with("NotSupportedError"));
}

// If-None-Match support

#[test]
fn if_none_match() {
    let etag = header_of(&get(&text_file(), &[]), "etag").unwrap().to_string();
    let res = get(&text_file(), &[("if-none-match", &etag)]);
    assert_eq!(res.status(), 304);
    assert_eq!(body(res), "");
    assert_eq!(get(&text_file(), &[("if-none-match", "*")]).status(), 304);
    let res = get(&text_file(), &[("if-none-match", "W/\"wrong-etag\"")]);
    assert_eq!(res.status(), 200);
    assert_eq!(body(res), "Hello, World!");
    let list = format!("W/\"wrong-1\", {etag}, W/\"wrong-2\"");
    assert_eq!(get(&text_file(), &[("if-none-match", &list)]).status(), 304);
}

#[test]
fn if_none_match_is_ignored_when_etags_are_off() {
    let etag = header_of(&get(&text_file(), &[]), "etag").unwrap().to_string();
    let res = with(&text_file(), &[("if-none-match", &etag)], StaticOptions { etag: ETag::Off, ..Default::default() });
    assert_eq!(res.status(), 200);
    assert_eq!(body(res), "Hello, World!");
}

#[test]
fn a_non_matching_if_none_match_overrides_if_modified_since() {
    let f = file("Hello, World!", "test.txt", "text/plain", JAN_2025);
    let res = get(&f, &[("if-none-match", "\"wrong-etag\""), ("if-modified-since", &date(JAN_2025))]);
    assert_eq!(res.status(), 200);
    assert_eq!(body(res), "Hello, World!");
}

// If-Match support

#[test]
fn if_match_uses_strong_comparison() {
    let weak = header_of(&get(&text_file(), &[]), "etag").unwrap().to_string();
    assert!(weak.starts_with("W/"));
    assert_eq!(get(&text_file(), &[("if-match", &weak)]).status(), 412, "weak tags never match");
    let tag = header_of(&with(&text_file(), &[], strong()), "etag").unwrap().to_string();
    let res = with(&text_file(), &[("if-match", &tag)], strong());
    assert_eq!(res.status(), 200);
    assert_eq!(body(res), "Hello, World!");
    assert_eq!(get(&text_file(), &[("if-match", "\"wrong-etag\"")]).status(), 412);
    assert_eq!(with(&text_file(), &[("if-match", "\"wrong-etag\"")], strong()).status(), 412);
    let res = get(&text_file(), &[("if-match", "*")]);
    assert_eq!(res.status(), 200);
    assert_eq!(body(res), "Hello, World!");
    assert_eq!(get(&text_file(), &[("if-match", "\"wrong-1\", \"wrong-2\"")]).status(), 412);
}

#[test]
fn a_failed_if_match_wins_over_a_matching_if_none_match() {
    let etag = header_of(&get(&text_file(), &[]), "etag").unwrap().to_string();
    assert_eq!(get(&text_file(), &[("if-match", "W/\"wrong-etag\""), ("if-none-match", &etag)]).status(), 412);
}

#[test]
fn if_match_is_ignored_when_etags_are_off() {
    let res = with(&text_file(), &[("if-match", "W/\"wrong-etag\"")], StaticOptions { etag: ETag::Off, ..Default::default() });
    assert_eq!(res.status(), 200);
    assert_eq!(body(res), "Hello, World!");
}

// If-Unmodified-Since support

#[test]
fn if_unmodified_since() {
    let f = file("Hello, World!", "test.txt", "text/plain", JAN_2025);
    let res = get(&f, &[("if-unmodified-since", &date(JAN_2026))]);
    assert_eq!(res.status(), 200);
    assert_eq!(body(res), "Hello, World!");
    assert_eq!(get(&f, &[("if-unmodified-since", &date(JAN_2025))]).status(), 200, "equal dates pass");
    assert_eq!(get(&f, &[("if-unmodified-since", &date(JAN_2024))]).status(), 412);
    assert_eq!(get(&f, &[("if-unmodified-since", "invalid-date")]).status(), 200, "malformed dates are ignored");
    let same_second = file("Hello, World!", "test.txt", "text/plain", 1_000_100);
    assert_eq!(get(&same_second, &[("if-unmodified-since", &date(1_000_900))]).status(), 200);
}

#[test]
fn if_match_decides_before_if_unmodified_since() {
    let f = file("Hello, World!", "test.txt", "text/plain", JAN_2025);
    assert_eq!(get(&f, &[("if-match", "W/\"wrong-etag\""), ("if-unmodified-since", &date(JAN_2026))]).status(), 412);
    let old = file("Hello, World!", "test.txt", "text/plain", JAN_2024);
    let tag = header_of(&with(&old, &[], strong()), "etag").unwrap().to_string();
    let res = with(&old, &[("if-match", &tag), ("if-unmodified-since", &date(JAN_2024 - 86_400_000))], strong());
    assert_eq!(res.status(), 200, "a passing If-Match skips If-Unmodified-Since");
    assert_eq!(body(res), "Hello, World!");
}

#[test]
fn if_unmodified_since_is_ignored_without_last_modified() {
    let res = with(&text_file(), &[("if-unmodified-since", &date(JAN_2024))], StaticOptions { last_modified: false, ..Default::default() });
    assert_eq!(res.status(), 200);
    assert_eq!(body(res), "Hello, World!");
}

// Last-Modified support

#[test]
fn last_modified_and_if_modified_since() {
    let f = file("Hello, World!", "test.txt", "text/plain", JAN_2025);
    let res = get(&f, &[]);
    assert_eq!(header_of(&res, "last-modified"), Some("Wed, 01 Jan 2025 00:00:00 GMT"));
    let res = with(&f, &[], StaticOptions { last_modified: false, ..Default::default() });
    assert_eq!(header_of(&res, "last-modified"), None);
    let res = get(&f, &[("if-modified-since", &date(JAN_2025))]);
    assert_eq!(res.status(), 304);
    assert_eq!(body(res), "");
    assert_eq!(get(&f, &[("if-modified-since", &date(JAN_2026))]).status(), 304);
    let res = get(&f, &[("if-modified-since", &date(JAN_2024))]);
    assert_eq!(res.status(), 200);
    assert_eq!(body(res), "Hello, World!");
    let same_second = file("Hello, World!", "test.txt", "text/plain", 1_000_999);
    assert_eq!(get(&same_second, &[("if-modified-since", &date(1_000_500))]).status(), 304);
    assert_eq!(get(&f, &[("if-none-match", "W/\"wrong-etag\""), ("if-modified-since", &date(JAN_2025))]).status(), 200, "ETags win");
}

// Range requests

#[test]
fn accept_ranges_header() {
    assert_eq!(header_of(&get(&file("fake video data", "video.mp4", "video/mp4", 0), &[]), "accept-ranges"), Some("bytes"));
    assert_eq!(header_of(&get(&file("Hello", "test.txt", "text/plain", 0), &[]), "accept-ranges"), None, "compressible");
    assert_eq!(header_of(&with(&file("Hello", "test.txt", "text/plain", 0), &[], ranges(true)), "accept-ranges"), Some("bytes"));
    assert_eq!(header_of(&with(&file("Hello", "test.txt", "text/plain", 0), &[], ranges(false)), "accept-ranges"), None);
}

#[test]
fn satisfiable_ranges() {
    for (range, text, content_range) in [("bytes=0-4", "01234", "bytes 0-4/10"), ("bytes=5-", "56789", "bytes 5-9/10"), ("bytes=-3", "789", "bytes 7-9/10"), ("bytes=0-999", "0123456789", "bytes 0-9/10")] {
        let res = get(&video(), &[("range", range)]);
        assert_eq!(res.status(), 206, "{range}");
        assert_eq!(header_of(&res, "content-range"), Some(content_range));
        assert_eq!(header_of(&res, "content-length"), Some(text.len().to_string().as_str()));
        assert_eq!(body(res), text);
    }
}

#[test]
fn ranges_apply_to_get_only() {
    let res = send(&video(), Method::POST, &[("range", "bytes=0-4")], &StaticOptions::default());
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (200, None));
    assert_eq!(body(res), "0123456789");
    let res = send(&file("0123456789", "test.txt", "text/plain", 0), Method::HEAD, &[("range", "bytes=0-4")], &ranges(true));
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range"), header_of(&res, "content-length")), (200, None, Some("10")));
    assert_eq!(body(res), "");
}

#[test]
fn unsatisfiable_and_malformed_ranges() {
    for range in ["bytes=20-30", "bytes=0-2,5-7"] {
        let res = get(&video(), &[("range", range)]);
        assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (416, Some("bytes */10")), "{range}");
    }
    for range in ["bytes=0-2,garbage", "bytes=5-2", "invalid", "bytes="] {
        let res = get(&video(), &[("range", range)]);
        assert_eq!(res.status(), 400, "{range}");
        assert_eq!(body(res), "Bad Request");
    }
}

#[test]
fn ranges_off_serve_the_whole_file() {
    let res = with(&file("0123456789", "test.txt", "text/plain", 0), &[("range", "bytes=0-4")], ranges(false));
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (200, None));
    assert_eq!(body(res), "0123456789");
}

#[test]
fn preconditions_run_before_ranges() {
    let res = get(&video(), &[("if-match", "W/\"wrong-etag\""), ("range", "bytes=0-4")]);
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (412, None));
    let tag = header_of(&with(&video(), &[], strong()), "etag").unwrap().to_string();
    let res = with(&video(), &[("if-match", &tag), ("range", "bytes=0-4")], strong());
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (206, Some("bytes 0-4/10")));
    assert_eq!(body(res), "01234");
    let dated = file("0123456789", "video.mp4", "video/mp4", JAN_2025);
    let res = get(&dated, &[("if-unmodified-since", &date(JAN_2026)), ("range", "bytes=0-4")]);
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (206, Some("bytes 0-4/10")));
    assert_eq!(body(res), "01234");
    let text = file("0123456789", "test.txt", "text/plain", JAN_2025);
    let res = get(&text, &[("if-unmodified-since", &date(JAN_2024)), ("range", "bytes=0-4")]);
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (412, None));
}

#[test]
fn cache_validators_run_before_ranges() {
    let etag = header_of(&get(&video(), &[]), "etag").unwrap().to_string();
    let res = get(&video(), &[("if-none-match", &etag), ("range", "bytes=0-4")]);
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (304, None));
    let res = get(&video(), &[("if-none-match", "\"wrong-etag\""), ("range", "bytes=0-4")]);
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (206, Some("bytes 0-4/10")));
    assert_eq!(body(res), "01234");
    let dated = file("0123456789", "video.mp4", "video/mp4", JAN_2025);
    let modified = header_of(&get(&dated, &[]), "last-modified").unwrap().to_string();
    let res = get(&dated, &[("if-modified-since", &modified), ("range", "bytes=0-4")]);
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (304, None));
    let res = get(&dated, &[("if-modified-since", &date(JAN_2024)), ("range", "bytes=0-4")]);
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (206, Some("bytes 0-4/10")));
    assert_eq!(body(res), "01234");
}

#[test]
fn if_range() {
    let modified = header_of(&get(&video(), &[]), "last-modified").unwrap().to_string();
    let res = get(&video(), &[("range", "bytes=0-4"), ("if-range", &modified)]);
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (206, Some("bytes 0-4/10")));
    assert_eq!(body(res), "01234");
    let weak = header_of(&get(&video(), &[]), "etag").unwrap().to_string();
    for (if_range, why) in [("Wed, 21 Oct 2015 07:28:00 GMT", "another date"), (weak.as_str(), "a weak ETag"), ("2025-01-01", "not an HTTP date"), ("not-a-valid-value", "malformed")] {
        let res = get(&video(), &[("range", "bytes=0-4"), ("if-range", if_range)]);
        assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (200, None), "{why}");
        assert_eq!(body(res), "0123456789");
    }
}

#[test]
fn if_range_is_ignored_without_ranges_or_last_modified() {
    let text = file("0123456789", "test.txt", "text/plain", 0);
    let headers = [("range", "bytes=0-4"), ("if-range", "Wed, 21 Oct 2015 07:28:00 GMT")];
    let res = with(&text, &headers, ranges(false));
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (200, None));
    assert_eq!(body(res), "0123456789");
    let res = with(&text, &headers, StaticOptions { last_modified: false, ..Default::default() });
    assert_eq!(res.status(), 200);
    assert_eq!(body(res), "0123456789");
}

#[test]
fn if_none_match_with_if_range() {
    let dated = file("0123456789", "video.mp4", "video/mp4", JAN_2025);
    let first = get(&dated, &[]);
    let (etag, modified) = (header_of(&first, "etag").unwrap().to_string(), header_of(&first, "last-modified").unwrap().to_string());
    let res = get(&dated, &[("if-none-match", &etag), ("if-range", &modified), ("range", "bytes=0-4")]);
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (304, None));
    let res = get(&dated, &[("if-none-match", "\"wrong-etag\""), ("if-range", &modified), ("range", "bytes=0-4")]);
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (206, Some("bytes 0-4/10")));
    assert_eq!(body(res), "01234");
    let res = get(&dated, &[("if-none-match", "\"wrong-etag\""), ("if-range", &date(JAN_2024)), ("range", "bytes=0-4")]);
    assert_eq!((res.status().as_u16(), header_of(&res, "content-range")), (200, None));
    assert_eq!(body(res), "0123456789");
}

// Cache-Control

#[test]
fn cache_control() {
    assert_eq!(header_of(&get(&text_file(), &[]), "cache-control"), None);
    let res = with(&text_file(), &[], StaticOptions { cache_control: Some("no-cache".into()), ..Default::default() });
    assert_eq!(header_of(&res, "cache-control"), Some("no-cache"));
}

// Content-Type

#[test]
fn content_types() {
    let charset = [("text/html", "test.html"), ("text/css", "test.css"), ("text/plain", "test.txt"), ("text/javascript", "test.js"), ("application/json", "test.json"), ("application/javascript", "app.js")];
    for (ty, name) in charset {
        let res = get(&file("test content", name, ty, 0), &[]);
        assert_eq!(res.status(), 200);
        assert_eq!(header_of(&res, "content-type"), Some(format!("{ty}; charset=utf-8").as_str()), "{name}");
    }
    for (ty, name) in [("image/svg+xml", "test.svg"), ("application/xml", "test.xml"), ("image/png", "test.png"), ("image/jpeg", "test.jpg"), ("application/pdf", "test.pdf"), ("application/zip", "test.zip")] {
        assert_eq!(header_of(&get(&file("test content", name, ty, 0), &[]), "content-type"), Some(ty), "{name}");
    }
    let res = get(&file("test content", "test.txt", "", 0), &[]);
    assert_eq!((res.status().as_u16(), header_of(&res, "content-type")), (200, None), "an empty type sends none");
}
