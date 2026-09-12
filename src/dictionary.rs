//! Dictionary encoding.
//!
//! Collect the distinct values in the column (in first-seen order), then
//! replace each value with its small integer index and bit-pack the indices.
//! Wins when the cardinality is low relative to the column length (status
//! codes, enums, repeated ids). Loses when almost every value is unique --
//! the dictionary is then nearly as large as the raw data, plus the index
//! stream.
//!
//! ## Stream format (little-endian)
//!
//! ```text
//! [ n: u64 ]                     total number of values
//! [ ndict: u32 ]                 number of distinct values
//! [ dict: ndict * T ]            dictionary entries, first-seen order
//! [ width: u8 ]                  bits per index
//! [ packed indices ]             n indices into the dictionary
//! ```

use std::collections::HashMap;

use crate::BitPackable;
use crate::bitpack::{pack, packed_len, required_bits, unpack};

/// Encode `values` as a first-seen dictionary plus bit-packed indices.
pub fn encode<T: BitPackable>(values: &[T]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(values.len() as u64).to_le_bytes());

    let mut dict: Vec<T> = Vec::new();
    let mut index: HashMap<u64, u32> = HashMap::new();
    let mut ids: Vec<u32> = Vec::with_capacity(values.len());

    for &v in values {
        let key = v.to_u64();
        if let Some(&i) = index.get(&key) {
            ids.push(i);
        } else {
            let i = dict.len() as u32;
            dict.push(v);
            index.insert(key, i);
            ids.push(i);
        }
    }

    out.extend_from_slice(&(dict.len() as u32).to_le_bytes());
    for &v in &dict {
        v.write_le(&mut out);
    }

    let max_id = dict.len().saturating_sub(1) as u32;
    let width = required_bits(max_id);
    out.push(width as u8);
    pack(&ids, width, &mut out);
    out
}

/// Decode a stream produced by [`encode`].
pub fn decode<T: BitPackable>(bytes: &[u8]) -> Vec<T> {
    let n = u64::from_le_bytes(bytes[0..8].try_into().expect("stream too short for header")) as usize;
    let ndict = u32::from_le_bytes(bytes[8..12].try_into().expect("stream too short for ndict")) as usize;
    let mut pos = 12usize;

    let mut dict: Vec<T> = Vec::with_capacity(ndict);
    for _ in 0..ndict {
        dict.push(T::read_le(&bytes[pos..pos + T::BYTES]));
        pos += T::BYTES;
    }

    let width = bytes[pos] as u32;
    pos += 1;

    let plen = packed_len(n, width);
    let mut ids: Vec<u32> = Vec::with_capacity(n);
    unpack(&bytes[pos..pos + plen], width, n, &mut ids);

    ids.into_iter().map(|i| dict[i as usize]).collect()
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
    fn single_distinct() {
        let values = vec![42u32; 2000];
        let enc = encode(&values);
        // 8 (n) + 4 (ndict) + 4 (dict) + 1 (width 0) + 0 packed.
        assert_eq!(enc.len(), 17);
        assert_eq!(decode::<u32>(&enc), values);
    }

    #[test]
    fn low_cardinality_is_compact() {
        let values: Vec<u32> = (0..4096).map(|i| i % 4).collect();
        let enc = encode(&values);
        let raw = values.len() * 4;
        assert!(enc.len() < raw / 8, "expected strong compression, got {} vs raw {}", enc.len(), raw);
        assert_eq!(decode::<u32>(&enc), values);
    }

    #[test]
    fn unique_values_expand() {
        let values: Vec<u32> = (0..200).collect();
        let enc = encode(&values);
        let raw = values.len() * 4;
        assert!(enc.len() > raw, "dictionary should expand unique data");
        assert_eq!(decode::<u32>(&enc), values);
    }

    #[test]
    fn u64_roundtrip() {
        let values = vec![0u64, 1, 0, u64::MAX, 1, u64::MAX];
        assert_eq!(decode::<u64>(&encode(&values)), values);
    }
}
