//! Conditional reporting preserves Context's unrestricted typed cause.

fn check(source: &str) -> Result<(), String> {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("main.plenty");
    std::fs::write(&file, source).unwrap();
    plenty::check_file(&file, None)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

const PRELUDE: &str = r#"
from std.error import Context, Subject
from std.report import Formatter, FormatError, Formattable
pub enum Good:
    Value
    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        output.text("good")
pub enum Other:
    Value
    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        output.text("other")
pub enum Bad:
    Value
def accepts[T: Formattable](value: &T) -> ():
    pass
pub protocol SameShape:
    def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        pass
def same_shape[T: SameShape](value: &T) -> ():
    pass
"#;

#[test]
fn unformattable_context_and_nested_context_remain_usable() {
    check(&format!(
        "{PRELUDE}\ndef main() -> ():\n    cause = Context(\"inner\", Subject.Absent, Bad.Value)\n    outer = Context(\"outer\", Subject.Absent, cause)\n    drop(outer.describe())\n"
    ))
    .unwrap();
}

#[test]
fn conditional_bounds_accept_nested_context_and_sum_cause() {
    check(&format!(
        "{PRELUDE}\ndef main() -> ():\n    cause: Good | Other = Good.Value\n    inner = Context(\"inner\", Subject.Absent, cause)\n    outer = Context(\"outer\", Subject.Absent, inner)\n    accepts(&outer)\n    same_shape(&outer)\n"
    ))
    .unwrap();
}

#[test]
fn unused_protocol_bounds_reject_unformattable_cause() {
    for function in ["accepts", "same_shape"] {
        let error = check(&format!(
            "{PRELUDE}\ndef main() -> ():\n    cause = Context(\"inner\", Subject.Absent, Bad.Value)\n    outer = Context(\"outer\", Subject.Absent, cause)\n    {function}(&outer)\n"
        ))
        .unwrap_err();
        assert!(error.contains("missing method `format`"), "{error}");
    }
}

#[test]
fn direct_call_rejects_unformattable_cause() {
    let error = check(&format!(
        "{PRELUDE}\ndef format_bad(value: &Context[Bad], output: &mut Formatter) -> Result[(), FormatError]:\n    value.format(output)\ndef main() -> ():\n    pass\n"
    ))
    .unwrap_err();
    assert!(error.contains("missing method `format`"), "{error}");
}

#[test]
fn direct_calls_lower_nested_context_methods() {
    check(&format!(
        "{PRELUDE}\ndef format_good(value: &Context[Context[Good]], output: &mut Formatter) -> Result[(), FormatError]:\n    value.format(output)\ndef main() -> ():\n    pass\n"
    ))
    .unwrap();
}

#[test]
fn private_cause_formatter_does_not_satisfy_context_bounds() {
    let source = PRELUDE.replace("pub def format", "def format");
    let error = check(&format!(
        "{source}\ndef main() -> ():\n    value = Context(\"operation\", Subject.Absent, Good.Value)\n    accepts(&value)\n"
    ))
    .unwrap_err();
    assert!(error.contains("private"), "{error}");
}

#[test]
fn local_context_name_keeps_ordinary_structural_behavior() {
    check(
        r#"
from std.report import Formatter, FormatError, Formattable
pub class Context[E]:
    cause: E
    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        output.text("independent")
def accepts[T: Formattable](value: &T) -> ():
    pass
def main() -> ():
    value = Context(42)
    accepts(&value)
"#,
    )
    .unwrap();
}
