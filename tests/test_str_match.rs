//! Issue #565: large string matches dispatch through one sorted-table search.
mod support;
use std::fmt::Write;
use std::process::Command;
use support::assert_stdout as runs;

/// Literals that sort differently by bytes and by length, share prefixes,
/// cross the seven-byte inline text limit, and include non-ASCII text.
const LITERALS: [&str; 20] = [
    "zz",
    "",
    "a",
    "ab",
    "abc",
    "abcdefg",
    "abcdefgh",
    "abcdefghi",
    "é",
    "e\u{301}",
    "日本",
    "🦀",
    "Z",
    "z",
    "B",
    "ba",
    "b",
    "abcdefgi",
    "abd",
    "~",
];

fn value(i: usize) -> usize {
    i * 37 + 5
}

/// Dump `stage` for every user function, with the dump's index.
fn dump(source: &str, stage: &str) -> (String, String) {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("strings.plenty");
    let output = dir.path().join("dump");
    std::fs::write(&input, source).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args(["--release", "--dump", stage])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let read = |file: &str| std::fs::read_to_string(output.join(file)).unwrap();
    (read(&format!("{stage}.txt")), read("index.txt"))
}

fn function<'a>(dump: &'a str, name: &str) -> &'a str {
    dump.split(&format!("==== {name} "))
        .nth(1)
        .unwrap_or_else(|| panic!("no {name} in dump"))
        .split("==== ")
        .next()
        .unwrap()
}

fn arms(count: usize, fallback: &str) -> String {
    let mut source = String::new();
    for (i, literal) in LITERALS.iter().take(count).enumerate() {
        writeln!(
            source,
            "        case \"{literal}\":\n            {}",
            value(i)
        )
        .unwrap();
    }
    source.push_str(fallback);
    source
}

#[test]
fn string_tables_keep_first_match_alternatives_captures_nesting_and_cleanup() {
    let mut source = String::from("def pick(s: &str) -> i64:\n    match s:\n");
    // An alternative and a capture fallback around the table's literals.
    source.push_str("        case \"x\" | \"xy\":\n            -7\n");
    source.push_str(&arms(
        LITERALS.len(),
        "        case _ as other:\n            -100 - other.byte_len()\n",
    ));
    // Fifteen literals stay a chain.
    source.push_str("def small(s: &str) -> i64:\n    match s:\n");
    source.push_str(&arms(15, "        case _:\n            -1\n"));
    // An owned scrutinee is released exactly once on every path, including
    // an arm that returns early and the fallback.
    source.push_str(
        "def finish(n: i64) -> i64:\n    n + 1\ndef owned(s: str) -> i64:\n    match s:\n",
    );
    for (i, literal) in LITERALS.iter().enumerate() {
        writeln!(
            source,
            "        case \"{literal}\":\n            return finish({})",
            value(i)
        )
        .unwrap();
    }
    source.push_str("        case _:\n            -1\n");
    // A nested pattern compares the payload's text, then its integer.
    source.push_str("enum Key:\n    Named(str, i64)\ndef nested(k: &Key) -> i64:\n    match k:\n        case Key.Named(\"ab\", 1):\n            1000\n");
    for (i, literal) in LITERALS.iter().enumerate() {
        writeln!(
            source,
            "        case Key.Named(\"{literal}\", _):\n            {}",
            value(i)
        )
        .unwrap();
    }
    source.push_str("        case Key.Named(_, n):\n            *n\n");

    // Runtime texts, so no probe shares a literal's address; short ones are
    // inline values. The separator appears in no literal.
    let joined = LITERALS.join(",");
    let probes = [
        "a\u{0}", "abcdefgj", "日", "🦀🦀", "A", "aa", "zzz", "x", "xy",
    ];
    writeln!(
        source,
        "def main() -> Result[i32, AllocError | IoError]:\n    names = \"{joined},{}\".split(\",\")?",
        probes.join(",")
    )
    .unwrap();
    let mut expected = Vec::new();
    for (i, literal) in LITERALS.iter().chain(&probes).enumerate() {
        let found = LITERALS.iter().position(|l| l == literal);
        let pick = match (found, *literal) {
            (Some(at), _) => value(at) as i64,
            (None, "x" | "xy") => -7,
            (None, other) => -100 - other.len() as i64,
        };
        let small = found
            .filter(|&at| at < 15)
            .map_or(-1, |at| value(at) as i64);
        let owned = found.map_or(-1, |at| value(at) as i64 + 1);
        let nested = match found {
            _ if *literal == "ab" => 1000,
            Some(at) => value(at) as i64,
            None => 2,
        };
        expected.push(format!("{pick} {small} {owned} {nested}"));
        let nested_input = if *literal == "ab" { 1 } else { 2 };
        writeln!(
            source,
            "    print(pick(&names[{i}]), small(&names[{i}]), owned(names[{i}]), nested(&Key.Named(names[{i}], {nested_input})))?"
        )
        .unwrap();
    }
    // Literal arguments take the same paths.
    source.push_str(
        "    print(pick(\"\"), pick(\"é\"), pick(\"e\"), owned(\"🦀\"))?\n    Ok(0i32)\n",
    );
    expected.push(format!("{} {} -101 {}", value(1), value(8), value(11) + 1));
    runs(&source, &(expected.join("\n") + "\n"));

    let (clif, _) = dump(&source, "clif-opt");
    for name in ["pick", "owned", "nested"] {
        let body = function(&clif, name);
        assert_eq!(body.matches("plenty_str_match").count(), 1, "{body}");
        assert!(!body.contains("plenty_str_eq"), "{body}");
        assert!(body.contains("br_table "), "{body}");
    }
    let small = function(&clif, "small");
    assert!(!small.contains("plenty_str_match"), "{small}");
    assert_eq!(small.matches("plenty_str_eq").count(), 15, "{small}");
}

#[test]
fn string_table_code_size_is_independent_of_literal_count() {
    // One `plenty_str_match` call replaces 300 `plenty_str_eq` calls, so the
    // remaining code is the jump table and one constant per arm.
    let mut source = String::from("def zone(s: &str) -> i64:\n    match s:\n");
    for i in 0..300 {
        writeln!(source, "        case \"Zone/Name{i}\":\n            {i}").unwrap();
    }
    source.push_str("        case _:\n            -1\ndef main() -> Result[(), IoError]:\n    print(zone(\"Zone/Name0\"), zone(\"Zone/Name299\"), zone(\"Zone/Name300\"), zone(\"Zone/Name\"))?\n    Ok(())\n");
    runs(&source, "0 299 -1 -1\n");
    let (asm, index) = dump(&source, "asm");
    let body = function(&asm, "zone");
    assert_eq!(body.matches("plenty_str_match").count(), 1, "{body}");
    assert!(!body.contains("plenty_str_eq"), "{body}");
    // The chain took about 40 bytes per arm.
    let bytes: usize = index
        .lines()
        .find(|line| line.ends_with(" zone"))
        .and_then(|line| line.split_whitespace().next()?.parse().ok())
        .unwrap_or_else(|| panic!("no zone in {index}"));
    assert!(bytes < 300 * 20, "{bytes} bytes: {body}");
}
