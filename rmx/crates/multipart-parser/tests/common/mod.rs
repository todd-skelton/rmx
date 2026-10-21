//! Helpers shared by the ports of Remix's multipart-parser tests (`test/utils.ts`).

#![allow(dead_code)]

use rmx_fetch::{block_on, Body};
use rmx_multipart_parser::{MultipartError, MultipartPart, MultipartStream};

pub const CRLF: &str = "\r\n";

pub enum Value<'a> {
    Text(&'a str),
    File { filename: Option<&'a str>, media_type: Option<&'a str>, content: &'a [u8] },
}

pub fn file<'a>(filename: &'a str, media_type: &'a str, content: &'a [u8]) -> Value<'a> {
    Value::File { filename: Some(filename), media_type: Some(media_type), content }
}

/// Remix's `createMultipartMessage`.
pub fn message(boundary: &str, parts: &[(&str, Value)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in parts {
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match value {
            Value::Text(text) => {
                out.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{text}\r\n").as_bytes());
            }
            Value::File { filename, media_type, content } => {
                let mut disposition = format!("Content-Disposition: form-data; name=\"{name}\"");
                if let Some(f) = filename {
                    disposition.push_str(&format!("; filename=\"{f}\""));
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

/// Remix's `getRandomBytes`, deterministic (xorshift) so failures reproduce.
pub fn random_bytes(size: usize) -> Vec<u8> {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15 ^ size as u64;
    (0..size)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

/// A body that arrives in `chunk_size` pieces, like Remix's `createChunkedStream`.
pub fn chunked_body(body: Vec<u8>, chunk_size: usize) -> Body {
    Body::from_task(move |tx| async move {
        for chunk in body.chunks(chunk_size) {
            tx.send(chunk.to_vec());
        }
    })
}

pub fn request(content_type: &str, body: Body) -> http::Request<Body> {
    http::Request::post("https://example.com").header("content-type", content_type).body(body).unwrap()
}

/// Every part the stream yields before it ends or fails.
pub fn collect(mut stream: MultipartStream<Body>) -> (Vec<MultipartPart>, Option<MultipartError>) {
    block_on(async move {
        let mut parts = Vec::new();
        while let Some(part) = stream.next_part().await {
            match part {
                Ok(part) => parts.push(part),
                Err(e) => return (parts, Some(e)),
            }
        }
        (parts, None)
    })
}

pub fn names_and_text(parts: &[MultipartPart]) -> Vec<(String, String)> {
    parts.iter().map(|p| (p.name().unwrap_or_default(), p.text().unwrap())).collect()
}

pub fn invalid_ending() -> MultipartError {
    MultipartError::Parse("Invalid multipart boundary ending".into())
}

/// The first index of `needle` in `hay` at or after `start`.
pub fn index_of(hay: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    hay.windows(needle.len()).skip(start).position(|w| w == needle).map(|i| i + start)
}
