//! Recoverable formatting borrows values and buffers before writing.
mod support;

fn native(source: &str, expected: &str) {
    let out = support::run(source);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        text.lines()
            .filter(|s| !s.starts_with("__test_"))
            .collect::<Vec<_>>()
            .join("\n"),
        expected
    );
}

#[test]
fn repr_and_print_preserve_values_and_string_escaping() {
    native(r#"
values = [1, 2]
match str.try_repr(values):
    case Ok(text):
        print(text)
    case Err(error):
        print(error)
match str.try_repr("é\n\0"):
    case Ok(text):
        print(text)
    case Err(error):
        print(error)
print(try_print(values))
print(try_print("plain"))
print(values)
"#, "[1, 2]\n\"é\\n\\0\"\n[1, 2]\nResult[(), IoError].Ok(())\nplain\nResult[(), IoError].Ok(())\n[1, 2]");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_formatting_preserves_source_and_writes_no_prefix() {
    for operation in ["str.try_repr(values)", "try_print(values)"] {
        native(
            &format!(
                r#"
values = [1, 2]
print("__test_fail_allocations_after_0__")
result = {operation}
print("__test_restore_allocations__")
print(result)
print(values)
"#
            ),
            if operation.starts_with("str.") {
                "Result[str, AllocError].Err(AllocError.OutOfMemory)\n[1, 2]"
            } else {
                "Result[(), IoError].Err(IoError.Data(DataError.Allocation(AllocError.OutOfMemory)))\n[1, 2]"
            },
        );
    }
}
