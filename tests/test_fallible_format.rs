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

#[cfg(feature = "runtime-checks")]
#[test]
fn every_buffer_growth_and_final_string_allocation_can_fail_cleanly() {
    let representation = format!("{:?}", (0..32).collect::<Vec<_>>());
    for operation in ["str.try_repr(values)", "try_print(values)"] {
        let mut succeeded = false;
        let mut failures = 0;
        for budget in 0..=12 {
            let out = support::run(&format!(
                r#"
values = [n for n in range(32)]
print("__test_fail_allocations_after_{budget}__")
result = {operation}
print("__test_restore_allocations__")
match result:
    case Ok(value):
        print("success")
    case Err(error):
        print("failed")
print(len(values))
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
                if operation.starts_with("try_print") {
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
