// The Rust equivalents of borrowed-K-N.plenty: a map with owned keys looked
// up by a borrowed range of a byte buffer; the owned key is built on a miss.
// `str`: HashMap<String, i64> by &str. Safe Rust must check the range as
// UTF-8 before the lookup, on every pass.
// `bytes`: HashMap<Vec<u8>, i64> by &[u8], with no validation at all.
// Build: rustc -C opt-level=3 borrowed.rs; run: borrowed str|bytes K PASSES
use std::collections::HashMap;
use std::hint::black_box;

fn count_str(counts: &mut HashMap<String, i64>, bytes: &[u8], by: i64) {
    let key = std::str::from_utf8(bytes).unwrap();
    match counts.get_mut(key) {
        Some(count) => *count += by,
        None => {
            counts.insert(key.to_owned(), by);
        }
    }
}

fn count_bytes(counts: &mut HashMap<Vec<u8>, i64>, key: &[u8], by: i64) {
    match counts.get_mut(key) {
        Some(count) => *count += by,
        None => {
            counts.insert(key.to_owned(), by);
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let form = args.next().unwrap();
    let k: usize = args.next().unwrap().parse().unwrap();
    let passes: usize = args.next().unwrap().parse().unwrap();
    let sequence = b"GGTATTTTAATTTATAGTACGTACGTAACCGGTT".to_vec();
    if form == "str" {
        let mut counts: HashMap<String, i64> = HashMap::new();
        for start in 0..4 {
            count_str(&mut counts, &sequence[start..start + k], 0);
        }
        for _ in 0..passes {
            let sequence = black_box(&sequence);
            count_str(&mut counts, &sequence[1..1 + k], 1);
        }
        println!("{}", counts[std::str::from_utf8(&sequence[1..1 + k]).unwrap()]);
    } else {
        let mut counts: HashMap<Vec<u8>, i64> = HashMap::new();
        for start in 0..4 {
            count_bytes(&mut counts, &sequence[start..start + k], 0);
        }
        for _ in 0..passes {
            let sequence = black_box(&sequence);
            count_bytes(&mut counts, &sequence[1..1 + k], 1);
        }
        println!("{}", counts[&sequence[1..1 + k]]);
    }
}
