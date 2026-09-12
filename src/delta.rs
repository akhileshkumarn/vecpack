//! Delta encoding.
//!
//! Where Frame-of-Reference removes a per-block *offset*, delta encoding removes
//! a per-block *trend*: it stores the difference between consecutive values.
//! For monotonic data (timestamps, ids, cumulative counters) those differences
//! are tiny, so they bit-pack to very few bits.
//!
//! ## Handling decreases: in-width zig-zag
//!
//! Consecutive differences can be negative (values go down). We compute the
//! difference with wrapping arithmetic *inside the value's own width* `W`
//! (so a `u32` delta stays 32-bit), then apply **zig-zag** within `W` bits,
//! which maps small-magnitude signed values (either sign) to small unsigned
//! values:
//!
//! ```text
//!  0 -> 0,  -1 -> 1,  +1 -> 2,  -2 -> 3,  +2 -> 4, ...
//! ```
//!
//! This keeps everything in the original integer width (no widening to 64-bit),
//! is exactly reversible, and yields tight bit-packing for both increasing and
//! decreasing sequences. See docs/CONCEPTS.md sec. 4.
//!
//! ## Stream format (little-endian)
//!
//! ```text
//! [ n: u64 ]                     total number of values
//! repeated per block of up to BLOCK values (count = block length):
//!   [ base: T (BYTES) ]          first value of the block (delta reference)
//!   [ width: u8 ]                bits per zig-zag delta
//!   [ packed zig-zag deltas ]    packed_len(count - 1, width) bytes
//! ```
//!
//! Blocks are independent (each carries its own `base`), which keeps random
//! access and future parallel decode simple.

use crate::BitPackable;
use crate::bitpack::{BLOCK, pack, packed_len, required_bits, unpack};

/// Low-`W`-bits mask for a `BitPackable` type (`u32::MAX`-style, but width-aware).
#[inline]
fn low_mask<T: BitPackable>() -> u64 {
    if T::WIDTH_BITS == 64 {
        u64::MAX
    } else {
        (1u64 << T::WIDTH_BITS) - 1
    }
}

/// Zig-zag encode a wrapping delta `d` within `T`'s bit width, returning the
/// unsigned code in `[0, 2^W)`.
#[inline]
fn zigzag<T: BitPackable>(d: T) -> u64 {
    let w = T::WIDTH_BITS;
    let mask = low_mask::<T>();
    let u = d.to_u64() & mask;
    let sign_extend = if (u >> (w - 1)) & 1 == 1 { mask } else { 0 };
    ((u << 1) ^ sign_extend) & mask
}

/// Inverse of [`zigzag`]: recover the wrapping delta `d` (as `T`) from its code.
#[inline]
fn unzigzag<T: BitPackable>(zz: u64) -> T {
    let mask = low_mask::<T>();
    let sign_extend = if zz & 1 == 1 { mask } else { 0 };
    let d = ((zz >> 1) ^ sign_extend) & mask;
    T::from_u64(d)
}

/// Encode `values` using per-block delta + in-width zig-zag + bit-packing.
pub fn encode<T: BitPackable>(values: &[T]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(values.len() as u64).to_le_bytes());

    let mut zz_deltas: Vec<T> = Vec::with_capacity(BLOCK);
    for chunk in values.chunks(BLOCK) {
        let base = chunk[0];

        let mut max_zz: u64 = 0;
        zz_deltas.clear();
        let mut prev = base;
        for &v in &chunk[1..] {
            let d = v.wrapping_sub(prev);
            let zz = zigzag(d);
            if zz > max_zz {
                max_zz = zz;
            }
            zz_deltas.push(T::from_u64(zz));
            prev = v;
        }
        let width = required_bits(T::from_u64(max_zz));

        base.write_le(&mut out);
        out.push(width as u8);
        pack(&zz_deltas, width, &mut out);
    }
    out
}

/// Decode a stream produced by [`encode`].
pub fn decode<T: BitPackable>(bytes: &[u8]) -> Vec<T> {
    let n = u64::from_le_bytes(bytes[0..8].try_into().expect("stream too short for header")) as usize;
    let mut pos = 8usize;
    let mut out: Vec<T> = Vec::with_capacity(n);

    let mut remaining = n;
    let mut zz_deltas: Vec<T> = Vec::with_capacity(BLOCK);
    while remaining > 0 {
        let count = remaining.min(BLOCK);

        let base = T::read_le(&bytes[pos..pos + T::BYTES]);
        pos += T::BYTES;
        let width = bytes[pos] as u32;
        pos += 1;

        let ndeltas = count - 1;
        let plen = packed_len(ndeltas, width);
        zz_deltas.clear();
        unpack(&bytes[pos..pos + plen], width, ndeltas, &mut zz_deltas);
        pos += plen;

        out.push(base);
        let mut prev = base;
        for &zz in &zz_deltas {
            let d = unzigzag::<T>(zz.to_u64());
            let v = prev.wrapping_add(d);
            out.push(v);
            prev = v;
        }

        remaining -= count;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zigzag_maps_small_magnitudes_small() {
        assert_eq!(zigzag(0u32), 0);
        assert_eq!(zigzag(1u32.wrapping_neg()), 1); // -1
        assert_eq!(zigzag(1u32), 2); // +1
        assert_eq!(zigzag(2u32.wrapping_neg()), 3); // -2
        assert_eq!(zigzag(2u32), 4); // +2
        // Round-trip.
        for d in [0u32, 1, 5, 100, u32::MAX, u32::MAX - 1, 1u32.wrapping_neg()] {
            assert_eq!(unzigzag::<u32>(zigzag(d)), d);
        }
    }

    #[test]
    fn monotonic_is_tiny() {
        // Constant step of 1: every zig-zag delta is 2 -> width 2 bits.
        let values: Vec<u32> = (0..2048u32).collect();
        let enc = encode(&values);
        let raw = values.len() * 4;
        assert!(enc.len() < raw / 10, "expected >10x, got {} vs raw {}", enc.len(), raw);
        assert_eq!(decode::<u32>(&enc), values);
    }

    #[test]
    fn decreasing_ok() {
        let values: Vec<u32> = (0..2048u32).rev().collect();
        let enc = encode(&values);
        assert_eq!(decode::<u32>(&enc), values);
    }

    #[test]
    fn empty_and_singletons() {
        for n in [0usize, 1, 2, 1023, 1024, 1025, 2049] {
            let values: Vec<u32> = (0..n as u32).map(|i| i.wrapping_mul(7)).collect();
            let enc = encode(&values);
            assert_eq!(decode::<u32>(&enc), values, "failed n={n}");
        }
    }

    #[test]
    fn u64_and_extremes() {
        let values = vec![0u64, u64::MAX, 0, 1, u64::MAX - 3, 10];
        let enc = encode(&values);
        assert_eq!(decode::<u64>(&enc), values);
    }
}
