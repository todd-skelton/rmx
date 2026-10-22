//! Port of Remix's `multipart.test.ts`, one test per case.
//!
//! Remix's "does not eagerly use Web Encoding globals while importing and parsing" has no
//! counterpart: Rust has no global `TextEncoder`/`TextDecoder` to replace. The property it
//! guards (parser syntax is matched as bytes and text is decoded only on demand) holds by
//! construction here: `MultipartPart::text` decodes when called.

mod common;

use common::*;
use rmx_multipart_parser::*;

const BOUNDARY: &str = "----WebKitFormBoundaryPMcT9NSv6M3P8D4Q";

fn opts() -> MultipartOptions {
    MultipartOptions::default()
}

fn parser() -> MultipartParser {
    MultipartParser::new(BOUNDARY, opts()).unwrap()
}

fn field(value: &str) -> Vec<u8> {
    message(BOUNDARY, &[("field", Value::Text(value))])
}

fn with_ending(msg: Vec<u8>, ending: &str) -> Vec<u8> {
    [msg, ending.as_bytes().to_vec()].concat()
}

fn split(msg: &[u8], at: usize) -> Result<Vec<MultipartPart>, MultipartError> {
    parse_multipart_chunks([&msg[..at], &msg[at..]], BOUNDARY, &opts())
}

/// Feed chunks to a parser, keeping the parts yielded before any error.
fn write_all(chunks: &[&[u8]]) -> (Vec<MultipartPart>, Option<MultipartError>) {
    let mut parser = parser();
    let mut parts = Vec::new();
    for chunk in chunks {
        match parser.write(chunk) {
            Ok(more) => parts.extend(more),
            Err(e) => return (parts, Some(e)),
        }
    }
    (parts, parser.finish().err())
}

fn stream(msg: Vec<u8>, chunk_size: usize, options: MultipartOptions) -> (Vec<MultipartPart>, Option<MultipartError>) {
    collect(parse_multipart_stream(chunked_body(msg, chunk_size), BOUNDARY, &options).unwrap())
}

fn three_fields() -> Vec<u8> {
    message(BOUNDARY, &[("field1", Value::Text("value1")), ("field2", Value::Text("value2")), ("field3", Value::Text("value3"))])
}

fn hello_world() -> Vec<u8> {
    message(BOUNDARY, &[("field1", Value::Text("hello")), ("field2", Value::Text("world"))])
}

fn other_part_after(ending: &str) -> Vec<u8> {
    field(&format!("value\r\n--{BOUNDARY}{ending}Content-Disposition: form-data; name=\"other\"\r\n\r\nnext"))
}

// MultipartParser

#[test]
fn accepts_boundaries_up_to_70_characters() {
    assert!(MultipartParser::new(&"b".repeat(70), opts()).is_ok());
}

#[test]
fn rejects_boundaries_longer_than_70_characters() {
    let error = MultipartParser::new(&"b".repeat(71), opts()).err().unwrap();
    assert_eq!(error, MultipartError::Parse("Multipart boundary exceeds maximum length of 70 characters".into()));
}

#[test]
fn waits_for_a_complete_delimiter_ending_before_yielding_a_part() {
    let mut parser = parser();
    let first = format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"field\"\r\n\r\nvalue\r\n--{BOUNDARY}");
    assert!(parser.write(first.as_bytes()).unwrap().is_empty());
    assert!(parser.write(&[13]).unwrap().is_empty());
    let parts = parser.write(&[10]).unwrap();
    assert_eq!(names_and_text(&parts), [("field".into(), "value".into())]);
}

#[test]
fn yields_the_final_part_from_write_when_both_closing_hyphens_arrive() {
    let mut parser = parser();
    let msg = field("value");
    assert!(parser.write(&msg[..msg.len() - 1]).unwrap().is_empty());
    let parts = parser.write(&msg[msg.len() - 1..]).unwrap();
    assert_eq!(names_and_text(&parts), [("field".into(), "value".into())]);
    assert_eq!(parser.finish(), Ok(()));
    assert_eq!(parser.finish(), Ok(()));
    assert_eq!(parser.write(&[88]).unwrap_err(), MultipartError::Parse("Unexpected data after end of stream".into()));
}

#[test]
fn does_not_yield_the_final_part_again_when_a_closing_crlf_arrives() {
    let mut parser = parser();
    let parts = parser.write(&field("value")).unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].text().unwrap(), "value");
    assert!(parser.write(&[32, 9, 13]).unwrap().is_empty());
    assert!(parser.write(&[10]).unwrap().is_empty());
    assert_eq!(parser.finish(), Ok(()));
}

#[test]
fn rejects_an_invalid_closing_suffix_after_yielding_the_final_part() {
    let mut parser = parser();
    let parts = parser.write(&field("value")).unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].text().unwrap(), "value");
    assert_eq!(parser.write(&[88]).unwrap_err(), invalid_ending());
}

#[test]
fn rejects_a_closing_cr_at_eof_after_yielding_the_final_part() {
    let mut parser = parser();
    let parts = parser.write(&field("value")).unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].text().unwrap(), "value");
    assert!(parser.write(&[13]).unwrap().is_empty());
    assert_eq!(parser.finish().unwrap_err(), invalid_ending());
}

#[test]
fn finishes_an_empty_multipart_message_at_eof_without_returning_a_part() {
    let mut parser = parser();
    assert!(parser.write(&message(BOUNDARY, &[])).unwrap().is_empty());
    assert_eq!(parser.finish(), Ok(()));
    assert_eq!(parser.finish(), Ok(()));
}

#[test]
fn enforces_max_parts_when_write_yields_the_final_part() {
    let mut parser = MultipartParser::new(BOUNDARY, MultipartOptions { max_parts: 0, ..opts() }).unwrap();
    assert_eq!(parser.write(&field("value")).unwrap_err(), MultipartError::MaxPartsExceeded(0));
}

// parseMultipart

#[test]
fn rejects_invalid_closing_delimiter_endings_at_every_chunk_split() {
    for ending in ["X", "\r", "\rX", "\n", " \tX"] {
        let msg = with_ending(field("value"), ending);
        for at in 0..=msg.len() {
            assert_eq!(split(&msg, at).unwrap_err(), invalid_ending(), "{ending:?} split at {at}");
        }
    }
}

#[test]
fn accepts_closing_eof_padding_and_crlf_endings_at_every_chunk_split() {
    for ending in ["", " \t", "\r\n", " \t\r\n"] {
        let msg = with_ending(field("value"), ending);
        for at in 0..=msg.len() {
            assert_eq!(names_and_text(&split(&msg, at).unwrap()), [("field".into(), "value".into())], "{ending:?} split at {at}");
        }
    }
}

#[test]
fn accepts_empty_multipart_messages_at_every_closing_delimiter_chunk_split() {
    for ending in ["", " \t", "\r\n", " \t\r\n"] {
        let msg = with_ending(message(BOUNDARY, &[]), ending);
        for at in 0..=msg.len() {
            assert!(split(&msg, at).unwrap().is_empty(), "{ending:?} split at {at}");
        }
    }
}

#[test]
fn ignores_an_epilogue_after_the_closing_crlf_at_every_chunk_split() {
    let msg = with_ending(field("value"), " \t\r\nepilogue\r\n");
    for at in 0..=msg.len() {
        let texts: Vec<String> = split(&msg, at).unwrap().iter().map(|p| p.text().unwrap()).collect();
        assert_eq!(texts, ["value"], "split at {at}");
    }
}

#[test]
fn rejects_invalid_opening_delimiter_endings() {
    let msg = format!("--{BOUNDARY}XYContent-Disposition: form-data; name=\"field\"\r\n\r\nvalue\r\n--{BOUNDARY}--");
    assert_eq!(parse_multipart(msg.as_bytes(), BOUNDARY, &opts()).unwrap_err(), invalid_ending());
}

#[test]
fn rejects_invalid_part_delimiter_endings_before_yielding_a_part() {
    for ending in ["XY", "\rX", "X\n", "\n\n", " \tXY", " --"] {
        let msg = other_part_after(ending);
        let (parts, error) = write_all(&[&msg]);
        assert_eq!(error, Some(invalid_ending()), "{ending:?}");
        assert!(parts.is_empty(), "{ending:?}");
    }
}

#[test]
fn rejects_invalid_delimiter_endings_at_every_chunk_split() {
    let msg = other_part_after("XY");
    for at in 1..msg.len() {
        let (parts, error) = write_all(&[&msg[..at], &msg[at..]]);
        assert_eq!(error, Some(invalid_ending()), "split at {at}");
        assert!(parts.is_empty(), "split at {at}");
    }
}

#[test]
fn parses_padded_delimiter_lines_at_every_chunk_split() {
    let msg = [
        format!("--{BOUNDARY} \t"),
        "Content-Disposition: form-data; name=\"first\"".into(),
        String::new(),
        "one".into(),
        format!("--{BOUNDARY}\t "),
        "Content-Disposition: form-data; name=\"second\"".into(),
        String::new(),
        "two".into(),
        format!("--{BOUNDARY}--"),
    ]
    .join(CRLF);
    for at in 1..msg.len() {
        assert_eq!(
            names_and_text(&split(msg.as_bytes(), at).unwrap()),
            [("first".into(), "one".into()), ("second".into(), "two".into())],
            "split at {at}"
        );
    }
}

#[test]
fn ignores_a_textual_rfc_2046_preamble_before_the_initial_boundary() {
    let body = [b"This is a MIME preamble.\r\n".to_vec(), field("hello")].concat();
    let parts = parse_multipart(&body, BOUNDARY, &opts()).unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].text().unwrap(), "hello");
}

#[test]
fn ignores_boundary_text_at_a_chunk_edge_in_the_middle_of_a_preamble_line() {
    let first = format!("some preamble text x--{BOUNDARY}\r");
    let second = [b"\nX-Preamble: should be ignored\r\n".to_vec(), field("hello")].concat();
    let parts = parse_multipart_chunks([first.as_bytes(), &second], BOUNDARY, &opts()).unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].headers().get("x-preamble"), None);
    assert_eq!(names_and_text(&parts), [("field".into(), "hello".into())]);
}

#[test]
fn ignores_a_leading_crlf_before_the_initial_boundary() {
    let body = [b"\r\n".to_vec(), field("hello")].concat();
    let parts = parse_multipart(&body, BOUNDARY, &opts()).unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].text().unwrap(), "hello");
}

#[test]
fn finds_an_opening_boundary_split_across_chunks_after_a_preamble() {
    let body = [b"This is a MIME preamble.\r\n".to_vec(), field("hello")].concat();
    let parts = parse_multipart_chunks(body.chunks(3), BOUNDARY, &opts()).unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].text().unwrap(), "hello");
}

#[test]
fn parse_multipart_throws_when_the_number_of_parts_exceeds_max_parts() {
    let error = parse_multipart(&three_fields(), BOUNDARY, &MultipartOptions { max_parts: 2, ..opts() }).unwrap_err();
    assert_eq!(error, MultipartError::MaxPartsExceeded(2));
}

#[test]
fn throws_when_aggregate_content_size_exceeds_max_total_size_for_iterable_input() {
    let msg = hello_world();
    let error = parse_multipart_chunks(msg.chunks(7), BOUNDARY, &MultipartOptions { max_total_size: Some(9), ..opts() }).unwrap_err();
    assert_eq!(error, MultipartError::MaxTotalSizeExceeded(9));
}

// parseMultipartStream

#[test]
fn rejects_an_invalid_closing_suffix_after_yielding_the_final_part_in_single_byte_chunks() {
    let (parts, error) = stream(with_ending(field("value"), "X"), 1, opts());
    assert_eq!(error, Some(invalid_ending()));
    assert_eq!(names_and_text(&parts), [("field".into(), "value".into())]);
}

#[test]
fn yields_the_final_part_for_eof_and_crlf_endings_in_single_byte_chunks() {
    for ending in ["", " \t", "\r\nepilogue"] {
        let (parts, error) = stream(with_ending(field("value"), ending), 1, opts());
        assert_eq!(error, None, "{ending:?}");
        let texts: Vec<String> = parts.iter().map(|p| p.text().unwrap()).collect();
        assert_eq!(texts, ["value"], "{ending:?}");
    }
}

#[test]
fn rejects_invalid_delimiter_endings_in_single_byte_chunks() {
    let (parts, error) = stream(other_part_after("XY"), 1, opts());
    assert_eq!(error, Some(invalid_ending()));
    assert!(parts.is_empty());
}

#[test]
fn parse_multipart_stream_throws_when_the_number_of_parts_exceeds_max_parts() {
    let (_, error) = stream(three_fields(), 11, MultipartOptions { max_parts: 2, ..opts() });
    assert_eq!(error, Some(MultipartError::MaxPartsExceeded(2)));
}

#[test]
fn parse_multipart_stream_throws_when_aggregate_content_size_exceeds_max_total_size() {
    let (_, error) = stream(hello_world(), 7, MultipartOptions { max_total_size: Some(9), ..opts() });
    assert_eq!(error, Some(MultipartError::MaxTotalSizeExceeded(9)));
}
