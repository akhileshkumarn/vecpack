//! Frame-of-Reference (FOR) encoding.
//!
//! The idea: within a block of values that are numerically *close* (common in
//! real columns: timestamps, ids, sensor readings), subtract the block minimum
//! so the residuals are small, then bit-pack the residuals using only as many
//! bits as the largest residual needs.
//!
//! Example: `[1000, 1002, 1001, 1003]` has range 3, so residuals
//! `[0, 2, 1, 3]` need only 2 bits each instead of ~10.
//!
//! ## Stream format (little-endian)
//!
//! ```text
//! [ n: u64 ]                      total number of values
//! repeated per block of up to BLOCK values:
//!   [ min: T (BYTES) ]            block minimum (the "reference")
//!   [ width: u8 ]                 bits per residual (0..=WIDTH_BITS)
//!   [ packed residuals ]          packed_len(block_count, width) bytes
//! ```
//!
//! The last block may contain fewer than [`crate::bitpack::BLOCK`] values; its
//! count is derived from `n`, so it is not stored explicitly.

use crate::BitPackable;
use crate::bitpack::{BLOCK, pack, packed_len, required_bits, unpack};

/// Encode `values` using per-block frame-of-reference + bit-packing.
pub fn encode<T: BitPackable>(values: &[T]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(values.len() as u64).to_le_bytes());

    let mut residuals: Vec<T> = Vec::with_capacity(BLOCK);
    for chunk in values.chunks(BLOCK) {
        // Block minimum via monotonic u64 comparison (no Ord bound needed).
        let min = chunk
            .iter()
            .copied()
            .reduce(|a, b| if a.to_u64() <= b.to_u64() { a } else { b })
            .expect("chunks() never yields empty slices");

        // Largest residual determines the packing width.
        let mut max_residual: u64 = 0;
        residuals.clear();
        for &v in chunk {
            let d = v.wrapping_sub(min);
            let du = d.to_u64();
            if du > max_residual {
                max_residual = du;
            }
            residuals.push(d);
        }
        let width = required_bits(T::from_u64(max_residual));

        min.write_le(&mut out);
        out.push(width as u8);
        pack(&residuals, width, &mut out);
    }
    out
}

/// Decode a stream produced by [`encode`].
pub fn decode<T: BitPackable>(bytes: &[u8]) -> Vec<T> {
    let n = u64::from_le_bytes(bytes[0..8].try_into().expect("stream too short for header")) as usize;
    let mut pos = 8usize;
    let mut out: Vec<T> = Vec::with_capacity(n);

    let mut remaining = n;
    let mut residuals: Vec<T> = Vec::with_capacity(BLOCK);
    while remaining > 0 {
        let count = remaining.min(BLOCK);

        let min = T::read_le(&bytes[pos..pos + T::BYTES]);
        pos += T::BYTES;
        let width = bytes[pos] as u32;
        pos += 1;

        let plen = packed_len(count, width);
        residuals.clear();
        unpack(&bytes[pos..pos + plen], width, count, &mut residuals);
        pos += plen;

        for &d in &residuals {
            out.push(d.wrapping_add(min));
        }
        remaining -= count;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        let values: Vec<u32> = vec![];
        let enc = encode(&values);
        let dec: Vec<u32> = decode(&enc);
        assert_eq!(dec, values);
    }

    #[test]
    fn all_equal_packs_to_zero_width() {
        let values = vec![777u32; 1000];
        let enc = encode(&values);
        // 8 (n) + 4 (min) + 1 (width) + 0 (packed) = 13 bytes for one block.
        assert_eq!(enc.len(), 13);
        let dec: Vec<u32> = decode(&enc);
        assert_eq!(dec, values);
    }

    #[test]
    fn small_range_is_compact() {
        // Values close together -> small residuals -> few bits.
        let values: Vec<u32> = (0..2048).map(|i| 1_000_000 + (i % 4)).collect();
        let enc = encode(&values);
        let raw = values.len() * 4;
        assert!(enc.len() < raw / 4, "expected strong compression, got {} vs raw {}", enc.len(), raw);
        let dec: Vec<u32> = decode(&enc);
        assert_eq!(dec, values);
    }

    #[test]
    fn block_boundaries() {
        for n in [0usize, 1, 1023, 1024, 1025, 2048, 4097] {
            let values: Vec<u32> = (0..n as u32).collect();
            let enc = encode(&values);
            let dec: Vec<u32> = decode(&enc);
            assert_eq!(dec, values, "failed at n={n}");
        }
    }

    #[test]
    fn u64_roundtrip() {
        let values: Vec<u64> = (0..3000).map(|i| (i as u64) * 1_000_003 + 5).collect();
        let enc = encode(&values);
        let dec: Vec<u64> = decode(&enc);
        assert_eq!(dec, values);
    }

    #[test]
    fn extremes() {
        let values = vec![0u32, u32::MAX, 0, u32::MAX];
        let enc = encode(&values);
        let dec: Vec<u32> = decode(&enc);
        assert_eq!(dec, values);
    }
}
