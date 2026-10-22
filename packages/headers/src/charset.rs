//! The WHATWG `TextDecoder` charsets that `filename*` values use (`Content-Disposition`).

/// Decode `bytes` in the charset a `TextDecoder` label names (case-insensitive). `None`
/// for labels outside UTF-8, UTF-16, windows-1252 (which `latin1`, `iso-8859-1` and
/// `us-ascii` also name) and ISO-8859-15.
pub(crate) fn decode(label: &str, bytes: &[u8]) -> Option<String> {
    let label = label.trim().to_ascii_lowercase();
    let text = match label.as_str() {
        "utf-8" | "utf8" | "unicode-1-1-utf-8" | "unicode11utf8" | "unicode20utf8" | "x-unicode20utf8" => {
            let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
            String::from_utf8_lossy(bytes).into_owned()
        }
        "utf-16" | "utf-16le" | "csunicode" | "iso-10646-ucs-2" | "ucs-2" | "unicode" | "unicodefeff" => {
            utf16(bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes), u16::from_le_bytes)
        }
        "utf-16be" | "unicodefffe" => utf16(bytes.strip_prefix(&[0xfe, 0xff]).unwrap_or(bytes), u16::from_be_bytes),
        "windows-1252" | "ansi_x3.4-1968" | "ascii" | "cp1252" | "cp819" | "csisolatin1" | "ibm819" | "iso-8859-1"
        | "iso-ir-100" | "iso8859-1" | "iso88591" | "iso_8859-1" | "iso_8859-1:1987" | "l1" | "latin1" | "us-ascii"
        | "x-cp1252" => bytes.iter().map(|&b| windows_1252(b)).collect(),
        "iso-8859-15" | "csisolatin9" | "iso8859-15" | "iso885915" | "iso_8859-15" | "l9" => {
            bytes.iter().map(|&b| iso_8859_15(b)).collect()
        }
        _ => return None,
    };
    Some(text)
}

fn utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units = bytes.as_chunks::<2>().0.iter().map(|&pair| unit(pair));
    let mut text: String = char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)).collect();
    if bytes.len() % 2 == 1 {
        text.push(char::REPLACEMENT_CHARACTER);
    }
    text
}

/// windows-1252, which differs from Latin-1 only in 0x80 to 0x9F.
fn windows_1252(byte: u8) -> char {
    const HIGH: [u16; 32] = [
        0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039, 0x152, 0x8d, 0x17d,
        0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0x9d,
        0x17e, 0x178,
    ];
    match byte {
        0x80..=0x9f => char::from_u32(u32::from(HIGH[usize::from(byte - 0x80)])).unwrap_or(char::REPLACEMENT_CHARACTER),
        _ => char::from(byte),
    }
}

/// ISO-8859-15, Latin-1 with eight replacements (the euro sign at 0xA4).
fn iso_8859_15(byte: u8) -> char {
    match byte {
        0xa4 => '\u{20ac}',
        0xa6 => '\u{160}',
        0xa8 => '\u{161}',
        0xb4 => '\u{17d}',
        0xb8 => '\u{17e}',
        0xbc => '\u{152}',
        0xbd => '\u{153}',
        0xbe => '\u{178}',
        _ => char::from(byte),
    }
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn labels_and_tables() {
        assert_eq!(decode("UTF-8", b"\xef\xbb\xbfA").as_deref(), Some("A"));
        assert_eq!(decode("latin1", b"\x80\x81\xff").as_deref(), Some("\u{20ac}\u{81}\u{ff}"));
        assert_eq!(decode("ISO-8859-15", b"\xa4\xa5").as_deref(), Some("\u{20ac}\u{a5}"));
        assert_eq!(decode("utf-16le", b"\xff\xfeA\0B").as_deref(), Some("A\u{fffd}"));
        assert_eq!(decode("utf-16be", b"\0A").as_deref(), Some("A"));
        assert_eq!(decode("latin9", b"x"), None);
    }
}
