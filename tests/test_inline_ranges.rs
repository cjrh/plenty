mod support;

fn expect(source: &str, output: &str) {
    let result = support::run(source);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), output);
}

#[test]
fn ranges_cross_calls_and_standard_sum_returns_by_value() {
    expect(r#"
def make(n: i64) -> range[i64]:
    range(n, n + 4)
def forward(r: range[i64]) -> range[i64]:
    r
def maybe(n: i64) -> Option[range[i64]]:
    if n < 0:
        return Nothing
    Some(make(n))
def checked(n: i64) -> Result[range[i64], range[i64]]:
    if n < 0:
        return Err(make(10))
    Ok(make(n))
def propagate(n: i64) -> Result[range[i64], range[i64]]:
    value = checked(n)?
    Ok(forward(value))
mut original = make(2)
saved = original
original = make(20)
print(list(saved).unwrap()).unwrap()
print(list(forward(original)).unwrap()).unwrap()
print(maybe(3)).unwrap()
print(maybe(-1)).unwrap()
print(propagate(-1)).unwrap()
print(propagate(7)).unwrap()
"#, "[2, 3, 4, 5]\n[20, 21, 22, 23]\nOption[range].Some(range(3, 7, 1))\nOption[range].Nothing\nResult[range, range].Err(range(10, 14, 1))\nResult[range, range].Ok(range(7, 11, 1))\n");
}

#[test]
fn ranges_survive_collection_reallocation_removal_and_copy() {
    expect(r#"
def build() -> list[range[i64]]:
    mut values: list[range[i64]] = [].unwrap()
    for n in range(20):
        values.append(range(n, n + 2)).unwrap()
    values
mut values = build()
first = values[0]
values.reverse()
removed = values.pop(0).unwrap()
values.reserve(100).unwrap()
print(first).unwrap()
print(removed).unwrap()
print(values[0]).unwrap()
dup = copy(values).unwrap()
values.clear()
print(dup[-1]).unwrap()
mut mapping = {"a": Some(range(2)), "b": Option[range[i64]].Nothing}.unwrap()
mapping["b"] = Some(range(5))
mapping.insert("c", Some(range(8))).unwrap()
snapshot = mapping.values().unwrap()
removed_value = mapping.pop("b").unwrap()
mapping.clear()
print(snapshot).unwrap()
print(removed_value).unwrap()
"#, "range(0, 2, 1)\nrange(19, 21, 1)\nrange(18, 20, 1)\nrange(0, 2, 1)\n[Option[range].Some(range(0, 2, 1)), Option[range].Some(range(0, 5, 1)), Option[range].Some(range(0, 8, 1))]\nOption[range].Some(range(0, 5, 1))\n");
}

#[test]
fn range_fields_references_and_generator_captures_keep_their_storage() {
    expect(r#"
class Crate:
    before: i64
    values: range[i64]
    optional: Option[range[i64]]
    after: i64
def replace(target: &mut range[i64]) -> ():
    *target = range(6, 9)
def replace_optional(target: &mut Option[range[i64]]) -> ():
    *target = Some(range(10, 12))
def produce(r: range[i64]) -> Generator[range[i64]]:
    saved = r
    yield saved
    yield range(7, 9)
def make_generator() -> Generator[range[i64]]:
    produce(range(3))
mut b = Crate(11, range(2), Nothing, 22)
replace(&mut b.values)
replace_optional(&mut b.optional)
print(b.before).unwrap()
print(b.values).unwrap()
print(b.optional).unwrap()
print(b.after).unwrap()
mut g = make_generator()
first = next(g).unwrap()
second = next(g).unwrap()
drop(g)
print(first).unwrap()
print(second).unwrap()
"#, "11\nrange(6, 9, 1)\nOption[range].Some(range(10, 12, 1))\n22\nrange(0, 3, 1)\nrange(7, 9, 1)\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn range_creation_iteration_and_returns_need_no_heap_allocation() {
    expect(
        r#"
def make() -> Result[range[u64], AllocError]:
    Ok(range[u64](18446744073709551612, 18446744073709551615))
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
values = make().unwrap()
mut total = 0u64
for value in values:
    total = total + (value - 18446744073709551612u64)
for value in values:
    total = total + (value - 18446744073709551612u64)
size = len(values)
last = values[-1]
contained = 18446744073709551614u64 in values
same = values == range[u64](18446744073709551612, 18446744073709551615)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(total).unwrap()
print(size).unwrap()
print(last).unwrap()
print(contained).unwrap()
print(same).unwrap()
"#,
        "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n6\n3\n18446744073709551614\nTrue\nTrue\n",
    );
}
