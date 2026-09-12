//! Run-length encoding.
//!
//! Consecutive equal values become a single `(value, count)` run. This wins
//! hard on data with long repeats (flags, categorical columns, padded regions)
//! and *loses* on data with no repeats -- each unique value becomes a run of
//! length 1, plus header overhead. That expansion is expected; we measure it
//! rather than hide it.
//!
//! ## Stream format (little-endian)
//!
//! ```text
//! [ n: u64 ]                     total number of values
//! [ nruns: u64 ]                 number of runs
//! [ count_width: u8 ]            bits per run length
//! [ values: nruns * T ]          each run's value, raw little-endian
//! [ packed counts ]              nruns run-lengths, bit-packed
//! ```
//!
//! Run lengths are stored as the count minus one (so a run of 1 encodes as 0)
//! and then bit-packed. Values are stored raw: cascading FOR/delta on the
//! value stream is an L4 concern.

use crate::BitPackable;
use crate::bitpack::{pack, packed_len, required_bits, unpack};

/// Encode `values` as `(value, count)` runs with bit-packed counts.
pub fn encode<T: BitPackable>(values: &[T]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(values.len() as u64).to_le_bytes());

    if values.is_empty() {
        out.extend_from_slice(&0u64.to_le_bytes());
        out.push(0);
        return out;
    }

    let mut run_values: Vec<T> = Vec::new();
    let mut run_counts: Vec<u32> = Vec::new();
    let mut current = values[0];
    let mut count: u32 = 1;
    for &v in &values[1..] {
        if v.to_u64() == current.to_u64() && count < u32::MAX {
            count += 1;
        } else {
            run_values.push(current);
            run_counts.push(count - 1);
            current = v;
            count = 1;
        }
    }
    run_values.push(current);
    run_counts.push(count - 1);

    let nruns = run_values.len() as u64;
    out.extend_from_slice(&nruns.to_le_bytes());

    let max_stored = run_counts.iter().copied().max().unwrap_or(0);
    let width = required_bits(max_stored);
    out.push(width as u8);

    for &v in &run_values {
        v.write_le(&mut out);
    }
    pack(&run_counts, width, &mut out);
    out
}

/// Decode a stream produced by [`encode`].
pub fn decode<T: BitPackable>(bytes: &[u8]) -> Vec<T> {
    let n = u64::from_le_bytes(bytes[0..8].try_into().expect("stream too short for header")) as usize;
    let nruns = u64::from_le_bytes(bytes[8..16].try_into().expect("stream too short for nruns")) as usize;
    let width = bytes[16] as u32;
    let mut pos = 17usize;

    let mut out: Vec<T> = Vec::with_capacity(n);
    if nruns == 0 {
        return out;
    }

    let mut values: Vec<T> = Vec::with_capacity(nruns);
    for _ in 0..nruns {
        values.push(T::read_le(&bytes[pos..pos + T::BYTES]));
        pos += T::BYTES;
    }

    let plen = packed_len(nruns, width);
    let mut counts: Vec<u32> = Vec::with_capacity(nruns);
    unpack(&bytes[pos..pos + plen], width, nruns, &mut counts);

    for (v, stored) in values.into_iter().zip(counts) {
        let count = stored + 1;
        out.extend(std::iter::repeat(v).take(count as usize));
    }
    debug_assert_eq!(out.len(), n);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        let values: Vec<u32> = vec![];
        assert_eq!(decode::<u32>(&encode(&values)), values);
    }

    #[test]
    fn all_equal_is_one_run() {
        let values = vec![9u32; 5000];
        let enc = encode(&values);
        // 8 (n) + 8 (nruns) + 1 (width) + 4 (value) + packed count.
        // 5000-1 = 4999 needs 13 bits -> 2 bytes.
        assert!(enc.len() < 32, "expected a tiny encoding, got {}", enc.len());
        assert_eq!(decode::<u32>(&enc), values);
    }

    #[test]
    fn no_repeats_expands() {
        let values: Vec<u32> = (0..100).collect();
        let enc = encode(&values);
        let raw = values.len() * 4;
        assert!(enc.len() > raw, "RLE should expand unique data");
        assert_eq!(decode::<u32>(&enc), values);
    }

    #[test]
    fn mixed_runs() {
        let mut values = Vec::new();
        values.extend(std::iter::repeat(1u32).take(10));
        values.extend(std::iter::repeat(2u32).take(1));
        values.extend(std::iter::repeat(3u32).take(200));
        let enc = encode(&values);
        assert_eq!(decode::<u32>(&enc), values);
    }

    #[test]
    fn u64_roundtrip() {
        let values = vec![0u64, 0, 7, 7, 7, u64::MAX, u64::MAX];
        assert_eq!(decode::<u64>(&encode(&values)), values);
    }
}
