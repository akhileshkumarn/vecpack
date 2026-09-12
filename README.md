# vecpack

Lightweight compression codecs, built from scratch in Rust, with a strong
emphasis on **correctness** and **honest, reproducible benchmarking**.

This is a learning + research project. It is deliberately *not* trying to be
"another columnar format" (that space is well-served by
[Vortex](https://github.com/vortex-data/vortex), `fastlanes`, and ALP -- see
[docs/DECISIONS.md](docs/DECISIONS.md)). Instead it uses the shared primitives of
lightweight compression -- bit-packing, frame-of-reference, (later) delta and
quantization -- as a vehicle to understand low-level performance engineering, and
aims toward an under-served niche: **lossless / near-lossless compression of ML
embeddings and tensors** to fit more data into limited memory.

## Status

v0 (MVP). Implemented and tested:

- `bitpack` -- fixed-width bit-packing / unpacking of `u32` and `u64`.
- `frame_of_reference` -- per-1024-block minimum subtraction + bit-packing.
- 16 tests passing (unit edge cases + randomized round-trip).
- A dependency-free benchmark harness (`examples/bench.rs`).

See [docs/research-notebook.md](docs/research-notebook.md) for measured results
and interpretation, and [ROADMAP / levels](docs/DECISIONS.md#roadmap) for what's
next (delta encoding, then vectorized decode).

## First results (Intel i5-9300H, AVX2)

```
workload               raw(MiB)    FOR ratio    deflate   enc(GiB/s)   dec(GiB/s)
timestamps_jitter          15.3        2.66x      3.07x         0.80         0.62
small_range_0_1000         15.3        3.19x      2.24x         0.86         0.62
random_u32                 15.3        1.00x      1.00x         0.43         0.32
```

Takeaways (details in the notebook):
- FOR **beats** general-purpose deflate on ratio for small-range data (3.19x vs
  2.24x) -- lightweight encodings win when the data structure is known.
- FOR **loses** on monotonic timestamps (2.66x vs 3.07x) -> motivates **delta
  encoding** next.
- Decode throughput (~0.6 GiB/s) is low because the naive variable-width layout
  does not auto-vectorize. This is the headline optimization target (FastLanes'
  transposed layout reaches 10-100x this) -> motivates **vectorized decode**.

## Build & test

Requires a Rust toolchain (see the toolchain notes in
[docs/DECISIONS.md](docs/DECISIONS.md#adr-0005-gnu-toolchain-on-windows) if you
are on a bare Windows machine).

```powershell
cargo test                          # 16 tests
cargo run --release --example bench # measured ratios + throughput
```

## Layout

```
src/lib.rs                 # crate root + BitPackable trait (u32/u64)
src/bitpack.rs             # fixed-width bit-packing
src/frame_of_reference.rs  # FOR codec + stream format
tests/roundtrip.rs         # randomized round-trip tests (dependency-free)
examples/bench.rs          # std-only benchmark harness
docs/DECISIONS.md          # architecture decision records (ADRs)
docs/CONCEPTS.md           # the theory, explained (bit-packing, FOR, vectorization, roofline)
docs/research-notebook.md  # experiments: hypothesis -> config -> result -> interpretation
CHANGELOG.md
```

## License

MIT OR Apache-2.0.

## Acknowledgements

Developed with AI assistance.
