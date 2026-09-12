# Changelog

All notable changes to this project are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); this project is pre-1.0 and the
API is unstable.

## [Unreleased]

### Added
- `bitpack`: fixed-width bit-packing/unpacking for `u32` and `u64` over a
  128-bit accumulator; `required_bits` and `packed_len` helpers.
- `frame_of_reference`: per-1024-block minimum-subtraction + bit-packing, with a
  self-describing little-endian stream format.
- `BitPackable` trait abstracting `u32`/`u64` so codecs share one tested path.
- 16 tests: unit edge cases (empty, all-equal, width 0, full width, block
  boundaries) + dependency-free randomized round-trip (SplitMix64).
- `examples/bench.rs`: std-only benchmark harness reporting compression ratio
  (vs deflate) and encode/decode throughput (GiB/s).
- Docs: `README`, `docs/DECISIONS.md` (ADRs 0001-0005), `docs/CONCEPTS.md`
  (theory), `docs/research-notebook.md` (Experiment 001).

### Notes
- v0 is intentionally dependency-free in the core library (see ADR-0004).
- Baseline results recorded (Experiment 001): FOR beats deflate on small-range
  data, loses on monotonic timestamps (motivates delta encoding), and decode is
  ~0.6 GiB/s (motivates a vectorized layout).
