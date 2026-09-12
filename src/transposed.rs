//! Transposed (FastLanes-style) bit-packing for full [`BLOCK`] vectors.
//!
//! The scalar packer in [`crate::bitpack`] writes values back-to-back. That
//! creates a serial data dependency: the start bit of value `i+1` depends on
//! value `i`. LLVM cannot vectorize that loop.
//!
//! This module stores the same number of bits in a **lane-major** layout:
//! 1024 values are a matrix of `lanes × rows`, where
//! `lanes = 1024 / WIDTH_BITS` (32 for `u32`, 16 for `u64`). All lanes share
//! the same bit offset at every step, so the inner loop is 32 (or 16)
//! independent shift/mask operations -- the shape LLVM can auto-vectorize.
//!
//! Compression *ratio* is identical to the scalar packer. Only layout (and
//! therefore decode throughput) changes.
//!
//! Partial blocks (`len != 1024`) still use the scalar packer; see
//! [`pack_auto`] / [`unpack_auto`].

use crate::BitPackable;
use crate::bitpack::{BLOCK, pack, packed_len, unpack};

fn width_mask(width: u32) -> u64 {
    if width == 0 {
        0
    } else if width >= 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    }
}

fn word_mask(tbits: u32) -> u128 {
    if tbits >= 64 {
        u64::MAX as u128
    } else {
        (1u128 << tbits) - 1
    }
}

fn write_word_le(out: &mut [u8], off: usize, word: u64, nbytes: usize) {
    let bytes = word.to_le_bytes();
    out[off..off + nbytes].copy_from_slice(&bytes[..nbytes]);
}

fn read_word_le(bytes: &[u8], off: usize, nbytes: usize) -> u64 {
    let mut tmp = [0u8; 8];
    tmp[..nbytes].copy_from_slice(&bytes[off..off + nbytes]);
    u64::from_le_bytes(tmp)
}

/// Pack a full 1024-value block in the transposed layout, appending to `out`.
pub fn pack_block<T: BitPackable>(values: &[T], width: u32, out: &mut Vec<u8>) {
    debug_assert_eq!(values.len(), BLOCK);
    if width == 0 {
        return;
    }
    debug_assert!(width <= T::WIDTH_BITS);
    // Stack accumulators: a heap Vec per block was drowning the kernel.
    if T::WIDTH_BITS == 32 {
        pack_fixed::<32, 4, T>(values, width, out);
    } else {
        pack_fixed::<16, 8, T>(values, width, out);
    }
}

fn pack_fixed<const LANES: usize, const WORD_BYTES: usize, T: BitPackable>(
    values: &[T],
    width: u32,
    out: &mut Vec<u8>,
) {
    let rows = BLOCK / LANES;
    let tbits = (WORD_BYTES * 8) as u32;
    let mask = width_mask(width) as u128;
    let start = out.len();
    out.resize(start + packed_len(BLOCK, width), 0);

    let mut acc = [0u128; LANES];
    let mut bits: u32 = 0;
    let mut cursor = 0usize;

    for row in 0..rows {
        for lane in 0..LANES {
            let v = values[row * LANES + lane].to_u64() as u128 & mask;
            acc[lane] |= v << bits;
        }
        bits += width;
        while bits >= tbits {
            for lane in 0..LANES {
                let word = (acc[lane] & word_mask(tbits)) as u64;
                write_word_le(out, start + cursor + lane * WORD_BYTES, word, WORD_BYTES);
                acc[lane] >>= tbits;
            }
            cursor += LANES * WORD_BYTES;
            bits -= tbits;
        }
    }
    if bits > 0 {
        for lane in 0..LANES {
            let word = (acc[lane] & word_mask(tbits)) as u64;
            write_word_le(out, start + cursor + lane * WORD_BYTES, word, WORD_BYTES);
        }
    }
}

/// Unpack a full 1024-value transposed block, appending `BLOCK` values to `out`.
pub fn unpack_block<T: BitPackable>(bytes: &[u8], width: u32, out: &mut Vec<T>) {
    if width == 0 {
        out.extend(std::iter::repeat(T::from_u64(0)).take(BLOCK));
        return;
    }
    if T::WIDTH_BITS == 32 {
        unpack_fixed::<32, 4, T>(bytes, width, out);
    } else {
        unpack_fixed::<16, 8, T>(bytes, width, out);
    }
}

fn unpack_fixed<const LANES: usize, const WORD_BYTES: usize, T: BitPackable>(
    bytes: &[u8],
    width: u32,
    out: &mut Vec<T>,
) {
    let rows = BLOCK / LANES;
    let tbits = (WORD_BYTES * 8) as u32;
    let mask = width_mask(width) as u128;
    let start = out.len();
    out.resize(start + BLOCK, T::from_u64(0));

    let mut acc = [0u128; LANES];
    let mut bits: u32 = 0;
    let mut cursor = 0usize;

    for row in 0..rows {
        while bits < width {
            for lane in 0..LANES {
                let word = read_word_le(bytes, cursor + lane * WORD_BYTES, WORD_BYTES) as u128;
                acc[lane] |= word << bits;
            }
            cursor += LANES * WORD_BYTES;
            bits += tbits;
        }
        for lane in 0..LANES {
            let v = (acc[lane] & mask) as u64;
            out[start + row * LANES + lane] = T::from_u64(v);
            acc[lane] >>= width;
        }
        bits -= width;
    }
}

/// Pack using the transposed layout for a full block, else the scalar packer.
pub fn pack_auto<T: BitPackable>(values: &[T], width: u32, out: &mut Vec<u8>) {
    if values.len() == BLOCK {
        pack_block(values, width, out);
    } else {
        pack(values, width, out);
    }
}

/// Unpack using the transposed layout for a full block, else the scalar unpacker.
pub fn unpack_auto<T: BitPackable>(bytes: &[u8], width: u32, count: usize, out: &mut Vec<T>) {
    if count == BLOCK {
        unpack_block(bytes, width, out);
    } else {
        unpack(bytes, width, count, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip_u32(values: &[u32; BLOCK], width: u32) {
        let mut packed = Vec::new();
        pack_block(values, width, &mut packed);
        assert_eq!(packed.len(), packed_len(BLOCK, width));
        let mut out = Vec::new();
        unpack_block::<u32>(&packed, width, &mut out);
        assert_eq!(out.as_slice(), values.as_slice(), "width={width}");
    }

    #[test]
    fn transposed_all_widths_u32() {
        let mut values = [0u32; BLOCK];
        for (i, v) in values.iter_mut().enumerate() {
            *v = (i as u32).wrapping_mul(17);
        }
        for width in 0u32..=32 {
            let mask = width_mask(width) as u32;
            let mut masked = values;
            for v in masked.iter_mut() {
                *v &= mask;
            }
            roundtrip_u32(&masked, width);
        }
    }

    #[test]
    fn transposed_u64_widths() {
        let mut values = [0u64; BLOCK];
        for (i, v) in values.iter_mut().enumerate() {
            *v = (i as u64).wrapping_mul(0x9E37_79B9);
        }
        for width in [0u32, 1, 7, 13, 32, 48, 64] {
            let mask = width_mask(width);
            let masked: Vec<u64> = values.iter().map(|v| v & mask).collect();
            let mut packed = Vec::new();
            pack_block(&masked, width, &mut packed);
            let mut out = Vec::new();
            unpack_block::<u64>(&packed, width, &mut out);
            assert_eq!(out, masked, "u64 width={width}");
        }
    }

    #[test]
    fn auto_falls_back_for_partial() {
        let values: Vec<u32> = (0..100).map(|i| i % 8).collect();
        let mut a = Vec::new();
        let mut b = Vec::new();
        pack_auto(&values, 3, &mut a);
        pack(&values, 3, &mut b);
        assert_eq!(a, b);
        let mut out = Vec::new();
        unpack_auto::<u32>(&a, 3, values.len(), &mut out);
        assert_eq!(out, values);
    }
}
