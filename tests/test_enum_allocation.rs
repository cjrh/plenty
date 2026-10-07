//! Fallible enum payload allocation without copying its arguments.
mod support;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        text.lines()
            .filter(|s| !s.starts_with("__test_"))
            .collect::<Vec<_>>()
            .join("\n"),
        expected
    );
}

#[test]
fn nullary_multi_payload_and_inline_variants() {
    native(r#"
enum Message:
    Empty
    Data(str, list[i64])
def data() -> Result[Message, AllocError]:
    Ok(Message.Data.new("hello", [1, 2].unwrap())?)
print(data()).unwrap()
print(Message.Empty.new()).unwrap()
print(Option[i64].Some.new(4)).unwrap()
"#, "Result[Message, AllocError].Ok(Message.Data(\"hello\", [1, 2]))\nResult[Message, AllocError].Ok(Message.Empty)\nResult[Option[i64], AllocError].Ok(Option[i64].Some(4))");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_releases_owned_payloads() {
    for budget in 0..=1 {
        native(
            &format!(
                r#"
enum Message:
    Data(list[i64], list[str])
a = [1, 2].unwrap()
b = ["hello"].unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = Message.Data.new(a, b)
print("__test_restore_allocations__").unwrap()
print(result).unwrap()
"#
            ),
            if budget == 0 {
                "Result[Message, AllocError].Err(AllocError.OutOfMemory)"
            } else {
                "Result[Message, AllocError].Ok(Message.Data([1, 2], [\"hello\"]))"
            },
        );
    }
}
