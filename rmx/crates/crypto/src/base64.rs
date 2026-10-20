//! Base64 (RFC 4648), standard and URL-safe alphabets, written without padding and read
//! with or without it.

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn encode(bytes: &[u8], alphabet: &[u8; 64]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..=chunk.len() {
            out.push(alphabet[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

fn decode(text: &str, alphabet: &[u8; 64]) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=');
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for b in text.bytes() {
        let v = alphabet.iter().position(|a| *a == b)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // Leftover bits must be zero padding.
    (acc == 0 && bits < 6).then_some(out)
}

/// Standard base64 without `=` padding (what Remix's cookie signatures use).
pub fn base64_encode(bytes: &[u8]) -> String {
    encode(bytes, STANDARD)
}

pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    decode(text, STANDARD)
}

/// URL-safe base64 without padding (`Buffer.toString('base64url')`).
pub fn base64url_encode(bytes: &[u8]) -> String {
    encode(bytes, URL)
}

pub fn base64url_decode(text: &str) -> Option<Vec<u8>> {
    decode(text, URL)
}
