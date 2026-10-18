//! Byte literals: `b"..."` has the built-in immutable type `bytes` (issue #156).
mod support;

fn output(source: &str) -> String {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Programs that import `std` need a file-based compilation.
fn file_output(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn rejected(source: &str) -> String {
    support::check_source(source)
        .expect_err("source should be rejected")
        .to_string()
}

#[test]
fn escapes_quotes_and_empty_literals() {
    let printed = output(
        r#"
def main() -> Result[(), Failure]:
    print(b"\x00\xff\x7F\n\r\t\0")?
    print(b'single "double"')?
    print(b"it's \"quoted\" \\ done")?
    print(len(b""))?
    print(b"" == b'')?
    print(b"""two
lines""")?
    print(len(b"\x00\xff"))?
    Ok(())
"#,
    );
    assert_eq!(
        printed,
        "b\"\\x00\\xff\\x7f\\x0a\\x0d\\x09\\x00\"\n\
         b\"single \\\"double\\\"\"\n\
         b\"it's \\\"quoted\\\" \\\\ done\"\n\
         0\nTrue\nb\"two\\x0alines\"\n2\n"
    );
}

#[test]
fn length_indexing_iteration_and_equality() {
    let printed = output(
        r#"
def magic() -> bytes:
    b"\x89PNG"

def first(value: &bytes) -> u8:
    value[0]

def main() -> Result[(), Failure]:
    signature = magic()
    copied = signature
    print(len(signature))?
    print(signature[0])?
    print(signature[-1])?
    print(first(&signature))?
    for byte in b"AB":
        print(byte)?
    for byte in reversed(b"AB"):
        print(byte)?
    print(copied == magic())?
    print(signature != b"PNG")?
    print(80u8 in signature)?
    print(list(b"hi")?)?
    Ok(())
"#,
    );
    assert_eq!(
        printed,
        "4\n137\n71\n137\n65\n66\n66\n65\nTrue\nTrue\nTrue\n[104, 105]\n"
    );
}

#[test]
fn bytes_values_are_ordinary_copyable_data() {
    let printed = output(
        r#"
class Format:
    name: str
    magic: bytes

def pick(flag: bool) -> Option[bytes]:
    if flag:
        return Some(b"yes")
    Nothing

def same[T](a: T, b: T) -> bool:
    a == b

def zeros(value: bytes) -> i64:
    mut count = 0
    for byte in value:
        if byte == 0u8:
            count += 1
    count

def main() -> Result[(), Failure]:
    gif = Format("gif", b"GIF8")
    print(gif.magic)?
    print(pick(True))?
    print(same(b"x", b"x"))?
    seen = {b"a", b"b", b"a"}?
    print(len(seen))?
    names = {b"GIF8": "gif"}?
    print(names[gif.magic])?
    print([b"a", b"\x01"]?)?
    with spawn(zeros, b"\x00a\x00")? as task:
        print(task.join())?
    tag = b"tag"
    measure = def [tag]() -> i64:
        len(tag)
    print(measure())?
    Ok(())
"#,
    );
    assert_eq!(
        printed,
        "b\"GIF8\"\nOption[bytes].Some(b\"yes\")\nTrue\n2\ngif\n[b\"a\", b\"\\x01\"]\n2\n3\n"
    );
}

#[test]
fn b_and_bytes_remain_ordinary_names() {
    let printed = output(
        r#"
def main() -> Result[(), Failure]:
    b = 2
    bytes = b + 1
    B = bytes * 2
    print(b)?
    print(bytes)?
    print(B)?
    Ok(())
"#,
    );
    assert_eq!(printed, "2\n3\n6\n");
    // A local named `bytes` is a value, never the type, before a method.
    let printed = output(
        r#"
def main() -> Result[(), Failure]:
    mut bytes = [1u8]?
    bytes.append(2u8)?
    print(bytes)?
    Ok(())
"#,
    );
    assert_eq!(printed, "[1, 2]\n");
}

#[test]
fn index_out_of_range_traps_at_the_index() {
    let output = support::run(
        r#"
def main() -> Result[(), Failure]:
    value = b"abc"
    print(value[3])?
    Ok(())
"#,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("4:11: index out of bounds: index 3, length 3"),
        "{stderr}"
    );
}

#[test]
fn owned_buffers_are_explicit_copies() {
    let printed = file_output(
        r#"
import std.bytes

def main() -> Result[(), Failure]:
    mut request = std.bytes.from_bytes(b"GET / HTTP/1.1\r\n\r\n")?
    end = std.bytes.from_bytes(b"\r\n\r\n")?
    print(request.find_bytes(&end, 0, request.len())?)?
    print(request.matches(0, &std.bytes.from_bytes(b"GET")?, 0, 3)?)?
    request.push(255u8)?
    print(request.len())?
    print(std.bytes.from_bytes(b"")?.len())?
    std.bytes.from_bytes(b"done\n")?.write_stdout(0, 5)?
    Ok(())
"#,
    );
    assert_eq!(printed, "Option[i64].Some(14)\nTrue\n19\n0\ndone\n");
}

#[test]
fn non_ascii_characters_are_rejected_at_their_position() {
    let message = rejected("value = b\"caf\u{e9}\"\n");
    assert!(message.contains("3:18: a byte literal holds only ASCII characters; write `\u{e9}` with `\\x` escapes or convert text with `std.bytes.from_text`"), "{message}");
}

#[test]
fn malformed_escapes_are_rejected() {
    for (source, expected) in [
        (
            r#"value = b"\x4""#,
            "`\\x` needs exactly two hexadecimal digits",
        ),
        (
            r#"value = b"\xg0""#,
            "`\\x` needs exactly two hexadecimal digits",
        ),
        (
            r#"value = b"\x+f""#,
            "`\\x` needs exactly two hexadecimal digits",
        ),
        (
            r#"value = b"\u0041""#,
            "unsupported escape \\u in a byte literal",
        ),
        (
            r#"value = b"\N""#,
            "unsupported escape \\N in a byte literal",
        ),
        ("value = b\"open\n", "newline in a quoted byte literal"),
    ] {
        let message = rejected(source);
        assert!(message.contains(expected), "{source}: {message}");
    }
    let message = plenty::check_source("def main() -> ():\n    value = b\"open")
        .expect_err("unterminated literal")
        .to_string();
    assert!(
        message.contains("2:13: unterminated byte literal"),
        "{message}"
    );
}

#[test]
fn byte_literals_keep_one_spelling() {
    for (source, expected) in [
        (
            r#"value = B"a""#,
            "byte literals use a lowercase `b` prefix",
        ),
        (
            "match b\"a\":\n    case b\"a\":\n        pass\n    case _:\n        pass\n",
            "a pattern cannot be a byte literal; compare with `==`",
        ),
    ] {
        let message = rejected(source);
        assert!(message.contains(expected), "{source}: {message}");
    }
}

#[test]
fn bytes_is_distinct_from_text_and_buffers() {
    for (source, expected) in [
        (r#"value: str = b"a""#, "expected str, got bytes"),
        (r#"value: bytes = "a""#, "expected bytes, got str"),
        (
            r#"value = b"a" + b"b""#,
            "operator `+` does not accept bytes",
        ),
        (
            r#"value = b"a" < b"b""#,
            "operator `<` does not accept bytes",
        ),
        (
            r#"value = b"a".count(97u8)"#,
            "bytes has no `count` method; `value.len()` counts its bytes",
        ),
    ] {
        let message = rejected(source);
        assert!(message.contains(expected), "{source}: {message}");
    }
}

/// A literal lives in read-only data, so using one never allocates.
#[cfg(feature = "runtime-checks")]
#[test]
fn byte_literals_never_allocate() {
    let printed = output(
        r#"
def total(value: bytes) -> i64:
    mut sum = 0
    for byte in value:
        sum += i64(byte)
    sum

def main() -> Result[(), Failure]:
    print("__test_begin_no_allocations__")?
    value = b"\x01\x02\x03"
    other = value
    result = total(value) + len(other) + i64(value[-1])
    equal = value == b"\x01\x02\x03"
    print("__test_end_no_allocations__")?
    print(result)?
    print(equal)?
    Ok(())
"#,
    );
    assert_eq!(
        printed,
        "__test_begin_no_allocations__\n__test_end_no_allocations__\n12\nTrue\n"
    );
}
