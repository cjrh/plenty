//! Bulk scans over byte slices behind `std.bytes`. Each is one runtime call
//! over a validated range, so a parser's inner loop costs one call per token
//! rather than one per byte. The word-at-a-time and subsequence searches are
//! written out here rather than taken from a third-party crate.

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

/// The first index of `needle` in `haystack`, in time linear in both lengths
/// and without allocation (issue #474). Short searches stay cheap: the
/// word-at-a-time `find` jumps to each occurrence of the needle's first byte
/// and the rest is compared. That costs up to `needle.len()` per candidate,
/// so once the comparisons outgrow a budget proportional to the bytes passed,
/// the search continues with `two_way`.
pub(super) fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    match needle.len() {
        0 => Some(0),
        1 => find(haystack, needle[0]),
        _ => match first_byte_search(haystack, needle) {
            Ok(found) => found,
            Err(from) => two_way(&haystack[from..], needle).map(|index| from + index),
        },
    }
}

/// Comparison bytes allowed per haystack byte passed, and in advance.
const BUDGET_PER_BYTE: usize = 8;
const BUDGET_SLACK: usize = 256;

/// The first-byte search, or `Err(index)` when its comparisons would exceed
/// the budget at candidate `index`; no match starts before `index`.
fn first_byte_search(haystack: &[u8], needle: &[u8]) -> Result<Option<usize>, usize> {
    let (first, rest) = needle.split_first().expect("a needle of two or more bytes");
    let Some(last) = haystack.len().checked_sub(needle.len()) else {
        return Ok(None);
    };
    let mut spent = 0usize;
    let mut from = 0;
    while from <= last {
        let Some(offset) = find(&haystack[from..=last], *first) else {
            return Ok(None);
        };
        let index = from + offset;
        spent += rest.len();
        if spent > BUDGET_SLACK + BUDGET_PER_BYTE * index {
            return Err(index);
        }
        if haystack[index + 1..index + needle.len()] == *rest {
            return Ok(Some(index));
        }
        from = index + 1;
    }
    Ok(None)
}

/// The Crochemore–Perrin two-way search, as in `core`'s `str` search, for a
/// needle of two or more bytes: split the needle at a critical position,
/// compare the right part forwards and then the left part backwards, and
/// shift by an amount that never skips a match. Where nothing of the needle
/// is known to match, `find` skips to the next occurrence of its first byte.
fn two_way(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let length = needle.len();
    let last = haystack.len().checked_sub(length)?;
    let (critical, mut period) = {
        let less = maximal_suffix(needle, false);
        let greater = maximal_suffix(needle, true);
        if less.0 > greater.0 { less } else { greater }
    };
    // A periodic needle keeps `memory`, the length of its prefix known to
    // match after a shift by `period`. Otherwise the period is replaced by a
    // safe lower bound and nothing is remembered.
    let periodic = needle[..critical] == needle[period..period + critical];
    if !periodic {
        period = critical.max(length - critical) + 1;
    }
    let mut position = 0;
    let mut memory = 0;
    'search: while position <= last {
        if memory == 0 && haystack[position] != needle[0] {
            position += find(&haystack[position..=last], needle[0])?;
        }
        let window = &haystack[position..position + length];
        for index in critical.max(memory)..length {
            if needle[index] != window[index] {
                position += index - critical + 1;
                memory = 0;
                continue 'search;
            }
        }
        for index in (memory..critical).rev() {
            if needle[index] != window[index] {
                position += period;
                if periodic {
                    memory = length - period;
                }
                continue 'search;
            }
        }
        return Some(position);
    }
    None
}

/// The start and period of the maximal suffix of `needle` under byte order,
/// or reversed order when `greater`. The later of the two starts is a
/// critical position.
fn maximal_suffix(needle: &[u8], greater: bool) -> (usize, usize) {
    let (mut start, mut candidate, mut offset, mut period) = (0, 1, 0, 1);
    while let Some(&next) = needle.get(candidate + offset) {
        let current = needle[start + offset];
        if (next < current) != greater && next != current {
            candidate += offset + 1;
            offset = 0;
            period = candidate - start;
        } else if next == current {
            if offset + 1 == period {
                candidate += offset + 1;
                offset = 0;
            } else {
                offset += 1;
            }
        } else {
            start = candidate;
            candidate += 1;
            offset = 0;
            period = 1;
        }
    }
    (start, period)
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

    fn slow_find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        (0..=haystack.len().checked_sub(needle.len())?)
            .find(|start| haystack[*start..].starts_with(needle))
    }

    /// Every needle up to seven bytes and haystack up to eleven bytes over a
    /// two-letter alphabet, where periodic needles and repeated partial
    /// matches are common; then longer mixed needles over three letters.
    #[test]
    fn subsequence_search_agrees_with_the_naive_search() {
        fn words(alphabet: &[u8], max: usize) -> Vec<Vec<u8>> {
            let mut all = vec![vec![]];
            let mut from = 0;
            for _ in 0..max {
                let to = all.len();
                for index in from..to {
                    for letter in alphabet {
                        let mut word = all[index].clone();
                        word.push(*letter);
                        all.push(word);
                    }
                }
                from = to;
            }
            all
        }
        // Inputs this small never spend the first-byte budget, so `two_way`
        // is checked directly as well.
        let needles = words(b"ab", 7);
        for haystack in words(b"ab", 11) {
            for needle in &needles {
                let expected = slow_find_bytes(&haystack, needle);
                assert_eq!(find_bytes(&haystack, needle), expected);
                if needle.len() >= 2 {
                    assert_eq!(
                        two_way(&haystack, needle),
                        expected,
                        "{haystack:?} {needle:?}"
                    );
                }
            }
        }
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut letter = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            b"abc"[(state % 3) as usize]
        };
        for _ in 0..2000 {
            let haystack: Vec<u8> = (0..200).map(|_| letter()).collect();
            let needle: Vec<u8> = (0..1 + letter() as usize % 16).map(|_| letter()).collect();
            for start in [0, 50, 150] {
                let haystack = &haystack[start..];
                let expected = slow_find_bytes(haystack, &needle);
                assert_eq!(find_bytes(haystack, &needle), expected);
                if needle.len() >= 2 {
                    assert_eq!(
                        two_way(haystack, &needle),
                        expected,
                        "{haystack:?} {needle:?}"
                    );
                }
            }
        }
    }

    /// A first-byte scan followed by comparison does about 2^37 byte
    /// comparisons on these inputs (issue #474); the linear search does a
    /// few million, so this finishes in milliseconds instead of hours.
    #[test]
    fn subsequence_search_is_linear_on_repetitive_input() {
        let haystack = vec![b'a'; 1 << 22];
        let mut needle = vec![b'a'; 1 << 15];
        needle.push(b'b');
        assert_eq!(find_bytes(&haystack, &needle), None);
        needle[0] = b'b';
        needle.pop();
        assert_eq!(find_bytes(&haystack, &needle), None);
        let mut haystack = b"ab".repeat(1 << 21);
        haystack.extend_from_slice(b"abb");
        let mut needle = b"ab".repeat(1 << 14);
        needle.push(b'b');
        assert_eq!(
            find_bytes(&haystack, &needle),
            Some(haystack.len() - needle.len())
        );
    }

    /// A needle of 33 `a` bytes then `b` in a run of `a` spends the budget at
    /// candidate 9 (33 per candidate against 256 + 8 per byte passed). A
    /// match before, at, and after that point is found, and so is a match
    /// that begins before the switch but ends after it.
    #[test]
    fn subsequence_search_switches_to_two_way_without_losing_matches() {
        let mut needle = vec![b'a'; 33];
        needle.push(b'b');
        let switch = (0..)
            .find(|index| 33 * (index + 1) > BUDGET_SLACK + BUDGET_PER_BYTE * index)
            .unwrap();
        assert_eq!(switch, 9);
        assert_eq!(first_byte_search(&[b'a'; 200], &needle), Err(switch));
        for end in needle.len()..200 {
            let mut haystack = vec![b'a'; 200];
            haystack[end - 1] = b'b';
            let expected = slow_find_bytes(&haystack, &needle);
            assert_eq!(find_bytes(&haystack, &needle), expected, "{end}");
            let naive = first_byte_search(&haystack, &needle);
            assert!(naive == Ok(expected) || naive == Err(switch), "{end}");
        }
        // Short needles compare at most 8 bytes per candidate: never spent.
        let mut short = vec![b'a'; 8];
        short.push(b'b');
        assert_eq!(first_byte_search(&[b'a'; 4096], &short), Ok(None));
        let header = b"Host: example.com\r\nAccept: */*\r\n\r\n";
        assert_eq!(first_byte_search(header, b"\r\n\r\n"), Ok(Some(30)));
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
