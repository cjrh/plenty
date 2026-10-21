//! Bulk scans over byte slices behind `std.bytes`. Each is one runtime call
//! over a validated range, so a parser's inner loop costs one call per token
//! rather than one per byte. The runtime has no dependencies, so the word-at-
//! a-time search is written out here.

const LOW: u64 = 0x0101_0101_0101_0101;
const HIGH: u64 = 0x8080_8080_8080_8080;

/// Nonzero exactly when some byte of `word` is zero. Bits above the first
/// zero byte may be spurious, so only "any" and the lowest set bit are used.
fn zero_byte(word: u64) -> u64 {
    word.wrapping_sub(LOW) & !word & HIGH
}

/// The first index holding any of `needles` (at most four), eight bytes per
/// step. Words are read little-endian so the lowest set bit is the first hit.
fn find_few(haystack: &[u8], needles: &[u8]) -> Option<usize> {
    let mut patterns = [0u64; 4];
    for (pattern, needle) in patterns.iter_mut().zip(needles) {
        *pattern = LOW * *needle as u64;
    }
    let patterns = &patterns[..needles.len()];
    let (words, tail) = haystack.as_chunks::<8>();
    let mut offset = 0;
    for chunk in words {
        let word = u64::from_le_bytes(*chunk);
        let mut hits = 0;
        for pattern in patterns {
            hits |= zero_byte(word ^ pattern);
        }
        if hits != 0 {
            return Some(offset + hits.trailing_zeros() as usize / 8);
        }
        offset += 8;
    }
    tail.iter()
        .position(|byte| needles.contains(byte))
        .map(|index| offset + index)
}

pub(super) fn find(haystack: &[u8], needle: u8) -> Option<usize> {
    find_few(haystack, &[needle])
}

/// The first byte that is (`member`) or is not (`!member`) in `set`.
pub(super) fn find_in(haystack: &[u8], set: &[u8], member: bool) -> Option<usize> {
    if member && set.len() <= 4 {
        return find_few(haystack, set);
    }
    let mut table = [!member; 256];
    for byte in set {
        table[*byte as usize] = member;
    }
    haystack.iter().position(|byte| table[*byte as usize])
}

/// The scalar position in UTF-8 `text` of the first scalar that is
/// (`member`) or is not (`!member`) an ASCII byte of `set` (issue #383).
/// Set bytes from 0x80 match nothing, so no byte of a multi-byte scalar is a
/// member: a member search skips them, and a non-member search stops at the
/// lead byte.
pub(super) fn text_find_in(text: &[u8], set: &[u8], member: bool) -> Option<usize> {
    let index = if set.is_ascii() {
        find_in(text, set, member)?
    } else {
        let (mut ascii, mut seen, mut len) = ([0u8; 128], [false; 128], 0);
        for &byte in set.iter().filter(|byte| byte.is_ascii()) {
            if !seen[byte as usize] {
                seen[byte as usize] = true;
                ascii[len] = byte;
                len += 1;
            }
        }
        find_in(text, &ascii[..len], member)?
    };
    // Before a non-member every byte is an ASCII member, one scalar each.
    Some(match member {
        true => text[..index]
            .iter()
            .filter(|byte| (**byte as i8) >= -0x40)
            .count(),
        false => index,
    })
}

/// Searches by first byte, then compares: at most `haystack.len()` candidate
/// positions of `needle.len()` bytes each.
pub(super) fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let Some((first, rest)) = needle.split_first() else {
        return Some(0);
    };
    let last = haystack.len().checked_sub(needle.len())?;
    let mut from = 0;
    while from <= last {
        let index = from + find(&haystack[from..=last], *first)?;
        if haystack[index + 1..index + needle.len()] == *rest {
            return Some(index);
        }
        from = index + 1;
    }
    None
}

pub(super) fn count(haystack: &[u8], needle: u8) -> usize {
    haystack.iter().filter(|byte| **byte == needle).count()
}

/// Length of the leading run of bytes below 0x80.
pub(super) fn ascii_run(haystack: &[u8]) -> usize {
    let (words, tail) = haystack.as_chunks::<8>();
    let mut offset = 0;
    for chunk in words {
        let high = u64::from_le_bytes(*chunk) & HIGH;
        if high != 0 {
            return offset + high.trailing_zeros() as usize / 8;
        }
        offset += 8;
    }
    offset
        + tail
            .iter()
            .position(|byte| *byte >= 0x80)
            .unwrap_or(tail.len())
}

/// Length of the longest prefix that is complete, valid UTF-8.
pub(super) fn utf8_run(haystack: &[u8]) -> usize {
    match std::str::from_utf8(haystack) {
        Ok(_) => haystack.len(),
        Err(error) => error.valid_up_to(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slow_find_in(haystack: &[u8], set: &[u8], member: bool) -> Option<usize> {
        haystack
            .iter()
            .position(|byte| set.contains(byte) == member)
    }

    /// Every alignment and length around the eight-byte step, with the hit at
    /// every position, including bytes that differ from a needle by one bit
    /// or by the borrow the zero-byte test propagates.
    #[test]
    fn word_search_agrees_with_the_byte_loop_at_every_offset() {
        let fillers = [0u8, 1, 0x7f, 0x80, 0xff, b'a'];
        let sets: [&[u8]; 6] = [
            b"",
            b",",
            b",\"",
            b",\"\n\r",
            &[0, 0x80, 0xff],
            b",\"\n\r\t ",
        ];
        for length in 0..40 {
            for filler in fillers {
                for set in sets {
                    for member in [true, false] {
                        let mut bytes = vec![filler; length];
                        assert_eq!(
                            find_in(&bytes, set, member),
                            slow_find_in(&bytes, set, member)
                        );
                        for hit in 0..length {
                            for value in [b',', b'\r', 0, 0x80, 0xff, b'-', 0x2d ^ 0x80] {
                                bytes[hit] = value;
                                assert_eq!(
                                    find_in(&bytes, set, member),
                                    slow_find_in(&bytes, set, member),
                                    "{bytes:?} {set:?} {member}"
                                );
                                bytes[hit] = filler;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Text scans agree with a per-scalar model for every byte value in the
    /// set, ASCII and non-ASCII scalars at every position, and both senses.
    #[test]
    fn text_scans_report_scalar_positions_and_never_match_non_ascii() {
        fn model(text: &str, set: &[u8], member: bool) -> Option<usize> {
            text.chars()
                .position(|scalar| (scalar.is_ascii() && set.contains(&(scalar as u8))) == member)
        }
        let scalars = [
            "a", "-", ",", "\u{0}", "\u{7f}", "é", "\u{80}", "ÿ", "€", "😀",
        ];
        let sets: [&[u8]; 7] = [
            b"",
            b",",
            b"a-",
            b"a-,\x7f",
            &[0xc3, 0xa9, 0x80, 0xff, b'a'],
            &[0, b'a', b'a', 0xe2, b'-'],
            b"abcdefghijklmnopqrstuvwxyz-,",
        ];
        for length in 0..12 {
            for filler in ["a", "é"] {
                for hit in 0..length.max(1) {
                    for value in scalars {
                        let mut text = String::new();
                        for at in 0..length {
                            text.push_str(if at == hit { value } else { filler });
                        }
                        for set in sets {
                            for member in [true, false] {
                                assert_eq!(
                                    text_find_in(text.as_bytes(), set, member),
                                    model(&text, set, member),
                                    "{text:?} {set:?} {member}"
                                );
                            }
                        }
                    }
                }
            }
        }
        for byte in 0..=255u8 {
            let set = [byte];
            let text = "\u{0}\u{7f}é\u{ff}";
            assert_eq!(
                text_find_in(text.as_bytes(), &set, true),
                model(text, &set, true)
            );
        }
    }

    #[test]
    fn a_byte_one_above_the_needle_after_a_match_is_not_reported_first() {
        // 0x01 after 0x00 borrows in the zero-byte test; the lowest hit wins.
        assert_eq!(find(&[9, 9, 0, 1, 9, 9, 9, 9], 0), Some(2));
        assert_eq!(find(&[9, 9, 9, 1, 9, 9, 9, 9], 0), None);
        assert_eq!(find(&[b'-', b',', b'-', 9, 9, 9, 9, 9], b','), Some(1));
        assert_eq!(find(&[b'-'; 16], b','), None);
    }

    #[test]
    fn subsequence_search_handles_overlap_edges_and_empty_needles() {
        assert_eq!(find_bytes(b"aaab", b"aab"), Some(1));
        assert_eq!(find_bytes(b"abcabd", b"abd"), Some(3));
        assert_eq!(find_bytes(b"abc", b""), Some(0));
        assert_eq!(find_bytes(b"", b""), Some(0));
        assert_eq!(find_bytes(b"ab", b"abc"), None);
        assert_eq!(find_bytes(b"abc", b"abc"), Some(0));
        assert_eq!(find_bytes(b"xxabc", b"abc"), Some(2));
        assert_eq!(find_bytes(b"abab", b"bb"), None);
        assert_eq!(find_bytes(b"line\r\nnext", b"\r\n"), Some(4));
        let long = [b"a".repeat(100), b"b".to_vec()].concat();
        assert_eq!(find_bytes(&long, b"aaab"), Some(97));
    }

    #[test]
    fn counts_and_runs() {
        assert_eq!(count(b"a\nb\n\nc", b'\n'), 3);
        assert_eq!(count(b"", 0), 0);
        assert_eq!(count(&[0xff; 300], 0xff), 300);
        for length in 0..40 {
            let mut bytes = vec![b'a'; length];
            assert_eq!(ascii_run(&bytes), length);
            for hit in 0..length {
                bytes[hit] = 0x80;
                assert_eq!(ascii_run(&bytes), hit);
                bytes[hit] = b'a';
            }
        }
        assert_eq!(utf8_run("aé".as_bytes()), 3);
        assert_eq!(utf8_run(&[b'a', 0xc3]), 1);
        assert_eq!(utf8_run(&[b'a', 0xff, b'b']), 1);
        assert_eq!(utf8_run(&[0xed, 0xa0, 0x80]), 0);
        assert_eq!(utf8_run(b""), 0);
    }
}
