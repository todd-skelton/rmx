//! Percent-encoding helpers.

const HEX: &[u8; 16] = b"0123456789ABCDEF";

fn push_encoded(out: &mut String, b: u8) {
    out.push('%');
    out.push(HEX[(b >> 4) as usize] as char);
    out.push(HEX[(b & 15) as usize] as char);
}

fn encode_with(s: &str, keep: impl Fn(u8) -> bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if keep(b) {
            out.push(b as char);
        } else {
            push_encoded(&mut out, b);
        }
    }
    out
}

fn unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-._~".contains(&b)
}

/// Like `encodeURIComponent`.
pub fn encode_component(s: &str) -> String {
    encode_with(s, |b| unreserved(b) || b"!'()*".contains(&b))
}

/// Encode one path segment: everything except unreserved and sub-delims, so `/` and `?`
/// inside a value cannot change the URL structure.
pub fn encode_path_segment(s: &str) -> String {
    encode_with(s, |b| unreserved(b) || b"!$&'()*+,;=:@".contains(&b))
}

/// Encode a whole path with the WHATWG URL path percent-encode set: C0 controls, non-ASCII,
/// space, `"`, `#`, `<`, `>`, `?`, backtick, `{` and `}`. Everything else, including `%` and
/// existing escapes, is kept as written.
pub fn encode_path(s: &str) -> String {
    encode_with(s, |b| b > 0x20 && b < 0x7f && !b"\"#<>?`{}".contains(&b))
}

/// `application/x-www-form-urlencoded` serialization of one name or value.
pub fn encode_form(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"*-._".contains(&b) {
            out.push(b as char);
        } else if b == b' ' {
            out.push('+');
        } else {
            push_encoded(&mut out, b);
        }
    }
    out
}

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        _ => b - b'A' + 10,
    }
}

fn decode_bytes(s: &str, plus_as_space: bool) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() && is_hex(bytes[i + 1]) && is_hex(bytes[i + 2]) => {
                out.push(hex_val(bytes[i + 1]) << 4 | hex_val(bytes[i + 2]));
                i += 3;
            }
            b'+' if plus_as_space => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Decode `%XX` escapes (invalid UTF-8 becomes U+FFFD).
pub fn decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    decode_bytes(s, false)
}

/// Decode a form-urlencoded name or value (`+` is a space).
pub fn decode_form(s: &str) -> String {
    decode_bytes(s, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        assert_eq!(encode_component("a b/c?d=é"), "a%20b%2Fc%3Fd%3D%C3%A9");
        assert_eq!(decode("a%20b%2Fc%3Fd%3D%C3%A9"), "a b/c?d=é");
        assert_eq!(encode_form("a b&c"), "a+b%26c");
        assert_eq!(decode_form("a+b%26c"), "a b&c");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz%4"), "%zz%4");
        assert_eq!(encode_path("/a b/%c3%a9"), "/a%20b/%c3%a9");
        assert_eq!(encode_path("/100%/%zz/caf\u{e9}"), "/100%/%zz/caf%C3%A9");
        assert_eq!(encode_path("/a\"b<c>d`e{f}g#h?"), "/a%22b%3Cc%3Ed%60e%7Bf%7Dg%23h%3F");
    }
}
