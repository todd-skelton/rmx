//! Header blocks: field decoding, sizes, PAX records and path checks.

use std::collections::BTreeMap;

use crate::{FilenameEncoding, PathPolicy, TarError, TarOptions, BLOCK};

/// The parsed header of an entry. Numeric fields are `None` when the field is unreadable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TarHeader {
    /// The entry path, after ustar prefixes and GNU/PAX overrides.
    pub name: String,
    pub mode: Option<i64>,
    pub uid: Option<i64>,
    pub gid: Option<i64>,
    /// Body size in bytes.
    pub size: u64,
    pub mtime: Option<i64>,
    /// Remix's `type`.
    pub kind: TarEntryType,
    pub linkname: Option<String>,
    pub uname: String,
    pub gname: String,
    pub devmajor: Option<i64>,
    pub devminor: Option<i64>,
    /// PAX records (global merged with local) attached to this entry.
    pub pax: Option<BTreeMap<String, String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TarEntryType {
    File,
    Link,
    Symlink,
    CharacterDevice,
    BlockDevice,
    Directory,
    Fifo,
    ContiguousFile,
    GnuLongLinkPath,
    GnuLongPath,
    PaxGlobalHeader,
    PaxHeader,
    Unknown,
}

impl TarEntryType {
    fn from_flag(flag: u8) -> TarEntryType {
        // Remix keys its table by `flag - '0'`, so 'K' is 27, 'L' 28, 'N' 30, 'g' 55, 'x' 72.
        match flag {
            0 | b'0' => TarEntryType::File,
            b'1' => TarEntryType::Link,
            b'2' => TarEntryType::Symlink,
            b'3' => TarEntryType::CharacterDevice,
            b'4' => TarEntryType::BlockDevice,
            b'5' => TarEntryType::Directory,
            b'6' => TarEntryType::Fifo,
            b'7' => TarEntryType::ContiguousFile,
            b'K' => TarEntryType::GnuLongLinkPath,
            b'L' | b'N' => TarEntryType::GnuLongPath,
            b'g' => TarEntryType::PaxGlobalHeader,
            b'x' => TarEntryType::PaxHeader,
            _ => TarEntryType::Unknown,
        }
    }

    /// The names Remix uses: `file`, `directory`, `pax-header` and so on.
    pub fn as_str(self) -> &'static str {
        match self {
            TarEntryType::File => "file",
            TarEntryType::Link => "link",
            TarEntryType::Symlink => "symlink",
            TarEntryType::CharacterDevice => "character-device",
            TarEntryType::BlockDevice => "block-device",
            TarEntryType::Directory => "directory",
            TarEntryType::Fifo => "fifo",
            TarEntryType::ContiguousFile => "contiguous-file",
            TarEntryType::GnuLongLinkPath => "gnu-long-link-path",
            TarEntryType::GnuLongPath => "gnu-long-path",
            TarEntryType::PaxGlobalHeader => "pax-global-header",
            TarEntryType::PaxHeader => "pax-header",
            TarEntryType::Unknown => "unknown",
        }
    }

    /// GNU long names and PAX records describe the next entry rather than being one.
    pub fn is_metadata(self) -> bool {
        matches!(self, TarEntryType::GnuLongPath | TarEntryType::GnuLongLinkPath | TarEntryType::PaxGlobalHeader | TarEntryType::PaxHeader)
    }
}

/// Parse one 512-byte header block on its own, applying the path policy.
pub fn parse_tar_header(block: &[u8], options: &TarOptions) -> Result<TarHeader, TarError> {
    let mut header = decode(block, options)?;
    validate_paths(&header, options.path_policy)?;
    header.size = parse_size_field(&block[124..136])?;
    Ok(header)
}

/// Decode a header block; `size` is left at 0 for the caller to fill in.
pub(crate) fn decode(block: &[u8], options: &TarOptions) -> Result<TarHeader, TarError> {
    if block.len() != BLOCK {
        return Err(TarError::parse("Invalid tar header size"));
    }
    if octal(&block[148..156]) != Some(checksum(block)) {
        return Err(TarError::parse("Invalid tar header. Maybe the tar is corrupted or needs to be gunzipped?"));
    }
    let encoding = options.filename_encoding;
    let mut header = TarHeader {
        name: string(&block[0..100], encoding),
        mode: octal(&block[100..108]),
        uid: octal(&block[108..116]),
        gid: octal(&block[116..124]),
        size: 0,
        mtime: octal(&block[136..148]),
        kind: TarEntryType::from_flag(block[156]),
        linkname: (block[157] != 0).then(|| string(&block[157..257], encoding)),
        uname: string(&block[265..297], FilenameEncoding::Utf8),
        gname: string(&block[297..329], FilenameEncoding::Utf8),
        devmajor: octal(&block[329..337]),
        devminor: octal(&block[337..345]),
        pax: None,
    };
    let (magic, version) = (&block[257..263], &block[263..265]);
    if magic == b"ustar\0" && version == b"00" {
        if block[345] != 0 {
            header.name = format!("{}/{}", string(&block[345..500], FilenameEncoding::Utf8), header.name);
        }
    } else if magic == b"ustar " && version == b" \0" {
        // GNU format: no prefix field.
    } else if !options.allow_unknown_format {
        return Err(TarError::parse("Invalid tar header, unknown format"));
    }
    Ok(header)
}

fn checksum(block: &[u8]) -> i64 {
    let sum: i64 = block[..148].iter().chain(&block[156..]).map(|b| *b as i64).sum();
    sum + 8 * 32
}

/// A NUL-terminated string field.
fn string(field: &[u8], encoding: FilenameEncoding) -> String {
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    match encoding {
        FilenameEncoding::Utf8 => String::from_utf8_lossy(&field[..end]).into_owned(),
        FilenameEncoding::Latin1 => field[..end].iter().map(|b| *b as char).collect(),
    }
}

/// A numeric field: octal text (space or NUL padded) or base-256 when the high bit is set.
/// Like `parseInt`, octal text stops at the first non-octal character.
fn octal(field: &[u8]) -> Option<i64> {
    if field[0] & 0x80 != 0 {
        return base256(field);
    }
    let mut start = field.iter().position(|b| *b != b' ').unwrap_or(field.len());
    let end = field[start..].iter().position(|b| *b == b' ').map_or(field.len(), |i| start + i);
    while start < end && field[start] == 0 {
        start += 1;
    }
    if start == end {
        return Some(0);
    }
    let digits = field[start..end].iter().take_while(|b| (b'0'..=b'7').contains(b)).count();
    if digits == 0 {
        return None;
    }
    let mut value: i64 = 0;
    for b in &field[start..start + digits] {
        value = value.checked_mul(8)?.checked_add((b - b'0') as i64)?;
    }
    Some(value)
}

/// Base-256 numbers: 0x80 marks positive, 0xff two's-complement negative.
fn base256(field: &[u8]) -> Option<i64> {
    let positive = match field[0] {
        0x80 => true,
        0xff => false,
        _ => return None,
    };
    let mut sum: i128 = 0;
    for b in &field[1..] {
        let b = if positive { *b } else { 0xff - *b };
        sum = sum * 256 + b as i128;
    }
    let sum = if positive { sum } else { -sum };
    i64::try_from(sum).ok()
}

/// JavaScript's `Number.MAX_SAFE_INTEGER`; Remix rejects sizes above it.
const MAX_SAFE_SIZE: u64 = (1 << 53) - 1;

/// The header's size field: trimmed octal digits only, or a positive base-256 number.
pub(crate) fn parse_size_field(field: &[u8]) -> Result<u64, TarError> {
    let invalid = || TarError::parse("Invalid tar entry size");
    let size = if field[0] & 0x80 != 0 {
        if field[0] != 0x80 {
            return Err(invalid());
        }
        base256(field).and_then(|v| u64::try_from(v).ok()).ok_or_else(invalid)?
    } else {
        let is_pad = |b: &u8| *b == 0 || *b == b' ';
        let start = field.iter().position(|b| !is_pad(b)).unwrap_or(field.len());
        let end = field.iter().rposition(|b| !is_pad(b)).map_or(start, |i| i + 1);
        let digits = &field[start..end];
        if !digits.iter().all(|b| (b'0'..=b'7').contains(b)) {
            return Err(invalid());
        }
        digits.iter().try_fold(0u64, |acc, b| acc.checked_mul(8)?.checked_add((b - b'0') as u64)).ok_or_else(invalid)?
    };
    if size > MAX_SAFE_SIZE {
        return Err(invalid());
    }
    Ok(size)
}

/// A PAX `size` record: decimal digits only.
pub(crate) fn parse_decimal_size(value: &str) -> Result<u64, TarError> {
    let invalid = || TarError::parse("Invalid tar entry size");
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    value.parse::<u64>().ok().filter(|size| *size <= MAX_SAFE_SIZE).ok_or_else(invalid)
}

/// A GNU long name or link: the data, minus one trailing NUL.
pub(crate) fn decode_long_path(buffer: &[u8]) -> String {
    let buffer = buffer.strip_suffix(&[0]).unwrap_or(buffer);
    String::from_utf8_lossy(buffer).into_owned()
}

/// PAX records: `"<len> <key>=<value>\n"`, where `len` counts the whole record.
pub(crate) fn decode_pax(mut buffer: &[u8]) -> BTreeMap<String, String> {
    let mut pax = BTreeMap::new();
    while !buffer.is_empty() {
        let space = buffer.iter().position(|b| *b == b' ').unwrap_or(buffer.len());
        let len: usize = std::str::from_utf8(&buffer[..space]).ok().and_then(leading_int).unwrap_or(0);
        if len == 0 {
            break;
        }
        // `subarray(space + 1, len - 1)`, clamped the same way.
        let start = (space + 1).min(buffer.len());
        let end = len.saturating_sub(1).min(buffer.len());
        let record = String::from_utf8_lossy(if start < end { &buffer[start..end] } else { &[] });
        let Some((key, value)) = record.split_once('=') else { break };
        pax.insert(key.to_string(), value.to_string());
        buffer = &buffer[len.min(buffer.len())..];
    }
    pax
}

/// `parseInt(s, 10)` for the non-negative case: leading digits, if any.
fn leading_int(s: &str) -> Option<usize> {
    let s = s.trim_start();
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    s[..digits].parse().ok()
}

pub(crate) fn validate_paths(header: &TarHeader, policy: PathPolicy) -> Result<(), TarError> {
    if policy == PathPolicy::Preserve {
        return Ok(());
    }
    if !is_relative_path(&header.name) || header.name.split('/').any(|part| part == "..") {
        return Err(TarError::parse("Invalid tar entry name"));
    }
    if !matches!(header.kind, TarEntryType::Symlink | TarEntryType::Link) {
        return Ok(());
    }
    let invalid_target = || TarError::parse("Invalid tar link target");
    let target = header.linkname.as_deref().filter(|t| is_relative_path(t)).ok_or_else(invalid_target)?;
    // Symlinks resolve from the link's parent directory, hard links from the archive root.
    let mut depth = if header.kind == TarEntryType::Symlink {
        header.name.split('/').filter(|p| !p.is_empty() && *p != ".").count().saturating_sub(1)
    } else {
        0
    };
    for part in target.split('/') {
        match part {
            ".." if depth == 0 => return Err(invalid_target()),
            ".." => depth -= 1,
            "" | "." => {}
            _ => depth += 1,
        }
    }
    Ok(())
}

/// Not empty, absolute, drive-prefixed (after any `./`), backslashed or NUL-containing.
fn is_relative_path(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return false;
    }
    let mut rest = path;
    while let Some(after) = rest.strip_prefix("./") {
        rest = after.trim_start_matches('/');
    }
    let bytes = rest.as_bytes();
    !(bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}
