# Concepts (the theory behind vecpack)

This is the "teach me" companion to the code. It explains the ideas the codecs
rely on, why they work, and where the performance goes. Read it alongside
`src/bitpack.rs`, `src/frame_of_reference.rs`, `src/delta.rs`, `src/rle.rs`,
and `src/dictionary.rs`.

---

## 1. Why lightweight compression at all?

There are two families of compressors:

- **General-purpose** (zstd, gzip/deflate, LZ4): treat data as an opaque byte
  stream, find repeated substrings, and entropy-code them. Great ratios, but
  decompression is slow (deflate ~1-2 GB/s) and *opaque*: to read one value you
  must decompress a whole block. No random access, no computing on compressed
  data.
- **Lightweight / structured** (bit-packing, frame-of-reference, delta, RLE,
  dictionary): exploit *known structure* in a column of same-typed values. Much
  faster to decode (potentially tens of GB/s), support random access, and can
  sometimes be computed on directly.

Modern analytics formats (Parquet, and especially BtrBlocks/Vortex) lean on
**cascades of lightweight encodings** instead of throwing a heavyweight
compressor on top. `vecpack` is exploring that lightweight family.

---

## 2. Bit-packing

A `u32` uses 32 bits even to store the number `5` (which needs 3 bits). If every
value in a block fits in `w` bits, we can store them back-to-back using exactly
`w` bits each.

Example, `w = 3`, values `[5, 2, 7, 1]`:

```
5=101  2=010  7=111  1=001   ->  bit stream: 101 010 111 001  -> packed bytes
```

- **Compression ratio** is simply `original_bits / w` (e.g. `32/3 ≈ 10.7x` if the
  data genuinely fits in 3 bits).
- The width `w` is `ceil(log2(max_value + 1))` -- implemented as
  `64 - max.leading_zeros()` in `bitpack::required_bits`.

Our `pack`/`unpack` use a **128-bit accumulator**: we shift incoming values into
it, and flush whole bytes out (pack) or refill from bytes and extract `w`-bit
values (unpack). 128 bits comfortably holds `64` (max width) `+ 7` (partial-byte
carry) bits at once.

> Note: this is the *simple* layout. It has a data dependency between
> consecutive values (each value's bit position depends on the previous), which
> is exactly what stops a compiler from vectorizing it. See section 5.

---

## 3. Frame-of-Reference (FOR)

Bit-packing only helps if values are *small*. But many real columns hold large
values in a *narrow range*: timestamps around 1.6 billion, ids around some base,
sensor readings around a mean.

FOR fixes this per block: subtract the block **minimum** ("the reference"), then
bit-pack the residuals.

```
[1000, 1002, 1001, 1003]   min = 1000
residuals = [0, 2, 1, 3]   -> fits in 2 bits instead of ~10
store: min (1000) + width (2) + packed residuals
```

This is what `frame_of_reference.rs` does, per 1024-value block, with a tiny
self-describing header. Decoding reverses it: unpack residuals, add `min`.

**Why per-block (1024) and not whole-column?** Locality. A block of 1024 nearby
rows tends to share magnitude, so its residuals are small. One global minimum
over millions of rows would leave large residuals. 1024 is the granularity used
by FastLanes/ALP: big enough to amortize the header, small enough to keep values
correlated.

---

## 4. FOR vs delta (a result-driven motivation)

FOR removes a *constant* offset per block. It does **not** exploit a *trend*.

Monotonic timestamps increase across the whole block, so even after subtracting
the block min, the last values are ~`1024 * step` above it -> still needs ~13
bits. That's why in our first benchmark FOR only got 2.66x on
`timestamps_jitter` and *lost* to deflate (3.07x).

**Delta encoding** stores `value[i] - value[i-1]` instead. For monotonic
timestamps those deltas are tiny (the step, 0-7 here).

One catch: a difference can be negative (the series can go down). We compute
the wrapping difference in the value's own width, then **zig-zag** map it so
small-magnitude signed values become small unsigned codes:

```
 0 -> 0,  -1 -> 1,  +1 -> 2,  -2 -> 3,  +2 -> 4, ...
```

Zig-zag of `+7` is `14`, which needs **4 bits**, not 3. So the ceiling on
`timestamps_jitter` is `32/4 = 8x`, not the ~10x we first guessed. Experiment
002 measured **7.92x** -- the ceiling minus headers. FOR was 2.66x; deflate
was 3.07x. Direction right, constant wrong, and we wrote that down.

The lesson: *the right encoding depends on the data's structure*, and you find
out which by **measuring**, not guessing. Experiment 002 made this concrete:
delta won timestamps, FOR/dictionary tied on small-range, RLE won long runs,
and nobody beat 1.00x on random. That is why cascading formats try several
encodings and pick the best per block.

---

## 4b. RLE and dictionary (the other two L2 encodings)

**Run-length encoding** replaces a streak of equal values with `(value, count)`.
A column of a million `0`s becomes one run. A column of unique values becomes
a million runs of length 1 and *expands*. We store counts as `count - 1`
bit-packed (so a run of 1 costs zero extra bits when every run has length 1).

**Dictionary encoding** builds a first-seen table of distinct values and
replaces each value with its index. Four labels -> 2-bit indices -> 16x.
A million unique values -> a dictionary as large as the input, plus indices
-> expansion (0.59x on `random_u32` in Experiment 002).

Neither is "better" than FOR or delta. Each matches a different structure.

---

## 5. Where the throughput goes (ratio is not the only metric)

Our decode runs at ~0.6 GiB/s. That sounds fine until you learn `fastlanes`
decodes at *tens of GiB/s*. Why the gap?

- **Data dependency.** In the simple layout, value `i`'s bits start right after
  value `i-1`'s. The CPU can't compute value `i` until it knows where it starts,
  so the loop is serial. Modern CPUs have wide SIMD (AVX2 = 256-bit, 8x`u32`) and
  execute several instructions per cycle, but only if the work is *independent*.
- **The FastLanes trick (L3).** Store values in a **transposed / interleaved**
  layout so that 32 (or more) independent "lanes" are unpacked in parallel with
  the same shifts/masks. The compiler then auto-vectorizes the inner loop into
  SIMD, and throughput jumps by an order of magnitude -- *with no change to the
  compression ratio*.

So a codec has (at least) two independent axes:
- **Compression ratio** -- bytes saved (driven by the encoding: FOR, delta, ...).
- **Decode throughput** -- GB/s (driven by the memory layout + vectorization).

We will optimize them separately and measure both. Never trade one silently for
the other.

---

## 6. The roofline model (preview for L3)

The **roofline** tells you the *ceiling* for a kernel on your machine:

```
attainable GB/s = min( peak_memory_bandwidth,  peak_compute * arithmetic_intensity )
```

- **Arithmetic intensity** = useful work (ops) per byte moved from memory.
- Decode is *low* intensity (a few shifts/masks per byte) -> it will be
  **memory-bandwidth bound** once vectorized. That means the real target for L3
  is "approach the machine's memory bandwidth," and once we're there, more
  compute cleverness won't help -- the honest ceiling.

We'll actually measure this machine's bandwidth and plot where our decode lands.

---

## 7. Glossary

- **Residual** -- value minus a reference (the block min in FOR).
- **Width (`w`)** -- bits used per packed value.
- **Block** -- fixed group of values (1024 here) sharing one header.
- **Auto-vectorization** -- the compiler turning scalar loops into SIMD.
- **SIMD** -- Single Instruction, Multiple Data (AVX2 on this CPU; NEON on ARM).
- **Roofline** -- performance ceiling from memory bandwidth vs compute.
- **Lossless** -- `decode(encode(x)) == x` exactly (our tests enforce this).
