# Architecture Decision Records (ADRs)

This file records *why* the project is the way it is. Each ADR captures the
context, the decision, and the tradeoff, so that six months from now the
reasoning is recoverable. Newest decisions are appended at the end.

---

## ADR-0001: Rust, not C++

**Context.** The project needs a systems language for low-level bit manipulation,
SIMD, and cache-sensitive code. The author is coming from Python and learning the
low-level layer as they go.

**Decision.** Use Rust.

**Why.**
- Memory safety catches the exact bug class (out-of-bounds, aliasing) that
  compression code with heavy pointer/bit arithmetic is prone to -- at compile
  time or as clean panics, not silent corruption.
- Zero performance sacrifice: both Rust and C++ lower through LLVM. The
  reference `fastlanes` **Rust** crate hits ">100 billion integers/sec".
- Tooling matches the "measure, don't claim" ethos: `cargo` + `criterion` +
  `cargo-show-asm` out of the box.
- The frontier for this niche is already Rust (`fastlanes`, `vortex`).

**When C++ would win (rejected here).** Embedding into an existing C++ engine
(DuckDB/Velox), reusing the C++ ALP reference directly, or an author already
fluent in C++. None apply.

---

## ADR-0002: North star = ML embedding/tensor compression (not a generic columnar format)

**Context (verified Sept 2026).** The generic lightweight-columnar-compression
space is *well* served: `fastlanes` (Rust), `cwida/alp` (C++), and **Vortex**
(a Linux Foundation project, v0.75, backed by Microsoft/Snowflake/Palantir/NVIDIA,
with GPU-native decode). Building "another columnar codec library" would be
derivative.

**Decision.** Keep the *MVP* as generic integer codecs (bit-packing, FOR, later
delta/dictionary), because those primitives are shared. Re-aim the *north star*
(around L5) toward **lossless / near-lossless compression of ML embeddings and
tensors** to relieve memory/IO pressure -- a hotter, less-saturated space
(cf. IBP arXiv:2606.30728; the solo `.cvc`/Decompressed format; Vortex
TurboQuant).

**Tradeoff.** The niche's ceiling (GPU-native decode) eventually wants a GPU. The
core, resume-worthy results (ratio, decode throughput, memory footprint) are all
demonstrable CPU-side first. The specific embedding gap must be re-verified at L5.

---

## ADR-0003: Compile for the native CPU when benchmarking

**Context.** The default `x86_64` target only guarantees SSE2. Auto-vectorization
of decode kernels benefits from AVX2 (and NEON on the future Mac).

**Decision.** `.cargo/config.toml` sets `rustflags = ["-C", "target-cpu=native"]`.

**Tradeoff.** Binaries are not portable to older CPUs. Acceptable for a local
benchmarking project; must be removed/overridden before distributing artifacts or
cross-compiling (e.g. for the Apple-Silicon Mac).

---

## ADR-0004: Dependency-free v0 (no proptest/criterion for now)

**Context.** The bare Windows machine has the GNU Rust toolchain but **no
assembler** (`as.exe`). Crates that need Windows import libraries
(`proptest`/`criterion`/`rand` -> `getrandom` -> `windows-sys`) fail to build:
`dlltool` is present in the toolchain's `self-contained` dir but cannot spawn an
assembler to generate the import libs. The author is not an administrator, so a
system-wide mingw/MSVC-SDK install is not currently available.

**Decision.** Make v0 dependency-free:
- Core library: **zero** dependencies (std only).
- Tests: a hand-rolled `SplitMix64` PRNG drives thousands of reproducible
  randomized round-trip cases (replacing `proptest`).
- Benchmark: a std-only timing harness (median of N samples) in
  `examples/bench.rs` (replacing `criterion`).
- Only `flate2` remains as a dev-dependency (pure-Rust `miniz_oxide`/`zlib-rs`
  backend, no `windows-sys`) for a general-purpose compression-ratio baseline.

**Tradeoff.** We lose `criterion`'s statistical rigor (confidence intervals) and
`proptest`'s shrinking. We regain a trivially reproducible build and a
dependency-light codebase (arguably fitting for a "from scratch" project).
**Revisit** when a full toolchain is available (admin + mingw-w64, or the Mac):
reintroduce `criterion`/`proptest`.

---

## ADR-0005: GNU toolchain on Windows

**Context.** MSVC is the recommended Windows Rust target, but this machine has
the VC++ linker without the Windows SDK import libraries (`kernel32.lib` etc.),
so MSVC linking fails. Installing the SDK needs admin + a multi-GB download.

**Decision.** Use `stable-x86_64-pc-windows-gnu`, whose `rust-mingw` component
ships a self-contained linker. For builds, prepend the toolchain's
`.../x86_64-pc-windows-gnu/bin/self-contained` directory to `PATH` so `dlltool`
and the bundled linker are found.

**Consequence.** Combined with ADR-0004, the project builds on this machine with
no external toolchain install. On the future Apple-Silicon Mac this is moot
(clang/linker present); we will switch to the standard `aarch64-apple-darwin`
toolchain there.

---

## ADR-0006: Zig-zag deltas stay in the original integer width

**Context.** Consecutive differences are signed. Widening every `u32` delta to
`i64` would work but doubles working-set size and complicates `BitPackable`.

**Decision.** Compute `wrapping_sub` in the value's own width, then zig-zag
within that width (`0, -1, +1, -2, ...` -> `0, 1, 2, 3, ...`). Blocks are
independent (each stores its first value as `base`) so a wrap at a block
boundary cannot poison the next block.

**Tradeoff.** Zig-zag of a positive step `k` is `2k`, so the max code for
steps 0-7 is 14 (4 bits) rather than 7 (3 bits). That is why the timestamp
ratio is ~8x rather than ~10x. We accept the extra bit for a uniform
signed-delta story that also handles decreases.

---

## ADR-0007: A `Codec` trait only after the second encoding existed

**Context.** L1 had one encoding. Introducing a trait then would have been
theatre.

**Decision.** Add `Codec` and a runtime `Scheme` enum at L2, when FOR, delta,
RLE, and dictionary all share `decode(encode(x)) == x` and the bench needs
to iterate them.

**Tradeoff.** The on-disk streams are still per-codec (no shared header byte).
A cascade selector that writes a scheme tag is L4; we will add the tag when
we actually select.

---

## ADR-0008: Transposed layout, stack accumulators, runtime width

**Context.** L3 asked whether a FastLanes-style layout would jump decode
throughput by an order of magnitude.

**Decision.** Adopt a lane-major pack for full 1024-value blocks (`u32`: 32
lanes; `u64`: 16). Keep a runtime `width` and a 128-bit per-lane
accumulator on the *stack*. Partial blocks stay on the scalar packer.

**Tradeoff.** +1.56x unpack vs scalar, FOR decode 0.62 -> 1.11 GiB/s. We did
not reach the memcpy roofline or published FastLanes numbers. Const-generic
per-width kernels would likely do better and are deliberately *not* written
until a workload needs them (32 copies of nearly the same function is a
lot of surface for a 2-4x maybe).

---

## Roadmap (levels)

The higher levels should emerge from measurements, not be forced.

- **L1 (done)** -- FOR + bit-packing, correctness, baseline benchmark.
- **L2 (done)** -- Delta (zig-zag), RLE, dictionary; `Codec` / `Scheme`;
  expanded benchmark. Experiment 002: delta 7.92x on timestamps (not the
  guessed 10x -- zig-zag needs 4 bits for steps 0-7).
- **L3 (done)** -- Transposed bit-pack. Experiment 003: 1.56x vs scalar
  unpack (1.37 vs 0.88 GiB/s), 21% of memcpy roofline (6.4 GiB/s). Not
  FastLanes-class; runtime width + u128 shifts limit auto-vec. Stack
  accumulators (ADR-0008).
- **L4** -- Real datasets + cascading (BtrBlocks-style per-block scheme
  selection); reproducible benchmark suite.
- **L5** -- Re-aim at the embedding/tensor niche (float split/byte-shuffle,
  FP16/INT8 with error bounds); metrics: ratio + decode GB/s + memory footprint.
- **L6 (optional, needs GPU)** -- on-device decode via Metal/MLX on the Mac.
- **L7** -- Consolidate the notebook into an honest experimental writeup.
