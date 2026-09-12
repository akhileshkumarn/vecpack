//! Dependency-light benchmark harness.
//!
//! Run with:
//!   cargo run --release --example bench
//!
//! For each workload we report, for every L2 scheme plus deflate:
//!   1. Compression ratio (raw bytes / encoded bytes).
//!   2. Encode / decode throughput in GiB/s over the *raw* byte volume.
//!
//! This uses a hand-rolled timing loop (median of N samples after warmup) rather
//! than Criterion; see docs/DECISIONS.md (ADR-0004).

use std::hint::black_box;
use std::io::Write;
use std::time::Instant;

use flate2::Compression;
use flate2::write::DeflateEncoder;

use vecpack::cascade;
use vecpack::Scheme;

/// Deterministic SplitMix64 PRNG for reproducible synthetic data.
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
    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

fn as_bytes(values: &[u32]) -> &[u8] {
    // Safety: u32 has no invalid bit patterns; byte length is 4x element count.
    unsafe { std::slice::from_raw_parts(values.as_ptr() as *const u8, values.len() * 4) }
}

fn deflate_len(values: &[u32]) -> usize {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
    enc.write_all(as_bytes(values)).unwrap();
    enc.finish().unwrap().len()
}

/// Run `f` a few times (after warmup) and return (median_ms, gib_per_s) where
/// throughput is computed over `raw_bytes`.
fn measure<F: FnMut() -> usize>(mut f: F, raw_bytes: usize) -> (f64, f64) {
    for _ in 0..3 {
        black_box(f());
    }
    let mut samples = Vec::new();
    for _ in 0..21 {
        let t = Instant::now();
        let r = f();
        let dt = t.elapsed().as_secs_f64();
        black_box(r);
        samples.push(dt);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = samples[samples.len() / 2];
    let gibps = raw_bytes as f64 / median / (1024.0 * 1024.0 * 1024.0);
    (median * 1000.0, gibps)
}

fn workloads() -> Vec<(&'static str, Vec<u32>)> {
    let n = 4_000_000usize;
    let mut rng = SplitMix64::new(0xC0FFEE);

    // 1. Monotonic timestamps with small jitter: the L1 case FOR lost.
    let mut ts = Vec::with_capacity(n);
    let mut t: u32 = 1_600_000_000;
    for _ in 0..n {
        t = t.wrapping_add(rng.below(8) as u32);
        ts.push(t);
    }

    // 2. Small-range values (e.g. quantized readings 0..1000).
    let small: Vec<u32> = (0..n).map(|_| rng.below(1000) as u32).collect();

    // 3. Full-range random u32: incompressible worst case for every scheme.
    let random: Vec<u32> = (0..n).map(|_| rng.next_u64() as u32).collect();

    // 4. Long runs of a few labels: RLE / dictionary should shine.
    let labels = [0u32, 1, 2, 3];
    let mut runs = Vec::with_capacity(n);
    while runs.len() < n {
        let label = labels[rng.below(labels.len() as u64) as usize];
        let len = 64 + rng.below(192) as usize;
        let take = len.min(n - runs.len());
        runs.extend(std::iter::repeat(label).take(take));
    }

    vec![
        ("timestamps_jitter", ts),
        ("small_range_0_1000", small),
        ("random_u32", random),
        ("long_runs_4_labels", runs),
    ]
}

fn main() {
    println!(
        "{:<20} {:<6} {:>10} {:>10} {:>12} {:>12}",
        "workload", "codec", "ratio", "enc(GiB/s)", "dec(GiB/s)", "dec(ms)"
    );
    println!("{}", "-".repeat(78));

    for (name, data) in workloads() {
        let raw = data.len() * 4;
        let defl_ratio = raw as f64 / deflate_len(&data) as f64;
        println!(
            "{:<20} {:<6} {:>9.2}x {:>10} {:>12} {:>12}",
            name, "defl", defl_ratio, "-", "-", "-"
        );

        for scheme in Scheme::ALL {
            let encoded = scheme.encode(&data);
            let ratio = raw as f64 / encoded.len() as f64;

            let (_, enc_gibps) = measure(|| scheme.encode(black_box(&data)).len(), raw);
            let (dec_ms, dec_gibps) = measure(
                || {
                    let out: Vec<u32> = scheme.decode(black_box(&encoded));
                    out.len()
                },
                raw,
            );

            println!(
                "{:<20} {:<6} {:>9.2}x {:>10.2} {:>12.2} {:>12.3}",
                name,
                scheme.name(),
                ratio,
                enc_gibps,
                dec_gibps,
                dec_ms,
            );
        }
        println!();
    }

    // Mixed column: 4 block types tiled. Cascade should beat any single leaf.
    let n = 4_000_000usize;
    let mut rng = SplitMix64::new(0xC0FFEE);
    let mut mixed = Vec::with_capacity(n);
    while mixed.len() < n {
        let kind = mixed.len() / 1024 % 4;
        let take = (n - mixed.len()).min(1024);
        match kind {
            0 => {
                let base = 1_600_000_000u32.wrapping_add(mixed.len() as u32);
                mixed.extend((0..take as u32).map(|i| base.wrapping_add(i)));
            }
            1 => mixed.extend(std::iter::repeat(rng.below(4) as u32).take(take)),
            2 => mixed.extend((0..take).map(|_| rng.below(1000) as u32)),
            _ => mixed.extend((0..take).map(|_| rng.next_u64() as u32)),
        }
    }
    let raw = mixed.len() * 4;
    let casc = cascade::encode(&mixed);
    let hist = cascade::tag_histogram(&casc);
    println!(
        "{:<20} {:<6} {:>9.2}x  tags for/delta/rle/dict/raw = {}/{}/{}/{}/{}",
        "mixed_blocks",
        "casc",
        raw as f64 / casc.len() as f64,
        hist[0],
        hist[1],
        hist[2],
        hist[3],
        hist[4]
    );
    for scheme in Scheme::ALL {
        let enc = scheme.encode(&mixed);
        println!(
            "{:<20} {:<6} {:>9.2}x",
            "mixed_blocks",
            scheme.name(),
            raw as f64 / enc.len() as f64
        );
    }
}
