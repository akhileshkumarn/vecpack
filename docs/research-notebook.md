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
   (L2 -- next.)
2. Can a **transposed (FastLanes-style) layout** get decode from 0.6 GiB/s toward
   the memory-bandwidth roofline? First measure this machine's bandwidth. (L3.)
3. What is this machine's actual memory bandwidth (to set the roofline ceiling)?

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
