mod support;

const DECLARATIONS: &str = r#"
protocol Takes:
    def take(self, amount: i64) -> i64:
        pass
class Concrete:
    def take(self, amount: i64 = 9) -> i64:
        amount
"#;

#[test]
fn protocol_calls_cannot_inherit_implementation_defaults() {
    for call in ["run(&value)", "run[Concrete](&value)"] {
        for body in ["value.take()", "alias = value\n    alias.take()"] {
            let source = format!(
                "{DECLARATIONS}\ndef run[T: Takes](value: &T) -> i64:\n    {body}\ndef main() -> ():\n    value = Concrete()\n    result = {call}\n"
            );
            let error = support::check_source(&source).unwrap_err().to_string();
            assert!(
                error.contains("protocol") && error.contains("default"),
                "{error}"
            );
        }
    }
}

#[test]
fn lifted_anonymous_body_retains_protocol_default_restriction() {
    let source = format!(
        "{DECLARATIONS}\ndef run[T: Takes](value: &T) -> i64:\n    callback = def(item: &T) -> i64:\n        item.take()\n    callback(value)\ndef main() -> ():\n    value = Concrete()\n    result = run(&value)\n"
    );
    let error = support::check_source(&source).unwrap_err().to_string();
    assert!(
        error.contains("protocol") && error.contains("default"),
        "{error}"
    );
}

#[test]
fn full_arity_protocol_calls_and_direct_default_calls_remain_valid() {
    let source = format!(
        "{DECLARATIONS}\ndef run[T: Takes](value: &T) -> i64:\n    value.take(4)\ndef main() -> ():\n    value = Concrete()\n    first = run(&value)\n    second = value.take()\n"
    );
    support::check_source(&source).unwrap();
}
