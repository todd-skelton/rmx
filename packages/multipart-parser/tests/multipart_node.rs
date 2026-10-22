//! Port of Remix's `multipart.node.test.ts`. rmx has one request type, so the Node
//! adapter's cases run against `parse_multipart_request` with an `http::Request`.

mod common;

use common::*;
use rmx_fetch::Body;
use rmx_multipart_parser::*;

const BOUNDARY: &str = "----WebKitFormBoundaryzv5f5B2cY6tjQ0Rn";
const LARGE_FILE_SIZE: usize = 128 * 1024;

fn node_request(body: Vec<u8>) -> http::Request<Body> {
    request(&format!("multipart/form-data; boundary={BOUNDARY}"), Body::from(body))
}

fn parse(request: http::Request<Body>, options: MultipartOptions) -> (Vec<MultipartPart>, Option<MultipartError>) {
    collect(parse_multipart_request(request, &options).unwrap())
}

#[test]
fn parses_mixed_case_media_types_with_surrounding_whitespace() {
    let body = message(BOUNDARY, &[("field", Value::Text("value"))]);
    let request = request(&format!(" Multipart/Form-Data; boundary=\"{BOUNDARY}\" "), Body::from(body));
    let (parts, error) = parse(request, MultipartOptions::default());
    assert_eq!(error, None);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].text().unwrap(), "value");
}

#[test]
fn parses_an_empty_multipart_message() {
    let (parts, error) = parse(node_request(message(BOUNDARY, &[])), MultipartOptions::default());
    assert_eq!(error, None);
    assert!(parts.is_empty());
}

#[test]
fn parses_a_simple_multipart_form() {
    let (parts, error) = parse(node_request(message(BOUNDARY, &[("field1", Value::Text("value1"))])), MultipartOptions::default());
    assert_eq!(error, None);
    assert_eq!(names_and_text(&parts), [("field1".into(), "value1".into())]);
}

#[test]
fn parses_large_file_uploads_correctly() {
    let content = random_bytes(LARGE_FILE_SIZE);
    let body = message(BOUNDARY, &[("file1", file("tesla.jpg", "image/jpeg", &content))]);
    let (parts, error) = parse(node_request(body), MultipartOptions { max_file_size: LARGE_FILE_SIZE, ..Default::default() });
    assert_eq!(error, None);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name().as_deref(), Some("file1"));
    assert_eq!(parts[0].filename().as_deref(), Some("tesla.jpg"));
    assert_eq!(parts[0].media_type().as_deref(), Some("image/jpeg"));
    assert!(parts[0].bytes() == content);
}

#[test]
fn throws_when_the_number_of_parts_exceeds_max_parts() {
    let body = message(BOUNDARY, &[("field1", Value::Text("value1")), ("field2", Value::Text("value2")), ("field3", Value::Text("value3"))]);
    let (_, error) = parse(node_request(body), MultipartOptions { max_parts: 2, ..Default::default() });
    assert_eq!(error, Some(MultipartError::MaxPartsExceeded(2)));
}

#[test]
fn throws_when_the_aggregate_content_size_exceeds_max_total_size() {
    let body = message(BOUNDARY, &[("field1", Value::Text("hello")), ("field2", Value::Text("world"))]);
    let (_, error) = parse(node_request(body), MultipartOptions { max_total_size: Some(9), ..Default::default() });
    assert_eq!(error, Some(MultipartError::MaxTotalSizeExceeded(9)));
}
