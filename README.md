# vecpack

Lightweight compression codecs, built from scratch in Rust, with a strong
emphasis on **correctness** and **honest, reproducible benchmarking**.

This is a learning + research project. It is deliberately *not* trying to be
"another columnar format" (that space is well-served by
[Vortex](https://github.com/vortex-data/vortex), `fastlanes`, and ALP -- see
[docs/DECISIONS.md](docs/DECISIONS.md)). Instead it uses the shared primitives of
lightweight compression -- bit-packing, frame-of-reference, delta, RLE,
dictionary -- as a vehicle to understand low-level performance engineering, and
aims toward an under-served niche: **lossless / near-lossless compression of ML
embeddings and tensors** to fit more data into limited memory.

## Status

L2. Implemented and tested (36 tests):

- `bitpack` -- fixed-width bit-packing / unpacking of `u32` and `u64`.
- `frame_of_reference` -- per-1024-block minimum subtraction + bit-packing.
- `delta` -- per-block consecutive differences, zig-zag encoded, then packed.
- `rle` -- `(value, count)` runs with bit-packed counts.
- `dictionary` -- first-seen table + bit-packed indices.
- `Codec` / `Scheme` -- one interface over all four encodings.
- A dependency-free benchmark harness (`examples/bench.rs`).

See [docs/research-notebook.md](docs/research-notebook.md) for measured results
and interpretation.

## Results (Intel i5-9300H, AVX2)

Compression ratio (raw / encoded). Bold is the best lightweight codec; deflate
is the general-purpose reference.

| workload            | deflate | FOR   | delta    | RLE      | dict  |
|---------------------|---------|-------|----------|----------|-------|
| timestamps_jitter   | 3.07x   | 2.66x | **7.92x** | 1.04x   | 0.64x |
| small_range_0_1000  | 2.24x   | 3.19x | 2.90x    | 0.94x    | **3.20x** |
| random_u32          | 1.00x   | 1.00x | 1.00x    | 1.00x    | 0.59x |
| long_runs_4_labels  | 212.93x | 15.85x | 11.13x  | **158.33x** | 16.00x |

Takeaways (details in the notebook):
- **No single codec wins.** Delta wins trends, FOR/dictionary win narrow
  ranges, RLE wins long repeats, nobody compresses random data. This is the
  measured case for per-block selection (L4).
- Experiment 001 guessed delta would hit ~10x on timestamps. It hit **7.92x**
  because zig-zag of step `+7` needs 4 bits, not 3. Still beats FOR (2.66x)
  and deflate (3.07x).
- Decode is still ~0.3-0.9 GiB/s (scalar bit-packing). RLE on long runs is
  the exception (3.44 GiB/s) because it emits far fewer symbols. Vectorized
  decode remains the L3 target.

## Build & test

Requires a Rust toolchain (see the toolchain notes in
[docs/DECISIONS.md](docs/DECISIONS.md#adr-0005-gnu-toolchain-on-windows) if you
are on a bare Windows machine).

```powershell
cargo test                          # 36 tests
cargo run --release --example bench # measured ratios + throughput
```

## Layout

```
src/lib.rs                 # crate root + BitPackable trait (u32/u64)
src/bitpack.rs             # fixed-width bit-packing
src/frame_of_reference.rs  # FOR codec
src/delta.rs               # delta + zig-zag
src/rle.rs                 # run-length encoding
src/dictionary.rs          # dictionary encoding
src/codec.rs               # Codec trait + Scheme enum
tests/roundtrip.rs         # randomized round-trip tests (dependency-free)
examples/bench.rs          # std-only benchmark harness
docs/DECISIONS.md          # architecture decision records (ADRs)
docs/CONCEPTS.md           # the theory, explained
docs/research-notebook.md  # experiments: hypothesis -> result -> interpretation
CHANGELOG.md
```

## License

MIT OR Apache-2.0.

## Acknowledgements

Developed with AI assistance.
