//! Checked text algorithms, including the complete Unicode 17 normalization corpus.

mod support;
use support::compile_file as compile;

fn run(body: &str) -> String {
    run_with_optimization(body, false)
}

fn run_with_optimization(body: &str, optimize: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let source = format!(
        "import std.text\nimport std.unicode\nimport std.bytes\ndef failed[T, E](value: Result[T, E]) -> bool:\n    match value:\n        case Ok(_):\n            False\n        case Err(_):\n            True\ndef absent[T](value: Option[T]) -> bool:\n    match value:\n        case Some(_):\n            False\n        case Nothing:\n            True\ndef main() -> Result[(), AllocError | IoError | std.text.TextError | std.bytes.BytesError]:\n{}\n    Ok(())\n",
        body.lines()
            .map(|l| format!("    {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, &source).unwrap();
    let options = plenty::CompileOptions {
        optimize,
        test_hooks: true,
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&path, &executable, None, &options)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn bounded_builders_join_and_scalar_wrapping() {
    assert_eq!(
        run(r#"
mut builder = std.text.builder(7)?
builder.append("é🙂")?
print(builder.byte_len(), builder.limit(), builder.to_text()?)?
print(failed(builder.append("xy")), builder.to_text()? == "é🙂")?
builder.append("\0")?
print(builder.byte_len(), len(builder.to_text()?))?
builder.clear()
builder.append("again")?
print(builder.finish()?)?
pieces = ["é", "", "🙂"]?
print(std.text.join(&pieces, ":", 8)?)?
print(failed(std.text.join(&pieces, ":", 7)))?
print(str.repr(std.text.hard_wrap("é🙂ab\nc\n", 2, 32)?)?)?
print(str.repr(std.text.hard_wrap("éx", 1, 32)?)?)?
empty = std.text.builder(0)?
print(empty.finish()? == "", std.text.join(&list[str].new()?, "", 0)? == "")?
bad = std.bytes.from_list(&[255u8]?)?
print(failed(bad.to_text()))?
"#),
        "6 7 é🙂\nTrue True\n7 3\nagain\né::🙂\nTrue\n\"é🙂\\nab\\nc\\n\"\n\"e\\ń\\nx\"\nTrue True\nTrue"
    );
}

#[test]
fn normalization_forms_classes_and_limits() {
    assert_eq!(
        run(r#"
print(std.unicode.version())?
print(std.unicode.nfc("é", 2)? == "é", std.unicode.nfd("é", 2)? == "é")?
print(std.unicode.nfkc("ﬃ", 3)? == "ffi", std.unicode.nfkd("①", 1)? == "1")?
print(std.unicode.nfc("각", 3)? == "각")?
print(std.unicode.combining_class("́").unwrap(), std.unicode.is_mark("́").unwrap())?
print(absent(std.unicode.combining_class("ab")), absent(std.unicode.is_mark("")))?
print(failed(std.unicode.nfd("é", 1)), failed(std.unicode.nfc("é", 1)))?
print(std.unicode.nfc("", 0)? == "")?
print(std.unicode.nfc("a\0é", 4)? == "a\0é")?
"#),
        "17.0.0\nTrue True\nTrue True\nTrue\n230 True\nTrue True\nTrue True\nTrue\nTrue"
    );
}

#[test]
fn a_scalar_limit_above_the_ceiling_traps() {
    // `max_scalars * 4` bounds the UTF-8 output, so larger limits would overflow.
    support::assert_file_traps(
        "import std.unicode\ndef main() -> ():\n    drop(std.unicode.nfd(\"\", NonNegative.try_from(2305843009213693952).unwrap()))\n",
        "std.unicode: max_scalars is above i64.MAX // 4",
    );
}

#[test]
fn out_of_domain_limits_are_compile_errors() {
    // Width is `Positive[i64]` and byte and scalar limits `NonNegative[i64]`,
    // replacing the old `InvalidLimit` cases.
    for (call, message) in [
        ("std.text.builder(-1)", "cannot be negative"),
        (
            "std.text.hard_wrap(\"x\", 0, 4)",
            "must be greater than zero",
        ),
        ("std.unicode.nfc(\"\", -1)", "cannot be negative"),
    ] {
        let source =
            format!("import std.text\nimport std.unicode\ndef main() -> ():\n    drop({call})\n");
        let error = support::check_file_error(&source);
        assert!(error.contains(message), "{call}: {error}");
    }
}

#[test]
fn builders_keep_ordinary_ownership_and_loan_checks() {
    for (body, expected) in [
        (
            "view = &builder\n    builder.append(\"changed\")?\n    print(view.byte_len())?",
            "conflicting borrow",
        ),
        ("moved = builder\n    print(builder.byte_len())?", "moved"),
        ("other = copy(builder)?", "cannot be copied"),
        (
            "drop(builder.finish())\n    print(builder.byte_len())?",
            "moved",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.plenty");
        std::fs::write(&path, format!("import std.text\nimport std.bytes\ndef main() -> Result[(), AllocError | IoError | std.text.TextError | std.bytes.BytesError]:\n    mut builder = std.text.builder(32)?\n    {body}\n    Ok(())\n")).unwrap();
        let error = plenty::check_file(&path, None).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn finishing_builder_preserves_errors_and_cleans_up_on_allocation_failure() {
    let mut succeeded = false;
    let mut failed = false;
    for budget in 0..=8 {
        let output = run(&format!(
            r#"
mut builder = std.text.builder(32)?
builder.append("final é🙂")?
print("__test_fail_allocations_after_{budget}__")?
result = builder.finish()
print("__test_restore_allocations__")?
match result:
    case Ok(text):
        print(text == "final é🙂")?
    case Err(error):
        print(error)?
"#
        ));
        if output == "True" {
            succeeded = true;
            break;
        }
        assert!(output.contains("Allocation"), "{output}");
        failed = true;
    }
    assert!(failed && succeeded, "must exercise failure and success");
}

#[test]
fn complete_unicode_17_normalization_conformance() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = String::new();
    let mut count = 0;
    for line in include_str!("fixtures/unicode-17/NormalizationTest.txt").lines() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() || line.starts_with('@') {
            continue;
        }
        let fields: Vec<String> = line
            .split(';')
            .take(5)
            .map(|field| {
                field
                    .split_whitespace()
                    .map(|hex| char::from_u32(u32::from_str_radix(hex, 16).unwrap()).unwrap())
                    .collect()
            })
            .collect();
        assert_eq!(fields.len(), 5);
        assert!(fields.iter().all(|f| !f.contains(['\t', '\n', '\r'])));
        fixture.push_str(&fields.join("\t"));
        fixture.push('\n');
        count += 1;
    }
    assert!(count > 19_000, "truncated conformance fixture: {count}");
    std::fs::write(dir.path().join("corpus.tsv"), fixture).unwrap();
    let source = r#"
import std.text
import std.bytes
import std.unicode
def main() -> Result[(), AllocError | IoError | std.text.TextError | std.bytes.BytesError | OverflowError[i64]]:
    corpus = read_text("corpus.tsv")?
    mut count = 0
    for row in corpus.split("\n")?:
        if row == "":
            continue
        columns = row.split("\t")?
        for index in range(5):
            source = columns[index]
            canonical_c = columns[1] if index < 3 else columns[3]
            canonical_d = columns[2] if index < 3 else columns[4]
            if std.unicode.nfc(&source, 1024)? != canonical_c:
                print("NFC", count, index)?
                return Ok(())
            if std.unicode.nfd(&source, 1024)? != canonical_d:
                print("NFD", count, index)?
                return Ok(())
            if std.unicode.nfkc(&source, 1024)? != columns[3]:
                print("NFKC", count, index)?
                return Ok(())
            if std.unicode.nfkd(&source, 1024)? != columns[4]:
                print("NFKD", count, index)?
                return Ok(())
        count += 1
    print(count)?
    Ok(())
"#;
    let output = std::process::Command::new(compile(dir.path(), source))
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        count.to_string()
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failures_leave_builder_unchanged_and_normalization_recoverable() {
    let mut reached_success = false;
    for budget in 0..=64 {
        let out = run(&format!(
            r#"
mut builder = std.text.builder(128)?
builder.append("original")?
print("__test_fail_allocations_after_{budget}__")?
appended = builder.append("a longer string needing storage")
print("__test_restore_allocations__")?
if failed(appended):
    print(builder.to_text()? == "original")?
else:
    print(builder.to_text()? == "originala longer string needing storage")?
print("__test_fail_allocations_after_{budget}__")?
normalized = std.unicode.nfkc("éﬃ각", 32)
print("__test_restore_allocations__")?
match normalized:
    case Ok(text):
        print(text == "éffi각")?
        print("success")?
    case Err(_):
        print(True)?
        print("recovered")?
"#
        ));
        assert!(
            out == "True\nTrue\nsuccess" || out == "True\nTrue\nrecovered",
            "budget {budget}: {out}"
        );
        if out.ends_with("success") {
            reached_success = true;
            break;
        }
    }
    assert!(
        reached_success,
        "all normalization allocation points must be exercised"
    );
}

#[test]
fn normalization_ceiling_and_insertion_boundaries_in_both_codegen_modes() {
    let body = r#"
# The ceiling is a scalar budget, not an eager allocation request.
ceiling = NonNegative.try_from(2305843009213693951).unwrap()
print(std.unicode.nfkd("", ceiling)? == "")?
print(std.unicode.nfkd("ﬃ", ceiling)? == "ffi")?
# Descending combining classes force the insertion index all the way to zero.
print(std.unicode.nfd("\u{315}\u{300}\u{327}", 3)? == "\u{327}\u{300}\u{315}")?
print(failed(std.unicode.nfd("\u{315}\u{300}\u{327}", 2)))?
# The scalar limit still covers all decomposed scalars before composition.
print(failed(std.unicode.nfc("각", 2)), std.unicode.nfc("각", 3)? == "각")?
print(std.unicode.nfkd("ﬃ", 3)? == "ffi", failed(std.unicode.nfkd("ﬃ", 2)))?
"#;
    for optimize in [false, true] {
        assert_eq!(
            run_with_optimization(body, optimize),
            "True\nTrue\nTrue\nTrue\nTrue True\nTrue True"
        );
    }
}

#[test]
fn text_readers_accept_bounded_views_without_an_owning_input_copy() {
    assert_eq!(
        run(r#"
source = "!é🙂\0ab!"
view = &source[1:-1]
mut output = std.text.builder(9)?
output.append(view)?
print(output.byte_len(), output.to_text()? == "é🙂\0ab")?
print(failed(output.append(&source[1:2])), output.to_text()? == "é🙂\0ab")?
print(std.text.find_in(view, b"a").unwrap(), std.text.find_not_in(view, b"a").unwrap())?
pieces = ["a", "b"]?
print(std.text.join(&pieces, &source[2:3], 6)? == "a🙂b")?
print(std.text.hard_wrap(view, 2, 20)? == "é🙂\n\0a\nb")?
words = "!alpha beta gamma!"
print(std.text.wrap(&words[1:-1], 10)? == ["alpha beta", "gamma"]?)?
print(std.text.fill(&words[1:-1], 10)? == "alpha beta\ngamma")?
wrapper = std.text.text_wrapper(10)
print(wrapper.wrap(&words[1:-1])? == ["alpha beta", "gamma"]?)?
print(wrapper.fill(&words[1:-1])? == "alpha beta\ngamma")?
mark = "![...]!"
print(std.text.shorten(&words[1:-1], 10, &mark[1:-1])? == "alpha[...]")?
indented = "!  alpha\n  beta\n!"
print(std.text.dedent(&indented[1:-1])? == "alpha\nbeta\n")?
print(std.text.indent(&words[1:6], &source[1:2])? == "éalpha")?
print(std.text.indent_with(&words[1:6], &source[1:2], def(line: &str) -> bool: len(line) > 0)? == "éalpha")?
empty = &source[1:1]
print(std.text.fill(empty, 10)? == "", std.text.join(&pieces, empty, 2)? == "ab")?
"#),
        "9 True\nTrue True\n3 0\nTrue\nTrue\nTrue\nTrue\nTrue\nTrue\nTrue\nTrue\nTrue\nTrue\nTrue True"
    );
}
