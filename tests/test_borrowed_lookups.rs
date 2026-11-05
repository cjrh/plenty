//! Lookups and byte accesses read a counted copy that its local, reference,
//! or field still holds in place: no `plenty_retain` before the runtime call
//! and no `plenty_release` after it (issue #744).
mod support;
use crate::test_text_bytes_fast_paths::runtime_calls;
use support::assert_stdout as expect;

const LOOKUPS: &str = r#"
from std.bytes import Bytes, from_text

def got(d: &dict[str, i64], k: str) -> Option[i64]:
    d.get(&k)

def bumped(d: &mut dict[str, i64], k: str) -> i64:
    match d.get_mut(&k):
        case Some(v):
            unchecked:
                *v += 1
            *v
        case Nothing:
            0

def indexed(d: &dict[str, i64], k: str) -> i64:
    d[k]

def in_dict(d: &dict[str, i64], k: str) -> bool:
    k in d

def in_set(s: &set[str], k: str) -> bool:
    k in s

def in_list(xs: &list[str], k: str) -> bool:
    k in xs

def found(xs: &list[str], k: str) -> Option[i64]:
    xs.find(&k)

def counted(xs: &list[str], k: str) -> i64:
    xs.count(&k)

def through(d: &dict[str, i64], k: &str) -> Option[i64]:
    d.get(k)

def popped(d: &mut dict[str, i64], k: str) -> Option[i64]:
    d.pop(&k)

def byte(data: &Bytes, i: i64) -> u8:
    data.get(i)

def lower(data: &mut Bytes, i: i64) -> ():
    data.set(i, data.get(i) | 32u8)

def count_words(words: &list[str]) -> Result[dict[str, i64], AllocError]:
    mut counts: dict[str, i64] = {}?
    for word in words:
        key = copy(word)?
        match counts.get_mut(&key):
            case Some(count):
                unchecked:
                    *count += 1
            case Nothing:
                counts.insert(key, 1)?
    Ok(counts)

def main() -> Result[(), AllocError | IoError]:
    heap = ("a key long enough to live on the heap, " + "a")?
    mut d: dict[str, i64] = {"a": 1, copy(heap)?: 2}?
    s = {"a", copy(heap)?}?
    xs = ["a", copy(heap)?, "a"]?
    print(got(&d, copy(heap)?), got(&d, "b"), bumped(&mut d, copy(heap)?), bumped(&mut d, "b"))?
    print(indexed(&d, copy(heap)?), in_dict(&d, "a"), in_set(&s, copy(heap)?), in_set(&s, "b"))?
    print(in_list(&xs, copy(heap)?), found(&xs, "a"), found(&xs, "b"), counted(&xs, "a"))?
    print(through(&d, &heap), popped(&mut d, copy(heap)?), popped(&mut d, copy(heap)?), len(d))?
    mut data = from_text("AC")?
    lower(&mut data, 1)
    print(byte(&data, 0), byte(&data, 1))?
    counts = count_words(&["x", copy(heap)?, "x", "y", copy(heap)?, "x"]?)?
    print(counts.get(&"x"), counts.get(&heap), counts.get(&"y"), len(counts), heap)?
    Ok(())
"#;

#[test]
fn borrowed_lookups_return_the_same_results() {
    // Under `runtime-checks`, exit also reports a leaked or doubly released
    // operand.
    expect(
        LOOKUPS,
        "Option[i64].Some(2) Option[i64].Nothing 3 0\n\
         3 True True False\n\
         True Option[i64].Some(0) Option[i64].Nothing 2\n\
         Option[i64].Some(3) Option[i64].Some(3) Option[i64].Nothing 1\n\
         65 99\n\
         Option[i64].Some(3) Option[i64].Some(2) Option[i64].Some(1) 3 a key long enough to live on the heap, a\n",
    );
}

#[test]
fn borrowed_lookups_skip_retain_and_release() {
    // Each owned `k` parameter is released once when it is dropped; the
    // lookup itself counts nothing.
    for (function, releases) in [
        ("got", 1),
        ("bumped", 1),
        ("indexed", 1),
        ("in_dict", 1),
        ("in_set", 1),
        ("in_list", 1),
        ("found", 1),
        ("counted", 1),
        ("through", 0),
        ("byte", 0),
        ("lower", 0),
        ("__plenty_class_std.bytes.Bytes.get", 0),
        ("__plenty_class_std.bytes.Bytes.set", 0),
    ] {
        let calls = runtime_calls(LOOKUPS, function);
        assert!(
            !calls.iter().any(|name| name == "plenty_retain"),
            "{function}: {calls:?}"
        );
        let released = calls
            .iter()
            .filter(|name| *name == "plenty_release")
            .count();
        assert_eq!(released, releases, "{function}: {calls:?}");
    }
    // The k-nucleotide loop: only the key's own release on a hit remains.
    let calls = runtime_calls(LOOKUPS, "count_words");
    let lookup = calls
        .iter()
        .position(|name| name == "plenty_op_113")
        .unwrap();
    assert_ne!(
        calls.get(lookup.wrapping_sub(1)).map(String::as_str),
        Some("plenty_retain"),
        "count_words: {calls:?}"
    );
    assert_ne!(
        calls.get(lookup + 1).map(String::as_str),
        Some("plenty_release"),
        "count_words: {calls:?}"
    );
    // `pop` releases a stored entry, which may be the key's source, so its
    // key copy keeps its count.
    let calls = runtime_calls(LOOKUPS, "popped");
    assert!(
        calls.iter().any(|name| name == "plenty_retain"),
        "popped: {calls:?}"
    );
}
