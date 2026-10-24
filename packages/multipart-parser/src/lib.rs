//! A streaming `multipart/*` parser, ported from `remix/multipart-parser`.
//!
//! [MultipartParser] is push-based: [MultipartParser::write] takes chunks as they arrive and
//! returns every part completed so far, so the whole body is never held at once (each part
//! is). [parse_multipart] parses a message already in memory, [parse_multipart_stream] reads
//! any `http_body::Body`, and [parse_multipart_request] checks a request's `Content-Type`
//! first.
//!
//! Like Remix, the parser skips an RFC 2046 preamble and epilogue, allows padding after a
//! boundary, and enforces limits on header size (8 KiB), part size (2 MiB), part count
//! (1000) and total content (20 parts' worth plus 1 MiB) by default.
//!
//! ```
//! use rmx_multipart_parser::{parse_multipart, MultipartOptions};
//!
//! let body = b"--b\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nDune\r\n--b--";
//! let parts = parse_multipart(body, "b", &MultipartOptions::default()).unwrap();
//! assert_eq!(parts[0].name().as_deref(), Some("title"));
//! assert_eq!(parts[0].text().unwrap(), "Dune");
//! ```

use std::collections::VecDeque;
use std::fmt;
use std::future::poll_fn;
use std::pin::Pin;

use bytes::Bytes;
use rmx_headers::{ContentDisposition, ContentType};

/// Why a parse failed. The messages match Remix's error messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MultipartError {
    /// A malformed message (`MultipartParseError`).
    Parse(String),
    MaxHeaderSizeExceeded(usize),
    MaxFileSizeExceeded(usize),
    MaxPartsExceeded(usize),
    MaxTotalSizeExceeded(usize),
    /// The body stream failed.
    Read(String),
}

impl MultipartError {
    fn parse(message: &str) -> MultipartError {
        MultipartError::Parse(message.to_string())
    }

    /// Whether a configured limit was exceeded (as opposed to a malformed message).
    pub fn is_limit(&self) -> bool {
        matches!(
            self,
            MultipartError::MaxHeaderSizeExceeded(_) | MultipartError::MaxFileSizeExceeded(_) | MultipartError::MaxPartsExceeded(_) | MultipartError::MaxTotalSizeExceeded(_)
        )
    }
}

impl fmt::Display for MultipartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MultipartError::Parse(message) | MultipartError::Read(message) => f.write_str(message),
            MultipartError::MaxHeaderSizeExceeded(max) => write!(f, "Multipart header size exceeds maximum allowed size of {max} bytes"),
            MultipartError::MaxFileSizeExceeded(max) => write!(f, "File size exceeds maximum allowed size of {max} bytes"),
            MultipartError::MaxPartsExceeded(max) => write!(f, "Multipart part count exceeds maximum allowed count of {max}"),
            MultipartError::MaxTotalSizeExceeded(max) => write!(f, "Multipart content size exceeds maximum allowed size of {max} bytes"),
        }
    }
}

impl std::error::Error for MultipartError {}

const KB: usize = 1024;
const MB: usize = 1024 * KB;
/// RFC 2046 limits boundaries to 70 characters.
const MAX_BOUNDARY_LENGTH: usize = 70;

/// Parser limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultipartOptions {
    /// Largest header block per part. Default 8 KiB.
    pub max_header_size: usize,
    /// Largest content per part (text fields included). Default 2 MiB.
    pub max_file_size: usize,
    /// Most parts per message. Default 1000.
    pub max_parts: usize,
    /// Most content across all parts. Default `max_file_size * 20 + 1 MiB`.
    pub max_total_size: Option<usize>,
}

impl Default for MultipartOptions {
    fn default() -> Self {
        MultipartOptions { max_header_size: 8 * KB, max_file_size: 2 * MB, max_parts: 1000, max_total_size: None }
    }
}

impl MultipartOptions {
    /// The effective total limit.
    pub fn total_size(&self) -> usize {
        self.max_total_size.unwrap_or_else(|| self.max_file_size.saturating_mul(20).saturating_add(MB))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Start,
    AfterBoundary,
    BoundaryPadding,
    Header,
    Body,
    ClosingBoundary,
    Epilogue,
    Done,
}

enum Carry {
    None,
    Partial(usize),
    Full(usize),
}

/// What [MultipartParser::scan] reports, in order: `Start`, zero or more `Data`, `End`.
pub(crate) enum Event<'a> {
    /// A part's raw header block.
    Start(Bytes),
    Data(&'a [u8]),
    End,
}

/// A push-based multipart parser.
pub struct MultipartParser {
    boundary: String,
    options: MultipartOptions,
    max_total_size: usize,
    /// `--boundary`
    opening: Vec<u8>,
    /// `\r\n--boundary`
    delimiter: Vec<u8>,
    state: State,
    at_start_of_stream: bool,
    buffer: Option<Vec<u8>>,
    /// A part has started and has not ended yet.
    in_part: bool,
    /// The part [MultipartParser::write] is collecting.
    part: Option<MultipartPart>,
    content_length: usize,
    part_count: usize,
    total_content_length: usize,
}

impl MultipartParser {
    /// A parser for `boundary`; boundaries over 70 characters are rejected.
    pub fn new(boundary: &str, options: MultipartOptions) -> Result<MultipartParser, MultipartError> {
        if boundary.len() > MAX_BOUNDARY_LENGTH {
            return Err(MultipartError::Parse(format!("Multipart boundary exceeds maximum length of {MAX_BOUNDARY_LENGTH} characters")));
        }
        Ok(MultipartParser {
            boundary: boundary.to_string(),
            options,
            max_total_size: options.total_size(),
            opening: format!("--{boundary}").into_bytes(),
            delimiter: format!("\r\n--{boundary}").into_bytes(),
            state: State::Start,
            at_start_of_stream: true,
            buffer: None,
            in_part: false,
            part: None,
            content_length: 0,
            part_count: 0,
            total_content_length: 0,
        })
    }

    pub fn boundary(&self) -> &str {
        &self.boundary
    }

    pub fn options(&self) -> &MultipartOptions {
        &self.options
    }

    /// Feed the next chunk and return the parts it completed.
    pub fn write(&mut self, input: &[u8]) -> Result<Vec<MultipartPart>, MultipartError> {
        let mut parts = Vec::new();
        let mut current = self.part.take();
        self.scan(input, &mut |event| match event {
            Event::Start(header) => current = Some(MultipartPart { header, content: Vec::new() }),
            Event::Data(data) => current.as_mut().expect("data follows a part start").content.push(Bytes::copy_from_slice(data)),
            Event::End => parts.push(current.take().expect("a part end follows its start")),
        })?;
        self.part = current;
        Ok(parts)
    }

    /// Feed the next chunk, reporting each part as soon as its headers arrive, then its
    /// content as it is found, then its end. Limits are checked before an event is reported.
    pub(crate) fn scan(&mut self, input: &[u8], sink: &mut dyn FnMut(Event<'_>)) -> Result<(), MultipartError> {
        if self.state == State::Done {
            return Err(MultipartError::parse("Unexpected data after end of stream"));
        }
        if self.state == State::Epilogue {
            return Ok(());
        }
        let delimiter_len = self.delimiter.len();
        let joined: Vec<u8>;
        let mut chunk: &[u8] = input;
        let mut index = 0;

        if let Some(carry) = self.buffer.take() {
            if self.state == State::Body {
                match self.analyze_carry(&carry, chunk) {
                    Carry::None => self.append(&carry, sink)?,
                    Carry::Partial(start) => {
                        self.append(&carry[..start], sink)?;
                        let mut tail = carry[start..].to_vec();
                        tail.extend_from_slice(chunk);
                        self.buffer = Some(tail);
                        return Ok(());
                    }
                    Carry::Full(start) => {
                        self.append(&carry[..start], sink)?;
                        self.state = State::AfterBoundary;
                        index = delimiter_len - (carry.len() - start);
                    }
                }
            } else {
                let mut all = carry;
                all.extend_from_slice(chunk);
                joined = all;
                chunk = &joined;
            }
        }

        loop {
            if self.state == State::Body {
                if chunk.len() - index < delimiter_len {
                    self.buffer = Some(chunk[index..].to_vec());
                    break;
                }
                let Some(found) = find(chunk, &self.delimiter, index) else {
                    // No boundary, but one may start at the end of the chunk.
                    match self.partial_tail(chunk) {
                        None => self.append(&chunk[index..], sink)?,
                        Some(tail) => {
                            if tail > index {
                                self.append(&chunk[index..tail], sink)?;
                            }
                            self.buffer = Some(chunk[tail..].to_vec());
                        }
                    }
                    break;
                };
                if found > index {
                    self.append(&chunk[index..found], sink)?;
                }
                index = found + delimiter_len;
                self.state = State::AfterBoundary;
            }

            if self.state == State::AfterBoundary {
                if chunk.len() - index < 2 {
                    self.buffer = Some(chunk[index..].to_vec());
                    break;
                }
                if chunk[index] == b'-' && chunk[index + 1] == b'-' {
                    index += 2;
                    self.state = State::ClosingBoundary;
                    self.end_part(sink);
                } else {
                    self.state = State::BoundaryPadding;
                }
            }

            if self.state == State::BoundaryPadding || self.state == State::ClosingBoundary {
                let closing = self.state == State::ClosingBoundary;
                while index < chunk.len() && (chunk[index] == b' ' || chunk[index] == b'\t') {
                    index += 1;
                }
                if index == chunk.len() || (chunk[index] == b'\r' && chunk.len() - index == 1) {
                    self.buffer = Some(chunk[index..].to_vec());
                    break;
                }
                if chunk[index] != b'\r' || chunk[index + 1] != b'\n' {
                    return Err(MultipartError::parse("Invalid multipart boundary ending"));
                }
                index += 2;
                self.state = if closing { State::Epilogue } else { State::Header };
                if closing {
                    break;
                }
                self.end_part(sink);
            }

            if self.state == State::Header {
                if chunk.len() - index < 4 {
                    self.buffer = Some(chunk[index..].to_vec());
                    break;
                }
                let Some(end) = find(chunk, b"\r\n\r\n", index) else {
                    if chunk.len() - index > self.options.max_header_size {
                        return Err(MultipartError::MaxHeaderSizeExceeded(self.options.max_header_size));
                    }
                    self.buffer = Some(chunk[index..].to_vec());
                    break;
                };
                if end - index > self.options.max_header_size {
                    return Err(MultipartError::MaxHeaderSizeExceeded(self.options.max_header_size));
                }
                self.start_part(Bytes::copy_from_slice(&chunk[index..end]), sink)?;
                index = end + 4;
                self.state = State::Body;
                continue;
            }

            if self.state == State::Start {
                if chunk.len() < self.opening.len() {
                    self.buffer = Some(chunk.to_vec());
                    break;
                }
                if self.at_start_of_stream && chunk.starts_with(&self.opening) {
                    index = self.opening.len();
                    self.state = State::AfterBoundary;
                    continue;
                }
                let Some(found) = find(chunk, &self.delimiter, 0) else {
                    // An RFC 2046 preamble: keep a tail in case the opening boundary is split.
                    let keep = delimiter_len - 1;
                    if chunk.len() <= keep {
                        self.buffer = Some(chunk.to_vec());
                    } else {
                        self.at_start_of_stream = false;
                        self.buffer = Some(chunk[chunk.len() - keep..].to_vec());
                    }
                    break;
                };
                index = found + delimiter_len;
                self.state = State::AfterBoundary;
            }
        }
        Ok(())
    }

    /// Check that the message ended with its closing boundary. Safe to call more than once.
    pub fn finish(&mut self) -> Result<(), MultipartError> {
        if self.state == State::ClosingBoundary {
            if self.buffer.as_ref().is_some_and(|b| !b.is_empty()) {
                return Err(MultipartError::parse("Invalid multipart boundary ending"));
            }
            self.state = State::Done;
        }
        if self.state == State::Epilogue {
            self.state = State::Done;
        }
        if self.state != State::Done {
            return Err(MultipartError::parse("Multipart stream not finished"));
        }
        Ok(())
    }

    fn append(&mut self, data: &[u8], sink: &mut dyn FnMut(Event<'_>)) -> Result<(), MultipartError> {
        if data.is_empty() {
            return Ok(());
        }
        if self.content_length + data.len() > self.options.max_file_size {
            return Err(MultipartError::MaxFileSizeExceeded(self.options.max_file_size));
        }
        if self.total_content_length + data.len() > self.max_total_size {
            return Err(MultipartError::MaxTotalSizeExceeded(self.max_total_size));
        }
        self.content_length += data.len();
        self.total_content_length += data.len();
        if self.in_part {
            sink(Event::Data(data));
        }
        Ok(())
    }

    /// Parts are counted as they begin, so an extra part fails before its content is read.
    fn start_part(&mut self, header: Bytes, sink: &mut dyn FnMut(Event<'_>)) -> Result<(), MultipartError> {
        self.part_count += 1;
        if self.part_count > self.options.max_parts {
            return Err(MultipartError::MaxPartsExceeded(self.options.max_parts));
        }
        self.in_part = true;
        self.content_length = 0;
        sink(Event::Start(header));
        Ok(())
    }

    fn end_part(&mut self, sink: &mut dyn FnMut(Event<'_>)) {
        if std::mem::take(&mut self.in_part) {
            sink(Event::End);
        }
    }

    /// Whether the held-back bytes plus `chunk` start a delimiter, fully or partly.
    fn analyze_carry(&self, carry: &[u8], chunk: &[u8]) -> Carry {
        let total = carry.len() + chunk.len();
        let at = |i: usize| if i < carry.len() { carry[i] } else { chunk[i - carry.len()] };
        for start in 0..carry.len() {
            let available = total - start;
            let compare = self.delimiter.len().min(available);
            if (0..compare).all(|i| at(start + i) == self.delimiter[i]) {
                return if available >= self.delimiter.len() { Carry::Full(start) } else { Carry::Partial(start) };
            }
        }
        Carry::None
    }

    /// The start of the longest delimiter prefix that ends `hay`.
    fn partial_tail(&self, hay: &[u8]) -> Option<usize> {
        let last = *hay.last()?;
        (0..self.delimiter.len())
            .rev()
            .filter(|&j| self.delimiter[j] == last && j < hay.len())
            .find(|&j| hay[hay.len() - 1 - j..] == self.delimiter[..=j])
            .map(|j| hay.len() - 1 - j)
    }
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= hay.len() || hay.len() - from < needle.len() {
        return None;
    }
    memchr::memmem::find(&hay[from..], needle).map(|i| i + from)
}

/// One part of a multipart message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultipartPart {
    header: Bytes,
    /// The content, in the chunks it arrived in.
    pub content: Vec<Bytes>,
}

/// A part's headers: lowercase names, trimmed values, repeats joined with `, `.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultipartHeaders(Vec<(String, String)>);

impl MultipartHeaders {
    pub fn get(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.0.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

impl MultipartPart {
    /// The raw header block (without the blank line).
    pub fn raw_headers(&self) -> &[u8] {
        &self.header
    }

    /// The parsed headers (decoded as UTF-8, invalid bytes replaced).
    pub fn headers(&self) -> MultipartHeaders {
        MultipartHeaders::parse(&self.header)
    }

    /// The `name` from `Content-Disposition`.
    pub fn name(&self) -> Option<String> {
        self.headers().name()
    }

    /// The file name, preferring a decoded `filename*` over `filename`.
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

    pub fn is_text(&self) -> bool {
        !self.is_file()
    }

    pub fn size(&self) -> usize {
        self.content.iter().map(Bytes::len).sum()
    }

    /// The content in one buffer.
    pub fn bytes(&self) -> Bytes {
        match self.content.as_slice() {
            [] => Bytes::new(),
            [one] => one.clone(),
            many => Bytes::from(many.concat()),
        }
    }

    /// The content as UTF-8 (an error for invalid UTF-8, like Remix's fatal decoder).
    pub fn text(&self) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(self.bytes().to_vec())
    }
}

impl MultipartHeaders {
    /// Parse a raw header block: lowercase names, trimmed values, repeats joined with `, `.
    pub(crate) fn parse(raw: &[u8]) -> MultipartHeaders {
        let mut headers: Vec<(String, String)> = Vec::new();
        for line in String::from_utf8_lossy(raw).split("\r\n") {
            let Some((name, value)) = line.split_once(':') else { continue };
            if name.is_empty() {
                continue;
            }
            let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
            match headers.iter_mut().find(|(k, _)| *k == name) {
                Some((_, existing)) => {
                    existing.push_str(", ");
                    existing.push_str(value);
                }
                None => headers.push((name, value.to_string())),
            }
        }
        MultipartHeaders(headers)
    }

    fn disposition(&self) -> ContentDisposition {
        ContentDisposition::parse(self.get("content-disposition"))
    }

    /// The `name` from `Content-Disposition`.
    pub fn name(&self) -> Option<String> {
        self.disposition().name
    }

    /// The file name, preferring a decoded `filename*` over `filename`.
    pub fn filename(&self) -> Option<String> {
        self.disposition().preferred_filename()
    }

    /// The media type from `Content-Type`, without parameters.
    pub fn media_type(&self) -> Option<String> {
        ContentType::parse(self.get("content-type")).media_type
    }

    /// A file part: it has a file name or is `application/octet-stream`.
    pub fn is_file(&self) -> bool {
        self.filename().is_some() || self.media_type().as_deref() == Some("application/octet-stream")
    }
}

/// Parse a whole message in memory.
pub fn parse_multipart(message: &[u8], boundary: &str, options: &MultipartOptions) -> Result<Vec<MultipartPart>, MultipartError> {
    parse_multipart_chunks([message], boundary, options)
}

/// Parse a message that arrives in pieces.
pub fn parse_multipart_chunks<'a>(chunks: impl IntoIterator<Item = &'a [u8]>, boundary: &str, options: &MultipartOptions) -> Result<Vec<MultipartPart>, MultipartError> {
    let mut parser = MultipartParser::new(boundary, *options)?;
    let mut parts = Vec::new();
    let mut any = false;
    for chunk in chunks {
        if chunk.is_empty() {
            continue;
        }
        any = true;
        parts.extend(parser.write(chunk)?);
    }
    if !any {
        // Remix returns nothing for an empty message.
        return Ok(parts);
    }
    parser.finish()?;
    Ok(parts)
}

/// Parts read from a body as it streams in. Get them with [MultipartStream::next_part].
pub struct MultipartStream<B> {
    body: B,
    parser: MultipartParser,
    ready: VecDeque<MultipartPart>,
    finished: bool,
}

/// Parse `body` as it arrives.
pub fn parse_multipart_stream<B>(body: B, boundary: &str, options: &MultipartOptions) -> Result<MultipartStream<B>, MultipartError> {
    Ok(MultipartStream { body, parser: MultipartParser::new(boundary, *options)?, ready: VecDeque::new(), finished: false })
}

impl<B> MultipartStream<B>
where
    B: http_body::Body<Data = Bytes> + Unpin,
    B::Error: fmt::Display,
{
    /// The next part, `None` after the last one, or the error that ended the stream.
    pub async fn next_part(&mut self) -> Option<Result<MultipartPart, MultipartError>> {
        loop {
            if let Some(part) = self.ready.pop_front() {
                return Some(Ok(part));
            }
            if self.finished {
                return None;
            }
            let frame = poll_fn(|cx| Pin::new(&mut self.body).poll_frame(cx)).await;
            let result = match frame {
                None => {
                    self.finished = true;
                    self.parser.finish().map(|_| Vec::new())
                }
                Some(Err(e)) => Err(MultipartError::Read(e.to_string())),
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) if !data.is_empty() => self.parser.write(&data),
                    _ => Ok(Vec::new()),
                },
            };
            match result {
                Ok(parts) => self.ready.extend(parts),
                Err(e) => {
                    self.finished = true;
                    return Some(Err(e));
                }
            }
        }
    }
}

/// The `boundary` parameter of a `Content-Type` value.
pub fn get_multipart_boundary(content_type: &str) -> Option<String> {
    ContentType::parse(Some(content_type)).boundary
}

/// Whether a `Content-Type` value is a `multipart/*` type.
pub fn is_multipart(content_type: Option<&str>) -> bool {
    ContentType::parse(content_type).media_type.is_some_and(|m| m.to_ascii_lowercase().starts_with("multipart/"))
}

/// Whether a request carries a `multipart/*` body.
pub fn is_multipart_request<B>(request: &http::Request<B>) -> bool {
    is_multipart(request.headers().get(http::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()))
}

/// Parse a request's multipart body as it streams in.
pub fn parse_multipart_request<B: http_body::Body>(request: http::Request<B>, options: &MultipartOptions) -> Result<MultipartStream<B>, MultipartError> {
    let boundary = request_boundary(&request)?;
    parse_multipart_stream(request.into_body(), &boundary, options)
}

/// The boundary of a multipart request with a body.
fn request_boundary<B: http_body::Body>(request: &http::Request<B>) -> Result<String, MultipartError> {
    if !is_multipart_request(request) {
        return Err(MultipartError::parse("Request is not a multipart request"));
    }
    // Like Remix: a missing body is reported before a missing boundary.
    if request.body().is_end_stream() {
        return Err(MultipartError::parse("Request body is empty"));
    }
    let content_type = request.headers().get(http::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or_default();
    get_multipart_boundary(content_type).ok_or_else(|| MultipartError::parse("Invalid Content-Type header: missing boundary"))
}

pub mod future;

#[cfg(test)]
mod tests;
