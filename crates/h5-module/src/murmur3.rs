//! MurmurHash3 (Austin Appleby's public-domain algorithm), the two variants the
//! format uses (PRD §5.2, AusarDocs):
//! - x86, 32-bit: `string_id` values are this hash of their string, seed 0.
//! - x64, 128-bit: compression block checksums (v27) and asset checksums.
//!
//! Implemented from the algorithm's published description; verified against
//! published reference values and against real block checksums (format-notes).

/// MurmurHash3 x86 32-bit of `data` with `seed`.
pub fn murmur3_x86_32(data: &[u8], seed: u32) -> u32 {
    const C1: u32 = 0xCC9E_2D51;
    const C2: u32 = 0x1B87_3593;
    let mix = |k: u32| k.wrapping_mul(C1).rotate_left(15).wrapping_mul(C2);

    let (blocks, tail) = data.as_chunks::<4>();
    let mut h = seed;
    for block in blocks {
        h ^= mix(u32::from_le_bytes(*block));
        h = h.rotate_left(13).wrapping_mul(5).wrapping_add(0xE654_6B64);
    }
    if !tail.is_empty() {
        let k = tail.iter().rev().fold(0u32, |k, &b| (k << 8) | b as u32);
        h ^= mix(k);
    }
    h ^= data.len() as u32;
    fmix32(h)
}

fn fmix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^ (h >> 16)
}

/// MurmurHash3 x64 128-bit of `data` with `seed`, as `(h1, h2)`: the first and
/// second 64-bit halves of the result.
pub fn murmur3_x64_128(data: &[u8], seed: u32) -> (u64, u64) {
    const C1: u64 = 0x87C3_7B91_1142_53D5;
    const C2: u64 = 0x4CF5_AD43_2745_937F;
    let mix1 = |k: u64| k.wrapping_mul(C1).rotate_left(31).wrapping_mul(C2);
    let mix2 = |k: u64| k.wrapping_mul(C2).rotate_left(33).wrapping_mul(C1);

    let (blocks, tail) = data.as_chunks::<16>();
    let (mut h1, mut h2) = (seed as u64, seed as u64);
    for block in blocks {
        let (lo, hi) = block.split_at(8);
        h1 ^= mix1(u64::from_le_bytes(lo.try_into().expect("8 bytes")));
        h1 = h1.rotate_left(27).wrapping_add(h2).wrapping_mul(5).wrapping_add(0x52DC_E729);
        h2 ^= mix2(u64::from_le_bytes(hi.try_into().expect("8 bytes")));
        h2 = h2.rotate_left(31).wrapping_add(h1).wrapping_mul(5).wrapping_add(0x3849_5AB5);
    }
    // The tail (0–15 bytes): bytes 8.. into k2, bytes ..8 into k1, little-endian.
    let le = |bytes: &[u8]| bytes.iter().rev().fold(0u64, |k, &b| (k << 8) | b as u64);
    if tail.len() > 8 {
        h2 ^= mix2(le(&tail[8..]));
    }
    if !tail.is_empty() {
        h1 ^= mix1(le(&tail[..tail.len().min(8)]));
    }

    let len = data.len() as u64;
    h1 ^= len;
    h2 ^= len;
    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);
    h1 = fmix64(h1);
    h2 = fmix64(h2);
    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);
    (h1, h2)
}

fn fmix64(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    k ^= k >> 33;
    k = k.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    k ^ (k >> 33)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Published reference values for MurmurHash3 x86 32-bit.
    #[test]
    fn x86_32_reference_values() {
        assert_eq!(murmur3_x86_32(b"", 0), 0);
        assert_eq!(murmur3_x86_32(b"", 1), 0x514E_28B7);
        assert_eq!(murmur3_x86_32(b"", 0xFFFF_FFFF), 0x81F1_6F39);
        assert_eq!(murmur3_x86_32(&[0, 0, 0, 0], 0), 0x2362_F9DE);
        assert_eq!(murmur3_x86_32(b"a", 0x9747_B28C), 0x7FA0_9EA6);
        assert_eq!(murmur3_x86_32(b"aa", 0x9747_B28C), 0x5D21_1726);
        assert_eq!(murmur3_x86_32(b"aaa", 0x9747_B28C), 0x283E_0130);
        assert_eq!(murmur3_x86_32(b"aaaa", 0x9747_B28C), 0x5A97_808A);
        assert_eq!(murmur3_x86_32(b"abcd", 0x9747_B28C), 0xF047_8627);
        assert_eq!(murmur3_x86_32(b"Hello, world!", 0x9747_B28C), 0x2488_4CBA);
        assert_eq!(murmur3_x86_32(b"The quick brown fox jumps over the lazy dog", 0x9747_B28C), 0x2FA8_26CD);
    }

    /// The string_id example from AusarDocs and PRD §5.2.
    #[test]
    fn string_id_of_default() {
        assert_eq!(murmur3_x86_32(b"__default__", 0), 0x9B55_5AD2);
    }

    /// Empty input with seed 0 hashes to zero; every tail length (0–15 bytes
    /// past a 16-byte block) gives a distinct hash.
    #[test]
    fn x64_128_basics() {
        assert_eq!(murmur3_x64_128(b"", 0), (0, 0));
        let data = [0xA5u8; 47];
        let hashes: std::collections::HashSet<(u64, u64)> = (0..=47).map(|n| murmur3_x64_128(&data[..n], 0)).collect();
        assert_eq!(hashes.len(), 48);
        assert_ne!(murmur3_x64_128(b"x", 0), murmur3_x64_128(b"x", 1));
    }
}
