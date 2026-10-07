//! Set removal reports membership changes without allocating.
mod support;
use rstest::rstest;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
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
fn discard_reports_changes_for_zero_false_and_sized_integer_keys() {
    native(
        r#"
mut numbers = {0u8, 1u8, 255u8}.unwrap()
print(numbers.discard(0u8)).unwrap()
print(numbers.discard(0u8)).unwrap()
print(1u8 in numbers).unwrap()
print(numbers.discard(255u8)).unwrap()
print(numbers.discard(1u8)).unwrap()
print(len(numbers)).unwrap()
mut flags = {False, True}.unwrap()
print(flags.discard(False)).unwrap()
print(flags.discard(False)).unwrap()
print(True in flags).unwrap()
mut empty = set[i64]().unwrap()
print(empty.discard(42)).unwrap()
"#,
        "True\nFalse\nTrue\nTrue\nTrue\n0\nTrue\nFalse\nTrue\nFalse",
    );
}

#[test]
fn discard_matches_string_contents_and_preserves_argument_owners() {
    native(
        r#"
mut names = {("é\0" + "🙂").unwrap(), "other"}.unwrap()
key = ("é" + "\0🙂").unwrap()
print(names.discard(key)).unwrap()
print(names.discard(key)).unwrap()
print(key == "é\0🙂").unwrap()
print("other" in names).unwrap()
print(len(names)).unwrap()
names.add(key).unwrap()
print(key in names).unwrap()
"#,
        "True\nFalse\nTrue\nTrue\n1\nTrue",
    );
}

#[test]
fn borrowed_fields_and_values_and_argument_effects_are_supported() {
    native(
        r#"
class Storage:
    values: set[i64]
def remove(values: &mut set[i64], value: &i64) -> bool:
    values.discard(value)
def value(values: &mut set[i64]) -> i64:
    print("value").unwrap()
    values.add(42).unwrap()
    42
class Custom:
    def discard(self, n: i64) -> i64:
        n
mut storage = Storage({1, 2}.unwrap()).unwrap()
print(storage.values.discard(value(&mut storage.values))).unwrap()
key = 1
print(remove(&mut storage.values, &key)).unwrap()
print(key).unwrap()
print(storage.values.discard(len(storage.values) + 1)).unwrap()
print(len(storage.values)).unwrap()
print(Custom().unwrap().discard(7)).unwrap()
"#,
        "value\nTrue\nTrue\n1\nTrue\n0\n7",
    );
}

#[test]
fn repeated_discard_and_reinsertion_preserve_probe_chains() {
    native(
        r#"
mut values = {n for n in range(100).unwrap()}.unwrap()
mut valid = True
for n in range(0, 100, 2).unwrap():
    valid = values.discard(n) and valid
    valid = not values.discard(n) and valid
for n in range(100).unwrap():
    valid = valid and (n in values) == (n % 2 == 1)
print(valid).unwrap()
print(len(values)).unwrap()
for n in range(0, 100, 2).unwrap():
    values.add(n).unwrap()
print(len(values)).unwrap()
for n in range(100).unwrap():
    valid = values.discard(n) and valid
print(valid).unwrap()
print(len(values)).unwrap()
"#,
        "True\n50\n100\nTrue\n0",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn discard_and_reinsertion_use_no_allocation() {
    native(
        r#"
mut names = {("A" + "da").unwrap(), "Bea"}.unwrap()
key = ("Ad" + "a").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
removed = names.discard(key)
missing = names.discard(key)
names.add(key).unwrap()
present = key in names
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(removed).unwrap()
print(missing).unwrap()
print(present).unwrap()
print(len(names)).unwrap()
print(key).unwrap()
"#,
        "True\nFalse\nTrue\n2\nAda",
    );
}

#[test]
fn argument_error_propagates_before_mutation() {
    native(
        r#"
def key() -> Option[i64]:
    Nothing
def remove(values: &mut set[i64]) -> Option[bool]:
    Some(values.discard(key()?))
mut values = {1}.unwrap()
print(remove(&mut values)).unwrap()
print(1 in values).unwrap()
"#,
        "Option[bool].Nothing\nTrue",
    );
}

#[rstest]
#[case("values = {1}.unwrap()\nvalues.discard(1)", "mutable")]
#[case(
    "mut values = {1}.unwrap()\nvalues.discard()",
    "discard requires one value argument"
)]
#[case(
    "mut values = {1}.unwrap()\nvalues.discard(1, 2)",
    "discard requires one value argument"
)]
#[case("mut values = {1u8}.unwrap()\nvalues.discard(1)", "expected u8")]
#[case("mut values = {1}.unwrap()\nvalues.discard(\"one\")", "expected i64")]
#[case("{1}.unwrap().discard(1)", "discard requires a mutable set")]
#[case(
    "mut values = [1].unwrap()\nvalues.discard(1)",
    "discard requires a mutable set"
)]
#[case(
    "mut values = {1: 2}.unwrap()\nvalues.discard(1)",
    "discard requires a mutable set"
)]
#[case(
    "mut values = {1}.unwrap()\nloan = &values\nvalues.discard(1)\nprint(loan).unwrap()",
    "borrow"
)]
fn rejects_invalid_discard(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
