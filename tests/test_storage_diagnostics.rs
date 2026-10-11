//! A rejected storage type names its own cause (issue #10): eligibility is
//! unchanged, only the explanation differs.
mod support;
use rstest::rstest;
use support::check_source;

const UNRESOLVED: &str =
    "a closure signature does not name a concrete environment, so it cannot be stored in";
const COUNT: &str =
    "def count(n: i64) -> Generator[i64]:\n    for i in range(n):\n        yield i\n";

fn main_body(body: &str) -> String {
    format!("{COUNT}def main() -> Result[(), Failure]:\n{body}    Ok(())\n")
}

#[rstest]
#[case::annotated_list(
    "def make_jobs(n: i64) -> Result[list[Closure[[], i64]], AllocError]:\n    callback = def [n]() -> i64:\n        n\n    [callback]\ndef main() -> ():\n    pass\n",
    "collections"
)]
#[case::annotated_dictionary_value(
    "def main() -> Result[(), Failure]:\n    x: dict[str, OnceClosure[[], i64]] = {}?\n    Ok(())\n",
    "collections"
)]
#[case::class_field(
    "class C:\n    field: Closure[[], i64]\ndef main() -> ():\n    pass\n",
    "class fields"
)]
#[case::enum_payload(
    "enum E:\n    A(Closure[[], i64])\ndef main() -> ():\n    pass\n",
    "enum payloads"
)]
#[case::generic_argument(
    "class Owner[T]:\n    value: T\ndef main() -> ():\n    x: Option[Owner[Closure[[], i64]]] = Nothing\n",
    "generic data"
)]
fn unresolved_closure_signatures_explain_the_missing_environment(
    #[case] source: &str,
    #[case] place: &str,
) {
    let error = check_source(source).unwrap_err().to_string();
    assert!(error.contains(&format!("{UNRESOLVED} {place};")), "{error}");
    assert!(error.contains("bounded by Callable"), "{error}");
    assert!(!error.contains("generator"), "{error}");
}

#[rstest]
#[case::list("    fs = [f]?\n", "collections")]
#[case::dictionary("    fs = {\"a\": f}?\n", "collections")]
#[case::tuple("    pair = (f, 1)\n", "tuples")]
#[case::boxed("    b = Box(f)?\n", "a box")]
fn borrowed_captures_are_not_reported_as_generators(#[case] store: &str, #[case] place: &str) {
    let source = main_body(&format!(
        "    values = [1, 2, 3]?\n    f = def [&values](i: i64) -> i64:\n        values[i]\n{store}"
    ));
    let error = check_source(&source).unwrap_err().to_string();
    let expected = format!("closures with borrowed captures cannot be stored in {place}");
    assert!(error.contains(&expected), "{error}");
}

#[rstest]
#[case::generator_display("    xs = [count(2)]?\n", "generators cannot be stored in collections")]
#[case::generator_in_sum(
    "    xs: list[Option[Generator[i64]]] = []?\n",
    "generators cannot be stored in collections"
)]
#[case::generator_box(
    "    b: Option[Box[Generator[i64]]] = Nothing\n",
    "generators cannot be stored in a box"
)]
#[case::captured_generator(
    "    source = count(2)\n    job = def once [mut source]() -> i64:\n        0\n    jobs = [job]?\n",
    "closures that capture generators cannot be stored in collections"
)]
#[case::reference_display(
    "    a = [1]?\n    xs = [&a]?\n",
    "references cannot be stored in collections"
)]
#[case::reference_tuple(
    "    pair: Option[tuple[&i64, i64]] = Nothing\n",
    "references cannot be stored in tuples"
)]
fn other_rejected_storage_names_its_cause(#[case] body: &str, #[case] expected: &str) {
    let error = check_source(&main_body(body)).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[test]
fn declarations_name_references_and_generators_separately() {
    for (source, expected) in [
        (
            "class C:\n    field: &i64",
            "references cannot be stored in class fields",
        ),
        (
            "class C:\n    field: Generator[i64]",
            "generators cannot be stored in class fields",
        ),
        (
            "enum E:\n    A(&i64)",
            "references cannot be stored in enum payloads",
        ),
        (
            "enum E:\n    A(Generator[i64])",
            "generators cannot be stored in enum payloads",
        ),
    ] {
        let error = check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn owned_resolved_environments_remain_storable() {
    let source = main_body(
        "    n = 2\n    f = def [n]() -> i64:\n        n\n    fs = [f]?\n    pair = (fs, 1)\n",
    );
    check_source(&source).unwrap();
}
