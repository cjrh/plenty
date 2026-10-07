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
    mut result = copy(source)?
    result.x = 99
    result.values.append(42)?
    Ok(result)
def change_dictionary(source: &dict[str, list[i64]]) -> Result[dict[str, list[i64]], AllocError]:
    mut result = copy(source)?
    result.insert("key", [99].unwrap())?
    Ok(result)
point = Point(1, [2, 3].unwrap()).unwrap()
mapping = {"key": [1].unwrap()}.unwrap()
print(changed(&point)).unwrap()
print(point).unwrap()
print(change_dictionary(&mapping)).unwrap()
print(mapping).unwrap()
print(copy({1, 2}.unwrap())).unwrap()
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
    copy(source)
source = Bundle(("cop" + "ied").unwrap(), [[1, 2].unwrap(), [3].unwrap()].unwrap(), Ok(Items.Batch([4, 5].unwrap()).unwrap())).unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = duplicate(&source)
print("__test_restore_allocations__").unwrap()
match result:
    case Ok(value):
        print(value == source).unwrap()
    case Err(error):
        print(error).unwrap()
print(source.name).unwrap()
print(source.left).unwrap()
print(source.right).unwrap()
match duplicate(&source):
    case Ok(value):
        print(value == source).unwrap()
    case Err(error):
        print("retry failed").unwrap()
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
source = {{("fi" + "rst").unwrap(): [1].unwrap(), ("sec" + "ond").unwrap(): [2].unwrap()}}.unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = copy(source)
print("__test_restore_allocations__").unwrap()
match result:
    case Ok(value):
        print(value == source).unwrap()
    case Err(error):
        print(error).unwrap()
print(source).unwrap()
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
text = ("hel" + "lo").unwrap()
label = Label.Text(text).unwrap()
absent: Option[list[i64]] = Nothing
error: Result[list[i64], str] = Err(text)
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
a = copy(text)
b = copy(label)
c = copy(absent)
d = copy(error)
e = copy(18446744073709551615u64)
f = copy(-0.0f32)
g = copy(AllocError.OutOfMemory)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(a).unwrap()
print(b).unwrap()
print(c).unwrap()
print(d).unwrap()
print(e).unwrap()
print(f).unwrap()
print(g).unwrap()
"#, "Result[str, AllocError].Ok(\"hello\")\nResult[Label, AllocError].Ok(Label.Text(\"hello\"))\nResult[Option[list[i64]], AllocError].Ok(Option[list[i64]].Nothing)\nResult[Result[list[i64], str], AllocError].Ok(Result[list[i64], str].Err(\"hello\"))\nResult[u64, AllocError].Ok(18446744073709551615)\nResult[f32, AllocError].Ok(-0.0)\nResult[AllocError, AllocError].Ok(AllocError.OutOfMemory)");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn propagation_drops_locals_without_consuming_the_borrowed_source() {
    native(
        r#"
class Guard:
    def __del__(self: &mut Guard) -> ():
        print("dropped").unwrap()
def duplicate(source: &list[i64], guard: Guard) -> Result[list[i64], AllocError]:
    result = copy(source)?
    print("unreachable").unwrap()
    Ok(result)
source = [1, 2].unwrap()
guard = Guard().unwrap()
print("__test_fail_allocations_after_0__").unwrap()
result = duplicate(&source, guard)
print("__test_restore_allocations__").unwrap()
print(result).unwrap()
print(source).unwrap()
"#,
        "dropped\nResult[list[i64], AllocError].Err(AllocError.OutOfMemory)\n[1, 2]",
    );
}

#[rstest]
#[case("copy()", "copy takes one argument")]
#[case("copy(1, 2)", "copy takes one argument")]
#[case("class Guard:\n    def __del__(self: &mut Guard) -> ():\n        pass\nsource = Guard().unwrap()\ncopy(source)", "cannot be copied")]
#[case("class Guard:\n    def __del__(self: &mut Guard) -> ():\n        pass\nsource = [Guard().unwrap()].unwrap()\ncopy(source)", "cannot be copied")]
#[case(
    "def values() -> Generator[i64]:\n    yield 1\nsource = values()\ncopy(source)",
    "cannot be copied"
)]
#[case(
    "source = [1].unwrap()\nloan = &mut source\nresult = copy(source)\nloan.append(2).unwrap()",
    "borrow"
)]
fn rejects_invalid_copies(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
