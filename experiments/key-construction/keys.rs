// The same work as keys-K-N.plenty: an owned key per pass from validated
// bytes. Keys of at most 7 bytes are packed into a word, as Plenty's inline
// strings are; longer keys are copied to the heap.
use std::hint::black_box;

enum Key {
    Inline(u64),
    Heap(String),
}

#[inline(never)]
fn key(sequence: &[u8], start: usize, stop: usize) -> Option<Key> {
    let bytes = &sequence[start..stop];
    let text = std::str::from_utf8(bytes).ok()?;
    if bytes.len() <= 7 {
        let mut word = [0u8; 8];
        word[0] = (bytes.len() as u8) << 1 | 1;
        word[1..=bytes.len()].copy_from_slice(bytes);
        Some(Key::Inline(u64::from_le_bytes(word)))
    } else {
        Some(Key::Heap(text.to_owned()))
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let k: usize = args.next().unwrap().parse().unwrap();
    let passes: usize = args.next().unwrap().parse().unwrap();
    let sequence = black_box(b"GGTATTTTAATTTATAGTACGTACGTAACCGGTT".to_vec());
    let span = sequence.len() - k + 1;
    let (mut total, mut start) = (0usize, 0usize);
    for _ in 0..passes {
        total += match black_box(key(&sequence, start, start + k)).unwrap() {
            Key::Inline(word) => (word as usize & 0xff) >> 1,
            Key::Heap(text) => text.len(),
        };
        start += 1;
        if start == span {
            start = 0;
        }
    }
    println!("{total}");
}
