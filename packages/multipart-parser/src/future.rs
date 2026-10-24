//! Streaming parts (Remix's `multipart-parser/future`).
//!
//! [MultipartStream::next_part] yields each part as soon as its headers arrive, and the part's
//! content streams from the body as you read it, so a large file is never held in memory.
//! Content you do not read is discarded when you ask for the next part, and a part borrows
//! the stream, so the borrow checker keeps parts in order. Dropping the stream drops (and so
//! cancels) the body. Limits are enforced as in the stable parser, on content you skip too.
//!
//! ```
//! use rmx_multipart_parser::future::parse_multipart_stream;
//! use rmx_multipart_parser::MultipartOptions;
//!
//! # rmx_fetch::block_on(async {
//! let body = rmx_fetch::Body::from("--b\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nDune\r\n--b--");
//! let mut parts = parse_multipart_stream(body, "b", &MultipartOptions::default()).unwrap();
//! while let Some(part) = parts.next_part().await {
//!     let part = part.unwrap();
//!     assert_eq!(part.name().as_deref(), Some("title"));
//!     assert_eq!(part.text().await.unwrap(), "Dune");
//! }
//! # });
//! ```

use std::collections::VecDeque;
use std::fmt;
use std::future::poll_fn;
use std::pin::Pin;

use bytes::Bytes;

use crate::{request_boundary, Event, MultipartParser};
pub use crate::{get_multipart_boundary, is_multipart, is_multipart_request, MultipartError, MultipartHeaders, MultipartOptions};

enum Item {
    Start(Bytes),
    Data(Bytes),
    End,
}

/// A multipart body read part by part. Get parts with [MultipartStream::next_part].
pub struct MultipartStream<B> {
    body: B,
    parser: MultipartParser,
    /// Events scanned from the latest body chunk and not read yet.
    queue: VecDeque<Item>,
    /// The body has ended or failed, or the message was invalid.
    done: bool,
    /// The error that ended the message, reported again on every later read.
    error: Option<MultipartError>,
    /// The latest part's content has not all been read.
    in_part: bool,
}

/// Read `body` as a multipart message with `boundary`; boundaries over 70 characters are
/// rejected.
pub fn parse_multipart_stream<B>(body: B, boundary: &str, options: &MultipartOptions) -> Result<MultipartStream<B>, MultipartError> {
    Ok(MultipartStream { body, parser: MultipartParser::new(boundary, *options)?, queue: VecDeque::new(), done: false, error: None, in_part: false })
}

/// Read a request's multipart body, after checking its `Content-Type` and that it has a body.
pub fn parse_multipart_request<B: http_body::Body>(request: http::Request<B>, options: &MultipartOptions) -> Result<MultipartStream<B>, MultipartError> {
    let boundary = request_boundary(&request)?;
    parse_multipart_stream(request.into_body(), &boundary, options)
}

impl<B> MultipartStream<B>
where
    B: http_body::Body<Data = Bytes> + Unpin,
    B::Error: fmt::Display,
{
    /// The next part, once its headers have arrived: `None` after the last part, or the
    /// error that ended the message (reported again on later calls). Unread content of the
    /// previous part is discarded first.
    pub async fn next_part(&mut self) -> Option<Result<MultipartPart<'_, B>, MultipartError>> {
        loop {
            match self.next_item().await? {
                Err(e) => return Some(Err(e)),
                Ok(Item::Start(header)) => {
                    self.in_part = true;
                    return Some(Ok(MultipartPart { header, stream: self }));
                }
                Ok(Item::Data(_) | Item::End) => {}
            }
        }
    }

    /// The next chunk of the latest part's content, or `None` at its end. This is what
    /// [MultipartPart::chunk] reads; it is public so a part's content can be read after the
    /// part itself has been dropped, as an upload handler that owns the stream does.
    pub async fn next_chunk(&mut self) -> Option<Result<Bytes, MultipartError>> {
        if !self.in_part {
            return None;
        }
        match self.next_item().await {
            Some(Ok(Item::Data(bytes))) => Some(Ok(bytes)),
            Some(Err(e)) => {
                self.in_part = false;
                Some(Err(e))
            }
            Some(Ok(Item::Start(header))) => {
                // Not reached: the parser ends a part before it starts the next one.
                self.queue.push_front(Item::Start(header));
                self.in_part = false;
                None
            }
            Some(Ok(Item::End)) | None => {
                self.in_part = false;
                None
            }
        }
    }

    async fn next_item(&mut self) -> Option<Result<Item, MultipartError>> {
        loop {
            if let Some(item) = self.queue.pop_front() {
                return Some(Ok(item));
            }
            if let Some(e) = &self.error {
                return Some(Err(e.clone()));
            }
            if self.done {
                return None;
            }
            let result = match poll_fn(|cx| Pin::new(&mut self.body).poll_frame(cx)).await {
                None => {
                    self.done = true;
                    self.parser.finish()
                }
                Some(Err(e)) => Err(MultipartError::Read(e.to_string())),
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) if !data.is_empty() => {
                        let queue = &mut self.queue;
                        self.parser.scan(&data, &mut |event| {
                            queue.push_back(match event {
                                Event::Start(header) => Item::Start(header),
                                Event::Data(bytes) => Item::Data(share(&data, bytes)),
                                Event::End => Item::End,
                            })
                        })
                    }
                    _ => Ok(()),
                },
            };
            // Events scanned before an error are still delivered, then the error.
            if let Err(e) = result {
                self.done = true;
                self.error = Some(e);
            }
        }
    }
}

/// `part` as a view of `chunk` when it lies inside it (no copy), else a copy (content
/// held back across chunks while the parser looked for a boundary).
fn share(chunk: &Bytes, part: &[u8]) -> Bytes {
    let (base, start) = (chunk.as_ptr() as usize, part.as_ptr() as usize);
    if start >= base && start + part.len() <= base + chunk.len() {
        chunk.slice_ref(part)
    } else {
        Bytes::copy_from_slice(part)
    }
}

/// A part whose headers have arrived. Its content streams from the body: read it with
/// [MultipartPart::chunk], [MultipartPart::bytes] or [MultipartPart::text].
pub struct MultipartPart<'a, B> {
    header: Bytes,
    stream: &'a mut MultipartStream<B>,
}

impl<B> fmt::Debug for MultipartPart<'_, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultipartPart").field("headers", &self.headers()).finish_non_exhaustive()
    }
}

impl<B> MultipartPart<'_, B> {
    /// The raw header block (without the blank line).
    pub fn raw_headers(&self) -> &[u8] {
        &self.header
    }

    /// The parsed headers (decoded as UTF-8, invalid bytes replaced).
    pub fn headers(&self) -> MultipartHeaders {
        MultipartHeaders::parse(&self.header)
    }

    /// The `name` from `Content-Disposition`, usually the form field's name.
    pub fn name(&self) -> Option<String> {
        self.headers().name()
    }

    /// The file name, preferring a decoded `filename*` over `filename`. Untrusted: never use
    /// it as a filesystem path.
    pub fn filename(&self) -> Option<String> {
        self.headers().filename()
    }

    /// The media type from `Content-Type`, without parameters.
    pub fn media_type(&self) -> Option<String> {
        self.headers().media_type()
    }

    /// A file part: it has a file name or is `application/octet-stream`.
    pub fn is_file(&self) -> bool {
        self.headers().is_file()
    }
}

impl<B> MultipartPart<'_, B>
where
    B: http_body::Body<Data = Bytes> + Unpin,
    B::Error: fmt::Display,
{
    /// The next chunk of content, or `None` at the end of the part.
    pub async fn chunk(&mut self) -> Option<Result<Bytes, MultipartError>> {
        self.stream.next_chunk().await
    }

    /// The rest of the content in one buffer.
    pub async fn bytes(mut self) -> Result<Bytes, MultipartError> {
        let first = match self.chunk().await {
            None => return Ok(Bytes::new()),
            Some(chunk) => chunk?,
        };
        let Some(second) = self.chunk().await else { return Ok(first) };
        let mut all = Vec::with_capacity(first.len() * 2);
        all.extend_from_slice(&first);
        all.extend_from_slice(&second?);
        while let Some(chunk) = self.chunk().await {
            all.extend_from_slice(&chunk?);
        }
        Ok(all.into())
    }

    /// The rest of the content as UTF-8; invalid UTF-8 is a [MultipartError::Parse], like
    /// Remix's fatal decoder.
    pub async fn text(self) -> Result<String, MultipartError> {
        let bytes = self.bytes().await?;
        String::from_utf8(bytes.into()).map_err(|e| MultipartError::Parse(format!("Invalid UTF-8 in multipart part: {e}")))
    }
}
