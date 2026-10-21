//! Byte reads and scalar text queries call narrow runtime helpers, reading a
//! borrowed place in place, with the dispatcher's results, bounds, and trap
//! sites (issue #364).
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
fn bounds_traps_keep_their_sites(#[case] body: &str, #[case] expected: &str) {
    assert_eq!(trap(body), format!("error: main.plenty:{expected}\n"));
}
