//! Stage one of issue #701: compile-time kinds and algebra, without Quantity.
mod support;
use support::assert_stdout;

const BASES: &str = "dimension Length(\"m\")\ndimension Time(\"s\")\n";
const TAG: &str = "@copy\nclass Tagged[D: Dimension]:\n    value: i64\n";

fn accepts(source: &str) {
    plenty::check_source(source).unwrap_or_else(|e| panic!("{source}\n{e}"));
}

fn rejects(source: &str, message: &str) {
    let error = plenty::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains(message),
        "expected {message:?}:\n{source}\n{error}"
    );
}

#[test]
fn aliases_normalize_forward_references_and_parentheses() {
    accepts(&format!(
        "{BASES}{TAG}
dimension Velocity = Length / Time
dimension Acceleration = Velocity / Time
dimension Again = Later * Time
dimension Later = Length / Time
dimension Cancelled = Length / Length
def main() -> ():
    a: Tagged[Length] = Tagged[Again](1)
    b: Tagged[Acceleration] = Tagged[Length / (Time * Time)](2)
    c: Tagged[One] = Tagged[Cancelled](3)
    d: Tagged[Length] = Tagged[Root[Pow[Length, 6], 3] / Length](4)
    e: Tagged[One / Time] = Tagged[Pow[Time, -1]](5)
    f: Tagged[Length] = Tagged[Length / Time * Time](6)
    pass
"
    ));
}

#[test]
fn dimensions_in_generic_arguments_are_inferred_and_erased() {
    assert_stdout(
        &format!(
            "{BASES}{TAG}
def value[D: Dimension](x: Tagged[D]) -> i64:
    x.value
def square[D: Dimension](x: Tagged[D]) -> Tagged[Pow[D, 2]]:
    Tagged[Pow[D, 2]](x.value * x.value)
def root[D: Dimension](x: Tagged[D]) -> Tagged[Root[D, 2]]:
    Tagged[Root[D, 2]](x.value)
def label[D: Dimension]() -> i64:
    7
def main() -> Result[(), IoError]:
    x = Tagged[Length](3)
    squared: Tagged[Length * Length] = square(x)
    original: Tagged[Length] = root(squared)
    print(value(x), value(squared), value(original), label[Length / Length]())?
    print(value[Length](x))?
    Ok(())
"
        ),
        "3 9 9 7\n3\n",
    );
}

#[test]
fn nonintegral_root_is_an_instance_error() {
    let source = format!(
        "{BASES}{TAG}
def root[D: Dimension](x: Tagged[D]) -> Tagged[Root[D, 2]]:
    Tagged[Root[D, 2]](x.value)
def main() -> ():
    x = root(Tagged[Pow[Length, 3]](1))
    pass
"
    );
    rejects(&source, "not a whole dimension");
    rejects(&source, "cannot instantiate");
}

#[test]
fn every_dimension_rule_has_a_diagnostic() {
    for (declarations, body, message) in [
        ("", "x = Length", "has no values"),
        ("", "x = Length()", "has no values"),
        ("", "x = One", "has no values"),
        ("", "x = One()", "has no values"),
        ("", "x: Length = 1", "has no values"),
        ("", "x: list[Length] = []?", "has no values"),
        ("class Bad:\n    value: Length\n", "pass", "has no values"),
        (
            "def bad(x: Length) -> ():\n    pass\n",
            "pass",
            "has no values",
        ),
        ("def bad() -> Length:\n    1\n", "pass", "has no values"),
        ("type Bad = Length\n", "pass", "has no values"),
        ("dimension Bad = i64\n", "pass", "not a dimension"),
        (
            "dimension Bad = Root[Length, 2]\n",
            "pass",
            "not a whole dimension",
        ),
        ("dimension Bad = Root[One, 0]\n", "pass", "nonzero degree"),
        (
            "dimension Bad = Pow[Length, 128]\n",
            "pass",
            "outside -128..127",
        ),
        (
            "dimension Bad = Pow[Length, -129]\n",
            "pass",
            "outside -128..127",
        ),
        (
            "dimension Bad = One / Pow[Length, -128]\n",
            "pass",
            "outside -128..127",
        ),
        (
            "dimension Bad = Pow[Length, 127] * Length\n",
            "pass",
            "outside -128..127",
        ),
        (
            "dimension Bad = Root[Pow[Length, -128], -1]\n",
            "pass",
            "outside -128..127",
        ),
        (
            "dimension Bad = Pow[Length, 1.0]\n",
            "pass",
            "integer literal",
        ),
        (
            "dimension Bad = Pow[Length, 2i64]\n",
            "pass",
            "integer literal",
        ),
        (
            "dimension Bad = Pow[Length, 9223372036854775808]\n",
            "pass",
            "integer literal",
        ),
        (
            "dimension Bad = Pow[Length, count]\n",
            "pass",
            "integer literal",
        ),
        ("dimension Bad = One[Length]\n", "pass", "requires 0"),
        (
            "dimension Bad = Other\ndimension Other = Bad\n",
            "pass",
            "cyclic type alias",
        ),
        (
            "dimension Other(\"m\")\n",
            "x: Tagged[Length] = Tagged[Other](1)",
            "expected Tagged[Length]",
        ),
        ("", "x = Tagged[i64](1)", "not a dimension"),
        (
            "class Plain[T]:\n    value: i64\n",
            "x = Plain[Length](1)",
            "has no values",
        ),
        (
            "def plain[T]() -> i64:\n    1\n",
            "x = plain[Length]()",
            "constrained by Dimension",
        ),
        (
            "def bad[D: Dimension](x: Tagged[D * Length]) -> i64:\n    x.value\n",
            "pass",
            "bare dimension parameter",
        ),
        (
            "def bad[D: Dimension](x: Tagged[Root[D, 2]]) -> i64:\n    x.value\n",
            "pass",
            "bare dimension parameter",
        ),
        ("", "dimension = 1", "reserved"),
    ] {
        rejects(
            &format!(
                "{BASES}{TAG}{declarations}def main() -> Result[(), AllocError]:\n    {body}\n    Ok(())\n"
            ),
            message,
        );
    }
}

#[test]
fn dimensions_follow_module_visibility_and_nominal_identity() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("units.plenty"), "pub dimension Length(\"m\")\npub dimension Time(\"s\")\npub dimension Speed = Length / Time\ndimension Hidden(\"h\")\n").unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(&main, format!("import units as u\nfrom units import Speed\n{TAG}def main() -> ():\n    x: Tagged[Speed] = Tagged[u.Length / u.Time](1)\n    pass\n")).unwrap();
    plenty::check_file(&main, None).unwrap();
    std::fs::write(&main, format!("import units as u\ndimension Length(\"m\")\n{TAG}def main() -> ():\n    x: Tagged[Length] = Tagged[u.Length](1)\n    pass\n")).unwrap();
    let error = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(error.contains("expected Tagged[Length]"), "{error}");
    std::fs::write(
        &main,
        "from units import Hidden\ndef main() -> ():\n    pass\n",
    )
    .unwrap();
    let error = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(error.contains("private"), "{error}");
}

#[test]
fn public_dimensions_cannot_expose_private_bases() {
    rejects(
        "dimension Hidden(\"h\")\npub dimension Exposed = Pow[Hidden, 2]\ndef main() -> ():\n    pass\n",
        "public signature exposes private dimension `Hidden`",
    );
}

#[test]
fn root_type_name_keeps_ordinary_lexical_lookup() {
    accepts("import std.fs\nimport std.archive\ndef main() -> ():\n    pass\n");
    accepts(
        "class Root[T]:\n    value: T\ndef read(root: Root[i64]) -> i64:\n    root.value\ndef main() -> ():\n    x = read(Root[i64](1))\n    pass\n",
    );
    accepts(
        "class Pow[T, U]:\n    a: T\n    b: U\ndef main() -> ():\n    x = Pow[i64, bool](1, True)\n    pass\n",
    );
}

#[test]
fn dimension_expressions_in_generic_method_inputs_are_rejected_before_substitution() {
    rejects(
        &format!(
            "{BASES}{TAG}class Owner[D: Dimension]:\n    value: i64\n    def read(self, x: Tagged[D * Length]) -> i64:\n        x.value\ndef main() -> ():\n    pass\n"
        ),
        "bare dimension parameter",
    );
}

#[test]
fn integer_spellings_and_exponent_limits() {
    accepts(
        "dimension Length(\"m\")\ndimension A = Pow[Length, 0x2]\ndimension B = Root[A, 0b10]\ndimension C = Pow[B, -1_2_8]\ndimension D = Pow[One, -9223372036854775808]\ndef main() -> ():\n    pass\n",
    );
    rejects(
        "dimension Length(\"m\")\ndimension Bad = Pow[Length, 1__2]\ndef main() -> ():\n    pass\n",
        "invalid integer separator",
    );
    let product = std::iter::repeat_n("Length", 1000)
        .collect::<Vec<_>>()
        .join(" * ");
    rejects(
        &format!("{BASES}dimension Bad = {product}\ndef main() -> ():\n    pass\n"),
        "nesting exceeds",
    );
}

#[test]
fn public_generic_results_check_bases_but_not_exponent_literals() {
    accepts(
        "pub dimension Length(\"m\")\npub class Tagged[D: Dimension]:\n    pub value: i64\npub def square[D: Dimension](x: Tagged[D]) -> Tagged[Pow[D, 2]]:\n    Tagged[Pow[D, 2]](x.value)\ndef main() -> ():\n    x = square(Tagged[Length](1))\n    pass\n",
    );
    rejects(
        "dimension Hidden(\"h\")\npub class Tagged[D: Dimension]:\n    pub value: i64\npub def product[D: Dimension](x: Tagged[D]) -> Tagged[D * Hidden]:\n    Tagged[D * Hidden](x.value)\ndef main() -> ():\n    pass\n",
        "private dimension `Hidden`",
    );
}

#[test]
fn enums_and_generic_methods_accept_dimension_arguments() {
    assert_stdout(
        &format!(
            "{BASES}{TAG}
enum Reading[D: Dimension]:
    Value(Tagged[D])
class Factory:
    def tagged[D: Dimension](self, n: i64) -> Tagged[D]:
        Tagged[D](n)
def read[D: Dimension](x: Reading[D]) -> i64:
    match x:
        case Reading.Value(tagged):
            tagged.value
def main() -> Result[(), IoError]:
    factory = Factory()
    value = factory.tagged[Length](8)
    print(read(Reading[Length].Value(value)))?
    Ok(())
"
        ),
        "8\n",
    );
}
