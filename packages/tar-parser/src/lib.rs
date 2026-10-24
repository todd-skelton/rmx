//! A streaming tar parser, ported from `remix/tar-parser`.
//!
//! [TarParser] is push-based: feed it chunks as they arrive and it reports each entry's
//! header, body chunks and end through [TarEvent]s, so bodies are never buffered whole.
//! [parse_tar] is the convenience form for an archive already in memory.
//!
//! By default entry names and link targets must be relative paths that stay inside the
//! archive ([PathPolicy::Relative]), and archives are limited to 5000 entries, 2 MiB per
//! entry and 20 MiB in total. Decompression (for `.tgz`) is the caller's job.
//!
//! ```
//! # let archive = rsmix_tar_parser::tests_support::one_file_archive();
//! let mut names = Vec::new();
//! rsmix_tar_parser::parse_tar(&archive, &Default::default(), |entry| names.push(entry.header.name)).unwrap();
//! assert_eq!(names, ["hello.txt"]);
//! ```

mod header;

use std::collections::BTreeMap;
use std::fmt;

pub use header::{parse_tar_header, TarEntryType, TarHeader};

const BLOCK: usize = 512;

/// Why a parse failed. The messages match Remix's error messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TarError {
    /// A malformed archive (`TarParseError`).
    Parse(String),
    MaxEntrySizeExceeded(u64),
    MaxTotalSizeExceeded(u64),
    MaxEntriesExceeded(u64),
}

impl TarError {
    pub(crate) fn parse(message: &str) -> TarError {
        TarError::Parse(message.to_string())
    }
}

impl fmt::Display for TarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TarError::Parse(message) => f.write_str(message),
            TarError::MaxEntrySizeExceeded(max) => write!(f, "Tar entry size exceeds maximum allowed size of {max} bytes"),
            TarError::MaxTotalSizeExceeded(max) => write!(f, "Tar archive size exceeds maximum allowed size of {max} bytes"),
            TarError::MaxEntriesExceeded(max) => write!(f, "Tar entry count exceeds maximum allowed count of {max}"),
        }
    }
}

impl std::error::Error for TarError {}

/// How entry names and link targets are checked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PathPolicy {
    /// Reject empty, absolute, drive-prefixed, backslashed and NUL-containing paths,
    /// names with `..` components, and link targets that climb above the archive root.
    #[default]
    Relative,
    /// Keep paths as written. Header structure and limits are still checked.
    Preserve,
}

/// The encoding of names in header fields. PAX and GNU long names are always UTF-8.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilenameEncoding {
    #[default]
    Utf8,
    Latin1,
}

/// Parser options. `None` disables a limit (Remix's `Infinity`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TarOptions {
    /// Accept headers that are neither ustar nor GNU. Default `true`.
    pub allow_unknown_format: bool,
    pub filename_encoding: FilenameEncoding,
    pub path_policy: PathPolicy,
    /// Largest entry body, including PAX/GNU metadata entries. Default 2 MiB.
    pub max_entry_size: Option<u64>,
    /// Largest archive, counting every input byte. Default 20 MiB.
    pub max_total_size: Option<u64>,
    /// Most entries, including metadata entries. Default 5000.
    pub max_entries: Option<u64>,
}

impl Default for TarOptions {
    fn default() -> TarOptions {
        TarOptions {
            allow_unknown_format: true,
            filename_encoding: FilenameEncoding::Utf8,
            path_policy: PathPolicy::Relative,
            max_entry_size: Some(2 * 1024 * 1024),
            max_total_size: Some(20 * 1024 * 1024),
            max_entries: Some(5000),
        }
    }
}

/// What [TarParser::write] reports, in order: `Entry`, zero or more `Data`, `End`.
#[derive(Debug)]
pub enum TarEvent<'a> {
    Entry(&'a TarHeader),
    Data(&'a [u8]),
    End,
}

/// A whole entry, as [parse_tar] delivers it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TarEntry {
    pub header: TarHeader,
    pub body: Vec<u8>,
}

impl TarEntry {
    pub fn name(&self) -> &str {
        &self.header.name
    }

    pub fn size(&self) -> u64 {
        self.header.size
    }

    /// The body as UTF-8 text, lossily. Use [TarEntry::body] for binary data.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Parse an in-memory archive, calling `handler` with each entry.
pub fn parse_tar(archive: &[u8], options: &TarOptions, handler: impl FnMut(TarEntry)) -> Result<(), TarError> {
    parse_tar_chunks([archive], options, handler)
}

/// Parse an archive that arrives in pieces, calling `handler` with each entry.
pub fn parse_tar_chunks<C: AsRef<[u8]>>(chunks: impl IntoIterator<Item = C>, options: &TarOptions, handler: impl FnMut(TarEntry)) -> Result<(), TarError> {
    TarParser::new(options.clone()).parse(chunks, handler)
}

/// A push parser. Call [TarParser::write] for each chunk, then [TarParser::finish].
#[derive(Debug)]
pub struct TarParser {
    options: TarOptions,
    /// Bytes of a header block that has not fully arrived.
    pending: Vec<u8>,
    total_size: u64,
    entry_count: u64,
    /// Consecutive zero blocks; two mark the end of the archive and later input is ignored.
    zero_blocks: u8,
    /// Bytes still to come for the current body, metadata entry or padding.
    missing: u64,
    header: Option<TarHeader>,
    in_body: bool,
    /// The current entry is a GNU long name or PAX record, collected in `long_header`.
    in_long_header: bool,
    long_header: Vec<u8>,
    gnu_long_path: Option<String>,
    gnu_long_link_path: Option<String>,
    pax_global: Option<BTreeMap<String, String>>,
    pax: Option<BTreeMap<String, String>>,
}

impl Default for TarParser {
    fn default() -> TarParser {
        TarParser::new(TarOptions::default())
    }
}

impl TarParser {
    pub fn new(options: TarOptions) -> TarParser {
        TarParser {
            options,
            pending: Vec::new(),
            total_size: 0,
            entry_count: 0,
            zero_blocks: 0,
            missing: 0,
            header: None,
            in_body: false,
            in_long_header: false,
            long_header: Vec::new(),
            gnu_long_path: None,
            gnu_long_link_path: None,
            pax_global: None,
            pax: None,
        }
    }

    pub fn options(&self) -> &TarOptions {
        &self.options
    }

    /// Parse a whole archive, calling `handler` with each complete entry. Like Remix's
    /// `TarParser.parse`, each call starts fresh, so one parser can read many archives
    /// (limits are counted per archive, and a failed parse leaves nothing behind).
    pub fn parse<C: AsRef<[u8]>>(&mut self, chunks: impl IntoIterator<Item = C>, mut handler: impl FnMut(TarEntry)) -> Result<(), TarError> {
        *self = TarParser::new(self.options.clone());
        let mut current: Option<TarEntry> = None;
        let mut on_event = |event: TarEvent<'_>| match event {
            // The declared size is attacker-controlled, so bodies grow from received bytes.
            TarEvent::Entry(header) => current = Some(TarEntry { header: header.clone(), body: Vec::new() }),
            TarEvent::Data(bytes) => current.as_mut().expect("data follows an entry").body.extend_from_slice(bytes),
            TarEvent::End => handler(current.take().expect("end follows an entry")),
        };
        for chunk in chunks {
            self.write(chunk.as_ref(), &mut on_event)?;
        }
        self.finish()
    }

    /// Feed the next chunk of the archive.
    pub fn write(&mut self, chunk: &[u8], on_event: &mut dyn FnMut(TarEvent<'_>)) -> Result<(), TarError> {
        if let Some(max) = self.options.max_total_size {
            if chunk.len() as u64 > max - self.total_size {
                return Err(TarError::MaxTotalSizeExceeded(max));
            }
        }
        self.total_size += chunk.len() as u64;
        // Trailing input after the end marker still counts toward `max_total_size`.
        if self.zero_blocks == 2 {
            return Ok(());
        }

        let joined;
        let mut data = if self.pending.is_empty() {
            chunk
        } else {
            let mut pending = std::mem::take(&mut self.pending);
            pending.extend_from_slice(chunk);
            joined = pending;
            &joined[..]
        };

        while !data.is_empty() {
            if self.missing > 0 {
                let n = self.missing.min(data.len() as u64) as usize;
                let (now, rest) = data.split_at(n);
                data = rest;
                self.missing -= n as u64;
                if self.in_body {
                    on_event(TarEvent::Data(now));
                    if self.missing == 0 {
                        self.in_body = false;
                        on_event(TarEvent::End);
                        self.missing = padding(self.current_size());
                    }
                } else if self.in_long_header {
                    self.long_header.extend_from_slice(now);
                    if self.missing == 0 {
                        self.finish_long_header();
                        self.missing = padding(self.current_size());
                    }
                }
                continue;
            }
            if data.len() < BLOCK {
                self.pending = data.to_vec();
                break;
            }
            let (block, rest) = data.split_at(BLOCK);
            data = rest;
            self.parse_header(block, on_event)?;
            if self.zero_blocks == 2 {
                break;
            }
        }
        Ok(())
    }

    /// Check that the archive did not stop part way through an entry.
    pub fn finish(&self) -> Result<(), TarError> {
        if self.missing != 0 {
            return Err(TarError::parse("Unexpected end of archive"));
        }
        Ok(())
    }

    fn current_size(&self) -> u64 {
        self.header.as_ref().map_or(0, |h| h.size)
    }

    fn parse_header(&mut self, block: &[u8], on_event: &mut dyn FnMut(TarEvent<'_>)) -> Result<(), TarError> {
        if block.iter().all(|b| *b == 0) {
            self.header = None;
            self.zero_blocks += 1;
            return Ok(());
        }
        self.zero_blocks = 0;
        self.entry_count += 1;
        if let Some(max) = self.options.max_entries {
            if self.entry_count > max {
                return Err(TarError::MaxEntriesExceeded(max));
            }
        }
        let mut header = header::decode(block, &self.options)?;
        let is_long = header.kind.is_metadata();
        // A pending local PAX header belongs to the next entry that is not itself metadata.
        let pax = if is_long {
            None
        } else {
            match (&self.pax_global, self.pax.take()) {
                (None, local) => local,
                (Some(global), local) => {
                    let mut merged = global.clone();
                    merged.extend(local.into_iter().flatten());
                    Some(merged)
                }
            }
        };
        let pax_size = pax.as_ref().and_then(|p| p.get("size")).filter(|s| !s.is_empty());
        header.size = match pax_size {
            Some(size) => header::parse_decimal_size(size)?,
            None => header::parse_size_field(&block[124..136])?,
        };
        if let Some(max) = self.options.max_entry_size {
            if header.size > max {
                return Err(TarError::MaxEntrySizeExceeded(max));
            }
        }

        if is_long {
            // As in Remix, an empty metadata entry has no effect.
            self.in_long_header = header.size > 0;
            self.long_header.clear();
            self.missing = header.size;
            self.header = Some(header);
            return Ok(());
        }

        if let Some(path) = self.gnu_long_path.take() {
            header.name = path;
        }
        if let Some(path) = self.gnu_long_link_path.take() {
            header.linkname = Some(path);
        }
        if let Some(pax) = pax {
            if let Some(path) = pax.get("path").filter(|p| !p.is_empty()) {
                header.name = path.clone();
            }
            if let Some(path) = pax.get("linkpath").filter(|p| !p.is_empty()) {
                header.linkname = Some(path.clone());
            }
            header.pax = Some(pax);
        }
        header::validate_paths(&header, self.options.path_policy)?;

        let empty = header.size == 0 || header.kind == TarEntryType::Directory;
        on_event(TarEvent::Entry(&header));
        if empty {
            on_event(TarEvent::End);
            self.missing = 0;
        } else {
            self.in_body = true;
            self.missing = header.size;
        }
        self.header = Some(header);
        Ok(())
    }

    fn finish_long_header(&mut self) {
        self.in_long_header = false;
        let buffer = std::mem::take(&mut self.long_header);
        match self.header.as_ref().map(|h| h.kind) {
            Some(TarEntryType::GnuLongPath) => self.gnu_long_path = Some(header::decode_long_path(buffer)),
            Some(TarEntryType::GnuLongLinkPath) => self.gnu_long_link_path = Some(header::decode_long_path(buffer)),
            Some(TarEntryType::PaxGlobalHeader) => self.pax_global.get_or_insert_with(BTreeMap::new).extend(header::decode_pax(&buffer)),
            Some(TarEntryType::PaxHeader) => self.pax = Some(header::decode_pax(&buffer)),
            _ => {}
        }
    }
}

/// Bytes of zero padding after a body of `size` bytes.
fn padding(size: u64) -> u64 {
    let rest = size % BLOCK as u64;
    if rest == 0 { 0 } else { BLOCK as u64 - rest }
}

#[doc(hidden)]
pub mod tests_support {
    /// A one-entry archive for doc examples.
    pub fn one_file_archive() -> Vec<u8> {
        let mut block = [0u8; 512];
        block[..9].copy_from_slice(b"hello.txt");
        block[100..107].copy_from_slice(b"0000644");
        block[124..135].copy_from_slice(b"00000000005");
        block[156] = b'0';
        block[257..263].copy_from_slice(b"ustar\0");
        block[263..265].copy_from_slice(b"00");
        let sum: u32 = block.iter().enumerate().map(|(i, b)| if (148..156).contains(&i) { 32 } else { *b as u32 }).sum();
        block[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        let mut archive = block.to_vec();
        let mut body = [0u8; 512];
        body[..5].copy_from_slice(b"hello");
        archive.extend_from_slice(&body);
        archive.extend_from_slice(&[0u8; 1024]);
        archive
    }
}

#[cfg(test)]
mod tests;
