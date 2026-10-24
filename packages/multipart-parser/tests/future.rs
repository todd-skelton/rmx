//! The streaming parser (`future`), after Remix's `future/multipart.test.ts`.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::future::Future;
use std::pin::{pin, Pin};
use std::task::{Context, Poll, Waker};

use bytes::Bytes;
use http_body::Frame;
use rmx_fetch::block_on;
use rmx_multipart_parser::future::{parse_multipart_request, parse_multipart_stream, MultipartStream};
use rmx_multipart_parser::{parse_multipart, MultipartError, MultipartOptions};

/// A body that yields `chunks`, then ends, or stays pending when `open` is set.
struct Chunks {
    chunks: VecDeque<Bytes>,
    open: bool,
}

impl Chunks {
    fn new<C: AsRef<[u8]>>(chunks: impl IntoIterator<Item = C>) -> Chunks {
        Chunks { chunks: chunks.into_iter().map(|c| Bytes::copy_from_slice(c.as_ref())).collect(), open: false }
    }

    fn split(message: &[u8], size: usize) -> Chunks {
        Chunks::new(message.chunks(size))
    }
}

impl http_body::Body for Chunks {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        match self.chunks.pop_front() {
            Some(chunk) => Poll::Ready(Some(Ok(Frame::data(chunk)))),
            None if self.open => Poll::Pending,
            None => Poll::Ready(None),
        }
    }
}

const BOUNDARY: &str = "----WebKitFormBoundaryzv0Og5zWtGjvzP2A";

enum Field<'a> {
    Text(&'a str, &'a str),
    File(&'a str, &'a str, &'a [u8]),
}

fn message(fields: &[Field<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for field in fields {
        out.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        match field {
            Field::Text(name, value) => out.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}").as_bytes()),
            Field::File(name, filename, content) => {
                out.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
                out.extend_from_slice(content);
            }
        }
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    out
}

fn bytes(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 31 % 251) as u8).collect()
}

fn stream(body: Chunks, options: MultipartOptions) -> MultipartStream<Chunks> {
    parse_multipart_stream(body, BOUNDARY, &options).unwrap()
}

/// Every part's name and content, or the error that ended the message.
fn read_all(mut parts: MultipartStream<Chunks>) -> Result<Vec<(Option<String>, Bytes)>, MultipartError> {
    block_on(async move {
        let mut out = Vec::new();
        while let Some(part) = parts.next_part().await {
            let part = part?;
            let name = part.name();
            out.push((name, part.bytes().await?));
        }
        Ok(out)
    })
}

fn poll_once<F: Future>(future: F) -> Poll<F::Output> {
    let mut future = pin!(future);
    future.as_mut().poll(&mut Context::from_waker(Waker::noop()))
}

#[test]
fn rejects_boundaries_longer_than_70_characters() {
    assert!(parse_multipart_stream(Chunks::new([b""]), &"a".repeat(70), &MultipartOptions::default()).is_ok());
    assert!(matches!(parse_multipart_stream(Chunks::new([b""]), &"a".repeat(71), &MultipartOptions::default()), Err(MultipartError::Parse(_))));
}

#[test]
fn parses_content_correctly_at_any_chunk_size() {
    let file = bytes(3000);
    let message = message(&[Field::Text("title", "Dune"), Field::File("cover", "dune.bin", &file), Field::Text("empty", ""), Field::Text("crlf", "a\r\n--b\r\n")]);
    let expected: Vec<_> = parse_multipart(&message, BOUNDARY, &MultipartOptions::default()).unwrap().into_iter().map(|p| (p.name(), p.bytes())).collect();
    assert_eq!(expected.len(), 4);
    for size in [1, 2, 3, 5, 7, 13, 41, 64, 500, 4096, message.len()] {
        assert_eq!(read_all(stream(Chunks::split(&message, size), MultipartOptions::default())).unwrap(), expected, "chunk size {size}");
    }
}

#[test]
fn yields_a_part_when_its_headers_arrive_before_the_rest_of_its_content() {
    // Longer than the boundary, so none of it is held back as a possible delimiter.
    let first = "first ".repeat(20);
    let head = format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"big\"; filename=\"big.bin\"\r\n\r\n{first}");
    let mut parts = stream(Chunks { chunks: VecDeque::from([Bytes::from(head)]), open: true }, MultipartOptions::default());
    let Poll::Ready(Some(Ok(mut part))) = poll_once(parts.next_part()) else { panic!("the part was not yielded from its headers") };
    assert_eq!(part.filename().as_deref(), Some("big.bin"));
    let Poll::Ready(Some(Ok(chunk))) = poll_once(part.chunk()) else { panic!("the first content was not readable") };
    assert_eq!(chunk, first.as_bytes());
    assert!(poll_once(part.chunk()).is_pending(), "the rest has not arrived");
}

#[test]
fn shares_the_body_chunks_instead_of_copying_content() {
    let file = bytes(100_000);
    let message = message(&[Field::File("f", "f.bin", &file)]);
    let body = Bytes::from(message);
    let range = body.as_ptr() as usize..body.as_ptr() as usize + body.len();
    let mut parts = parse_multipart_stream(rmx_fetch::Body::from(body.clone()), BOUNDARY, &MultipartOptions::default()).unwrap();
    block_on(async {
        let mut part = parts.next_part().await.unwrap().unwrap();
        let chunk = part.chunk().await.unwrap().unwrap();
        assert_eq!(chunk.len(), file.len());
        assert!(range.contains(&(chunk.as_ptr() as usize)), "the content is a view of the body chunk");
    });
}

#[test]
fn discards_the_content_of_parts_that_are_not_read() {
    let message = message(&[Field::File("skip", "a.bin", &bytes(5000)), Field::Text("keep", "kept"), Field::File("partial", "b.bin", &bytes(5000)), Field::Text("last", "end")]);
    let mut parts = stream(Chunks::split(&message, 100), MultipartOptions::default());
    let texts = block_on(async {
        let mut texts = Vec::new();
        while let Some(part) = parts.next_part().await {
            let mut part = part.unwrap();
            match part.name().as_deref() {
                Some("keep" | "last") => texts.push(part.text().await.unwrap()),
                Some("partial") => assert!(!part.chunk().await.unwrap().unwrap().is_empty()),
                _ => {}
            }
        }
        texts
    });
    assert_eq!(texts, ["kept", "end"]);
}

#[test]
fn throws_when_the_number_of_parts_exceeds_max_parts_as_soon_as_the_extra_part_begins() {
    let message = message(&[Field::Text("a", "a"), Field::Text("b", "b"), Field::Text("c", "c")]);
    let mut parts = stream(Chunks::split(&message, 1000), MultipartOptions { max_parts: 2, ..Default::default() });
    let (names, error) = block_on(async {
        let mut names = Vec::new();
        loop {
            match parts.next_part().await {
                Some(Ok(part)) => names.push(part.name().unwrap()),
                Some(Err(e)) => return (names, Some(e)),
                None => return (names, None),
            }
        }
    });
    assert_eq!(names, ["a", "b"]);
    assert_eq!(error, Some(MultipartError::MaxPartsExceeded(2)));
}

#[test]
fn reads_parts_scanned_before_a_limit_error_in_the_same_chunk() {
    let message = message(&[Field::Text("first", "small"), Field::File("second", "b.bin", &bytes(10_000))]);
    let mut parts = stream(Chunks::new([&message]), MultipartOptions { max_file_size: 5000, ..Default::default() });
    block_on(async {
        let first = parts.next_part().await.unwrap().unwrap();
        assert_eq!(first.text().await.unwrap(), "small");
        let second = parts.next_part().await.unwrap().unwrap();
        assert_eq!(second.name().as_deref(), Some("second"));
        assert_eq!(second.bytes().await, Err(MultipartError::MaxFileSizeExceeded(5000)));
        assert_eq!(parts.next_part().await.map(|p| p.map(|_| ())), Some(Err(MultipartError::MaxFileSizeExceeded(5000))), "the error ends the message");
    });
}

#[test]
fn errors_when_the_message_ends_inside_a_part() {
    let message = message(&[Field::File("f", "f.bin", &bytes(1000))]);
    let result = read_all(stream(Chunks::new([&message[..600]]), MultipartOptions::default()));
    assert_eq!(result, Err(MultipartError::Parse("Multipart stream not finished".into())));
}

#[test]
fn rejects_an_invalid_closing_suffix_after_reading_the_final_part() {
    let mut message = message(&[Field::Text("a", "1")]);
    message.truncate(message.len() - 2);
    message.extend_from_slice(b"xx");
    for size in [1, 3, message.len()] {
        let mut parts = stream(Chunks::split(&message, size), MultipartOptions::default());
        block_on(async {
            assert_eq!(parts.next_part().await.unwrap().unwrap().text().await.unwrap(), "1");
            assert_eq!(parts.next_part().await.map(|p| p.map(|_| ())), Some(Err(MultipartError::Parse("Invalid multipart boundary ending".into()))), "chunk size {size}");
        });
    }
}

#[test]
fn yields_no_parts_for_an_empty_multipart_message() {
    assert_eq!(read_all(stream(Chunks::new([message(&[])]), MultipartOptions::default())).unwrap(), []);
}

#[test]
fn parses_a_request() {
    let body = message(&[Field::Text("title", "Dune")]);
    let request = http::Request::post("/").header("content-type", format!("multipart/form-data; boundary={BOUNDARY}")).body(rmx_fetch::Body::from(body)).unwrap();
    let mut parts = parse_multipart_request(request, &MultipartOptions::default()).unwrap();
    let text = block_on(async { parts.next_part().await.unwrap().unwrap().text().await.unwrap() });
    assert_eq!(text, "Dune");
    let not_multipart = http::Request::post("/").header("content-type", "text/plain").body(rmx_fetch::Body::from("x")).unwrap();
    assert_eq!(parse_multipart_request(not_multipart, &MultipartOptions::default()).err(), Some(MultipartError::Parse("Request is not a multipart request".into())));
}
