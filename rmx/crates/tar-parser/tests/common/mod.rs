//! Helpers shared by the ports of Remix's tar-parser tests.

#![allow(dead_code)]

use std::io::Read;

use rmx_tar_parser::*;

/// A fixture, gunzipped when it is a `.tgz` or `.tar.gz` (Remix's `readFixture`).
pub fn fixture(name: &str) -> Vec<u8> {
    let bytes = std::fs::read(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    if name.ends_with(".tgz") || name.ends_with(".tar.gz") {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&bytes[..]).read_to_end(&mut out).unwrap();
        out
    } else {
        bytes
    }
}

/// Remix's `computeChecksum` written into the checksum field as `"%06o\0 "`.
pub fn set_checksum(block: &mut [u8]) {
    let sum: u32 = 8 * 32 + block[..148].iter().chain(&block[156..]).map(|b| *b as u32).sum::<u32>();
    block[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
}

/// The size field of a header: octal digits padded to 11, or raw bytes.
#[derive(Clone)]
pub enum Size {
    Octal(u64),
    Text(String),
    Raw(Vec<u8>),
}

impl From<u64> for Size {
    fn from(size: u64) -> Size {
        Size::Octal(size)
    }
}

impl From<&str> for Size {
    fn from(text: &str) -> Size {
        Size::Text(text.into())
    }
}

/// Remix's test `createHeader`: a ustar header block with only the given fields set.
#[derive(Clone)]
pub struct Header {
    pub name: String,
    pub prefix: String,
    pub kind: char,
    pub size: Size,
    pub linkname: String,
}

pub fn header(name: &str) -> Header {
    Header { name: name.into(), prefix: String::new(), kind: '0', size: Size::Octal(0), linkname: String::new() }
}

/// `createHeader(size, type)` from `tar-size.test.ts`.
pub fn sized(size: impl Into<Size>, kind: char) -> Vec<u8> {
    header("entry.txt").size(size).kind(kind).bytes()
}

impl Header {
    pub fn prefix(mut self, prefix: &str) -> Header {
        self.prefix = prefix.into();
        self
    }

    pub fn kind(mut self, kind: char) -> Header {
        self.kind = kind;
        self
    }

    pub fn size(mut self, size: impl Into<Size>) -> Header {
        self.size = size.into();
        self
    }

    pub fn link(mut self, target: &str) -> Header {
        self.linkname = target.into();
        self
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut block = vec![0u8; 512];
        let put = |block: &mut Vec<u8>, at: usize, bytes: &[u8]| block[at..at + bytes.len()].copy_from_slice(bytes);
        put(&mut block, 0, self.name.as_bytes());
        let size = match &self.size {
            Size::Octal(n) => format!("{n:011o}").into_bytes(),
            Size::Text(text) => text.clone().into_bytes(),
            Size::Raw(bytes) => bytes.clone(),
        };
        put(&mut block, 124, &size);
        put(&mut block, 156, self.kind.to_string().as_bytes());
        put(&mut block, 157, self.linkname.as_bytes());
        put(&mut block, 257, b"ustar\x0000");
        put(&mut block, 345, self.prefix.as_bytes());
        set_checksum(&mut block);
        block
    }
}

/// A 12-byte base-256 size field.
pub fn base256(mut size: u128) -> Vec<u8> {
    let mut field = vec![0u8; 12];
    field[0] = 0x80;
    for i in (1..12).rev() {
        field[i] = (size & 255) as u8;
        size >>= 8;
    }
    field
}

/// A metadata entry (`L`, `K`, `x` or `g`): header, body and padding.
pub fn metadata(kind: char, value: &[u8]) -> Vec<Vec<u8>> {
    let header = header("metadata").kind(kind).size(value.len() as u64).bytes();
    vec![header, value.to_vec(), vec![0; (512 - value.len() % 512) % 512]]
}

/// One PAX record, `"<len> <key>=<value>\n"`.
pub fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let record = format!(" {key}={value}\n");
    let mut length = record.len() + 1;
    while length.to_string().len() + record.len() != length {
        length = length.to_string().len() + record.len();
    }
    format!("{length}{record}").into_bytes()
}

pub fn pax(key: &str, value: &str, kind: char) -> Vec<Vec<u8>> {
    metadata(kind, &pax_record(key, value))
}

pub fn pax_path(value: &str, kind: char) -> Vec<Vec<u8>> {
    pax("path", value, kind)
}

/// Every complete entry, or the error that stopped the parse with the entries before it.
pub fn parse_all(chunks: &[Vec<u8>], options: &TarOptions) -> (Vec<TarEntry>, Option<TarError>) {
    let mut entries = Vec::new();
    let result = parse_tar_chunks(chunks, options, |entry| entries.push(entry));
    (entries, result.err())
}

pub fn entries(chunks: &[Vec<u8>], options: &TarOptions) -> Vec<TarEntry> {
    let (entries, error) = parse_all(chunks, options);
    assert_eq!(error, None);
    entries
}

pub fn error(chunks: &[Vec<u8>], options: &TarOptions) -> TarError {
    parse_all(chunks, options).1.expect("expected an error")
}

pub fn invalid(message: &str) -> TarError {
    TarError::Parse(message.into())
}

/// What the streaming API reported, in order.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Entry(String),
    Data(Vec<u8>),
    End,
}

/// Write chunks to a `TarParser` and record its events, stopping at the first error.
pub fn events(chunks: &[Vec<u8>], options: &TarOptions) -> (Vec<Event>, Result<(), TarError>) {
    let mut parser = TarParser::new(options.clone());
    let mut events = Vec::new();
    let mut record = |event: TarEvent<'_>| {
        events.push(match event {
            TarEvent::Entry(header) => Event::Entry(header.name.clone()),
            TarEvent::Data(bytes) => Event::Data(bytes.to_vec()),
            TarEvent::End => Event::End,
        })
    };
    for chunk in chunks {
        if let Err(error) = parser.write(chunk, &mut record) {
            return (events, Err(error));
        }
    }
    let result = parser.finish();
    (events, result)
}

pub fn concat(parts: Vec<Vec<Vec<u8>>>) -> Vec<Vec<u8>> {
    parts.into_iter().flatten().collect()
}
