# Changelog

All notable changes to this project are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); this project is pre-1.0 and the
API is unstable.

## [Unreleased]

### Added (L4)
- `cascade`: per-block greedy selection among FOR/delta/RLE/dictionary/raw.
- `examples/l4_cascade.rs`. Experiment 004: 2.88x vs best leaf 2.84x.
  ADR-0009.

### Added (L3)
- `transposed`: lane-major bit-pack/unpack for full 1024-value blocks; FOR
  and delta use it automatically. Partial blocks stay scalar.
- `examples/l3_roofline.rs`: scalar vs transposed vs memcpy.
- Experiment 003: 1.56x unpack (1.37 vs 0.88 GiB/s), 21% of a 6.4 GiB/s
  memcpy roofline. ADR-0008.

### Added (L2)
- `delta`: per-block consecutive differences, zig-zag encoded inside the
  original integer width, then bit-packed. Independent `base` per block.
- `rle`: `(value, count-1)` runs with bit-packed counts.
- `dictionary`: first-seen dictionary + bit-packed indices.
- `Codec` trait and runtime `Scheme` enum over FOR / delta / RLE / dictionary.
- Benchmark harness now reports every scheme on four workloads (added
  `long_runs_4_labels`).
- Docs: ADRs 0006-0007, CONCEPTS zig-zag / RLE / dictionary, Experiment 002.

### Notes
- Experiment 002: delta is 7.92x on `timestamps_jitter` (FOR 2.66x, deflate
  3.07x). The pre-L2 ~10x guess was wrong -- zig-zag of step +7 needs 4 bits,
  ceiling 8x. RLE wins long runs (158x) but loses to deflate (213x). Dictionary
  encode is HashMap-bound (~0.03-0.29 GiB/s).
- 36 tests. Core library still has zero runtime dependencies.

### Added (L1)
- `bitpack`: fixed-width bit-packing/unpacking for `u32` and `u64` over a
  128-bit accumulator; `required_bits` and `packed_len` helpers.
- `frame_of_reference`: per-1024-block minimum-subtraction + bit-packing, with a
  self-describing little-endian stream format.
- `BitPackable` trait abstracting `u32`/`u64` so codecs share one tested path.
- Dependency-free randomized round-trip tests (SplitMix64).
- `examples/bench.rs`: std-only benchmark harness.
- Docs: `README`, `docs/DECISIONS.md` (ADRs 0001-0005), `docs/CONCEPTS.md`
  (theory), `docs/research-notebook.md` (Experiment 001).
