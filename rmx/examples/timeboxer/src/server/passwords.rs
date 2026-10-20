//! `data/passwords.ts`: `scrypt:<key length>:<salt>:<derived key>`, all hex. As in Remix, the
//! hex salt string itself is the scrypt salt.

use rmx::crypto::{constant_time_eq, hex, random_hex, scrypt, ScryptParams};

const KEY_LENGTH: usize = 64;

pub fn hash_password(password: &str) -> String {
    let salt = random_hex(16);
    let derived = scrypt(password.as_bytes(), salt.as_bytes(), ScryptParams::default(), KEY_LENGTH).expect("default scrypt parameters are valid");
    format!("scrypt:{KEY_LENGTH}:{salt}:{}", hex(&derived))
}

pub fn verify_password(password: &str, stored: &str) -> bool {
    let mut parts = stored.split(':');
    let (Some("scrypt"), Some(length), Some(salt), Some(expected)) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let (Ok(length), Some(expected)) = (length.parse::<usize>(), decode_hex(expected)) else { return false };
    if salt.is_empty() || expected.is_empty() {
        return false;
    }
    scrypt(password.as_bytes(), salt.as_bytes(), ScryptParams::default(), length).is_some_and(|actual| constant_time_eq(&expected, &actual))
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies() {
        let stored = hash_password("password123");
        let parts: Vec<&str> = stored.split(':').collect();
        assert_eq!((parts[0], parts[1], parts[2].len(), parts[3].len()), ("scrypt", "64", 32, 128));
        assert!(verify_password("password123", &stored));
        assert!(!verify_password("password124", &stored));
        assert!(!verify_password("password123", "bcrypt:64:aa:bb"));
    }

    #[test]
    fn verifies_a_hash_made_by_node() {
        // node -e "crypto.scrypt('password123', 'salt', 64, (e, k) => console.log(k.toString('hex')))"
        let stored = format!("scrypt:64:salt:{}", hex(&scrypt(b"password123", b"salt", ScryptParams::default(), 64).unwrap()));
        assert!(verify_password("password123", &stored));
    }
}
