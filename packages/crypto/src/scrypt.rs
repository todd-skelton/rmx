//! scrypt (RFC 7914), as Node's `crypto.scrypt` computes it.

use crate::pbkdf2_sha256;

/// Cost parameters. [ScryptParams::default] matches Node: N = 16384, r = 8, p = 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScryptParams {
    /// CPU/memory cost; a power of two greater than 1.
    pub n: u32,
    /// Block size.
    pub r: u32,
    /// Parallelization.
    pub p: u32,
}

impl Default for ScryptParams {
    fn default() -> ScryptParams {
        ScryptParams { n: 16384, r: 8, p: 1 }
    }
}

/// Derive `length` bytes. Returns `None` for invalid parameters, or ones needing more than
/// Node's default 32 MiB of memory (`128 * n * r`).
pub fn scrypt(password: &[u8], salt: &[u8], params: ScryptParams, length: usize) -> Option<Vec<u8>> {
    let ScryptParams { n, r, p } = params;
    if n < 2 || !n.is_power_of_two() || r == 0 || p == 0 {
        return None;
    }
    let block_len = 128 * r as usize;
    if 128 * n as u64 * r as u64 > 32 << 20 {
        return None;
    }
    let mut b = pbkdf2_sha256(password, salt, 1, p as usize * block_len);
    let words = block_len / 4;
    let mut x = vec![0u32; words];
    let mut v = vec![0u32; words * n as usize];
    let mut scratch = vec![0u32; words];
    for chunk in b.chunks_mut(block_len) {
        for (w, bytes) in x.iter_mut().zip(chunk.chunks(4)) {
            *w = u32::from_le_bytes(bytes.try_into().unwrap());
        }
        ro_mix(&mut x, &mut v, &mut scratch, n as usize, r as usize);
        for (w, bytes) in x.iter().zip(chunk.chunks_mut(4)) {
            bytes.copy_from_slice(&w.to_le_bytes());
        }
    }
    Some(pbkdf2_sha256(password, &b, 1, length))
}

fn ro_mix(x: &mut [u32], v: &mut [u32], scratch: &mut [u32], n: usize, r: usize) {
    let words = x.len();
    for i in 0..n {
        v[i * words..(i + 1) * words].copy_from_slice(x);
        block_mix(x, scratch, r);
    }
    for _ in 0..n {
        let j = x[(2 * r - 1) * 16] as usize & (n - 1);
        for (a, b) in x.iter_mut().zip(&v[j * words..(j + 1) * words]) {
            *a ^= b;
        }
        block_mix(x, scratch, r);
    }
}

/// BlockMix with Salsa20/8: even output blocks first, then odd ones.
fn block_mix(b: &mut [u32], y: &mut [u32], r: usize) {
    let mut x = [0u32; 16];
    x.copy_from_slice(&b[(2 * r - 1) * 16..]);
    for i in 0..2 * r {
        for (a, c) in x.iter_mut().zip(&b[i * 16..(i + 1) * 16]) {
            *a ^= c;
        }
        salsa20_8(&mut x);
        let dest = if i % 2 == 0 { i / 2 } else { r + i / 2 };
        y[dest * 16..(dest + 1) * 16].copy_from_slice(&x);
    }
    b.copy_from_slice(y);
}

fn salsa20_8(b: &mut [u32; 16]) {
    let mut x = *b;
    for _ in 0..4 {
        // Column round, then row round.
        for (a, c, d, e) in [(4, 0, 12, 7), (8, 4, 0, 9), (12, 8, 4, 13), (0, 12, 8, 18), (9, 5, 1, 7), (13, 9, 5, 9), (1, 13, 9, 13), (5, 1, 13, 18), (14, 10, 6, 7), (2, 14, 10, 9), (6, 2, 14, 13), (10, 6, 2, 18), (3, 15, 11, 7), (7, 3, 15, 9), (11, 7, 3, 13), (15, 11, 7, 18)] {
            x[a] ^= x[c].wrapping_add(x[d]).rotate_left(e);
        }
        for (a, c, d, e) in [(1, 0, 3, 7), (2, 1, 0, 9), (3, 2, 1, 13), (0, 3, 2, 18), (6, 5, 4, 7), (7, 6, 5, 9), (4, 7, 6, 13), (5, 4, 7, 18), (11, 10, 9, 7), (8, 11, 10, 9), (9, 8, 11, 13), (10, 9, 8, 18), (12, 15, 14, 7), (13, 12, 15, 9), (14, 13, 12, 13), (15, 14, 13, 18)] {
            x[a] ^= x[c].wrapping_add(x[d]).rotate_left(e);
        }
    }
    for (o, i) in b.iter_mut().zip(x) {
        *o = o.wrapping_add(i);
    }
}
