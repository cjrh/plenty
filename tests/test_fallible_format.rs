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
values = [1, 2].unwrap()
match str.repr(values):
    case Ok(text):
        print(text).unwrap()
    case Err(error):
        print(error).unwrap()
match str.repr("é\n\0"):
    case Ok(text):
        print(text).unwrap()
    case Err(error):
        print(error).unwrap()
print(print(values)).unwrap()
print(print("plain")).unwrap()
print(values).unwrap()
"#, "[1, 2]\n\"é\\n\\0\"\n[1, 2]\nResult[(), IoError].Ok(())\nplain\nResult[(), IoError].Ok(())\n[1, 2]");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_formatting_preserves_source_and_writes_no_prefix() {
    for operation in ["str.repr(values)", "print(values)"] {
        native(
            &format!(
                r#"
values = [1, 2].unwrap()
print("__test_fail_allocations_after_0__").unwrap()
result = {operation}
print("__test_restore_allocations__").unwrap()
print(result).unwrap()
print(values).unwrap()
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

#[cfg(feature = "runtime-checks")]
#[test]
fn every_buffer_growth_and_final_string_allocation_can_fail_cleanly() {
    let representation = format!("{:?}", (0..32).collect::<Vec<_>>());
    for operation in ["str.repr(values)", "print(values)"] {
        let mut succeeded = false;
        let mut failures = 0;
        for budget in 0..=12 {
            let out = support::run(&format!(
                r#"
values = [n for n in range(32).unwrap()].unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = {operation}
print("__test_restore_allocations__").unwrap()
match result:
    case Ok(value):
        print("success").unwrap()
    case Err(error):
        print("failed").unwrap()
print(len(values)).unwrap()
"#
            ));
            assert!(
                out.status.success(),
                "budget {budget}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let stdout = String::from_utf8_lossy(&out.stdout);
            let lines = stdout
                .lines()
                .filter(|s| !s.starts_with("__test_"))
                .collect::<Vec<_>>();
            if lines.contains(&"success") {
                succeeded = true;
                if operation.starts_with("print") {
                    assert_eq!(lines, [representation.as_str(), "success", "32"]);
                } else {
                    assert_eq!(lines, ["success", "32"]);
                }
                break;
            }
            failures += 1;
            assert_eq!(
                lines,
                ["failed", "32"],
                "no formatted prefix may escape on allocation failure"
            );
        }
        assert!(
            succeeded && failures > 1,
            "exercise both buffer growth and success"
        );
    }
}
