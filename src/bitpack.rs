//! Fixed-width bit-packing of unsigned integers.
//!
//! Given a slice of values that each fit in `width` bits, [`pack`] writes them
//! back-to-back using exactly `width` bits per value (no per-value padding).
//! [`unpack`] reverses this given the same `width` and the number of values.
//!
//! This is the classic, *scalar* bit-packing baseline. It is intentionally
//! simple and correct rather than maximally fast: it is the honest starting
//! point we will later profile and compare against a transposed, auto-vectorized
//! layout (e.g. the `fastlanes` crate). See `docs/CONCEPTS.md` for the theory.

use crate::BitPackable;

/// Number of values per block used by higher-level codecs.
///
/// 1024 is the granularity used by FastLanes/ALP and friends: large enough to
/// amortize per-block headers, small enough that a block's values share
/// locality (similar magnitude), which is exactly what lightweight encodings
/// exploit.
pub const BLOCK: usize = 1024;

/// Minimum number of bits required to represent `max` (0 if `max == 0`).
#[inline]
pub fn required_bits<T: BitPackable>(max: T) -> u32 {
    let m = max.to_u64();
    if m == 0 { 0 } else { 64 - m.leading_zeros() }
}

/// Number of bytes produced by packing `count` values at `width` bits each.
#[inline]
pub fn packed_len(count: usize, width: u32) -> usize {
    (count * width as usize + 7) / 8
}

/// Bit-pack `values` at `width` bits each, appending to `out`.
///
/// Every value must fit in `width` bits; higher bits are masked off defensively.
/// `width == 0` is valid and encodes to zero bytes (used when all values are
/// equal, e.g. after frame-of-reference subtraction).
pub fn pack<T: BitPackable>(values: &[T], width: u32, out: &mut Vec<u8>) {
    debug_assert!(width <= T::WIDTH_BITS, "width exceeds type width");
    if width == 0 {
        return;
    }
    let mask: u128 = (1u128 << width) - 1;
    let mut acc: u128 = 0;
    let mut bits: u32 = 0;
    for &v in values {
        acc |= ((v.to_u64() as u128) & mask) << bits;
        bits += width;
        while bits >= 8 {
            out.push((acc & 0xFF) as u8);
            acc >>= 8;
            bits -= 8;
        }
    }
    if bits > 0 {
        out.push((acc & 0xFF) as u8);
    }
}

/// Unpack `count` values of `width` bits each from `bytes`, appending to `out`.
///
/// This is the inverse of [`pack`]. The caller must supply the same `width` and
/// `count`, and `bytes` must contain at least [`packed_len`] bytes.
pub fn unpack<T: BitPackable>(bytes: &[u8], width: u32, count: usize, out: &mut Vec<T>) {
    if width == 0 {
        out.extend(std::iter::repeat(T::from_u64(0)).take(count));
        return;
    }
    let mask: u128 = (1u128 << width) - 1;
    let mut acc: u128 = 0;
    let mut bits: u32 = 0;
    let mut idx = 0usize;
    for _ in 0..count {
        while bits < width {
            acc |= (bytes[idx] as u128) << bits;
            idx += 1;
            bits += 8;
        }
        let val = (acc & mask) as u64;
        acc >>= width;
        bits -= width;
        out.push(T::from_u64(val));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_bits_basics() {
        assert_eq!(required_bits(0u32), 0);
        assert_eq!(required_bits(1u32), 1);
        assert_eq!(required_bits(2u32), 2);
        assert_eq!(required_bits(255u32), 8);
        assert_eq!(required_bits(256u32), 9);
        assert_eq!(required_bits(u32::MAX), 32);
        assert_eq!(required_bits(u64::MAX), 64);
    }

    fn roundtrip_u32(values: &[u32], width: u32) {
        let mut packed = Vec::new();
        pack(values, width, &mut packed);
        assert_eq!(packed.len(), packed_len(values.len(), width));
        let mut out: Vec<u32> = Vec::new();
        unpack(&packed, width, values.len(), &mut out);
        assert_eq!(&out, values);
    }

    #[test]
    fn pack_unpack_small_widths() {
        roundtrip_u32(&[0, 1, 0, 1, 1, 0, 1], 1);
        roundtrip_u32(&[0, 1, 2, 3, 2, 1, 0], 2);
        roundtrip_u32(&[7, 3, 5, 1, 6, 0, 4, 2], 3);
    }

    #[test]
    fn pack_unpack_width_zero() {
        roundtrip_u32(&[0, 0, 0, 0], 0);
    }

    #[test]
    fn pack_unpack_full_width() {
        roundtrip_u32(&[0, u32::MAX, 12345, u32::MAX - 1], 32);
    }

    #[test]
    fn pack_unpack_u64_full_width() {
        let values = [0u64, u64::MAX, 1, u64::MAX - 7, 42];
        let mut packed = Vec::new();
        pack(&values, 64, &mut packed);
        let mut out: Vec<u64> = Vec::new();
        unpack(&packed, 64, values.len(), &mut out);
        assert_eq!(out, values);
    }

    #[test]
    fn pack_unpack_block_sized() {
        let values: Vec<u32> = (0..BLOCK as u32).map(|i| i % 512).collect();
        roundtrip_u32(&values, 9);
    }
}
