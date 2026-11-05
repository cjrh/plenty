//! Issue #757: matches on a sum local learn its tag, so a match arm that
//! rebuilds the same variant reuses the matched value, a `Nothing` arm leaves
//! nothing to release, and a one-bit tag dispatches on that bit.
mod support;
use std::process::Command;

/// Compile and run `source` with the default (optimized) CLI and with the
/// unoptimized fixture options; both lower matches the same way.
fn runs(source: &str, expected: &str) {
    let dir = tempfile::tempdir().unwrap();
    let path = support::write_main(dir.path(), source);
    let optimized = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg(&path)
        .env("PLENTY_CACHE", "off")
        .output()
        .unwrap();
    for output in [optimized, support::run_file(source)] {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{source}\n{stderr}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let visible: Vec<_> = stdout
            .lines()
            .filter(|line| !line.starts_with("__test_"))
            .collect();
        assert_eq!(visible.join("\n"), expected.trim_end(), "{source}");
    }
}

fn text_in_clif() -> String {
    let dir = tempfile::tempdir().unwrap();
    let input = support::write_main(
        dir.path(),
        "import std.bytes\n\ndef main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:\n    b = std.bytes.from_text(\"hi\")?\n    print(b.to_text()?)?\n    Ok(())\n",
    );
    let output = dir.path().join("dump");
    let result = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args([
            "--dump",
            "clif-opt",
            "--dump-std",
            "--dump-filter",
            "text_in",
        ])
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
    let dump = std::fs::read_to_string(output.join("clif-opt.txt")).unwrap();
    dump.split("==== ")
        .find(|part| part.starts_with("std.bytes.text_in "))
        .expect("text_in in the dump")
        .to_owned()
}

#[test]
fn text_in_reuses_the_matched_result_and_releases_nothing() {
    let clif = text_in_clif();
    let count = |needle: &str| clif.matches(needle).count();
    // The only call converts the bytes. Before #757 a second call released
    // the matched `Option[str]` local on the `Nothing` path.
    assert_eq!(count(" call "), 1, "{clif}");
    // The `Ok` arm of `allocated` returns its input. Only the `Err` arm
    // shifts in the new outer tag, and the `?` reads the inner tag once.
    assert!(count(" ishl") <= 1, "{clif}");
    assert!(count(" ushr") <= 1, "{clif}");
    assert!(count("brif ") <= 3, "{clif}");
}

#[test]
fn text_conversion_keeps_its_results_and_errors() {
    runs(
        r#"
import std.bytes

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut b = std.bytes.with_capacity(8)?
    b.push(104u8)?
    b.push(105u8)?
    print(b.to_text()?)?
    print(b.to_text_range(1, 2)?)?
    print(len(b.to_text_range(2, 2)?) == 0)?
    b.push(255u8)?
    match b.to_text():
        case Ok(text):
            print(f"unexpected {text}"?)?
        case Err(error):
            match error:
                case std.bytes.BytesError.InvalidUtf8:
                    print("invalid")?
                case _:
                    print("other error")?
    Ok(())
"#,
        "hi\ni\nTrue\ninvalid",
    );
}

#[test]
fn text_range_outside_the_buffer_traps() {
    support::assert_file_traps(
        r#"
import std.bytes

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    b = std.bytes.from_text("hi")?
    print(b.to_text_range(1, 3)?)?
    Ok(())
"#,
        "Bytes.to_text_range: start..stop is outside 0..=len()",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn text_conversion_propagates_allocation_failure() {
    runs(
        r#"
import std.bytes

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    # Longer than an inline string, so the copy allocates.
    b = std.bytes.from_text("hello, world")?
    print("__test_fail_allocations_after_0__")?
    result = b.to_text()
    print("__test_restore_allocations__")?
    match result:
        case Ok(text):
            print(f"unexpected {text}"?)?
        case Err(error):
            match error:
                case std.bytes.BytesError.Allocation(cause):
                    print(f"allocation {cause}"?)?
                case _:
                    print("other error")?
    print(b.to_text()?)?
    Ok(())
"#,
        "allocation AllocError.OutOfMemory\nhello, world",
    );
}

#[test]
fn generic_retag_keeps_payloads_of_every_width() {
    runs(
        r#"
def wrap[T](result: Result[T, AllocError]) -> Result[T, str]:
    match result:
        case Ok(value):
            Ok(value)
        case Err(error):
            Err("alloc")

def main() -> Result[(), AllocError | IoError]:
    n: Result[i64, AllocError] = Ok(-7)
    print(str.repr(wrap(n))?)?
    b: Result[bool, AllocError] = Ok(True)
    print(str.repr(wrap(b))?)?
    s: Result[str, AllocError] = Ok("seven")
    print(str.repr(wrap(s))?)?
    o: Result[Option[i64], AllocError] = Ok(Some(3))
    print(str.repr(wrap(o))?)?
    e: Result[str, AllocError] = Err(AllocError.OutOfMemory)
    print(str.repr(wrap(e))?)?
    Ok(())
"#,
        "Result[i64, str].Ok(-7)\nResult[bool, str].Ok(True)\nResult[str, str].Ok(\"seven\")\nResult[Option[i64], str].Ok(Option[i64].Some(3))\nResult[str, str].Err(\"alloc\")",
    );
}

#[test]
fn one_bit_matches_and_nothing_arms_read_the_local() {
    runs(
        r#"
def describe(x: Option[str]) -> Result[str, AllocError]:
    match x:
        case Nothing:
            Ok(f"none {x}"?)
        case _:
            Ok(f"other {x}"?)

def first(x: Option[str]) -> i64:
    match x:
        case Some(text):
            len(text)
        case Nothing:
            -1

def main() -> Result[(), AllocError | IoError]:
    print(describe(Nothing)?)?
    print(describe(Some("a"))?)?
    print(first(Some("abc")))?
    print(first(Nothing))?
    Ok(())
"#,
        "none Option[str].Nothing\nother Option[str].Some(\"a\")\n3\n-1",
    );
}

#[test]
fn retag_applies_only_in_the_arm_that_knows_the_tag() {
    // `r` is built in the function, so every projection of it returns the
    // same low word. Re-tagging that word as `Ok` in the `Err` arm must build
    // a new value, not reuse `r`.
    runs(
        r#"
def swap(n: i64) -> Result[i64, str]:
    r: Result[i64, i64] = Err(n)
    match r:
        case Ok(v):
            Ok(v)
        case Err(e):
            Ok(e)

def keep(n: i64) -> Result[i64, str]:
    r: Result[i64, i64] = Ok(n)
    match r:
        case Err(e):
            Ok(e)
        case Ok(v):
            Ok(v)

def main() -> Result[(), AllocError | IoError]:
    print(str.repr(swap(5))?)?
    print(str.repr(keep(6))?)?
    Ok(())
"#,
        "Result[i64, str].Ok(5)\nResult[i64, str].Ok(6)",
    );
}
