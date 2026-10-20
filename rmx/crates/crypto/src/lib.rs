//! The small slice of Web Crypto that Remix's server packages use, in plain Rust:
//! SHA-256, HMAC-SHA-256, PBKDF2-HMAC-SHA-256, scrypt, base64, constant-time comparison and the
//! operating system's random number generator (`crypto.getRandomValues`,
//! `crypto.randomUUID`). No dependencies.

mod base64;
mod random;
mod scrypt;
mod sha256;

pub use base64::{base64_decode, base64_encode, base64url_decode, base64url_encode};
pub use random::{fill_random, random_bytes, random_hex, random_uuid};
pub use scrypt::{scrypt, ScryptParams};
pub use sha256::{hmac_sha256, pbkdf2_sha256, sha256, Sha256};

/// Lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}

/// Compare without stopping at the first difference, so timing does not reveal how much
/// of a secret matched.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() != b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

#[cfg(test)]
mod tests;
