//! Per-block cascade: try the L2 encodings, keep the smallest.
//!
//! Experiment 002 showed no single codec wins every workload. This module
//! makes that operational: each 1024-value block is encoded with FOR, delta,
//! RLE, and dictionary (and as raw), and we store the shortest payload plus a
//! 1-byte tag. Decode reads the tag and dispatches.
//!
//! Encode is slower (it tries every scheme). Decode is one extra byte of
//! branching per block. Ratio should approach the envelope of the best
//! per-block codec -- and beat any single codec on *mixed* columns.

use crate::BitPackable;
use crate::Scheme;
use crate::bitpack::BLOCK;

const TAG_FOR: u8 = 0;
const TAG_DELTA: u8 = 1;
const TAG_RLE: u8 = 2;
const TAG_DICT: u8 = 3;
const TAG_RAW: u8 = 4;

fn raw_encode<T: BitPackable>(chunk: &[T]) -> Vec<u8> {
    let mut out = Vec::with_capacity(chunk.len() * T::BYTES);
    for &v in chunk {
        v.write_le(&mut out);
    }
    out
}

fn raw_decode<T: BitPackable>(bytes: &[u8], count: usize) -> Vec<T> {
    let mut out = Vec::with_capacity(count);
    let mut pos = 0;
    for _ in 0..count {
        out.push(T::read_le(&bytes[pos..pos + T::BYTES]));
        pos += T::BYTES;
    }
    out
}

fn candidates<T: BitPackable>(chunk: &[T]) -> [(u8, Vec<u8>); 5] {
    [
        (TAG_FOR, Scheme::FrameOfReference.encode(chunk)),
        (TAG_DELTA, Scheme::Delta.encode(chunk)),
        (TAG_RLE, Scheme::RunLength.encode(chunk)),
        (TAG_DICT, Scheme::Dictionary.encode(chunk)),
        (TAG_RAW, raw_encode(chunk)),
    ]
}

/// Encode `values`, picking the shortest scheme for each block.
pub fn encode<T: BitPackable>(values: &[T]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(values.len() as u64).to_le_bytes());
    for chunk in values.chunks(BLOCK) {
        let mut best_tag = TAG_RAW;
        let mut best_len = usize::MAX;
        let mut best = Vec::new();
        for (tag, cand) in candidates(chunk) {
            if cand.len() < best_len {
                best_tag = tag;
                best_len = cand.len();
                best = cand;
            }
        }
        out.push(best_tag);
        out.extend_from_slice(&(best.len() as u32).to_le_bytes());
        out.extend_from_slice(&best);
    }
    out
}

/// Decode a stream produced by [`encode`].
pub fn decode<T: BitPackable>(bytes: &[u8]) -> Vec<T> {
    let n = u64::from_le_bytes(bytes[0..8].try_into().expect("cascade header")) as usize;
    let mut pos = 8usize;
    let mut out = Vec::with_capacity(n);
    let mut remaining = n;
    while remaining > 0 {
        let count = remaining.min(BLOCK);
        let tag = bytes[pos];
        pos += 1;
        let plen = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        let payload = &bytes[pos..pos + plen];
        pos += plen;
        let block: Vec<T> = match tag {
            TAG_FOR => Scheme::FrameOfReference.decode(payload),
            TAG_DELTA => Scheme::Delta.decode(payload),
            TAG_RLE => Scheme::RunLength.decode(payload),
            TAG_DICT => Scheme::Dictionary.decode(payload),
            TAG_RAW => raw_decode(payload, count),
            other => panic!("unknown cascade tag {other}"),
        };
        debug_assert_eq!(block.len(), count);
        out.extend(block);
        remaining -= count;
    }
    out
}

/// How many blocks were stored under each tag. Used by tests/benches to
/// show the selector is actually switching.
pub fn tag_histogram(bytes: &[u8]) -> [usize; 5] {
    let n = u64::from_le_bytes(bytes[0..8].try_into().unwrap()) as usize;
    let mut pos = 8usize;
    let mut remaining = n;
    let mut hist = [0usize; 5];
    while remaining > 0 {
        let count = remaining.min(BLOCK);
        let tag = bytes[pos] as usize;
        pos += 1;
        let plen = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4 + plen;
        if tag < 5 {
            hist[tag] += 1;
        }
        remaining -= count;
    }
    hist
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_roundtrip() {
        let empty: Vec<u32> = vec![];
        assert_eq!(decode::<u32>(&encode(&empty)), empty);
        let values: Vec<u32> = (0..3000).map(|i| i * 3).collect();
        assert_eq!(decode::<u32>(&encode(&values)), values);
    }

    #[test]
    fn mixed_column_uses_several_tags() {
        let mut values = Vec::new();
        // Block 0: monotonic -- delta should win.
        values.extend((0..1024u32).map(|i| 1_000_000 + i));
        // Block 1: long runs -- RLE should win.
        values.extend(std::iter::repeat(7u32).take(1024));
        // Block 2: two labels -- dict or RLE.
        values.extend((0..1024u32).map(|i| i % 2));
        let enc = encode(&values);
        assert_eq!(decode::<u32>(&enc), values);
        let hist = tag_histogram(&enc);
        let used = hist.iter().filter(|&&c| c > 0).count();
        assert!(used >= 2, "expected several tags, hist={hist:?}");
    }
}
