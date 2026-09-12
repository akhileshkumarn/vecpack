//! Randomized round-trip tests (dependency-free).
//!
//! The core correctness invariant of any lossless codec is:
//!
//! ```text
//! decode(encode(x)) == x   for all x
//! ```
//!
//! We don't use `proptest` here (see docs/DECISIONS.md, ADR-0004); instead we
//! drive thousands of pseudo-random inputs through a small, deterministic
//! `SplitMix64` generator. Each case is seeded reproducibly, so a failure prints
//! the exact seed/length needed to reproduce it.

use vecpack::bitpack;
use vecpack::frame_of_reference as for_codec;

/// Minimal, fast, deterministic PRNG (SplitMix64). Not cryptographic; perfect
/// for reproducible test/benchmark data.
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }
    /// Uniform-ish value in `[0, n)` for `n > 0`.
    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

#[test]
fn for_roundtrip_u32_random() {
    for seed in 0..2000u64 {
        let mut rng = SplitMix64::new(seed.wrapping_mul(0xDEAD_BEEF).wrapping_add(1));
        let len = rng.below(5000) as usize;
        let values: Vec<u32> = (0..len).map(|_| rng.next_u32()).collect();
        let enc = for_codec::encode(&values);
        let dec: Vec<u32> = for_codec::decode(&enc);
        assert_eq!(dec, values, "u32 mismatch at seed={seed} len={len}");
    }
}

#[test]
fn for_roundtrip_u64_random() {
    for seed in 0..2000u64 {
        let mut rng = SplitMix64::new(seed.wrapping_mul(0x1234_5678).wrapping_add(7));
        let len = rng.below(5000) as usize;
        let values: Vec<u64> = (0..len).map(|_| rng.next_u64()).collect();
        let enc = for_codec::encode(&values);
        let dec: Vec<u64> = for_codec::decode(&enc);
        assert_eq!(dec, values, "u64 mismatch at seed={seed} len={len}");
    }
}

#[test]
fn for_roundtrip_clustered() {
    // Small dynamic range (the regime FOR is designed for): base + small offset.
    for seed in 0..2000u64 {
        let mut rng = SplitMix64::new(seed.wrapping_mul(0xABCD).wrapping_add(3));
        let base = rng.next_u32();
        let spread = 1 + rng.below(1024) as u32;
        let len = rng.below(3000) as usize;
        let values: Vec<u32> = (0..len).map(|_| base.wrapping_add(rng.next_u32() % spread)).collect();
        let enc = for_codec::encode(&values);
        let dec: Vec<u32> = for_codec::decode(&enc);
        assert_eq!(dec, values, "clustered mismatch at seed={seed} len={len} spread={spread}");
    }
}

#[test]
fn bitpack_roundtrip_all_widths() {
    for width in 0u32..=32 {
        let mut rng = SplitMix64::new(0x5151 ^ width as u64);
        let len = 1500 + rng.below(1000) as usize;
        let mask: u64 = if width == 0 {
            0
        } else if width == 32 {
            u32::MAX as u64
        } else {
            (1u64 << width) - 1
        };
        let values: Vec<u32> = (0..len).map(|_| (rng.next_u64() & mask) as u32).collect();

        let mut packed = Vec::new();
        bitpack::pack(&values, width, &mut packed);
        assert_eq!(packed.len(), bitpack::packed_len(values.len(), width), "packed_len mismatch width={width}");

        let mut out: Vec<u32> = Vec::new();
        bitpack::unpack(&packed, width, values.len(), &mut out);
        assert_eq!(out, values, "bitpack mismatch at width={width}");
    }
}
