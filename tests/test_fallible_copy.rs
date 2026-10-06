//! Fallible duplication preserves the source and reclaims every partial result.
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
fn copied_records_and_collections_are_independent_and_sources_remain_usable() {
    native(r#"
class Point:
    x: i64
    values: list[i64]
def changed(source: &Point) -> Result[Point, AllocError]:
    mut result = try_copy(source)?
    result.x = 99
    result.values.try_append(42)?
    Ok(result)
def change_dictionary(source: &dict[str, list[i64]]) -> Result[dict[str, list[i64]], AllocError]:
    mut result = try_copy(source)?
    result.try_insert("key", [99])?
    Ok(result)
point = Point(1, [2, 3])
mapping = {"key": [1]}
print(changed(&point))
print(point)
print(change_dictionary(&mapping))
print(mapping)
print(try_copy({1, 2}))
"#, "Result[Point, AllocError].Ok(Point(x=99, values=[2, 3, 42]))\nPoint(x=1, values=[2, 3])\nResult[dict[str, list[i64]], AllocError].Ok({\"key\": [99]})\n{\"key\": [1]}\nResult[set[i64], AllocError].Ok({1, 2})");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn every_nested_record_enum_and_list_copy_allocation_can_fail() {
    // Bundle header (1), outer list (2), its two inner lists (4),
    // Items record (1), and its list payload (2).
    for budget in 0..=10 {
        native(
            &format!(
                r#"
enum Items:
    Batch(list[i64])
    Empty
class Bundle:
    name: str
    left: list[list[i64]]
    right: Result[Items, str]
def duplicate(source: &Bundle) -> Result[Bundle, AllocError]:
    try_copy(source)
source = Bundle("cop" + "ied", [[1, 2], [3]], Ok(Items.Batch([4, 5])))
print("__test_fail_allocations_after_{budget}__")
result = duplicate(&source)
print("__test_restore_allocations__")
match result:
    case Ok(value):
        print(value == source)
    case Err(error):
        print(error)
print(source.name)
print(source.left)
print(source.right)
match duplicate(&source):
    case Ok(value):
        print(value == source)
    case Err(error):
        print("retry failed")
"#
            ),
            &format!(
                "{}\ncopied\n[[1, 2], [3]]\nResult[Items, str].Ok(Items.Batch([4, 5]))\nTrue",
                if budget == 10 {
                    "True"
                } else {
                    "AllocError.OutOfMemory"
                }
            ),
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_dictionary_value_copy_releases_the_pending_key_and_prior_entries() {
    // Dictionary buffers and header (3), then each value's buffer/header (2).
    for budget in 0..=7 {
        native(
            &format!(
                r#"
source = {{"fi" + "rst": [1], "sec" + "ond": [2]}}
print("__test_fail_allocations_after_{budget}__")
result = try_copy(source)
print("__test_restore_allocations__")
match result:
    case Ok(value):
        print(value == source)
    case Err(error):
        print(error)
print(source)
"#
            ),
            &format!(
                "{}\n{{\"first\": [1], \"second\": [2]}}",
                if budget == 7 {
                    "True"
                } else {
                    "AllocError.OutOfMemory"
                }
            ),
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn immutable_values_and_inactive_owned_variants_copy_without_allocating() {
    native(r#"
enum Label:
    Text(str)
text = "hel" + "lo"
label = Label.Text(text)
absent: Option[list[i64]] = Nothing
error: Result[list[i64], str] = Err(text)
print("__test_begin_no_allocations__")
print("__test_fail_allocations_after_0__")
a = try_copy(text)
b = try_copy(label)
c = try_copy(absent)
d = try_copy(error)
e = try_copy(18446744073709551615u64)
f = try_copy(-0.0f32)
g = try_copy(AllocError.OutOfMemory)
print("__test_restore_allocations__")
print("__test_end_no_allocations__")
print(a)
print(b)
print(c)
print(d)
print(e)
print(f)
print(g)
"#, "Result[str, AllocError].Ok(\"hello\")\nResult[Label, AllocError].Ok(Label.Text(\"hello\"))\nResult[Option[list[i64]], AllocError].Ok(Option[list[i64]].Nothing)\nResult[Result[list[i64], str], AllocError].Ok(Result[list[i64], str].Err(\"hello\"))\nResult[u64, AllocError].Ok(18446744073709551615)\nResult[f32, AllocError].Ok(-0.0)\nResult[AllocError, AllocError].Ok(AllocError.OutOfMemory)");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn propagation_drops_locals_without_consuming_the_borrowed_source() {
    native(
        r#"
class Guard:
    def __del__(self: &mut Guard) -> ():
        print("dropped")
def duplicate(source: &list[i64], guard: Guard) -> Result[list[i64], AllocError]:
    result = try_copy(source)?
    print("unreachable")
    Ok(result)
source = [1, 2]
guard = Guard()
print("__test_fail_allocations_after_0__")
result = duplicate(&source, guard)
print("__test_restore_allocations__")
print(result)
print(source)
"#,
        "dropped\nResult[list[i64], AllocError].Err(AllocError.OutOfMemory)\n[1, 2]",
    );
}

#[rstest]
#[case("try_copy()", "try_copy takes one argument")]
#[case("try_copy(1, 2)", "try_copy takes one argument")]
#[case("class Guard:\n    def __del__(self: &mut Guard) -> ():\n        pass\nsource = Guard()\ntry_copy(source)", "cannot be copied")]
#[case("class Guard:\n    def __del__(self: &mut Guard) -> ():\n        pass\nsource = [Guard()]\ntry_copy(source)", "cannot be copied")]
#[case(
    "def values() -> Generator[i64]:\n    yield 1\nsource = values()\ntry_copy(source)",
    "cannot be copied"
)]
#[case(
    "source = [1]\nloan = &mut source\nresult = try_copy(source)\nloan.append(2)",
    "borrow"
)]
fn rejects_invalid_copies(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
