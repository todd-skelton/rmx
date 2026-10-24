//! Ports of Remix's `multipart.test.ts` and `multipart-request.test.ts`.

use rsmix_fetch::{block_on, Body};

use super::*;

const BOUNDARY: &str = "----WebKitFormBoundaryPMcT9NSv6M3P8D4Q";

enum Value<'a> {
    Text(&'a str),
    File { filename: Option<&'a str>, filename_ext: Option<&'a str>, media_type: Option<&'a str>, content: &'a [u8] },
}

/// Remix's `createMultipartMessage` test helper.
fn message(boundary: &str, parts: &[(&str, Value)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in parts {
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match value {
            Value::Text(text) => {
                out.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{text}\r\n").as_bytes());
            }
            Value::File { filename, filename_ext, media_type, content } => {
                let mut disposition = format!("Content-Disposition: form-data; name=\"{name}\"");
                if let Some(f) = filename {
                    disposition.push_str(&format!("; filename=\"{f}\""));
                }
                if let Some(f) = filename_ext {
                    disposition.push_str(&format!("; filename*={f}"));
                }
                out.extend_from_slice(disposition.as_bytes());
                out.extend_from_slice(b"\r\n");
                if let Some(m) = media_type {
                    out.extend_from_slice(format!("Content-Type: {m}\r\n").as_bytes());
                }
                out.extend_from_slice(b"\r\n");
                out.extend_from_slice(content);
                out.extend_from_slice(b"\r\n");
            }
        }
    }
    out.extend_from_slice(format!("--{boundary}--").as_bytes());
    out
}

fn field(value: &str) -> Vec<u8> {
    message(BOUNDARY, &[("field", Value::Text(value))])
}

fn opts() -> MultipartOptions {
    MultipartOptions::default()
}

fn split(message: &[u8], at: usize) -> Result<Vec<MultipartPart>, MultipartError> {
    parse_multipart_chunks([&message[..at], &message[at..]], BOUNDARY, &opts())
}

fn names_and_text(parts: &[MultipartPart]) -> Vec<(String, String)> {
    parts.iter().map(|p| (p.name().unwrap_or_default(), p.text().unwrap())).collect()
}

fn invalid_ending() -> MultipartError {
    MultipartError::Parse("Invalid multipart boundary ending".into())
}

#[test]
fn boundary_length() {
    assert!(MultipartParser::new(&"b".repeat(70), opts()).is_ok());
    let err = MultipartParser::new(&"b".repeat(71), opts()).err().unwrap();
    assert_eq!(err.to_string(), "Multipart boundary exceeds maximum length of 70 characters");
}

#[test]
fn waits_for_a_complete_delimiter_ending() {
    let mut parser = MultipartParser::new(BOUNDARY, opts()).unwrap();
    let first = format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"field\"\r\n\r\nvalue\r\n--{BOUNDARY}");
    assert!(parser.write(first.as_bytes()).unwrap().is_empty());
    assert!(parser.write(b"\r").unwrap().is_empty());
    let parts = parser.write(b"\n").unwrap();
    assert_eq!(names_and_text(&parts), [("field".into(), "value".into())]);
}

#[test]
fn final_part_and_end_of_stream() {
    let msg = field("value");
    let mut parser = MultipartParser::new(BOUNDARY, opts()).unwrap();
    assert!(parser.write(&msg[..msg.len() - 1]).unwrap().is_empty());
    assert_eq!(parser.write(&msg[msg.len() - 1..]).unwrap().len(), 1);
    parser.finish().unwrap();
    parser.finish().unwrap();
    assert_eq!(parser.write(b"X").unwrap_err().to_string(), "Unexpected data after end of stream");

    let mut parser = MultipartParser::new(BOUNDARY, opts()).unwrap();
    assert_eq!(parser.write(&msg).unwrap().len(), 1);
    assert!(parser.write(b" \t\r").unwrap().is_empty());
    assert!(parser.write(b"\n").unwrap().is_empty(), "no second yield for the closing CRLF");
    parser.finish().unwrap();

    let mut parser = MultipartParser::new(BOUNDARY, opts()).unwrap();
    parser.write(&msg).unwrap();
    assert_eq!(parser.write(b"X").unwrap_err(), invalid_ending());

    let mut parser = MultipartParser::new(BOUNDARY, opts()).unwrap();
    parser.write(&msg).unwrap();
    assert!(parser.write(b"\r").unwrap().is_empty());
    assert_eq!(parser.finish().unwrap_err(), invalid_ending(), "a closing CR at EOF");

    let mut parser = MultipartParser::new(BOUNDARY, opts()).unwrap();
    assert!(parser.write(&message(BOUNDARY, &[])).unwrap().is_empty());
    parser.finish().unwrap();

    let mut parser = MultipartParser::new(BOUNDARY, MultipartOptions { max_parts: 0, ..opts() }).unwrap();
    assert_eq!(parser.write(&msg).unwrap_err(), MultipartError::MaxPartsExceeded(0));
}

#[test]
fn closing_endings_at_every_split() {
    for ending in ["X", "\r", "\rX", "\n", " \tX"] {
        let msg = [field("value"), ending.as_bytes().to_vec()].concat();
        for at in 0..=msg.len() {
            assert_eq!(split(&msg, at).unwrap_err(), invalid_ending(), "{ending:?} split at {at}");
        }
    }
    for ending in ["", " \t", "\r\n", " \t\r\n"] {
        let msg = [field("value"), ending.as_bytes().to_vec()].concat();
        for at in 0..=msg.len() {
            assert_eq!(names_and_text(&split(&msg, at).unwrap()), [("field".into(), "value".into())], "{ending:?} split at {at}");
        }
        let empty = [message(BOUNDARY, &[]), ending.as_bytes().to_vec()].concat();
        for at in 0..=empty.len() {
            assert!(split(&empty, at).unwrap().is_empty());
        }
    }
    let epilogue = [field("value"), b" \t\r\nepilogue\r\n".to_vec()].concat();
    for at in 0..=epilogue.len() {
        assert_eq!(split(&epilogue, at).unwrap().len(), 1);
    }
}

#[test]
fn invalid_delimiter_endings() {
    let opening = format!("--{BOUNDARY}XYContent-Disposition: form-data; name=\"field\"\r\n\r\nvalue\r\n--{BOUNDARY}--");
    assert_eq!(parse_multipart(opening.as_bytes(), BOUNDARY, &opts()).unwrap_err(), invalid_ending());

    for ending in ["XY", "\rX", "X\n", "\n\n", " \tXY", " --"] {
        let msg = field(&format!("value\r\n--{BOUNDARY}{ending}Content-Disposition: form-data; name=\"other\"\r\n\r\nnext"));
        assert_eq!(parse_multipart(&msg, BOUNDARY, &opts()).unwrap_err(), invalid_ending(), "{ending:?}");
        // Streaming: no part is produced before the error.
        let mut parser = MultipartParser::new(BOUNDARY, opts()).unwrap();
        assert!(parser.write(&msg).is_err());
    }
    let msg = field(&format!("value\r\n--{BOUNDARY}XYContent-Disposition: form-data; name=\"other\"\r\n\r\nnext"));
    for at in 1..msg.len() {
        let mut parser = MultipartParser::new(BOUNDARY, opts()).unwrap();
        let first = parser.write(&msg[..at]);
        let second = first.as_ref().ok().map(|_| parser.write(&msg[at..]));
        assert!(first.as_ref().is_ok_and(|p| p.is_empty()) || first.is_err(), "split {at}");
        assert!(first.is_err() || second.is_some_and(|r| r.is_err()), "split {at}");
    }
}

#[test]
fn padded_delimiters_at_every_split() {
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
    .join("\r\n");
    for at in 1..msg.len() {
        assert_eq!(names_and_text(&split(msg.as_bytes(), at).unwrap()), [("first".into(), "one".into()), ("second".into(), "two".into())], "split {at}");
    }
}

#[test]
fn preambles() {
    let msg = [b"This is a MIME preamble.\r\n".to_vec(), field("hello")].concat();
    assert_eq!(parse_multipart(&msg, BOUNDARY, &opts()).unwrap()[0].text().unwrap(), "hello");
    for at in 0..=msg.len() {
        assert_eq!(split(&msg, at).unwrap()[0].text().unwrap(), "hello", "split {at}");
    }

    let first = format!("some preamble text x--{BOUNDARY}\r");
    let second = [b"\nX-Preamble: should be ignored\r\n".to_vec(), field("hello")].concat();
    let parts = parse_multipart_chunks([first.as_bytes(), &second], BOUNDARY, &opts()).unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].headers().get("x-preamble"), None);
    assert_eq!(names_and_text(&parts), [("field".into(), "hello".into())]);

    let crlf = [b"\r\n".to_vec(), field("hello")].concat();
    assert_eq!(parse_multipart(&crlf, BOUNDARY, &opts()).unwrap()[0].text().unwrap(), "hello");
}

#[test]
fn limits() {
    let three = message(BOUNDARY, &[("a", Value::Text("1")), ("b", Value::Text("2")), ("c", Value::Text("3"))]);
    assert_eq!(parse_multipart(&three, BOUNDARY, &MultipartOptions { max_parts: 2, ..opts() }).unwrap_err(), MultipartError::MaxPartsExceeded(2));
    let chunks: Vec<&[u8]> = three.chunks(7).collect();
    assert_eq!(
        parse_multipart_chunks(chunks, BOUNDARY, &MultipartOptions { max_total_size: Some(2), ..opts() }).unwrap_err(),
        MultipartError::MaxTotalSizeExceeded(2)
    );
    let big = message(BOUNDARY, &[("f", Value::File { filename: Some("a.bin"), filename_ext: None, media_type: None, content: &[7; 100] })]);
    let err = parse_multipart(&big, BOUNDARY, &MultipartOptions { max_file_size: 99, ..opts() }).unwrap_err();
    assert_eq!(err.to_string(), "File size exceeds maximum allowed size of 99 bytes");
    assert!(err.is_limit());
    let header = format!("--{BOUNDARY}\r\nX-Long: {}\r\n\r\nx\r\n--{BOUNDARY}--", "a".repeat(200));
    assert_eq!(parse_multipart(header.as_bytes(), BOUNDARY, &MultipartOptions { max_header_size: 100, ..opts() }).unwrap_err(), MultipartError::MaxHeaderSizeExceeded(100));
    assert_eq!(MultipartOptions::default().total_size(), 41 * 1024 * 1024);
}

#[test]
fn parts_files_and_headers() {
    let png = [0x89, b'P', b'N', b'G', 0, 1, 2, 255];
    let msg = message(
        BOUNDARY,
        &[
            ("title", Value::Text("Dune")),
            ("empty", Value::Text("")),
            ("cover", Value::File { filename: Some("dune.png"), filename_ext: None, media_type: Some("image/png"), content: &png }),
            ("blob", Value::File { filename: None, filename_ext: None, media_type: Some("application/octet-stream"), content: b"raw" }),
            ("na\u{ef}ve", Value::File { filename: Some("caf\u{e9}.txt"), filename_ext: None, media_type: Some("text/plain"), content: b"x" }),
            ("ext", Value::File { filename: Some("fallback.txt"), filename_ext: Some("UTF-8''na%C3%AFve%20file.txt"), media_type: None, content: b"y" }),
            ("pct", Value::File { filename: Some("100%25 done.txt"), filename_ext: None, media_type: None, content: b"z" }),
        ],
    );
    let parts = parse_multipart(&msg, BOUNDARY, &opts()).unwrap();
    assert_eq!(parts.len(), 7);
    assert_eq!((parts[0].name().unwrap(), parts[0].text().unwrap(), parts[0].is_text()), ("title".into(), "Dune".into(), true));
    assert_eq!((parts[1].text().unwrap(), parts[1].size()), (String::new(), 0));
    let cover = &parts[2];
    assert!(cover.is_file());
    assert_eq!((cover.filename().as_deref(), cover.media_type().as_deref()), (Some("dune.png"), Some("image/png")));
    assert_eq!(&cover.bytes()[..], &png);
    assert_eq!(cover.headers().get("Content-Type"), Some("image/png"));
    assert!(parts[3].is_file(), "octet-stream without a file name is a file");
    assert_eq!((parts[4].name().as_deref(), parts[4].filename().as_deref()), (Some("na\u{ef}ve"), Some("caf\u{e9}.txt")));
    assert_eq!(parts[5].filename().as_deref(), Some("na\u{ef}ve file.txt"), "filename* wins");
    assert_eq!(parts[6].filename().as_deref(), Some("100%25 done.txt"), "plain filenames are not decoded");
}

#[test]
fn large_files_and_boundaries_at_chunk_edges() {
    let content: Vec<u8> = (0..3_000_000u32).map(|i| (i * 7 % 251) as u8).collect();
    let msg = message(BOUNDARY, &[("file", Value::File { filename: Some("big.bin"), filename_ext: None, media_type: None, content: &content }), ("after", Value::Text("tail"))]);
    let options = MultipartOptions { max_file_size: 4 * 1024 * 1024, ..opts() };
    for size in [1000, 65536, 4093] {
        let chunks: Vec<&[u8]> = msg.chunks(size).collect();
        let parts = parse_multipart_chunks(chunks, BOUNDARY, &options).unwrap();
        assert_eq!(parts[0].bytes().len(), content.len(), "chunk size {size}");
        assert!(parts[0].bytes() == content, "chunk size {size}");
        assert_eq!(parts[1].text().unwrap(), "tail");
    }
    // A delimiter split at every position across two chunks.
    let small = message(BOUNDARY, &[("a", Value::Text("x\r\n-")), ("b", Value::Text("y"))]);
    for at in 0..=small.len() {
        assert_eq!(names_and_text(&split(&small, at).unwrap()), [("a".into(), "x\r\n-".into()), ("b".into(), "y".into())], "split {at}");
    }
    // Single-byte chunks.
    let chunks: Vec<&[u8]> = small.chunks(1).collect();
    assert_eq!(parse_multipart_chunks(chunks, BOUNDARY, &opts()).unwrap().len(), 2);
}

#[test]
fn malformed_messages() {
    assert_eq!(parse_multipart(b"no boundary here", BOUNDARY, &opts()).unwrap_err().to_string(), "Multipart stream not finished");
    let unterminated = format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nvalue");
    assert_eq!(parse_multipart(unterminated.as_bytes(), BOUNDARY, &opts()).unwrap_err().to_string(), "Multipart stream not finished");
    assert!(parse_multipart(b"", BOUNDARY, &opts()).unwrap().is_empty());
    // A part without Content-Disposition still parses; it has no name.
    let nameless = format!("--{BOUNDARY}\r\nX-Other: 1\r\n\r\nvalue\r\n--{BOUNDARY}--");
    let parts = parse_multipart(nameless.as_bytes(), BOUNDARY, &opts()).unwrap();
    assert_eq!((parts[0].name(), parts[0].text().unwrap()), (None, "value".into()));
}

fn request(content_type: &str, body: Body) -> http::Request<Body> {
    http::Request::post("http://x/").header("content-type", content_type).body(body).unwrap()
}

fn collect(mut stream: MultipartStream<Body>) -> Result<Vec<MultipartPart>, MultipartError> {
    block_on(async move {
        let mut parts = Vec::new();
        while let Some(part) = stream.next_part().await {
            parts.push(part?);
        }
        Ok(parts)
    })
}

#[test]
fn requests() {
    assert_eq!(get_multipart_boundary("multipart/form-data; boundary=abc").as_deref(), Some("abc"));
    assert_eq!(get_multipart_boundary("multipart/form-data; charset=utf-8; boundary=\"Ab C\"").as_deref(), Some("Ab C"));
    assert_eq!(get_multipart_boundary("multipart/form-data; xboundary=nope").as_deref(), None);
    assert_eq!(get_multipart_boundary("multipart/form-data; boundary=first; boundary=second").as_deref(), Some("first"));
    assert_eq!(get_multipart_boundary("multipart/form-data"), None);
    assert!(is_multipart(Some("Multipart/Form-Data; boundary=x")));
    assert!(is_multipart(Some("multipart/mixed")));
    assert!(!is_multipart(Some("text/plain")));

    let msg = message(BOUNDARY, &[("a", Value::Text("1")), ("b", Value::Text("2"))]);
    let ct = format!("multipart/form-data; charset=utf-8; boundary={BOUNDARY}");
    let stream = parse_multipart_request(request(&ct, Body::from(msg.clone())), &opts()).unwrap();
    assert_eq!(names_and_text(&collect(stream).unwrap()), [("a".into(), "1".into()), ("b".into(), "2".into())]);

    // A body that arrives in small chunks.
    let chunked = Body::from_task(move |tx| async move {
        for chunk in msg.chunks(5) {
            tx.send(chunk.to_vec());
        }
    });
    assert_eq!(collect(parse_multipart_request(request(&ct, chunked), &opts()).unwrap()).unwrap().len(), 2);

    let err = |res: Result<MultipartStream<Body>, MultipartError>| res.err().unwrap().to_string();
    assert_eq!(err(parse_multipart_request(request("text/plain", Body::from("x")), &opts())), "Request is not a multipart request");
    assert_eq!(err(parse_multipart_request(request("multipart/form-data", Body::from("x")), &opts())), "Invalid Content-Type header: missing boundary");
    assert_eq!(err(parse_multipart_request(request(&ct, Body::empty()), &opts())), "Request body is empty");
    let missing_end = format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n");
    let stream = parse_multipart_request(request(&ct, Body::from(missing_end)), &opts()).unwrap();
    assert_eq!(collect(stream).unwrap_err().to_string(), "Multipart stream not finished");
    let stream = parse_multipart_request(request(&ct, Body::from(message(BOUNDARY, &[]))), &opts()).unwrap();
    assert!(collect(stream).unwrap().is_empty());
}

// Port of Remix's buffer-search.test.ts (createSearch is find, createPartialTailSearch is
// MultipartParser::partial_tail).

fn tail_search(pattern: &str) -> MultipartParser {
    let mut parser = MultipartParser::new(BOUNDARY, opts()).unwrap();
    parser.delimiter = pattern.as_bytes().to_vec();
    parser
}

#[test]
fn search_finds_the_first_occurrence_of_a_pattern_in_a_buffer() {
    assert_eq!(find(b"hello world", b"world", 0), Some(6));
}

#[test]
fn search_returns_none_if_the_pattern_is_not_found() {
    assert_eq!(find(b"hello worl", b"world", 0), None);
}

#[test]
fn search_handles_long_patterns_whose_length_would_wrap_a_byte_sized_skip_table() {
    assert_eq!(find(&[b'a'; 512], &[b'x'; 256], 0), None);
}

#[test]
fn search_finds_a_256_byte_pattern() {
    let hay = [vec![b'a'; 10], vec![b'x'; 256]].concat();
    assert_eq!(find(&hay, &[b'x'; 256], 0), Some(10));
}

#[test]
fn partial_tail_search_finds_the_last_partial_occurrence_of_a_pattern_in_a_buffer() {
    assert_eq!(tail_search("world").partial_tail(b"hello worl"), Some(6));
}

#[test]
fn partial_tail_search_returns_none_if_the_pattern_is_not_found_at_the_end() {
    assert_eq!(tail_search("world").partial_tail(b"hello worlds"), None);
}

/// Not in Remix: with a self-overlapping pattern the longest partial match wins, so no byte
/// that could start the pattern is released as content.
#[test]
fn partial_tail_search_prefers_the_longest_partial_match() {
    assert_eq!(tail_search("abab").partial_tail(b"xaba"), Some(1));
}
