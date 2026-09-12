//! L3: scalar vs transposed bit-unpack, plus a memcpy roofline.
//!
//!   cargo run --release --example l3_roofline

use std::hint::black_box;
use std::time::Instant;

use vecpack::bitpack::{self, BLOCK};
use vecpack::frame_of_reference as for_codec;
use vecpack::transposed;

fn measure<F: FnMut() -> usize>(mut f: F, raw_bytes: usize) -> f64 {
    for _ in 0..5 {
        black_box(f());
    }
    let mut samples = Vec::new();
    for _ in 0..21 {
        let t = Instant::now();
        black_box(f());
        samples.push(t.elapsed().as_secs_f64());
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = samples[samples.len() / 2];
    raw_bytes as f64 / median / (1024.0 * 1024.0 * 1024.0)
}

fn main() {
    let n_blocks = 4096usize;
    let n = n_blocks * BLOCK;
    let raw = n * 4;
    let width = 10u32;
    let values: Vec<u32> = (0..n as u32).map(|i| i % 1000).collect();

    let mut scalar_packed = Vec::new();
    let mut tx_packed = Vec::new();
    for chunk in values.chunks(BLOCK) {
        bitpack::pack(chunk, width, &mut scalar_packed);
        transposed::pack_block(chunk, width, &mut tx_packed);
    }

    let scalar_dec = measure(
        || {
            let mut out: Vec<u32> = Vec::with_capacity(n);
            let plen = bitpack::packed_len(BLOCK, width);
            for i in 0..n_blocks {
                bitpack::unpack(&scalar_packed[i * plen..(i + 1) * plen], width, BLOCK, &mut out);
            }
            out.len()
        },
        raw,
    );
    let tx_dec = measure(
        || {
            let mut out: Vec<u32> = Vec::with_capacity(n);
            let plen = bitpack::packed_len(BLOCK, width);
            for i in 0..n_blocks {
                transposed::unpack_block(&tx_packed[i * plen..(i + 1) * plen], width, &mut out);
            }
            out.len()
        },
        raw,
    );

    // memcpy roofline: copy raw bytes into a destination buffer.
    let src: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut dst = vec![0u8; src.len()];
    let memcpy_gibps = measure(
        || {
            dst.copy_from_slice(black_box(&src));
            dst[0] as usize + dst.len()
        },
        raw,
    );

    // FOR end-to-end (now using transposed on full blocks).
    let encoded = for_codec::encode(&values);
    let for_dec = measure(
        || {
            let out: Vec<u32> = for_codec::decode(black_box(&encoded));
            out.len()
        },
        raw,
    );

    println!("n={n} values ({:.1} MiB raw), width={width}", raw as f64 / (1024.0 * 1024.0));
    println!("memcpy (roofline)     {memcpy_gibps:.2} GiB/s");
    println!("scalar unpack         {scalar_dec:.2} GiB/s");
    println!("transposed unpack     {tx_dec:.2} GiB/s   ({:.2}x vs scalar)", tx_dec / scalar_dec);
    println!("FOR decode (L3)       {for_dec:.2} GiB/s");
    println!(
        "transposed vs memcpy   {:.1}% of roofline",
        100.0 * tx_dec / memcpy_gibps
    );
}
