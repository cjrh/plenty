//! Binary string methods share one byte-view implementation, never UTF-8 text.
mod support;

fn native(body: &str, expected: &str) {
    let source = format!(
        "import std.bytes\nfrom std.bytes import Bytes\ndef main() -> Result[(), AllocError | IoError]:\n{}    Ok(())\n",
        body.lines()
            .map(|line| format!("    {line}\n"))
            .collect::<String>()
    );
    assert_eq!(
        support::run_file_stdout(&source).trim_end(),
        expected.trim()
    );
}

#[test]
fn literal_view_and_buffer_searches_are_binary_and_view_relative() {
    native(
        r#"data = b"\xffab\x00ab\xff"
print(data.find(b"ab"), data.rfind(b"ab"), data.find(0u8), data.rfind(255u8))?
print(data.count(b"ab"), data.count(255u8), data.count(b""))?
print(b"ab" in data, 0u8 in data, b"missing" not in data, b"" in data)?
view = &data[1:-1]
print(view.find(b"ab"), view.rfind(b"ab"), view.count(b"ab"))?
print(view.startswith(b"ab"), view.endswith(b"ab"), b"\xff" not in view)?
buffer = std.bytes.from_bytes(&data)?
print(buffer.find(b"ab"), buffer.rfind(b"ab"), buffer.count(b"ab"), 0u8 in buffer)?
print(buffer.find(0u8, 0, len(buffer)), buffer.count(255u8, 0, len(buffer)))?
print(b"aaaaa".rfind(b"aaa"), b"abc".rfind(b""), b"".rfind(b""), b"a".rfind(b"aa"))?
print(b"".find(b""), b"".count(b""), b"".startswith(b""), b"".endswith(b""), b"a".startswith(b"aa"))?"#,
        "Option[i64].Some(1) Option[i64].Some(4) Option[i64].Some(3) Option[i64].Some(6)\n2 2 8\nTrue True True True\nOption[i64].Some(0) Option[i64].Some(3) 2\nTrue True True\nOption[i64].Some(1) Option[i64].Some(4) 2 True\nOption[i64].Some(3) 2\nOption[i64].Some(2) Option[i64].Some(3) Option[i64].Some(0) Option[i64].Nothing\nOption[i64].Some(0) 1 True True False",
    );
}

#[test]
fn classification_and_case_conversion_leave_high_bytes_alone() {
    native(
        r#"print(b"".isascii(), b"".isspace(), b"".isdigit(), b"".isalpha(), b"".isalnum())?
print(b" \t\n\r\x0b\x0c".isspace(), b"\x00".isspace(), b"\x85".isspace())?
print(b"123".isdigit(), b"AbZ".isalpha(), b"a9".isalnum(), b"\xff".isalnum())?
print(b"AbZ\x00\xff\xc3\xa9".lower()?, b"aBz\x00\xff\xc3\xa9".upper()?)?
source = b"<<AbC\xff>>"
view = &source[2:-2]
print(view.lower()?, view.upper()?, source)?
print(b" Ab ".lower()?.strip()?.upper()?)?"#,
        r#"True False False False False
True False False
True True True False
b"abz\x00\xff\xc3\xa9" b"ABZ\x00\xff\xc3\xa9"
b"abc\xff" b"ABC\xff" b"<<AbC\xff>>"
b"AB""#,
    );
}

#[test]
fn strip_split_replace_and_join_preserve_binary_content() {
    native(
        r#"print(b" \tA\xff\x00 \r\n".strip()?)?
print(b"xxAxy".lstrip(b"xy")?, b"xxAxy".rstrip(b"xy")?, b"xy".strip(b"xy")?)?
print(b"\xffA\xff".strip(b"\xff")?, b" x ".strip(b"")?)?
print(b",a,,b,".split(b",")?, b"".split(b",")?)?
print(b" \ta\x00\xff  b\r\n".split()?, b" \t".split()?)?
print(b"ababa".replace(b"aba", b"X")?, b"ab".replace(b"", b"-")?, b"".replace(b"", b"!" )?)?
parts = [b"a\x00", b"", b"\xff"]?
print(b",".join(&parts)?)?
owned_parts: list[Bytes] = [std.bytes.from_bytes(b"a")?, std.bytes.from_bytes(b"\xff")?]?
print(b"\x00".join(&owned_parts)?)?
empty_parts: list[bytes] = []?
print(b"separator".join(&empty_parts)?)?"#,
        r#"b"A\xff\x00"
b"Axy" b"xxA" b""
b"A" b" x "
[b"", b"a", b"", b"b", b""] [b""]
[b"a\x00\xff", b"b"] []
b"Xba" b"-a-b-" b"!"
b"a\x00,,\xff"
b"a\x00\xff"
b"""#,
    );
}

#[test]
fn literal_methods_load_the_library_without_an_import() {
    let source = r#"def main() -> Result[(), AllocError | IoError]:
    values = [b"A", b"B"]?
    print(b",".join(&values)?, b"A".lower()?)?
    Ok(())
"#;
    assert_eq!(support::run_file_stdout(source).trim(), "b\"A,B\" b\"a\"");
}

#[test]
fn empty_split_separator_traps() {
    support::assert_file_traps(
        "def main() -> Result[(), AllocError]:\n    drop(b\"abc\".split(b\"\")?)\n    Ok(())\n",
        "bytes.split: separator is empty",
    );
}

#[test]
fn membership_preserves_operand_evaluation_order() {
    let source = r#"def needle() -> Result[bytes, IoError]:
    print("needle")?
    Ok(b"a")
def source() -> Result[bytes, IoError]:
    print("source")?
    Ok(b"abc")
def main() -> Result[(), IoError]:
    print(needle()? in source()?)?
    Ok(())
"#;
    assert_eq!(
        support::run_file_stdout(source).trim(),
        "needle\nsource\nTrue"
    );
}

#[test]
fn byte_methods_keep_sources_available_and_reject_text() {
    let error = support::check_error("data = b\"abc\"\nx = data.find(\"a\")\n");
    assert!(error.contains("takes a slice view"), "{error}");
    let source = r#"import std.bytes
def main() -> Result[(), AllocError | IoError]:
    mut source = std.bytes.from_bytes(b"AbC")?
    view = &source[:]
    print(view.lower()?, b"b" in view)?
    source.push(255u8)?
    print(source)?
    Ok(())
"#;
    assert_eq!(
        support::run_file_stdout(source).trim(),
        "b\"abc\" True\nb\"AbC\\xff\""
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn searches_do_not_allocate_and_transforms_report_allocation_failure() {
    native(
        r#"parts = [b"some long value", b"another long value"]?
print("__test_fail_allocations_after_0__")?
a = b"a sufficiently long string".lower()
b = b"a sufficiently long string".strip()
c = b"a,b".split(b",")
d = b"a".replace(b"a", b"a sufficiently long replacement")
e = b",".join(&parts)
f = b"abababa".rfind(b"aba")
g = b"\xff" in b"a\xff"
print("__test_restore_allocations__")?
print(a.is_err(), b.is_err(), c.is_err(), d.is_err(), e.is_err(), f, g)?"#,
        "True True True True True Option[i64].Some(4) True",
    );
}

#[test]
fn typed_byte_view_parameters_and_reference_range_methods_work_without_imports() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("module.plenty");
    std::fs::write(
        &path,
        "pub def ascii(source: &[u8]) -> bool:\n    source.isascii()\n",
    )
    .unwrap();
    plenty::check_module_file(&path, None).unwrap();
    let source = r#"import std.bytes
from std.bytes import Bytes
def scan(source: &Bytes) -> Result[(), AllocError | IoError]:
    print(source.find(65u8, 0, len(source)), source.count(65u8, 0, len(source)))?
    print(source.find(b"A"), source.lower()?)?
    Ok(())
def main() -> Result[(), AllocError | IoError]:
    source = std.bytes.from_bytes(b"ABC")?
    scan(&source)?
    Ok(())
"#;
    assert_eq!(
        support::run_file_stdout(source).trim(),
        "Option[i64].Some(0) 1\nOption[i64].Some(0) b\"abc\""
    );
}

#[test]
fn direct_shared_slice_expressions_support_binary_operations() {
    native(
        r#"mut source = std.bytes.from_bytes(b"A\x00\xffB")?
print((&source[:]).lower()?, (&source[:]).upper()?)?
view = &source[:]
print(b"A" in &source[:], &source[:1] in view, &source[:1] in &source[:])?
print(0u8 in &source[:], &source[1:3] in &source[:], (&source[:]).find(&source[1:3]))?
exclusive = &mut source[:]
print(b"A" in exclusive, exclusive.find(b"B"))?"#,
        r#"b"a\x00\xffb" b"A\x00\xffB"
True True True
True True Option[i64].Some(1)
True Option[i64].Some(3)"#,
    );
}

#[test]
fn explicit_exclusive_arguments_keep_shared_parameter_permission_checks() {
    let error = support::check_error(
        "import std.bytes\ndef main() -> ():\n    mut source = std.bytes.from_bytes(b\"ABC\").unwrap()\n    x = (&mut source[:]).lower()\n",
    );
    assert!(error.contains("a shared slice view"), "{error}");
}

#[test]
fn text_byte_method_chains_load_without_imports_printing_or_byte_literals() {
    let source = r#"def main() -> Result[(), AllocError]:
    text = " ABC "
    if not text.as_bytes().isascii():
        return Err(AllocError.CapacityOverflow)
    match text.as_bytes().find(65u8):
        case Some(index):
            if index != 1:
                return Err(AllocError.CapacityOverflow)
        case Nothing:
            return Err(AllocError.CapacityOverflow)
    clean = text.as_bytes().lower()?.strip()?
    if len(clean) != 3 or clean.count(97u8) != 1:
        return Err(AllocError.CapacityOverflow)
    Ok(())
"#;
    assert_eq!(support::run_file_stdout(source), "");
}
