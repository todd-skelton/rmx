//! The operating system's cryptographically secure random number generator.

/// Fill `buf` with random bytes from the OS (`crypto.getRandomValues`).
pub fn fill_random(buf: &mut [u8]) {
    os::fill(buf);
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    fill_random(&mut buf);
    buf
}

/// `n` random bytes as lowercase hex.
pub fn random_hex(n: usize) -> String {
    let mut buf = vec![0u8; n];
    fill_random(&mut buf);
    crate::hex(&buf)
}

/// A version 4 UUID (`crypto.randomUUID()`).
pub fn random_uuid() -> String {
    let mut b: [u8; 16] = random_bytes();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = crate::hex(&b);
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

#[cfg(windows)]
mod os {
    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(algorithm: *mut core::ffi::c_void, buffer: *mut u8, len: u32, flags: u32) -> i32;
    }
    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x2;

    pub(super) fn fill(buf: &mut [u8]) {
        for chunk in buf.chunks_mut(u32::MAX as usize) {
            // SAFETY: the buffer pointer and length describe `chunk`, which is writable.
            let status = unsafe {
                BCryptGenRandom(core::ptr::null_mut(), chunk.as_mut_ptr(), chunk.len() as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG)
            };
            assert!(status >= 0, "BCryptGenRandom failed: {status:#x}");
        }
    }
}

#[cfg(unix)]
mod os {
    use std::io::Read;

    pub(super) fn fill(buf: &mut [u8]) {
        std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(buf)).expect("read /dev/urandom");
    }
}

#[cfg(not(any(windows, unix)))]
mod os {
    pub(super) fn fill(_: &mut [u8]) {
        unimplemented!("rsmix-crypto has no random source for this target")
    }
}
