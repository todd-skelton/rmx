//! Port of Remix's `multipart-request.test.ts`, one test per case.
//!
//! Remix's `parseMultipartRequest` is an async generator, so request errors surface on the
//! first iteration; `parse_multipart_request` returns them up front. Part headers are
//! looked up case-insensitively (`MultipartHeaders::get`) and stored with lowercase names,
//! which is what Remix's frozen, lowercase-keyed plain object gives.

mod common;

use common::*;
use rmx_fetch::Body;
use rmx_multipart_parser::*;

const BOUNDARY: &str = "----WebKitFormBoundaryz8Zv2UxQ7f4a0Z3H";
const LARGE_FILE_SIZE: usize = 128 * 1024;

fn opts() -> MultipartOptions {
    MultipartOptions::default()
}

fn boundary(content_type: &str) -> Option<String> {
    get_multipart_boundary(content_type)
}

fn multipart_request(body: impl Into<Body>) -> http::Request<Body> {
    request(&format!("multipart/form-data; boundary={BOUNDARY}"), body.into())
}

fn parse(request: http::Request<Body>, options: MultipartOptions) -> (Vec<MultipartPart>, Option<MultipartError>) {
    match parse_multipart_request(request, &options) {
        Ok(stream) => collect(stream),
        Err(e) => (Vec::new(), Some(e)),
    }
}

fn parts(request: http::Request<Body>) -> Vec<MultipartPart> {
    let (parts, error) = parse(request, opts());
    assert_eq!(error, None);
    parts
}

fn error(request: http::Request<Body>, options: MultipartOptions) -> MultipartError {
    parse(request, options).1.expect("expected an error")
}

fn two_fields() -> Vec<u8> {
    message(BOUNDARY, &[("field1", Value::Text("value1")), ("field2", Value::Text("value2"))])
}

fn chunked_request(body: Vec<u8>, chunk_size: usize) -> http::Request<Body> {
    multipart_request(chunked_body(body, chunk_size))
}

fn delimiter_index(body: &[u8]) -> usize {
    let index = index_of(body, format!("\r\n--{BOUNDARY}").as_bytes(), 1).unwrap();
    assert!(index > 0);
    index
}

fn assert_two_fields(parts: &[MultipartPart]) {
    assert_eq!(names_and_text(parts), [("field1".into(), "value1".into()), ("field2".into(), "value2".into())]);
}

fn parse_error() -> impl Fn(&MultipartError) -> bool {
    |e| matches!(e, MultipartError::Parse(_))
}

// getMultipartBoundary

#[test]
fn returns_the_boundary_from_the_content_type_header() {
    assert_eq!(boundary("multipart/form-data; boundary=boundary123").as_deref(), Some("boundary123"));
}

#[test]
fn ignores_parameter_names_that_end_with_boundary() {
    assert_eq!(boundary("multipart/form-data; xboundary=other; boundary=boundary123").as_deref(), Some("boundary123"));
    assert_eq!(boundary("multipart/form-data; xboundary=other"), None);
}

#[test]
fn ignores_boundary_text_inside_another_quoted_parameter() {
    assert_eq!(boundary("multipart/form-data; note=\"value; boundary=other\"; boundary=boundary123").as_deref(), Some("boundary123"));
    assert_eq!(boundary("multipart/form-data; note=\"boundary=other\""), None);
}

#[test]
fn honors_escaped_quotes_while_reading_parameters() {
    assert_eq!(boundary(r#"multipart/form-data; note="value\"; boundary=other"; boundary=boundary123"#).as_deref(), Some("boundary123"));
    assert_eq!(boundary(r#"multipart/form-data; boundary="value\"; boundary=other""#).as_deref(), Some("value\"; boundary=other"));
}

#[test]
fn preserves_boundary_case_and_handles_parameter_whitespace() {
    assert_eq!(boundary("multipart/form-data; BOUNDARY = \"Boundary123\"").as_deref(), Some("Boundary123"));
    assert_eq!(boundary("multipart/form-data; boundary=Boundary123 ; charset=utf-8").as_deref(), Some("Boundary123"));
}

#[test]
fn keeps_the_first_boundary_when_the_parameter_is_repeated() {
    assert_eq!(boundary("multipart/form-data; boundary=first; boundary=second").as_deref(), Some("first"));
}

#[test]
fn does_not_read_parameters_inside_an_unterminated_quoted_value() {
    assert_eq!(boundary("multipart/form-data; note=\"value; boundary=other"), None);
}

#[test]
fn reads_an_unterminated_quoted_boundary_to_the_end_of_the_header() {
    assert_eq!(boundary("multipart/form-data; boundary=\"abc").as_deref(), Some("abc"));
    assert_eq!(boundary(r#"multipart/form-data; boundary="abc\"; charset=utf-8"#).as_deref(), Some("abc\"; charset=utf-8"));
}

#[test]
fn returns_null_when_boundary_is_missing() {
    assert_eq!(boundary("multipart/form-data"), None);
}

#[test]
fn returns_null_when_content_type_header_is_not_multipart() {
    assert_eq!(boundary("text/plain"), None);
}

// isMultipartRequest

#[test]
fn recognizes_mixed_case_media_types_with_parameters() {
    assert!(is_multipart_request(&request("Multipart/Mixed; boundary=\"ExampleBoundary\"", Body::empty())));
}

#[test]
fn returns_true_for_multipart_form_data_requests() {
    assert!(is_multipart_request(&request("multipart/form-data", Body::empty())));
}

#[test]
fn returns_true_for_multipart_mixed_requests() {
    assert!(is_multipart_request(&request("multipart/mixed", Body::empty())));
}

#[test]
fn returns_false_for_other_content_types() {
    assert!(!is_multipart_request(&request("text/plain", Body::empty())));
}

// parseMultipartRequest

#[test]
fn uses_the_boundary_parameter_after_unrelated_parameters() {
    let content_type = format!("multipart/form-data; xboundary=other; note=\"value; boundary=other\"; boundary={BOUNDARY}");
    let parts = parts(request(&content_type, Body::from(message(BOUNDARY, &[("field", Value::Text("value"))]))));
    assert_eq!(names_and_text(&parts), [("field".into(), "value".into())]);
}

#[test]
fn parses_a_request_with_an_unterminated_quoted_boundary() {
    let content_type = format!("multipart/form-data; boundary=\"{BOUNDARY}");
    let parts = parts(request(&content_type, Body::from(message(BOUNDARY, &[("field", Value::Text("value"))]))));
    assert_eq!(names_and_text(&parts), [("field".into(), "value".into())]);
}

#[test]
fn rejects_a_boundary_inside_an_unterminated_quoted_parameter() {
    let content_type = format!("multipart/form-data; note=\"value; boundary={BOUNDARY}");
    let (parts, error) = parse(request(&content_type, Body::from(message(BOUNDARY, &[("field", Value::Text("value"))]))), opts());
    assert!(parts.is_empty());
    assert_eq!(error, Some(MultipartError::Parse("Invalid Content-Type header: missing boundary".into())));
}

#[test]
fn parses_an_empty_multipart_message() {
    assert!(parts(multipart_request(format!("--{BOUNDARY}--"))).is_empty());
}

#[test]
fn parses_a_simple_multipart_form() {
    let parts = parts(multipart_request(message(BOUNDARY, &[("field1", Value::Text("value1"))])));
    assert_eq!(names_and_text(&parts), [("field1".into(), "value1".into())]);
}

#[test]
fn parses_multiple_parts_correctly() {
    assert_two_fields(&parts(multipart_request(two_fields())));
}

#[test]
fn parses_empty_parts_correctly() {
    let parts = parts(multipart_request(message(BOUNDARY, &[("empty", Value::Text(""))])));
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name().as_deref(), Some("empty"));
    assert_eq!(parts[0].bytes().len(), 0);
}

#[test]
fn parses_file_uploads_correctly() {
    let parts = parts(multipart_request(message(BOUNDARY, &[("file1", file("test.txt", "text/plain", b"File content"))])));
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name().as_deref(), Some("file1"));
    assert_eq!(parts[0].filename().as_deref(), Some("test.txt"));
    assert_eq!(parts[0].media_type().as_deref(), Some("text/plain"));
    assert_eq!(parts[0].text().unwrap(), "File content");
}

#[test]
fn exposes_decoded_part_headers_as_a_plain_object() {
    let body = [
        format!("--{BOUNDARY}"),
        "Content-Disposition: form-data; name=\"file\"; filename=\"test.txt\"".into(),
        "Content-Type: text/plain".into(),
        "X-Custom: one".into(),
        "X-Custom: two".into(),
        String::new(),
        "File content".into(),
        format!("--{BOUNDARY}--"),
    ]
    .join(CRLF);
    let parts = parts(multipart_request(body));
    assert_eq!(parts.len(), 1);
    let headers = parts[0].headers();
    assert_eq!(headers.get("content-disposition"), Some("form-data; name=\"file\"; filename=\"test.txt\""));
    assert_eq!(headers.get("content-type"), Some("text/plain"));
    assert_eq!(headers.get("x-custom"), Some("one, two"));
    let names: Vec<&str> = headers.iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["content-disposition", "content-type", "x-custom"], "names are stored lowercase");
}

#[test]
fn preserves_non_ascii_field_names_and_filenames() {
    let parts = parts(multipart_request(message(BOUNDARY, &[("\u{540d}\u{524d}", file("\u{30c6}\u{30b9}\u{30c8}\u{753b}\u{50cf}.png", "image/png", b"image content"))])));
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name().as_deref(), Some("\u{540d}\u{524d}"));
    assert_eq!(parts[0].filename().as_deref(), Some("\u{30c6}\u{30b9}\u{30c8}\u{753b}\u{50cf}.png"));
    assert_eq!(parts[0].media_type().as_deref(), Some("image/png"));
}

#[test]
fn preserves_literal_percent_encoded_sequences_in_filenames() {
    let parts = parts(multipart_request(message(BOUNDARY, &[("file", file("%2Fetc%2Fpasswd", "text/plain", b"File content"))])));
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].filename().as_deref(), Some("%2Fetc%2Fpasswd"));
}

#[test]
fn parses_multiple_fields_and_a_file_upload() {
    let body = message(BOUNDARY, &[("field1", Value::Text("value1")), ("field2", Value::Text("value2")), ("file1", file("test.txt", "text/plain", b"File content"))]);
    let parts = parts(multipart_request(body));
    assert_eq!(parts.len(), 3);
    assert_two_fields(&parts[..2]);
    assert_eq!(parts[2].name().as_deref(), Some("file1"));
    assert_eq!(parts[2].filename().as_deref(), Some("test.txt"));
    assert_eq!(parts[2].media_type().as_deref(), Some("text/plain"));
    assert_eq!(parts[2].text().unwrap(), "File content");
}

#[test]
fn parses_large_file_uploads_correctly() {
    let content = random_bytes(LARGE_FILE_SIZE);
    let body = message(BOUNDARY, &[("file1", file("random.dat", "application/octet-stream", &content))]);
    let (parts, error) = parse(multipart_request(body), MultipartOptions { max_file_size: LARGE_FILE_SIZE, ..opts() });
    assert_eq!(error, None);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name().as_deref(), Some("file1"));
    assert_eq!(parts[0].filename().as_deref(), Some("random.dat"));
    assert_eq!(parts[0].media_type().as_deref(), Some("application/octet-stream"));
    assert!(parts[0].bytes() == content);
}

#[test]
fn parses_when_boundary_is_split_across_chunks() {
    let body = two_fields();
    let index = delimiter_index(&body);
    assert_two_fields(&parts(chunked_request(body, index + 3)));
}

#[test]
fn parses_when_a_partial_boundary_tail_is_at_chunk_edge() {
    let body = two_fields();
    let index = delimiter_index(&body);
    // The first chunk ends with only the CR of the delimiter.
    assert_two_fields(&parts(chunked_request(body, index + 1)));
}

#[test]
fn parses_when_boundary_starts_exactly_at_next_chunk_edge() {
    let body = two_fields();
    let index = delimiter_index(&body);
    assert_two_fields(&parts(chunked_request(body, index)));
}

#[test]
fn throws_when_content_type_is_not_multipart_form_data() {
    let error = error(request("text/plain", Body::empty()), opts());
    assert_eq!(error, MultipartError::Parse("Request is not a multipart request".into()));
}

#[test]
fn throws_when_initial_boundary_is_missing() {
    let error = error(request("multipart/form-data", Body::from("Content-Disposition: form-data; name=\"field1\"\r\n\r\nvalue1")), opts());
    assert!(parse_error()(&error), "{error:?}");
}

#[test]
fn throws_when_header_exceeds_maximum_size() {
    let body = [
        format!("--{BOUNDARY}"),
        "Content-Disposition: form-data; name=\"field1\"".into(),
        format!("X-Large-Header: {}", "X".repeat(6 * 1024)),
        String::new(),
        "value1".into(),
        format!("--{BOUNDARY}--"),
    ]
    .join(CRLF);
    let error = error(multipart_request(body), MultipartOptions { max_header_size: 4 * 1024, ..opts() });
    assert_eq!(error, MultipartError::MaxHeaderSizeExceeded(4 * 1024));
}

#[test]
fn throws_when_a_file_exceeds_maximum_size() {
    let content = random_bytes(LARGE_FILE_SIZE + 1);
    let body = message(BOUNDARY, &[("file1", file("random.dat", "application/octet-stream", &content))]);
    let error = error(multipart_request(body), MultipartOptions { max_file_size: LARGE_FILE_SIZE, ..opts() });
    assert_eq!(error, MultipartError::MaxFileSizeExceeded(LARGE_FILE_SIZE));
}

#[test]
fn throws_when_the_number_of_parts_exceeds_max_parts() {
    let body = message(BOUNDARY, &[("field1", Value::Text("value1")), ("field2", Value::Text("value2")), ("field3", Value::Text("value3"))]);
    let error = error(multipart_request(body), MultipartOptions { max_parts: 2, ..opts() });
    assert_eq!(error, MultipartError::MaxPartsExceeded(2));
}

#[test]
fn throws_when_the_aggregate_content_size_exceeds_max_total_size() {
    let body = message(BOUNDARY, &[("field1", Value::Text("hello")), ("field2", Value::Text("world"))]);
    let error = error(multipart_request(body), MultipartOptions { max_total_size: Some(9), ..opts() });
    assert_eq!(error, MultipartError::MaxTotalSizeExceeded(9));
}

#[test]
fn parses_malformed_parts() {
    let body = [format!("--{BOUNDARY}"), "Invalid-Header".into(), String::new(), "Some content".into(), format!("--{BOUNDARY}--")].join(CRLF);
    let parts = parts(multipart_request(body));
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].headers().get("invalid-header"), None);
    assert_eq!(parts[0].text().unwrap(), "Some content");
}

#[test]
fn throws_error_when_final_boundary_is_missing() {
    let body = [format!("--{BOUNDARY}"), "Content-Disposition: form-data; name=\"field1\"".into(), String::new(), "value1".into(), format!("--{BOUNDARY}")].join(CRLF);
    let error = error(multipart_request(body), opts());
    assert!(parse_error()(&error), "{error:?}");
}

#[test]
fn throws_error_when_request_body_is_empty() {
    let error = error(multipart_request(Body::empty()), opts());
    assert_eq!(error, MultipartError::Parse("Request body is empty".into()));
}

/// Remix checks for a body before it looks for the boundary.
#[test]
fn an_empty_body_is_reported_before_a_missing_boundary() {
    let error = error(request("multipart/form-data", Body::empty()), opts());
    assert_eq!(error, MultipartError::Parse("Request body is empty".into()));
}
