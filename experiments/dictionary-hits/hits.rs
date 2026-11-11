// The Rust equivalent of hits-K-N.plenty: std HashMap (SipHash-1-3, SwissTable)
// with a borrowed key, so no allocation in the loop.
// Build: rustc -C opt-level=3 hits.rs; run: hits K PASSES
use std::collections::HashMap;
use std::hint::black_box;

fn main() {
    let mut args = std::env::args().skip(1);
    let k: usize = args.next().unwrap().parse().unwrap();
    let passes: usize = args.next().unwrap().parse().unwrap();
    let key = &"GGTATTTTAATTTATAGT"[..k];
    let mut counts: HashMap<&str, i64> = HashMap::new();
    counts.insert(key, 0);
    counts.insert("ACG", 0);
    counts.insert("TTTATAGTGGTATTTTAA", 0);
    for _ in 0..passes {
        *counts.entry(black_box(key)).or_insert(0) += 1;
    }
    println!("{}", counts[key]);
}
