//! Recoverable collection growth, including real allocator failure injection.
mod support;
use rstest::rstest;
use support::assert_visible_stdout as native;

#[test]
fn fallible_mutations_and_reservation_work_with_borrows_and_fields() {
    native(
        r#"
class Data:
    values: list[i64]
def extend(values: &mut list[i64]) -> Result[(), AllocError]:
    values.reserve(2)?
    values.append(42)?
    Ok(())
def build() -> Result[i64, AllocError]:
    mut data = Data([].unwrap())
    extend(&mut data.values)?
    data.values.append(7)?
    mut seen = set[str]().unwrap()
    seen.reserve(2)?
    seen.add("yes")?
    seen.add("yes")?
    mut mapping: dict[str, i64] = {}.unwrap()
    mapping.reserve(2)?
    mapping.insert("answer", 41)?
    mapping.insert("answer", data.values[0])?
    print(seen).unwrap()
    print(mapping).unwrap()
    Ok(data.values[1])
print(str.repr(build()).unwrap()).unwrap()
"#,
        "{\"yes\"}\n{\"answer\": 42}\nResult[i64, AllocError].Ok(7)",
    );
}

#[test]
fn invalid_capacities_are_recoverable_and_errors_are_nominal() {
    native(
        r#"
type MemoryError = AllocError
def reserve(values: &mut list[i64], count: i64) -> Result[(), MemoryError]:
    values.reserve(count)?
    Ok(())
mut values = [1, 2].unwrap()
print(str.repr(reserve(&mut values, -1)).unwrap()).unwrap()
print(str.repr(reserve(&mut values, 9223372036854775807)).unwrap()).unwrap()
print(values).unwrap()
error = AllocError.CapacityOverflow
print(copy(error).unwrap() == error).unwrap()
match error:
    case AllocError.OutOfMemory:
        print("oom").unwrap()
    case AllocError.CapacityOverflow:
        print("capacity").unwrap()
"#,
        "Result[(), AllocError].Err(AllocError.CapacityOverflow)\nResult[(), AllocError].Err(AllocError.CapacityOverflow)\n[1, 2]\nTrue\ncapacity",
    );
}

#[cfg(feature = "runtime-checks")]
#[rstest]
#[case("list", "[n for n in range(8)].unwrap()", "values.append(99)", 1)]
#[case("set", "{n for n in range(8)}.unwrap()", "values.add(99)", 3)]
#[case(
    "dict",
    "{n: n for n in range(8)}.unwrap()",
    "values.insert(99, 99)",
    3
)]
fn every_growth_allocation_can_fail_and_then_be_retried(
    #[case] kind: &str,
    #[case] initial: &str,
    #[case] operation: &str,
    #[case] allocations: usize,
) {
    for budget in 0..=allocations {
        let source = format!(
            r#"
mut values = {initial}
print("__test_fail_allocations_after_{budget}__").unwrap()
result = {operation}
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
print(len(values)).unwrap()
print(0 in values).unwrap()
print(7 in values).unwrap()
print(99 in values).unwrap()
print(str.repr({operation}).unwrap()).unwrap()
print(len(values)).unwrap()
"#
        );
        let success = budget == allocations;
        let result = if success {
            "Ok(())"
        } else {
            "Err(AllocError.OutOfMemory)"
        };
        let final_len = if success && kind == "list" { 10 } else { 9 };
        native(
            &source,
            &format!(
                "Result[(), AllocError].{result}\n{}\nTrue\nTrue\n{}\nResult[(), AllocError].Ok(())\n{final_len}",
                if success { 9 } else { 8 },
                if success { "True" } else { "False" }
            ),
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[rstest]
#[case("[n for n in range(8)].unwrap()", 1)]
#[case("{n for n in range(8)}.unwrap()", 3)]
#[case("{n: n for n in range(8)}.unwrap()", 3)]
fn reservation_failure_keeps_existing_contents(#[case] initial: &str, #[case] allocations: usize) {
    for budget in 0..=allocations {
        native(
            &format!(
                r#"
mut values = {initial}
print("__test_fail_allocations_after_{budget}__").unwrap()
result = values.reserve(100)
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
print(len(values)).unwrap()
print(7 in values).unwrap()
print(str.repr(values.reserve(100)).unwrap()).unwrap()
"#
            ),
            &format!(
                "Result[(), AllocError].{}\n8\nTrue\nResult[(), AllocError].Ok(())",
                if budget == allocations {
                    "Ok(())"
                } else {
                    "Err(AllocError.OutOfMemory)"
                }
            ),
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn reserved_storage_duplicate_keys_and_error_values_need_no_allocation() {
    native(
        r#"
def work(values: &mut list[i64], seen: &mut set[str], mapping: &mut dict[str, i64], numbers: &mut set[i64], counts: &mut dict[i64, i64]) -> Result[(), AllocError]:
    values.reserve(0)?
    seen.add("existing")?
    mapping.insert("existing", 99)?
    mut n = 0
    while n < 32:
        values.append(n)?
        numbers.add(n)?
        counts.insert(n, n)?
        # n starts at 0 and the loop requires n < 32; the increment fits.
        unchecked:
            n = n + 1
    Ok(())
mut values: list[i64] = [].unwrap()
mut seen = {"existing"}.unwrap()
mut mapping = {"existing": 1}.unwrap()
mut numbers = set[i64]().unwrap()
mut counts = dict[i64, i64]().unwrap()
print(str.repr(values.reserve(32)).unwrap()).unwrap()
print(str.repr(numbers.reserve(32)).unwrap()).unwrap()
print(str.repr(counts.reserve(32)).unwrap()).unwrap()
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
result = work(&mut values, &mut seen, &mut mapping, &mut numbers, &mut counts)
error = AllocError.OutOfMemory
same = copy(error).unwrap() == error
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
print(same).unwrap()
print(len(values)).unwrap()
print(mapping["existing"]).unwrap()
print(len(numbers)).unwrap()
print(counts[31]).unwrap()
"#,
        "Result[(), AllocError].Ok(())\nResult[(), AllocError].Ok(())\nResult[(), AllocError].Ok(())\nResult[(), AllocError].Ok(())\nTrue\n32\n99\n32\n31",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_insertion_destroys_moved_inputs_and_propagates_without_allocating() {
    native(
        r#"
class Guard:
    def __del__(self: &mut Guard) -> ():
        print("dropped").unwrap()
def append(values: &mut list[Guard], item: Guard) -> Result[(), AllocError]:
    values.append(item)?
    print("unreachable").unwrap()
    Ok(())
mut values: list[Guard] = [].unwrap()
item = Guard()
print("__test_fail_allocations_after_0__").unwrap()
result = append(&mut values, item)
print("__test_begin_no_allocations__").unwrap()
match result:
    case Ok(done):
        print("unexpected success").unwrap()
    case Err(error):
        match error:
            case AllocError.OutOfMemory:
                print("handled").unwrap()
            case AllocError.CapacityOverflow:
                print("wrong error").unwrap()
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(len(values)).unwrap()
"#,
        "dropped\nhandled\n0",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_get_or_insert_destroys_moved_inputs_and_present_keys_need_no_allocation() {
    native(
        r#"
class Guard:
    label: str
    def __del__(self: &mut Guard) -> ():
        print(self.label).unwrap()
mut guards = {"a": Guard("a")}.unwrap()
unused = Guard("unused")
missing = Guard("b")
# An empty dictionary has no storage, so its first entry must allocate.
mut empty = dict[str, Guard]().unwrap()
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
match guards.get_or_insert("a", unused):
    case Ok(guard):
        print(guard.label).unwrap()
    case Err(error):
        print("unexpected failure").unwrap()
result = empty.get_or_insert("b", missing)
match result:
    case Ok(guard):
        print("unexpected success").unwrap()
    case Err(error):
        match error:
            case AllocError.OutOfMemory:
                print("handled").unwrap()
            case AllocError.CapacityOverflow:
                print("wrong error").unwrap()
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(len(guards), len(empty)).unwrap()
"#,
        "unused\na\nb\nhandled\n1 0\na",
    );
}

// Issue #949: a hit by borrowed bytes allocates nothing; a miss that cannot
// allocate its key or row reports `DataError.Allocation`, destroys the
// default and leaves the dictionary unchanged.
#[cfg(feature = "runtime-checks")]
#[test]
fn failed_get_or_insert_utf8_destroys_the_default_and_hits_need_no_allocation() {
    native(
        r#"
class Guard:
    label: str
    def __del__(self: &mut Guard) -> ():
        print(self.label).unwrap()
def report(result: Result[&mut Guard, DataError]) -> ():
    match result:
        case Ok(guard):
            print(guard.label).unwrap()
        case Err(DataError.Allocation(AllocError.OutOfMemory)):
            print("handled").unwrap()
        case Err(_):
            print("wrong error").unwrap()
mut guards = {"a": Guard("a"), "GGTATTTTAATTTATAGT": Guard("long")}.unwrap()
unused = Guard("unused")
spare = Guard("spare")
short = Guard("b")
long = Guard("c")
# An empty dictionary has no storage, so its first entry must allocate.
mut empty = dict[str, Guard]().unwrap()
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
report(guards.get_or_insert_utf8(b"a", unused))
report(guards.get_or_insert_utf8(b"GGTATTTTAATTTATAGT", spare))
report(empty.get_or_insert_utf8(b"b", short))
# A key over seven bytes is a heap text: its allocation fails first.
report(guards.get_or_insert_utf8(b"TTTATAGTGGTATTTTAA", long))
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(len(guards), len(empty)).unwrap()
"#,
        "unused\na\nspare\nlong\nb\nhandled\nc\nhandled\n2 0\na\nlong",
    );
}

#[rstest]
#[case("values = [1].unwrap()\nvalues.append(2)", "immutable")]
#[case("mut values = [1].unwrap()\nvalues.reserve(1u8)", "expected i64")]
#[case("mut values = {1}.unwrap()\nvalues.insert(1, 2)", "unsupported method")]
#[case("mut values = [1].unwrap()\nvalues.append()", "requires 1 argument")]
#[case(
    "def bad() -> AllocError:\n    AllocError.OutOfMemory?",
    "requires a function returning Result or Option"
)]
#[case(
    "def bad() -> Result[(), AllocError]:\n    AllocError.OutOfMemory?\n    Ok(())",
    "same Result or Option family"
)]
#[case(
    "mut values: list[list[i64]] = [].unwrap()\nitem = [1].unwrap()\nresult = values.append(item)\nprint(item).unwrap()",
    "moved"
)]
fn rejects_invalid_fallible_operations(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
