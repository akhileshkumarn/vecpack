//! L4: cascade ratio vs each leaf on a mixed-block column.
//!
//!   cargo run --release --example l4_cascade

use vecpack::cascade;
use vecpack::Scheme;

fn main() {
    let n = 4_000_000usize;
    let mut mixed = Vec::with_capacity(n);
    let mut x = 0xC0FFEE_u64;
    let mut next = || {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    while mixed.len() < n {
        let kind = mixed.len() / 1024 % 4;
        let take = (n - mixed.len()).min(1024);
        match kind {
            0 => {
                let base = 1_600_000_000u32.wrapping_add(mixed.len() as u32);
                mixed.extend((0..take as u32).map(|i| base.wrapping_add(i)));
            }
            1 => mixed.extend(std::iter::repeat((next() % 4) as u32).take(take)),
            2 => mixed.extend((0..take).map(|_| (next() % 1000) as u32)),
            _ => mixed.extend((0..take).map(|_| next() as u32)),
        }
    }
    let raw = mixed.len() * 4;
    let casc = cascade::encode(&mixed);
    let hist = cascade::tag_histogram(&casc);
    println!("mixed_blocks  raw={:.1} MiB", raw as f64 / (1024.0 * 1024.0));
    println!(
        "cascade       {:.2}x   tags for/delta/rle/dict/raw = {}/{}/{}/{}/{}",
        raw as f64 / casc.len() as f64,
        hist[0],
        hist[1],
        hist[2],
        hist[3],
        hist[4]
    );
    for scheme in Scheme::ALL {
        let enc = scheme.encode(&mixed);
        println!("{:<12}  {:.2}x", scheme.name(), raw as f64 / enc.len() as f64);
    }
    let dec: Vec<u32> = cascade::decode(&casc);
    assert_eq!(dec, mixed);
}
