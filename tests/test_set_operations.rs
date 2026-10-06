//! Set operations observe their operands and preserve explicit ownership.
mod support;
use rstest::rstest;

#[test]
fn union_borrows_sources_and_returns_an_independent_owner() {
    native(
        r#"
def combine(a: &set[str], b: &set[str]) -> Result[set[str], AllocError]:
    a.try_union(b)
def unwrap(result: Result[set[str], AllocError]) -> set[str]:
    match result:
        case Ok(value):
            value
        case Err(error):
            set[str]()
mut a = {"é" + "", "left"}
b = {"é", "right"}
mut output = unwrap(combine(&a, &b))
a.clear()
print(len(output))
print("é" in output)
print("left" in output and "right" in output)
print(output.discard("left"))
print(len(b))
print(len(a))
match {True}.try_union({False}):
    case Ok(value):
        print(len(value))
    case Err(error):
        print(-1)
match set[u8]().try_union(set[u8]()):
    case Ok(value):
        print(len(value))
    case Err(error):
        print(-1)
"#,
        "3\nTrue\nTrue\nTrue\n2\n0\n2\n0",
    );
}

#[cfg(feature = "runtime-checks")]
#[rstest]
#[case(0)]
#[case(1)]
#[case(2)]
#[case(3)]
fn union_recovers_from_each_storage_failure(#[case] budget: usize) {
    let source = format!(
        r#"
a = {{"é" + "", "left"}}
b = {{"é", "right"}}
print("__test_fail_allocations_after_{budget}__")
result = a.try_union(b)
print("__test_restore_allocations__")
match result:
    case Ok(value):
        print(len(value))
    case Err(error):
        print(-1)
print(len(a))
print(len(b))
print("left" in a and "right" in b)
"#
    );
    native(
        &source,
        &format!("{}\n2\n2\nTrue", if budget == 3 { 3 } else { -1 }),
    );
}

#[rstest]
#[case("print({1}.try_union({1u8}))", "expected set[i64]")]
#[case("print({1}.try_union())", "requires one set argument")]
#[case("print([1].try_union({1}))", "requires a set receiver")]
fn invalid_union(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let visible = stdout
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(visible, expected.trim_end(), "{source}");
}

#[test]
fn relations_observe_same_typed_sets_and_support_aliases() {
    native(
        r#"
def check(a: &set[str], b: &set[str]) -> bool:
    a.issubset(b) and b.issuperset(a)
a = {"é" + "", "🙂"}
b = {"é", "🙂", "extra"}
print(check(&a, &b))
print(a.issubset(a))
print(a.isdisjoint(a))
print(a.isdisjoint({"other"}))
print(b.issubset(a))
print(set[i64]().issubset({1}))
print(set[i64]().isdisjoint(set[i64]()))
print({True}.issubset({True, False}))
print({1u8}.issuperset({1u8}))
print(len(a))
class Custom:
    def issubset(self) -> i64:
        42
print(Custom().issubset())
"#,
        "True\nTrue\nFalse\nTrue\nFalse\nTrue\nTrue\nTrue\nTrue\n2\n42",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn relations_do_not_allocate() {
    native(
        r#"
a = {n for n in range(100)}
b = {n for n in range(50)}
print("__test_fail_allocations_after_0__")
print("__test_begin_no_allocations__")
x = a.issuperset(b)
y = b.issubset(a)
z = a.isdisjoint(b)
print("__test_restore_allocations__")
print("__test_end_no_allocations__")
print(x)
print(y)
print(z)
"#,
        "True\nTrue\nFalse",
    );
}

#[rstest]
#[case("print({1}.issubset({1u8}))", "expected set[i64]")]
#[case("print({1}.isdisjoint())", "requires one set argument")]
#[case("print([1].issuperset({1}))", "requires a set receiver")]
#[case("mut a = {1}\nr = &mut a\nprint(a.issubset(a))\nr.add(2)", "borrow")]
fn invalid_relations(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
