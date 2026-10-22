//! Port of Remix's `response/src/lib/file.test.ts` (`createFileResponse` is `response::file`).
//! Remix's `etag: false | 'weak' | 'strong'` is [ETag]; a digest name is a [DigestAlgorithm], so
//! an unsupported name (`'MD5'`) fails to parse instead of rejecting at request time. Dates like
//! `new Date('2025-01-01')` are millisecond constants, and `toUTCString()` is
//! `rmx_headers::format_http_date`.

use std::time::{Duration, UNIX_EPOCH};

use http::Method;
use rmx_crypto::DigestAlgorithm;
use rmx_response::{file as create_file_response, Digest, ETag, FileLike, FileResponseOptions};
use rmx_fetch::{block_on, Body, File, Request, Response};
use rmx_headers::format_http_date;
use rmx_lazy_file::{BlobPart, LazyFile, LazyFileOptions};

/// `new Date('2024-01-01').getTime()`.
const JAN_2024: u64 = 1_704_067_200_000;
/// `new Date('2025-01-01').getTime()`.
const JAN_2025: u64 = 1_735_689_600_000;
/// `new Date('2026-01-01').getTime()`.
const JAN_2026: u64 = 1_767_225_600_000;

fn file(content: &str, name: &str, content_type: &str) -> File {
    File::new(name, content_type, content.as_bytes().to_vec())
}

fn file_at(content: &str, name: &str, content_type: &str, last_modified: u64) -> File {
    let mut file = file(content, name, content_type);
    file.last_modified = last_modified;
    file
}

fn request(method: Method, headers: &[(&str, &str)]) -> Request {
    let mut builder = http::Request::builder().method(method).uri("http://localhost/test.txt");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::empty()).unwrap()
}

fn get(headers: &[(&str, &str)]) -> Request {
    request(Method::GET, headers)
}

fn respond<F: FileLike>(file: &F, request: Request) -> Response {
    respond_with(file, request, &FileResponseOptions::default())
}

fn respond_with<F: FileLike>(file: &F, request: Request, options: &FileResponseOptions<F>) -> Response {
    block_on(create_file_response(file, &request, options)).unwrap()
}

fn strong<F>() -> FileResponseOptions<F> {
    FileResponseOptions { etag: ETag::Strong, ..FileResponseOptions::default() }
}

fn header(response: &Response, name: &str) -> Option<String> {
    response.headers().get(name).map(|v| v.to_str().unwrap().to_string())
}

fn etag(response: &Response) -> String {
    header(response, "etag").expect("an ETag")
}

fn text(response: Response) -> String {
    block_on(response.into_body().text()).unwrap()
}

/// `new Date(ms).toUTCString()`.
fn utc(ms: u64) -> String {
    format_http_date(UNIX_EPOCH + Duration::from_millis(ms))
}

/// `/^W\/"[\d]+-[\d]+\.?[\d]*"$/`.
fn is_weak_etag(etag: &str) -> bool {
    let Some(inner) = etag.strip_prefix("W/\"").and_then(|s| s.strip_suffix('"')) else { return false };
    let Some((size, modified)) = inner.split_once('-') else { return false };
    let (whole, fraction) = modified.split_once('.').unwrap_or((modified, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    !size.is_empty() && digits(size) && !whole.is_empty() && digits(whole) && digits(fraction)
}

/// The length of the hex digest in `"<hex>"` (`/^"[a-f0-9]+"$/`), or `None` for anything else.
fn hex_etag_len(etag: &str) -> Option<usize> {
    let hex = etag.strip_prefix('"')?.strip_suffix('"')?;
    (!hex.is_empty() && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))).then_some(hex.len())
}

/// A method, request headers, and the expected status.
type Case = (Method, &'static [(&'static str, &'static str)], u16);

#[test]
fn includes_nosniff_for_full_partial_conditional_and_error_responses() {
    let file = file_at("hello", "example.bin", "application/octet-stream", 1_000_000);
    let cases: [Case; 7] = [
        (Method::GET, &[], 200),
        (Method::HEAD, &[], 200),
        (Method::GET, &[("Range", "bytes=0-1")], 206),
        (Method::GET, &[("If-None-Match", "W/\"5-1000000\"")], 304),
        (Method::GET, &[("If-Match", "\"other\"")], 412),
        (Method::GET, &[("Range", "bytes=10-20")], 416),
        (Method::GET, &[("Range", "bytes=invalid")], 400),
    ];
    let options = FileResponseOptions { accept_ranges: Some(true), ..FileResponseOptions::default() };

    for (method, headers, status) in cases {
        let response = respond_with(&file, request(method, headers), &options);
        assert_eq!(response.status(), status);
        assert_eq!(header(&response, "X-Content-Type-Options").as_deref(), Some("nosniff"));
    }
}

#[test]
fn preserves_html_content_types_for_inline_file_responses() {
    let file = file("<p>Example</p>", "example.html", "text/html");
    let response = respond(&file, get(&[]));

    assert_eq!(header(&response, "Content-Type").as_deref(), Some("text/html; charset=utf-8"));
    assert_eq!(header(&response, "Content-Disposition"), None);
    assert_eq!(text(response), "<p>Example</p>");
}

#[test]
fn serves_a_file() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let response = respond(&file, get(&[]));

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Type").as_deref(), Some("text/plain; charset=utf-8"));
    assert_eq!(header(&response, "Content-Length").as_deref(), Some("13"));
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn serves_a_lazy_file() {
    let options = LazyFileOptions { content_type: "text/plain".into(), ..LazyFileOptions::default() };
    let lazy_file = LazyFile::new([BlobPart::from("Hello, World!")], "test.txt", options);
    let response = respond(&lazy_file, get(&[]));

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Type").as_deref(), Some("text/plain; charset=utf-8"));
    assert_eq!(header(&response, "Content-Length").as_deref(), Some("13"));
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn serves_a_file_with_head_request() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let response = respond(&file, request(Method::HEAD, &[]));

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Type").as_deref(), Some("text/plain; charset=utf-8"));
    assert_eq!(header(&response, "Content-Length").as_deref(), Some("13"));
    assert_eq!(text(response), "");
}

// ETag support

#[test]
fn includes_weak_etag_header_by_default() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", 1_000_000);
    let response = respond(&file, get(&[]));

    assert_eq!(response.status(), 200);
    assert!(is_weak_etag(&etag(&response)), "{}", etag(&response));
}

#[test]
fn does_not_include_etag_when_etag_is_off() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let response = respond_with(&file, get(&[]), &FileResponseOptions { etag: ETag::Off, ..FileResponseOptions::default() });

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "ETag"), None);
}

#[test]
fn generates_strong_etag_when_etag_is_strong() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let response = respond_with(&file, get(&[]), &strong());

    let etag = etag(&response);
    assert_eq!(response.status(), 200);
    assert!(!etag.starts_with("W/"), "Should not be a weak ETag");
    assert!(hex_etag_len(&etag).is_some(), "Should be a hex digest wrapped in quotes");
}

#[test]
fn uses_sha_256_by_default_for_strong_etags() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let response = respond_with(&file, get(&[]), &strong());

    // SHA-256 produces 64 hex characters (32 bytes * 2)
    assert_eq!(hex_etag_len(&etag(&response)), Some(64));
}

#[test]
fn supports_custom_digest_algorithm() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let options = FileResponseOptions { digest: DigestAlgorithm::Sha512.into(), ..strong() };
    let response = respond_with(&file, get(&[]), &options);

    // SHA-512 produces 128 hex characters (64 bytes * 2)
    assert_eq!(hex_etag_len(&etag(&response)), Some(128));
}

#[test]
fn supports_custom_digest_function() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let options = FileResponseOptions { digest: Digest::with(|_: &File| async { "custom-hash-12345".to_string() }), ..strong() };
    let response = respond_with(&file, get(&[]), &options);

    assert_eq!(etag(&response), "\"custom-hash-12345\"");
}

#[test]
fn throws_error_for_unsupported_algorithm() {
    // Web Crypto rejects 'MD5' with a NotSupportedError; here the name never becomes a digest.
    assert!("MD5".parse::<DigestAlgorithm>().is_err());
}

#[test]
fn supports_sha_1_algorithm() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let options = FileResponseOptions { digest: "SHA-1".parse::<DigestAlgorithm>().unwrap().into(), ..strong() };
    let response = respond_with(&file, get(&[]), &options);

    // SHA-1 produces 40 hex characters (20 bytes * 2)
    assert_eq!(hex_etag_len(&etag(&response)), Some(40));
}

#[test]
fn supports_sha_384_algorithm() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let options = FileResponseOptions { digest: "SHA-384".parse::<DigestAlgorithm>().unwrap().into(), ..strong() };
    let response = respond_with(&file, get(&[]), &options);

    // SHA-384 produces 96 hex characters (48 bytes * 2)
    assert_eq!(hex_etag_len(&etag(&response)), Some(96));
}

// If-None-Match support

#[test]
fn returns_304_when_if_none_match_matches_etag() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", 1_000_000);
    let tag = etag(&respond(&file, get(&[])));

    let response = respond(&file, get(&[("If-None-Match", &tag)]));

    assert_eq!(response.status(), 304);
    assert_eq!(text(response), "");
}

#[test]
fn returns_304_when_if_none_match_is_star() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let response = respond(&file, get(&[("If-None-Match", "*")]));

    assert_eq!(response.status(), 304);
}

#[test]
fn returns_200_when_if_none_match_does_not_match() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let response = respond(&file, get(&[("If-None-Match", "W/\"wrong-etag\"")]));

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn handles_multiple_etags_in_if_none_match() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", 1_000_000);
    let tag = etag(&respond(&file, get(&[])));

    let response = respond(&file, get(&[("If-None-Match", &format!("W/\"wrong-1\", {tag}, W/\"wrong-2\""))]));

    assert_eq!(response.status(), 304);
}

#[test]
fn ignores_if_none_match_when_etag_is_disabled() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    // First, get the ETag that would be generated
    let tag = etag(&respond(&file, get(&[])));

    // Now test with etag disabled but send the matching ETag
    let options = FileResponseOptions { etag: ETag::Off, ..FileResponseOptions::default() };
    let response = respond_with(&file, get(&[("If-None-Match", &tag)]), &options);

    // Should return 200, not 304, because etag is disabled
    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn ignores_if_modified_since_when_if_none_match_is_present_but_does_not_match() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", JAN_2025);
    // If-Modified-Since would normally return 304
    let response = respond(&file, get(&[("If-None-Match", "\"wrong-etag\""), ("If-Modified-Since", &utc(JAN_2025))]));

    // Should return 200, not 304, because If-None-Match takes precedence
    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "Hello, World!");
}

// If-Match support: precondition validation

#[test]
fn returns_412_when_resource_has_weak_etag() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", 1_000_000);
    let tag = etag(&respond(&file, get(&[])));
    assert!(tag.starts_with("W/"));

    // If-Match uses strong comparison, so weak ETags never match
    let response = respond(&file, get(&[("If-Match", &tag)]));

    assert_eq!(response.status(), 412);
}

#[test]
fn returns_200_when_resource_has_strong_etag_and_if_match_matches() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let tag = etag(&respond_with(&file, get(&[]), &strong()));
    assert!(!tag.starts_with("W/"));

    let response = respond_with(&file, get(&[("If-Match", &tag)]), &strong());

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn returns_412_when_if_match_does_not_match_weak_etag() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let response = respond(&file, get(&[("If-Match", "\"wrong-etag\"")]));

    assert_eq!(response.status(), 412);
}

#[test]
fn returns_412_when_if_match_does_not_match_strong_etag() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let response = respond_with(&file, get(&[("If-Match", "\"wrong-etag\"")]), &strong());

    assert_eq!(response.status(), 412);
}

#[test]
fn returns_200_when_if_match_is_star() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let response = respond(&file, get(&[("If-Match", "*")]));

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn returns_412_when_if_match_contains_multiple_etags_and_none_match() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let response = respond(&file, get(&[("If-Match", "\"wrong-1\", \"wrong-2\"")]));

    assert_eq!(response.status(), 412);
}

#[test]
fn returns_412_when_if_match_fails_even_if_if_none_match_would_match() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", 1_000_000);
    let tag = etag(&respond(&file, get(&[])));

    let response = respond(&file, get(&[("If-Match", "W/\"wrong-etag\""), ("If-None-Match", &tag)]));

    assert_eq!(response.status(), 412);
}

#[test]
fn ignores_if_match_when_etag_is_disabled() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    assert!(header(&respond(&file, get(&[])), "ETag").is_some());

    // If we weren't ignoring it, this would return 412
    let options = FileResponseOptions { etag: ETag::Off, ..FileResponseOptions::default() };
    let response = respond_with(&file, get(&[("If-Match", "W/\"wrong-etag\"")]), &options);

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "Hello, World!");
}

// If-Unmodified-Since support

#[test]
fn returns_200_when_if_unmodified_since_is_after_last_modified() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", JAN_2025);
    let response = respond(&file, get(&[("If-Unmodified-Since", &utc(JAN_2026))]));

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn returns_200_when_if_unmodified_since_matches_last_modified() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", JAN_2025);
    let response = respond(&file, get(&[("If-Unmodified-Since", &utc(JAN_2025))]));

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn returns_412_when_if_unmodified_since_is_before_last_modified() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", JAN_2025);
    let response = respond(&file, get(&[("If-Unmodified-Since", &utc(JAN_2024))]));

    assert_eq!(response.status(), 412);
}

#[test]
fn ignores_malformed_if_unmodified_since() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", JAN_2025);
    let response = respond(&file, get(&[("If-Unmodified-Since", "invalid-date")]));

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn if_unmodified_since_treats_dates_with_same_second_but_different_milliseconds_as_equal() {
    // File last modified at 1000100ms, If-Unmodified-Since at 1000900ms: the same second
    let file = file_at("Hello, World!", "test.txt", "text/plain", 1_000_100);
    let response = respond(&file, get(&[("If-Unmodified-Since", &utc(1_000_900))]));

    // Should return 200 because both round down to the same second
    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn returns_412_when_if_match_fails_even_if_if_unmodified_since_would_pass() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", JAN_2025);
    let response = respond(&file, get(&[("If-Match", "W/\"wrong-etag\""), ("If-Unmodified-Since", &utc(JAN_2026))]));

    assert_eq!(response.status(), 412);
}

#[test]
fn ignores_if_unmodified_since_when_if_match_is_present_with_strong_etag() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", JAN_2024);
    let tag = etag(&respond_with(&file, get(&[]), &strong()));
    assert!(!tag.starts_with("W/"));

    // If-Match passes, so If-Unmodified-Since is ignored. Remix sends the file's own date, which
    // would pass anyway; one second earlier would fail if it were evaluated.
    let response = respond_with(&file, get(&[("If-Match", &tag), ("If-Unmodified-Since", &utc(JAN_2024 - 1000))]), &strong());

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn ignores_if_unmodified_since_when_last_modified_is_disabled() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let options = FileResponseOptions { last_modified: false, ..FileResponseOptions::default() };
    let response = respond_with(&file, get(&[("If-Unmodified-Since", &utc(JAN_2024))]), &options);

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "Hello, World!");
}

// Last-Modified support

#[test]
fn includes_last_modified_header() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", JAN_2025);
    let response = respond(&file, get(&[]));

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Last-Modified").as_deref(), Some("Wed, 01 Jan 2025 00:00:00 GMT"));
}

#[test]
fn does_not_include_last_modified_when_last_modified_is_false() {
    let file = file("Hello, World!", "test.txt", "text/plain");
    let options = FileResponseOptions { last_modified: false, ..FileResponseOptions::default() };
    let response = respond_with(&file, get(&[]), &options);

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Last-Modified"), None);
}

#[test]
fn returns_304_when_if_modified_since_matches_last_modified() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", JAN_2025);
    let response = respond(&file, get(&[("If-Modified-Since", &utc(JAN_2025))]));

    assert_eq!(response.status(), 304);
    assert_eq!(text(response), "");
}

#[test]
fn returns_304_when_if_modified_since_is_after_last_modified() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", JAN_2025);
    let response = respond(&file, get(&[("If-Modified-Since", &utc(JAN_2026))]));

    assert_eq!(response.status(), 304);
}

#[test]
fn returns_200_when_if_modified_since_is_before_last_modified() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", JAN_2025);
    let response = respond(&file, get(&[("If-Modified-Since", &utc(JAN_2024))]));

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "Hello, World!");
}

#[test]
fn if_modified_since_treats_dates_with_same_second_but_different_milliseconds_as_equal() {
    // File last modified at 1000999ms, If-Modified-Since at 1000500ms: the same second
    let file = file_at("Hello, World!", "test.txt", "text/plain", 1_000_999);
    let response = respond(&file, get(&[("If-Modified-Since", &utc(1_000_500))]));

    // Should return 304 because both round down to the same second
    assert_eq!(response.status(), 304);
}

#[test]
fn prioritizes_etag_over_if_modified_since_when_both_are_present() {
    let file = file_at("Hello, World!", "test.txt", "text/plain", JAN_2025);
    let response = respond(&file, get(&[("If-None-Match", "W/\"wrong-etag\""), ("If-Modified-Since", &utc(JAN_2025))]));

    assert_eq!(response.status(), 200);
}

// Range requests

fn video() -> File {
    file("0123456789", "video.mp4", "video/mp4")
}

#[test]
fn includes_accept_ranges_header_for_non_compressible_media_types_by_default() {
    let file = file("fake video data", "video.mp4", "video/mp4");
    let response = respond(&file, get(&[]));

    assert_eq!(header(&response, "Accept-Ranges").as_deref(), Some("bytes"));
}

#[test]
fn does_not_include_accept_ranges_header_for_compressible_media_types_by_default() {
    let file = file("Hello", "test.txt", "text/plain");
    let response = respond(&file, get(&[]));

    assert_eq!(header(&response, "Accept-Ranges"), None);
}

#[test]
fn includes_accept_ranges_header_when_explicitly_enabled() {
    let file = file("Hello", "test.txt", "text/plain");
    let options = FileResponseOptions { accept_ranges: Some(true), ..FileResponseOptions::default() };
    let response = respond_with(&file, get(&[]), &options);

    assert_eq!(header(&response, "Accept-Ranges").as_deref(), Some("bytes"));
}

#[test]
fn omits_accept_ranges_header_when_accept_ranges_is_false() {
    let file = file("Hello", "test.txt", "text/plain");
    let options = FileResponseOptions { accept_ranges: Some(false), ..FileResponseOptions::default() };
    let response = respond_with(&file, get(&[]), &options);

    assert_eq!(header(&response, "Accept-Ranges"), None);
}

#[test]
fn handles_simple_range_request() {
    let response = respond(&video(), get(&[("Range", "bytes=0-4")]));

    assert_eq!(response.status(), 206);
    assert_eq!(header(&response, "Content-Range").as_deref(), Some("bytes 0-4/10"));
    assert_eq!(header(&response, "Content-Length").as_deref(), Some("5"));
    assert_eq!(text(response), "01234");
}

#[test]
fn handles_range_with_only_start() {
    let response = respond(&video(), get(&[("Range", "bytes=5-")]));

    assert_eq!(response.status(), 206);
    assert_eq!(header(&response, "Content-Range").as_deref(), Some("bytes 5-9/10"));
    assert_eq!(text(response), "56789");
}

#[test]
fn handles_suffix_range_last_n_bytes() {
    let response = respond(&video(), get(&[("Range", "bytes=-3")]));

    assert_eq!(response.status(), 206);
    assert_eq!(header(&response, "Content-Range").as_deref(), Some("bytes 7-9/10"));
    assert_eq!(text(response), "789");
}

#[test]
fn clamps_end_byte_to_file_size_when_it_exceeds() {
    let response = respond(&video(), get(&[("Range", "bytes=0-999")]));

    assert_eq!(response.status(), 206);
    assert_eq!(header(&response, "Content-Range").as_deref(), Some("bytes 0-9/10"));
    assert_eq!(header(&response, "Content-Length").as_deref(), Some("10"));
    assert_eq!(text(response), "0123456789");
}

#[test]
fn ignores_range_header_for_non_get_head_requests() {
    let response = respond(&video(), request(Method::POST, &[("Range", "bytes=0-4")]));

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Range"), None);
    assert_eq!(text(response), "0123456789");
}

#[test]
fn ignores_range_header_for_head_requests() {
    let file = file("0123456789", "test.txt", "text/plain");
    let response = respond(&file, request(Method::HEAD, &[("Range", "bytes=0-4")]));

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Range"), None);
    assert_eq!(header(&response, "Content-Length").as_deref(), Some("10"));
    assert_eq!(text(response), "");
}

#[test]
fn returns_416_for_unsatisfiable_range() {
    let response = respond(&video(), get(&[("Range", "bytes=20-30")]));

    assert_eq!(response.status(), 416);
    assert_eq!(header(&response, "Content-Range").as_deref(), Some("bytes */10"));
}

#[test]
fn returns_416_for_multipart_ranges_not_supported() {
    let response = respond(&video(), get(&[("Range", "bytes=0-2,5-7")]));

    assert_eq!(response.status(), 416);
    assert_eq!(header(&response, "Content-Range").as_deref(), Some("bytes */10"));
}

#[test]
fn returns_400_for_malformed_multipart_range_syntax() {
    let response = respond(&video(), get(&[("Range", "bytes=0-2,garbage")]));

    assert_eq!(response.status(), 400);
    assert_eq!(text(response), "Bad Request");
}

#[test]
fn returns_400_for_start_greater_than_end() {
    let response = respond(&video(), get(&[("Range", "bytes=5-2")]));

    assert_eq!(response.status(), 400);
    assert_eq!(text(response), "Bad Request");
}

#[test]
fn returns_400_for_malformed_range() {
    let response = respond(&video(), get(&[("Range", "invalid")]));

    assert_eq!(response.status(), 400);
    assert_eq!(text(response), "Bad Request");
}

#[test]
fn returns_400_for_bytes_with_no_range() {
    let response = respond(&video(), get(&[("Range", "bytes=")]));

    assert_eq!(response.status(), 400);
    assert_eq!(text(response), "Bad Request");
}

#[test]
fn returns_full_file_when_accept_ranges_is_false() {
    let file = file("0123456789", "test.txt", "text/plain");
    let options = FileResponseOptions { accept_ranges: Some(false), ..FileResponseOptions::default() };
    let response = respond_with(&file, get(&[("Range", "bytes=0-4")]), &options);

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Range"), None);
    assert_eq!(text(response), "0123456789");
}

#[test]
fn returns_412_when_if_match_fails_before_processing_range() {
    let response = respond(&video(), get(&[("If-Match", "W/\"wrong-etag\""), ("Range", "bytes=0-4")]));

    assert_eq!(response.status(), 412);
    assert_eq!(header(&response, "Content-Range"), None);
}

#[test]
fn returns_206_when_if_match_succeeds_with_range_request_strong_etag() {
    let file = video();
    let tag = etag(&respond_with(&file, get(&[]), &strong()));
    assert!(!tag.starts_with("W/"));

    // If-Match passes, Range should be processed
    let response = respond_with(&file, get(&[("If-Match", &tag), ("Range", "bytes=0-4")]), &strong());

    assert_eq!(response.status(), 206);
    assert_eq!(header(&response, "Content-Range").as_deref(), Some("bytes 0-4/10"));
    assert_eq!(text(response), "01234");
}

#[test]
fn returns_206_when_if_unmodified_since_passes_with_range_request() {
    let file = file_at("0123456789", "video.mp4", "video/mp4", JAN_2025);
    let response = respond(&file, get(&[("If-Unmodified-Since", &utc(JAN_2026)), ("Range", "bytes=0-4")]));

    assert_eq!(response.status(), 206);
    assert_eq!(header(&response, "Content-Range").as_deref(), Some("bytes 0-4/10"));
    assert_eq!(text(response), "01234");
}

#[test]
fn returns_412_when_if_unmodified_since_fails_before_processing_range() {
    let file = file_at("0123456789", "test.txt", "text/plain", JAN_2025);
    let response = respond(&file, get(&[("If-Unmodified-Since", &utc(JAN_2024)), ("Range", "bytes=0-4")]));

    assert_eq!(response.status(), 412);
    assert_eq!(header(&response, "Content-Range"), None);
}

#[test]
fn returns_304_when_if_none_match_matches_etag_with_range_request() {
    let file = file_at("0123456789", "video.mp4", "video/mp4", 1_000_000);
    let tag = etag(&respond(&file, get(&[])));

    let response = respond(&file, get(&[("If-None-Match", &tag), ("Range", "bytes=0-4")]));

    assert_eq!(response.status(), 304);
    assert_eq!(header(&response, "Content-Range"), None);
}

#[test]
fn returns_206_when_if_none_match_does_not_match_with_range_request() {
    let response = respond(&video(), get(&[("If-None-Match", "\"wrong-etag\""), ("Range", "bytes=0-4")]));

    assert_eq!(response.status(), 206);
    assert_eq!(header(&response, "Content-Range").as_deref(), Some("bytes 0-4/10"));
    assert_eq!(text(response), "01234");
}

#[test]
fn returns_304_when_if_modified_since_matches_with_range_request() {
    let file = file_at("0123456789", "video.mp4", "video/mp4", JAN_2025);
    let last_modified = header(&respond(&file, get(&[])), "Last-Modified").expect("Last-Modified");

    let response = respond(&file, get(&[("If-Modified-Since", &last_modified), ("Range", "bytes=0-4")]));

    assert_eq!(response.status(), 304);
    assert_eq!(header(&response, "Content-Range"), None);
}

#[test]
fn returns_206_when_if_modified_since_does_not_match_with_range_request() {
    let file = file_at("0123456789", "video.mp4", "video/mp4", JAN_2025);
    let response = respond(&file, get(&[("If-Modified-Since", &utc(JAN_2024)), ("Range", "bytes=0-4")]));

    assert_eq!(response.status(), 206);
    assert_eq!(header(&response, "Content-Range").as_deref(), Some("bytes 0-4/10"));
    assert_eq!(text(response), "01234");
}

#[test]
fn returns_206_when_if_range_matches_last_modified_date() {
    let file = video();
    let last_modified = header(&respond(&file, get(&[])), "Last-Modified").expect("Last-Modified");

    let response = respond(&file, get(&[("Range", "bytes=0-4"), ("If-Range", &last_modified)]));

    assert_eq!(response.status(), 206);
    assert_eq!(header(&response, "Content-Range").as_deref(), Some("bytes 0-4/10"));
    assert_eq!(text(response), "01234");
}

#[test]
fn returns_200_full_file_when_if_range_does_not_match_last_modified_date() {
    let response = respond(&video(), get(&[("Range", "bytes=0-4"), ("If-Range", "Wed, 21 Oct 2015 07:28:00 GMT")]));

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Range"), None);
    assert_eq!(text(response), "0123456789");
}

#[test]
fn ignores_if_range_with_weak_etag_value() {
    let file = file_at("0123456789", "video.mp4", "video/mp4", 1_000_000);
    let tag = etag(&respond(&file, get(&[])));

    let response = respond(&file, get(&[("Range", "bytes=0-4"), ("If-Range", &tag)]));

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "0123456789");
}

#[test]
fn returns_full_file_when_if_range_has_invalid_date_format() {
    let response = respond(&video(), get(&[("Range", "bytes=0-4"), ("If-Range", "2025-01-01")]));

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Range"), None);
    assert_eq!(text(response), "0123456789");
}

#[test]
fn returns_full_file_when_if_range_is_malformed() {
    let response = respond(&video(), get(&[("Range", "bytes=0-4"), ("If-Range", "not-a-valid-value")]));

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Range"), None);
    assert_eq!(text(response), "0123456789");
}

#[test]
fn ignores_if_range_when_accept_ranges_is_disabled() {
    let file = file("0123456789", "test.txt", "text/plain");
    let options = FileResponseOptions { accept_ranges: Some(false), ..FileResponseOptions::default() };
    let response = respond_with(&file, get(&[("Range", "bytes=0-4"), ("If-Range", "Wed, 21 Oct 2015 07:28:00 GMT")]), &options);

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Range"), None);
    assert_eq!(text(response), "0123456789");
}

#[test]
fn ignores_if_range_when_last_modified_is_disabled() {
    let file = file("0123456789", "test.txt", "text/plain");
    let options = FileResponseOptions { last_modified: false, ..FileResponseOptions::default() };
    let response = respond_with(&file, get(&[("Range", "bytes=0-4"), ("If-Range", "Wed, 21 Oct 2015 07:28:00 GMT")]), &options);

    assert_eq!(response.status(), 200);
    assert_eq!(text(response), "0123456789");
}

#[test]
fn returns_304_with_if_none_match_and_if_range_when_if_none_match_matches() {
    let file = file_at("0123456789", "video.mp4", "video/mp4", JAN_2025);
    let first = respond(&file, get(&[]));
    let tag = etag(&first);
    let last_modified = header(&first, "Last-Modified").expect("Last-Modified");

    let response = respond(&file, get(&[("If-None-Match", &tag), ("If-Range", &last_modified), ("Range", "bytes=0-4")]));

    assert_eq!(response.status(), 304);
    assert_eq!(header(&response, "Content-Range"), None);
}

#[test]
fn returns_206_with_if_none_match_and_if_range_when_if_range_matches_and_if_none_match_does_not() {
    let file = file_at("0123456789", "video.mp4", "video/mp4", JAN_2025);
    let last_modified = header(&respond(&file, get(&[])), "Last-Modified").expect("Last-Modified");

    let response = respond(&file, get(&[("If-None-Match", "\"wrong-etag\""), ("If-Range", &last_modified), ("Range", "bytes=0-4")]));

    assert_eq!(response.status(), 206);
    assert_eq!(header(&response, "Content-Range").as_deref(), Some("bytes 0-4/10"));
    assert_eq!(text(response), "01234");
}

#[test]
fn returns_200_with_if_none_match_and_if_range_when_neither_matches() {
    let file = file_at("0123456789", "video.mp4", "video/mp4", JAN_2025);
    let response = respond(&file, get(&[("If-None-Match", "\"wrong-etag\""), ("If-Range", &utc(JAN_2024)), ("Range", "bytes=0-4")]));

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Range"), None);
    assert_eq!(text(response), "0123456789");
}

// Cache-Control

#[test]
fn does_not_include_cache_control_header_by_default() {
    let response = respond(&file("Hello", "test.txt", "text/plain"), get(&[]));

    assert_eq!(header(&response, "Cache-Control"), None);
}

#[test]
fn uses_custom_cache_control_header() {
    let options = FileResponseOptions { cache_control: Some("no-cache".into()), ..FileResponseOptions::default() };
    let response = respond_with(&file("Hello", "test.txt", "text/plain"), get(&[]), &options);

    assert_eq!(header(&response, "Cache-Control").as_deref(), Some("no-cache"));
}

// Content-Type

#[test]
fn sets_content_type_from_file_with_charset_for_text_based_types() {
    let cases = [("text/html", "test.html"), ("text/css", "test.css"), ("text/plain", "test.txt"), ("text/javascript", "test.js"), ("application/json", "test.json")];

    for (content_type, name) in cases {
        let response = respond(&file("test content", name, content_type), get(&[]));
        assert_eq!(response.status(), 200);
        assert_eq!(header(&response, "Content-Type"), Some(format!("{content_type}; charset=utf-8")));
    }
}

#[test]
fn does_not_add_charset_to_xml_types() {
    for (content_type, name) in [("image/svg+xml", "test.svg"), ("application/xml", "test.xml")] {
        let response = respond(&file("test content", name, content_type), get(&[]));
        assert_eq!(response.status(), 200);
        assert_eq!(header(&response, "Content-Type").as_deref(), Some(content_type));
    }
}

#[test]
fn sets_content_type_with_charset_for_application_javascript() {
    let response = respond(&file("test content", "app.js", "application/javascript"), get(&[]));

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Type").as_deref(), Some("application/javascript; charset=utf-8"));
}

#[test]
fn does_not_add_charset_to_binary_types() {
    let cases = [("image/png", "test.png"), ("image/jpeg", "test.jpg"), ("application/pdf", "test.pdf"), ("application/zip", "test.zip")];

    for (content_type, name) in cases {
        let response = respond(&file("test content", name, content_type), get(&[]));
        assert_eq!(response.status(), 200);
        assert_eq!(header(&response, "Content-Type").as_deref(), Some(content_type));
    }
}

#[test]
fn handles_file_with_empty_type() {
    let response = respond(&file("test content", "test.txt", ""), get(&[]));

    assert_eq!(response.status(), 200);
    assert_eq!(header(&response, "Content-Type"), None);
}

// rmx only: a ranged LazyFile streams just the requested bytes.

#[test]
fn serves_a_range_of_a_lazy_file() {
    let options = LazyFileOptions { content_type: "video/mp4".into(), ..LazyFileOptions::default() };
    let lazy_file = LazyFile::new([BlobPart::from("0123"), BlobPart::from("456789")], "video.mp4", options);
    let response = respond(&lazy_file, get(&[("Range", "bytes=2-6")]));

    assert_eq!(response.status(), 206);
    assert_eq!(header(&response, "Content-Range").as_deref(), Some("bytes 2-6/10"));
    assert_eq!(header(&response, "Content-Length").as_deref(), Some("5"));
    assert_eq!(text(response), "23456");
}
