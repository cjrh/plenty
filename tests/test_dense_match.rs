//! Issue #395: native dispatch policy and semantics at integer/tag boundaries.
mod support;
use std::fmt::Write;
use std::process::Command;
use support::assert_stdout as runs;

fn dispatch(name: &str, ty: &str, keys: impl Iterator<Item = i128>) -> String {
    let mut source = format!("def {name}(x: {ty}) -> i64:\n    match x:\n");
    for key in keys {
        writeln!(source, "        case {key}{ty}:\n            {}", key % 97).unwrap();
    }
    source.push_str("        case _:\n            -1000\n");
    source
}

fn clif(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("dense.plenty");
    let output = dir.path().join("dump");
    std::fs::write(&input, source).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args(["--release", "--dump", "clif-opt"])
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
    std::fs::read_to_string(output.join("clif-opt.txt")).unwrap()
}

#[test]
fn dense_integer_tables_preserve_all_widths_offsets_and_boundaries() {
    let fixtures = [
        (
            "i8",
            i128::from(i8::MIN),
            i128::from(i8::MIN),
            i128::from(i8::MAX),
        ),
        ("u8", 0, 0, i128::from(u8::MAX)),
        ("i16", -400, i128::from(i16::MIN), i128::from(i16::MAX)),
        ("i16", -128, i128::from(i16::MIN), i128::from(i16::MAX)),
        (
            "i32",
            i128::from(i32::MIN),
            i128::from(i32::MIN),
            i128::from(i32::MAX),
        ),
        (
            "i64",
            i128::from(i64::MIN),
            i128::from(i64::MIN),
            i128::from(i64::MAX),
        ),
        ("i64", -128, i128::from(i64::MIN), i128::from(i64::MAX)),
        (
            "i64",
            i128::from(i64::MAX) - 255,
            i128::from(i64::MIN),
            i128::from(i64::MAX),
        ),
        ("u16", i128::from(u16::MAX) - 255, 0, i128::from(u16::MAX)),
        ("u32", i128::from(u32::MAX) - 255, 0, i128::from(u32::MAX)),
        ("u64", i128::from(u64::MAX) - 255, 0, i128::from(u64::MAX)),
        ("u64", (1i128 << 63) - 128, 0, i128::from(u64::MAX)),
    ];
    let mut source = String::new();
    let mut checks = String::new();
    for (i, (ty, base, min, max)) in fixtures.into_iter().enumerate() {
        let name = format!("pick_{i}");
        // Reverse source order: tables must map keys to the original bodies.
        source.push_str(&dispatch(&name, ty, (base..base + 256).rev()));
        for key in base..base + 256 {
            writeln!(
                checks,
                "    if {name}({key}{ty}) != {}:\n        return Ok(1i32)",
                key % 97
            )
            .unwrap();
        }
        for key in [min, max, base - 1, base + 256, base + (1i128 << 32)] {
            if (min..=max).contains(&key) && !(base..base + 256).contains(&key) {
                writeln!(
                    checks,
                    "    if {name}({key}{ty}) != -1000:\n        return Ok(2i32)"
                )
                .unwrap();
            }
        }
    }
    source.push_str("def main() -> Result[i32, IoError]:\n");
    source.push_str(&checks);
    source.push_str("    print(\"ok\")?\n    Ok(0i32)\n");
    runs(&source, "ok\n");
    // Also validate optimized code, independently of the runtime result.
    assert_eq!(clif(&source).matches("br_table ").count(), fixtures.len());
}

#[test]
fn small_sparse_and_duplicate_alternatives_keep_their_behavior() {
    let mut source = dispatch("small", "i64", 0..255);
    source.push_str(&dispatch("sparse", "i64", (0..256).map(|n| n * 2)));
    source.push_str(
        "def alternatives(x: i64) -> i64:\n    match x:\n        case 0 | 1:\n            777\n",
    );
    for key in 2..256 {
        writeln!(source, "        case {key}:\n            {key}").unwrap();
    }
    source.push_str("        case _:\n            -1\ndef main() -> Result[(), IoError]:\n    print(small(254), small(255), sparse(510), sparse(509))?\n    print(alternatives(0), alternatives(1), alternatives(255), alternatives(256))?\n    Ok(())\n");
    runs(&source, "60 -1000 25 -1000\n777 777 255 -1\n");
    // Alternatives share their original body; duplicates remain a diagnostic.
    let dump = clif(&source);
    for name in ["small", "sparse"] {
        let function = dump
            .split(&format!("==== {name} "))
            .nth(1)
            .unwrap()
            .split("==== ")
            .next()
            .unwrap();
        assert!(!function.contains("br_table "), "{function}");
    }
    assert!(
        support::check_diagnostic(&source.replace("case 0 | 1:", "case 0 | 0 | 1:"))
            .contains("unreachable `|` alternative")
    );
}

#[test]
fn dense_enum_dispatch_keeps_nested_payload_order_borrows_and_tail_exits() {
    let mut source = String::from("enum Kind:\n");
    for i in 0..256 {
        let fields = if i == 0 { "(list[i64], i64)" } else { "" };
        writeln!(source, "    V{i}{fields}").unwrap();
    }
    source.push_str("enum Wrapped:\n    Of(Kind)\ndef finish(n: i64) -> i64:\n    n + 1\ndef pick(k: &Kind) -> i64:\n    match k:\n");
    source.push_str("        case Kind.V0(_, 1):\n            return 1001\n        case Kind.V0(_, 2):\n            return 1002\n");
    for i in (0..256).rev() {
        let (pattern, result) = if i == 0 {
            ("(xs, _)", "xs[0]")
        } else {
            ("", "10")
        };
        writeln!(
            source,
            "        case Kind.V{i}{pattern}:\n            return finish({result} + {i})"
        )
        .unwrap();
    }
    source.push_str("def consume(k: Kind) -> i64:\n    match k:\n");
    for i in 0..256 {
        let fields = if i == 0 { "(_, _)" } else { "" };
        writeln!(
            source,
            "        case Kind.V{i}{fields}:\n            return finish({i})"
        )
        .unwrap();
    }
    source.push_str("def nested(k: Wrapped) -> i64:\n    match k:\n        case Wrapped.Of(Kind.V0(xs, _)):\n            xs[0]\n        case Wrapped.Of(_):\n            -1\n");
    // Keep each function below the language's 256 parameter/local slot limit;
    // constructing owned payloads also introduces temporary cleanup slots.
    for group in 0..16 {
        writeln!(source, "def check_{group}() -> Result[i32, AllocError]:").unwrap();
        for i in group * 16..(group + 1) * 16 {
            let value = if i == 0 {
                "Kind.V0([10]?, 0)".to_string()
            } else {
                format!("Kind.V{i}")
            };
            writeln!(
                source,
                "    if pick(&{value}) != {}:\n        return Ok(1i32)",
                i + 11
            )
            .unwrap();
            writeln!(
                source,
                "    if consume({value}) != {}:\n        return Ok(2i32)",
                i + 1
            )
            .unwrap();
        }
        source.push_str("    Ok(0i32)\n");
    }
    source.push_str("def main() -> Result[i32, AllocError | IoError]:\n");
    for group in 0..16 {
        writeln!(
            source,
            "    if check_{group}()? != 0i32:\n        return Ok(1i32)"
        )
        .unwrap();
    }
    source.push_str("    print(pick(&Kind.V0([10]?, 1)), pick(&Kind.V0([10]?, 2)))?\n    print(nested(Wrapped.Of(Kind.V0([42]?, 0))))?\n    print(nested(Wrapped.Of(Kind.V255)))?\n    Ok(0i32)\n");
    runs(&source, "1001 1002\n42\n-1\n");
    let dump = clif(&source);
    let consume = dump
        .split("==== consume ")
        .nth(1)
        .unwrap()
        .split("==== ")
        .next()
        .unwrap();
    assert!(consume.contains("br_table "), "{consume}");
    // Exhaustive enum dispatch still has a defensive invalid-tag trap.
    assert!(consume.contains("trap user1"), "{consume}");
}
