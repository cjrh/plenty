//! Byte reads and scalar text queries call narrow runtime helpers, reading a
//! borrowed place in place, with the dispatcher's results, bounds, and trap
//! sites (issue #364). `ord`, `bytes` literal reads, and queries of owned
//! locals and temporaries do too, without retaining the receiver (issue #379).
mod support;
use rstest::rstest;
use std::process::Command;
use support::assert_stdout as expect;

/// Names of the runtime symbols `function` calls in a compiled object.
fn runtime_calls(source: &str, function: &str) -> Vec<String> {
    use object::{Object, ObjectSection, ObjectSymbol, RelocationTarget};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("queries.o");
    plenty::compile_source_to_object(source, &path).unwrap();
    let bytes = std::fs::read(path).unwrap();
    let file = object::File::parse(bytes.as_slice()).unwrap();
    let suffix = format!("_fn_{function}");
    let symbol = file
        .symbols()
        .find(|s| s.name().is_ok_and(|name| name.ends_with(&suffix)))
        .unwrap();
    let section = file
        .section_by_index(symbol.section_index().unwrap())
        .unwrap();
    section
        .relocations()
        .filter(|(offset, _)| {
            *offset >= symbol.address() && *offset < symbol.address() + symbol.size()
        })
        .filter_map(|(_, relocation)| match relocation.target() {
            RelocationTarget::Symbol(index) => Some(
                file.symbol_by_index(index)
                    .unwrap()
                    .name()
                    .unwrap()
                    .to_owned(),
            ),
            _ => None,
        })
        // Caller context around Plenty calls is issue #366, not a query cost.
        .filter(|name| name.starts_with("plenty_"))
        .filter(|name| !name.starts_with("plenty_trap_") && !name.starts_with("plenty_caller_"))
        // Literal data, not a call.
        .filter(|name| !name.starts_with("plenty_str_"))
        .collect()
}

const QUERIES: &str = r#"
import std.bytes as by
from std.bytes import Bytes, BytesError

def count_spaces(data: &Bytes) -> i64:
    mut n = 0
    mut i = 0
    while i < data.len():
        if data[i] == 32u8:
            n += 1
        i += 1
    n

def bump(data: &mut Bytes, i: i64) -> ():
    data[i] = data[i] + 1u8

def sizes(s: &str) -> i64:
    len(s) * 100 + s.byte_len()

def ascii(s: &str) -> bool:
    s.isascii()

def at(s: &str, i: i64) -> str:
    s[i]

def commas(s: &str) -> i64:
    mut n = 0
    for scalar in s:
        if scalar == ",":
            n += 1
    n

def backwards(s: str) -> i64:
    mut n = 0
    for scalar in reversed(s):
        if scalar == ",":
            n += 1
    n

def main() -> Result[(), AllocError | IoError | BytesError]:
    mut data = by.from_text("a b c")?
    bump(&mut data, -1)
    heap = "a,b,c,é,🙂,long enough to live on the heap"
    print(count_spaces(&data), data[-1], sizes("héllo"), sizes(heap))?
    print(ascii("abc"), ascii(heap), at(heap, -1), at(heap, 8), at("ab", 0))?
    print(commas(heap), backwards(heap), commas(""), backwards(""))?
    Ok(())
"#;

#[test]
fn queries_return_the_dispatcher_results() {
    expect(QUERIES, "2 100 506 4145\nTrue False p 🙂 a\n5 5 0 0\n");
}

#[test]
fn borrowed_queries_call_only_narrow_helpers() {
    for (function, expected) in [
        ("count_spaces", &["plenty_bytes_item"][..]),
        ("bump", &["plenty_bytes_item", "plenty_bytes_set_item"][..]),
        ("sizes", &["plenty_text_len", "plenty_text_byte_len"][..]),
        ("ascii", &["plenty_text_is_ascii"][..]),
        ("at", &["plenty_text_at"][..]),
    ] {
        assert_eq!(runtime_calls(QUERIES, function), expected, "{function}");
    }
    // Iteration reads its hidden copy of the text through a reference, so the
    // cursor helpers replace the dispatcher; the loop still copies and
    // compares each inline scalar.
    for (function, step) in [
        ("commas", "plenty_text_next_byte"),
        ("backwards", "plenty_text_prev_byte"),
    ] {
        let calls = runtime_calls(QUERIES, function);
        for helper in ["plenty_text_byte_len", "plenty_text_at_byte", step] {
            assert!(
                calls.iter().any(|name| name == helper),
                "{function}: {calls:?}"
            );
        }
        assert!(
            !calls.iter().any(|name| name == "plenty_collection"),
            "{function}: {calls:?}"
        );
    }
}

#[test]
fn owned_and_temporary_receivers_keep_their_values() {
    expect(
        r#"
import std.bytes as by
from std.bytes import Bytes, BytesError

class Holder:
    name: str
    data: Bytes

def make(text: str) -> Bytes:
    by.from_text(text).unwrap()

def main() -> Result[(), AllocError | IoError | BytesError]:
    mut h = Holder(("ab" + "cdefghijkl")?, by.from_text("xyz")?)
    h.data[0] = 65u8
    alias = &h
    print(make("hi")[1], make("").len(), h.data[0], alias.data.len(), len(alias.name))?
    print(h.name[-1], alias.name.byte_len(), ("é" + "x")?[0], len(("é" + "x")?), "é".isascii())?
    names = ["short", "a much longer heap string"]?
    print(names[1][2], len(names[0]), names[1].byte_len())?
    Ok(())
"#,
        "105 0 65 3 12\nl 12 é 2 False\nm 5 25\n",
    );
}

#[test]
fn writable_references_answer_shared_queries() {
    expect(
        r#"
import std.bytes as by
from std.bytes import Bytes, BytesError

def grow(text: &mut str, data: &mut Bytes) -> i64:
    len(text) * 100 + text.byte_len() * 10 + data.len()

def main() -> Result[(), AllocError | IoError | BytesError]:
    mut text = "héllo"
    mut data = by.from_text("abc")?
    print(grow(&mut text, &mut data), text.isascii())?
    Ok(())
"#,
        "563 False\n",
    );
}

const OWNED: &str = r#"
def code(s: &str) -> Result[u32, ParseError]:
    ord(s)

def sizes(s: str) -> i64:
    n = len(s) * 1000 + s.byte_len() * 10
    if s.isascii():
        return n + 1
    n

def local_code(s: &str) -> Result[u32, ParseError]:
    t = (s + "").unwrap()
    ord(t)

def scalars(s: &str) -> Result[u64, ParseError]:
    mut total = 0u64
    for c in s:
        total += u64(ord(c)?)
    Ok(total)

def temporary(s: &str) -> Result[i64, AllocError]:
    Ok(s.upper()?.byte_len())

def literal(i: i64) -> u8:
    b"az\xff"[i]

def literal_sum() -> u64:
    mut total = 0u64
    for b in b"az\xff":
        total += u64(b)
    total + u64(b"az\xff".len())

def main() -> Result[(), AllocError | IoError | ParseError]:
    heap = "a heap string, long enough to be counted"
    for text in ["a", "é", "🙂", "", "ab", heap]?:
        print(str.repr(code(text))?, str.repr(local_code(text))?)?
    print(ord("x")?, ord(("é" + "")?)?, sizes(heap), sizes(("é" + heap)?))?
    print(scalars(heap)?, str.repr(scalars(""))?, temporary(heap)?)?
    print(literal(0), literal(-1), literal_sum(), heap)?
    Ok(())
"#;

#[test]
fn ord_literals_and_owned_receivers_return_the_dispatcher_results() {
    // The expected output is the dispatcher's, from the compiler before #379.
    // Under `runtime-checks`, exit also reports a leaked or doubly released
    // receiver.
    expect(
        OWNED,
        "Result[u32, ParseError].Ok(97) Result[u32, ParseError].Ok(97)\n\
         Result[u32, ParseError].Ok(233) Result[u32, ParseError].Ok(233)\n\
         Result[u32, ParseError].Ok(128578) Result[u32, ParseError].Ok(128578)\n\
         Result[u32, ParseError].Err(ParseError.Invalid) Result[u32, ParseError].Err(ParseError.Invalid)\n\
         Result[u32, ParseError].Err(ParseError.Invalid) Result[u32, ParseError].Err(ParseError.Invalid)\n\
         Result[u32, ParseError].Err(ParseError.Invalid) Result[u32, ParseError].Err(ParseError.Invalid)\n\
         120 233 40401 41420\n\
         3700 Result[u64, ParseError].Ok(0) 40\n\
         97 255 477 a heap string, long enough to be counted\n",
    );
}

#[test]
fn ord_literals_and_owned_receivers_skip_the_dispatcher_and_retain() {
    for (function, expected) in [
        ("code", &["plenty_text_ord"][..]),
        ("literal", &["plenty_byte_str_item"][..]),
        // Sorted. The releases are the parameter's drop on each return path.
        (
            "sizes",
            &[
                "plenty_release",
                "plenty_release",
                "plenty_text_byte_len",
                "plenty_text_is_ascii",
                "plenty_text_len",
            ][..],
        ),
        (
            "literal_sum",
            &[
                "plenty_byte_str_item",
                "plenty_text_byte_len",
                "plenty_text_byte_len",
            ][..],
        ),
    ] {
        let mut calls = runtime_calls(OWNED, function);
        calls.sort();
        assert_eq!(calls, expected, "{function}");
    }
    // A local, an iteration scalar, and a full-expression temporary are read
    // in their slots: the only retain left is of the borrowed argument, which
    // `+`, `upper`, and the loop's hidden copy consume.
    for (function, helper) in [
        ("local_code", "plenty_text_ord"),
        ("scalars", "plenty_text_ord"),
        ("temporary", "plenty_text_byte_len"),
    ] {
        let calls = runtime_calls(OWNED, function);
        assert!(
            calls.iter().any(|name| name == helper),
            "{function}: {calls:?}"
        );
        let retains = calls.iter().filter(|name| *name == "plenty_retain").count();
        assert_eq!(retains, 1, "{function}: {calls:?}");
        if function == "scalars" {
            assert!(
                !calls.iter().any(|name| name == "plenty_collection"),
                "{function}: {calls:?}"
            );
        }
    }
}

const PUSHES: &str = r#"
import std.bytes as by
from std.bytes import Bytes, BytesError

class Holder:
    data: Bytes

def fill(data: &mut Bytes, count: i64) -> Result[(), BytesError]:
    for i in range(count):
        data.push(u8.try_from(i % 256).unwrap())?
    Ok(())

def total(data: &Bytes) -> i64:
    mut sum = 0
    for i in range(data.len()):
        sum += i64(data[i])
    sum

def main() -> Result[(), AllocError | IoError | BytesError]:
    mut local = by.empty()?
    fill(&mut local, 1000)?
    mut held = Holder(by.with_capacity(2)?)
    held.data.push(7u8)?
    held.data.push(8u8)?
    held.data.push(9u8)?
    mut items = [by.from_text("ab")?]?
    items[0].push(99u8)?
    print(local.len(), total(&local), local[255], local[256], local[-1], local.capacity() >= 1000)?
    print(held.data.len(), held.data[2], items[0].len(), items[0][2])?
    Ok(())
"#;

// Issue #389: `Bytes.push` appended through a retained copy of the storage
// owner and the dispatcher; it now calls one helper on the borrowed field.
#[test]
fn push_grows_buffers_in_every_place() {
    expect(PUSHES, "1000 124716 255 0 231 True\n3 9 3 99\n");
}

#[test]
fn push_calls_only_its_narrow_helper() {
    assert_eq!(
        runtime_calls(PUSHES, "__plenty_class_std.bytes.Bytes.push"),
        ["plenty_bytes_push"]
    );
}

// Failure leaves the buffer unchanged and retrying succeeds; exit reports a
// leaked or doubly released owner.
#[cfg(feature = "runtime-checks")]
#[test]
fn push_growth_failure_is_reported_and_retried() {
    support::assert_visible_stdout(
        r#"
import std.bytes as by
from std.bytes import Bytes, BytesError

class Holder:
    data: Bytes

def main() -> Result[(), AllocError | IoError | BytesError]:
    mut held = Holder(by.with_capacity(2)?)
    held.data.push(1u8)?
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    within = held.data.push(2u8)
    print("__test_end_no_allocations__")?
    grown = held.data.push(3u8)
    print("__test_restore_allocations__")?
    print(str.repr(within)?, str.repr(grown)?, held.data.len(), held.data.capacity())?
    held.data.push(3u8)?
    print(held.data.len(), held.data[0], held.data[1], held.data[2])?
    Ok(())
"#,
        "Result[(), std.bytes.BytesError].Ok(()) \
         Result[(), std.bytes.BytesError].Err(std.bytes.BytesError.Allocation(AllocError.OutOfMemory)) 2 2\n\
         3 1 2 3\n",
    );
}

fn trap(body: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let source = format!(
        "import std.bytes as by\nfrom std.bytes import Bytes, BytesError\ndef read(data: &Bytes, i: i64) -> u8:\n    data[i]\ndef text(s: &str, i: i64) -> str:\n    s[i]\ndef main() -> Result[(), AllocError | IoError | BytesError]:\n{body}    Ok(())\n"
    );
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let program = dir.path().join("program");
    plenty::compile_file_to_executable(&dir.path().join("main.plenty"), &program, Some(dir.path()))
        .unwrap();
    let output = Command::new(&program).output().unwrap();
    assert_eq!(output.status.code(), Some(1), "{:?}", output.status);
    String::from_utf8(output.stderr).unwrap()
}

#[rstest]
#[case::borrowed_byte(
    "    data = by.from_text(\"ab\")?\n    print(read(&data, 2))?\n",
    "4:5: index out of bounds: index 2, length 2"
)]
#[case::negative_byte(
    "    data = by.from_text(\"ab\")?\n    print(read(&data, -3))?\n",
    "4:5: index out of bounds: index -3, length 2"
)]
#[case::empty_byte(
    "    data = by.from_text(\"\")?\n    print(data[0])?\n",
    "9:11: index out of bounds: index 0, length 0"
)]
#[case::temporary_byte(
    "    print(by.from_text(\"ab\")?[5])?\n",
    "8:29: index out of bounds: index 5, length 2"
)]
#[case::byte_assignment(
    "    mut data = by.from_text(\"ab\")?\n    data[-3] = 1u8\n",
    "9:5: index out of bounds: index -3, length 2"
)]
#[case::borrowed_text(
    "    print(text(\"héllo\", 5))?\n",
    "6:5: index out of bounds: index 5, length 5"
)]
#[case::heap_text(
    "    word = \"a heap string, long enough\"\n    print(text(&word, -27))?\n",
    "6:5: index out of bounds: index -27, length 26"
)]
#[case::empty_text(
    "    word = \"\"\n    print(word[0])?\n",
    "9:11: index out of bounds: index 0, length 0"
)]
#[case::byte_literal(
    "    i = -3\n    print(b\"ab\"[i])?\n",
    "9:11: index out of bounds: index -3, length 2"
)]
fn bounds_traps_keep_their_sites(#[case] body: &str, #[case] expected: &str) {
    assert_eq!(trap(body), format!("error: main.plenty:{expected}\n"));
}
