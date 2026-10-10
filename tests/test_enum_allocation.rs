//! Enum construction stores its payloads inline without allocating.
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
    native(
        r#"
enum Message:
    Empty
    Data(str, list[i64])
def data() -> Message:
    Message.Data.new("hello", [1, 2].unwrap())
print(data()).unwrap()
print(Message.Empty.new()).unwrap()
print(Option[i64].Some.new(4)).unwrap()
"#,
        "Message.Data(\"hello\", [1, 2])\nMessage.Empty\nOption[i64].Some(4)",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn multi_field_variants_move_payloads_without_allocating() {
    native(
        r#"
enum Message:
    Data(list[i64], list[str])
a = [1, 2].unwrap()
b = ["hello"].unwrap()
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
result = Message.Data.new(a, b)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(result).unwrap()
"#,
        "Message.Data([1, 2], [\"hello\"])",
    );
}
