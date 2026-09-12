# Research Notebook

A running log of experiments. Each entry follows: **Hypothesis -> Motivation ->
Experiment -> Configuration -> Baseline -> Result -> Interpretation -> Failure ->
Next question**. The goal is an honest engineering record, including things that
did not work.

---

## Environment (default unless overridden per-experiment)

- **CPU:** Intel Core i5-9300H @ 2.40 GHz, 4 cores / 8 threads (Coffee Lake).
  SIMD: SSE/AVX/**AVX2** (no AVX-512).
- **RAM:** ~8 GB.
- **OS:** Windows 10.0.26100.
- **Toolchain:** rustc 1.98.1, `stable-x86_64-pc-windows-gnu`.
- **Build flags:** `-C target-cpu=native`, `opt-level=3`, `lto=thin`,
  `codegen-units=1` (release profile).
- **Timing:** `examples/bench.rs`, median of 21 samples after 3 warmups; 4M
  values (15.3 MiB raw) per workload; throughput over raw bytes.

---

## Experiment 001 -- FOR + bit-packing baseline

**Hypothesis.** A simple per-block Frame-of-Reference + bit-packing codec will
(a) compress "structured" integer columns meaningfully, and (b) be correct. We
make no throughput claim yet -- this establishes the baseline.

**Motivation.** We need a correct, measured starting point before optimizing.
Every later change is judged against this.

**Experiment.** Encode/decode three synthetic 4M-value `u32` workloads; record
compression ratio (vs raw and vs deflate) and encode/decode throughput.

**Configuration.** As above. Workloads:
- `timestamps_jitter`: monotonically increasing, step 0-7.
- `small_range_0_1000`: uniform in [0, 1000).
- `random_u32`: full-range uniform.

**Baseline.** Raw = 4 bytes/value. General-purpose reference = deflate
(flate2, default level).

**Result.**

| workload            | FOR ratio | deflate ratio | enc GiB/s | dec GiB/s |
|---------------------|-----------|---------------|-----------|-----------|
| timestamps_jitter   | 2.66x     | 3.07x         | 0.80      | 0.62      |
| small_range_0_1000  | 3.19x     | 2.24x         | 0.86      | 0.62      |
| random_u32          | 1.00x     | 1.00x         | 0.43      | 0.32      |

Correctness: 16/16 tests pass, including randomized round-trip over thousands of
u32/u64 vectors crossing block boundaries.

**Interpretation.**
- **small_range**: FOR (3.19x) *beats* deflate (2.24x). Expected -- values fit in
  ~10 bits and FOR captures exactly that, while deflate pays entropy-coding
  overhead. Structured beats general-purpose when structure is known.
- **timestamps**: FOR (2.66x) *loses* to deflate (3.07x). Diagnosed in
  CONCEPTS.md sec. 4: FOR removes only the per-block *offset*, not the *trend*;
  residuals still span ~1024*step (~13 bits). deflate exploits the repetition.
- **random_u32**: 1.00x for both -- correct and expected. Random 32-bit data is
  incompressible by these means; the width is the full 32 bits. Good sanity check
  that we are not fabricating gains.
- **throughput**: ~0.6 GiB/s decode is *low*. Root cause (CONCEPTS.md sec. 5):
  the simple variable-width layout has a serial data dependency between values,
  so it does not auto-vectorize.

**Failure / surprise.** None catastrophic. The mild surprise is how *badly* FOR
does on monotonic data -- a useful, concrete motivator rather than a bug.

**Next question(s).**
1. Does **delta encoding** turn the timestamp case from 2.66x into ~10x?
   (L2 -- this experiment.)
2. Can a **transposed (FastLanes-style) layout** get decode from 0.6 GiB/s toward
   the memory-bandwidth roofline? First measure this machine's bandwidth. (L3.)
3. What is this machine's actual memory bandwidth (to set the roofline ceiling)?

---

## Experiment 002 -- L2 encodings (delta, RLE, dictionary)

**Hypothesis.** Delta encoding turns `timestamps_jitter` from 2.66x (FOR) into
about 10x, and beats deflate (3.07x) on that workload. RLE and dictionary win
on long-run / low-cardinality data and lose (expand) on unique/random data.

**Motivation.** Experiment 001 showed FOR cannot capture a *trend*. Delta is
the encoding that should. RLE and dictionary complete the classic lightweight
set so we can later pick per-block (L4) instead of guessing.

**Experiment.** Same three L1 workloads plus a new `long_runs_4_labels`
(runs of length 64-256 over 4 labels). Compare FOR, delta, RLE, dictionary,
and deflate. 36 tests (including `decode(encode(x)) == x` on every scheme).

**Configuration.** Same machine/toolchain as Experiment 001. Harness now
reports every scheme.

**Baseline.** Experiment 001 FOR + deflate numbers, plus raw = 4 bytes/value.

**Result.**

| workload            | deflate | FOR    | delta  | RLE     | dict   |
|---------------------|---------|--------|--------|---------|--------|
| timestamps_jitter   | 3.07x   | 2.66x  | **7.92x** | 1.04x | 0.64x  |
| small_range_0_1000  | 2.24x   | 3.19x  | 2.90x  | 0.94x   | 3.20x  |
| random_u32          | 1.00x   | 1.00x  | 1.00x  | 1.00x   | 0.59x  |
| long_runs_4_labels  | **212.93x** | 15.85x | 11.13x | 158.33x | 16.00x |

Decode throughput stayed in the same 0.3-0.9 GiB/s band as L1, except RLE on
`long_runs` (3.44 GiB/s) -- it emits far fewer runs than values. Dictionary
*encode* is an outlier at 0.03-0.29 GiB/s (HashMap build).

**Interpretation.**
- **Delta on timestamps: 7.92x, not ~10x.** The hypothesis was slightly
  optimistic. Steps are 0-7; zig-zag of `+7` is `14`, which needs **4 bits**,
  so the information-theoretic ceiling is `32/4 = 8x`. 7.92x is that ceiling
  minus per-block headers. The ~10x guess assumed unsigned 3-bit steps and
  forgot zig-zag widens the max code. Honest miss; the *direction* was right
  (2.66x -> 7.92x, and we now beat deflate 3.07x).
- **small_range:** FOR (3.19x) and dictionary (3.20x) tie. Both reduce to ~10
  bits (range 1000, or 1000 distinct ids). Delta is a bit worse (2.90x): a
  random walk inside a range has larger consecutive jumps than the range
  itself would suggest. RLE expands (0.94x) -- almost no repeats.
- **random:** everyone ~1.00x except dictionary, which *expands* to 0.59x
  (dictionary as large as the data, plus an index stream). Expected.
- **long_runs:** RLE (158x) is the lightweight winner; deflate (213x) still
  beats it. Dictionary/FOR sit at 16x (2-bit codes for 4 labels) and cannot
  see run length. This is why cascade selection exists.
- **Throughput** is still scalar-bound. L3 (vectorized layout) is unchanged
  as the next *speed* question. L2 answered the *ratio* question.

**Failure / surprise.**
1. The 10x timestamp prediction missed zig-zag's extra bit. Documented.
2. Dictionary encode is ~20x slower than FOR -- `HashMap` lookup/insert per
   value. Fine for v0; a specialized map is a later option, not a priority
   until a workload needs it.
3. RLE did not beat deflate on the run-heavy workload. Lightweight encodings
   are not universally smaller; they are *faster to decode* and *selectable*.

**Next question.**
1. L3: can a transposed bit-pack layout move decode from ~0.6 GiB/s toward
   this machine's memory bandwidth?
2. What *is* this machine's memory bandwidth? Measure it before claiming a
   roofline.
3. A greedy per-block selector (try cheap encodings, pick best) is now
   justified -- no single codec won every workload. That is L4.

---

## Experiment 003 -- Transposed bit-unpack vs scalar vs memcpy

**Hypothesis.** A FastLanes-style lane-major layout will auto-vectorize and
move decode from ~0.6 GiB/s toward this machine's memory bandwidth.

**Motivation.** Experiment 001/002 diagnosed the scalar packer's serial bit
cursor as the throughput ceiling. Ratio is a solved (per-workload) problem;
speed is not.

**Experiment.** Unpack 4096 blocks of 1024 `u32`s at width 10 three ways:
scalar, transposed, and a raw `memcpy` of the uncompressed 16 MiB as the
roofline. Also measure end-to-end FOR decode (which now uses the transposed
kernel on full blocks).

**Configuration.** Same machine. `examples/l3_roofline.rs`. Accumulators are
on the stack (`[u128; 32]`) -- an earlier heap `Vec` per block was measured
and discarded as an implementation tax, not a layout result.

**Baseline.** Scalar unpack on the same data. memcpy as the bandwidth ceiling.

**Result.**

| kernel              | GiB/s (raw) | vs scalar | vs memcpy |
|---------------------|-------------|-----------|-----------|
| memcpy (roofline)   | 6.41        | --        | 100%      |
| scalar unpack       | 0.88        | 1.00x     | 14%       |
| transposed unpack   | 1.37        | **1.56x** | 21%       |
| FOR decode (L3)     | 1.11        | --        | 17%       |

**Interpretation.**
- The layout change is a real, reproducible **1.56x** on the unpack kernel
  and lifts FOR decode from ~0.62 (L1) to **1.11 GiB/s**.
- It is **not** the 10-100x FastLanes advertises. Three reasons, all
  structural: (1) `width` is a runtime value, so LLVM cannot unroll a
  width-specific kernel; FastLanes monomorphizes every `W`. (2) A 128-bit
  accumulator is shifted in scalar integer units on this CPU -- AVX2 does
  not have a cheap 128-bit variable shift in the way the loop is written.
  (3) We still scatter/gather via byte offsets rather than a fully
  transposed SIMD load.
- The roofline is ~6.4 GiB/s memcpy. We sit at 21% of it. There is headroom,
  but closing it means *const-generic per-width kernels*, not more of this
  runtime-width loop. That is a later optimization, not a failed L3 -- L3
  answered "does layout matter?" with yes, 1.56x, and "are we at the
  roofline?" with no.

**Failure / surprise.** The first implementation allocated a `Vec<u128>` per
block and looked like a wash. That was a measurement of malloc, not of
layout. Stack arrays recovered the 1.56x. Hypothesis of "order-of-magnitude
from layout alone" was wrong.

**Next question.** L4: pick the best encoding *per block*. The speed question
is parked until we have a reason to write 32 width-specialized kernels.

---

## Experiment 004 -- Per-block cascade on a mixed column

**Hypothesis.** Encoding each 1024-value block with every leaf codec and
keeping the shortest will beat any single codec on a column that *changes
structure* across blocks.

**Motivation.** Experiment 002: no codec won every homogeneous workload. Real
tables are not homogeneous.

**Experiment.** 4M `u32`s, blocks cycling through monotonic, constant-run,
small-range, and random. Compare cascade ratio to FOR/delta/RLE/dictionary.
Record the tag histogram.

**Configuration.** `examples/l4_cascade.rs`. Same machine.

**Baseline.** Each leaf codec on the same mixed column.

**Result.**

| codec    | ratio | notes                                      |
|----------|-------|--------------------------------------------|
| cascade  | **2.88x** | tags: FOR 1954, delta 977, raw 976, RLE 0, dict 0 |
| delta    | 2.84x | best leaf                                  |
| FOR      | 2.45x |                                            |
| RLE      | 0.99x | expands on mixed                           |
| dict     | 0.86x | expands on mixed                           |

**Interpretation.**
- Cascade *does* win, but only by **1.4%** over delta (2.88 vs 2.84). The
  mixed column is half "FOR-friendly or raw" and a quarter monotonic; delta
  is already a decent default.
- The histogram is the real finding: the selector used **three** tags (FOR
  for runs *and* small-range, delta for monotonic, raw for random). RLE lost
  even on constant blocks because FOR-of-equals is 13 bytes vs RLE's ~30.
- Encode cost is ~5x a single codec (we try all five). Decode is one tag
  branch per block -- cheap. Cascade is a *ratio* tool, not a speed tool.
- "Real datasets" here are structured synthetic blocks, not downloaded
  tables. The structure is the thing being tested; a Parquet file would
  change the *mix*, not the mechanism.

**Failure / surprise.** RLE never won a block. Constant runs are FOR's
best case (width 0). RLE needs *long-but-varied* runs to beat FOR, and our
"run" blocks were a single value.

**Next question.** L5: apply the same measurement discipline to floats /
embeddings (lossless shuffle, FP16, INT8).

---

<!-- Template for the next entry:

## Experiment 00N -- <title>

**Hypothesis.**
**Motivation.**
**Experiment.**
**Configuration.**
**Baseline.**
**Result.**
**Interpretation.**
**Failure.**
**Next question.**
-->
