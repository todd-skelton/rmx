//! The small slice of Web Crypto that Remix's server packages use, in plain Rust:
//! SHA-1/256/384/512 (`crypto.subtle.digest`), HMAC-SHA-256, PBKDF2-HMAC-SHA-256, scrypt,
//! base64, constant-time comparison and the
//! operating system's random number generator (`crypto.getRandomValues`,
//! `crypto.randomUUID`). No dependencies.

mod base64;
mod random;
mod scrypt;
mod sha1;
mod sha256;
mod sha512;

pub use base64::{base64_decode, base64_encode, base64url_decode, base64url_encode};
pub use random::{fill_random, random_bytes, random_hex, random_uuid};
pub use scrypt::{scrypt, ScryptParams};
pub use sha1::{sha1, Sha1};
pub use sha256::{hmac_sha256, pbkdf2_sha256, sha256, Sha256};
pub use sha512::{sha384, sha512, Sha512};

/// The algorithms of Web Crypto's `crypto.subtle.digest(algorithm, data)`. An unsupported
/// name is a parse error here rather than a `NotSupportedError` at call time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DigestAlgorithm {
    Sha1,
    #[default]
    Sha256,
    Sha384,
    Sha512,
}

impl DigestAlgorithm {
    /// The digest of `data`.
    pub fn digest(self, data: &[u8]) -> Vec<u8> {
        match self {
            DigestAlgorithm::Sha1 => sha1(data).to_vec(),
            DigestAlgorithm::Sha256 => sha256(data).to_vec(),
            DigestAlgorithm::Sha384 => sha384(data).to_vec(),
            DigestAlgorithm::Sha512 => sha512(data).to_vec(),
        }
    }

    /// The Web Crypto name (`"SHA-256"`).
    pub fn name(self) -> &'static str {
        match self {
            DigestAlgorithm::Sha1 => "SHA-1",
            DigestAlgorithm::Sha256 => "SHA-256",
            DigestAlgorithm::Sha384 => "SHA-384",
            DigestAlgorithm::Sha512 => "SHA-512",
        }
    }
}

impl std::str::FromStr for DigestAlgorithm {
    type Err = String;

    /// A Web Crypto name, compared case-insensitively like `normalizeAlgorithm`.
    fn from_str(name: &str) -> Result<DigestAlgorithm, String> {
        [DigestAlgorithm::Sha1, DigestAlgorithm::Sha256, DigestAlgorithm::Sha384, DigestAlgorithm::Sha512]
            .into_iter()
            .find(|a| a.name().eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("NotSupportedError: unrecognized digest algorithm {name:?}"))
    }
}

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
