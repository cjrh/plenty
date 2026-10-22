//! Dictionary projections borrow their source and key views have set semantics.
mod support;
use rstest::rstest;
use support::assert_visible_stdout as native;

#[test]
fn set_like_operands_infer_empty_and_narrow_key_sets() {
    native(
        r#"
data = {1u8: 2, 2u8: 3}.unwrap()
keys = data.keys()
print(keys.issuperset(&{1}.unwrap())).unwrap()
print(keys.intersection(&{1}.unwrap()).unwrap() == {1u8}.unwrap()).unwrap()
print(keys.union(&set().unwrap()).unwrap() == {1u8, 2u8}.unwrap()).unwrap()
print(keys == {2, 1}.unwrap()).unwrap()
empty = dict[str, i64]().unwrap()
print(empty.keys() == set().unwrap()).unwrap()
print(empty.keys().isdisjoint(&set().unwrap())).unwrap()
print(empty.keys().union(&{"name"}.unwrap()).unwrap() == {"name"}.unwrap()).unwrap()
"#,
        "True\nTrue\nTrue\nTrue\nTrue\nTrue\nTrue",
    );
}

#[test]
fn key_views_are_bindable_borrowed_ordered_and_set_like() {
    native(
        r#"
mut data = {"b": 2, "a": 1}.unwrap()
data.insert("b", 3).unwrap()
data.pop("a")
data.insert("a", 4).unwrap()
view = data.keys()
alias = view
reference = &alias
other = {"a": [1].unwrap(), "b": [2].unwrap()}.unwrap()
all = {"a", "b", "c"}.unwrap()
print(len(view), "b" in view, "z" not in view).unwrap()
print(view == other.keys(), view != all, all == view).unwrap()
print(view.issubset(&all), view.issuperset(&other.keys()), view.isdisjoint(&set[str]().unwrap())).unwrap()
print([*k for k in alias].unwrap()).unwrap()
for key in reference:
    print(*key).unwrap()
print(view.union(&all).unwrap() == all).unwrap()
print(view.intersection(&all).unwrap() == view.to_set().unwrap()).unwrap()
print(view.difference(&other.keys()).unwrap() == set[str]().unwrap()).unwrap()
print(view.symmetric_difference(&all).unwrap() == {"c"}.unwrap()).unwrap()
print([*v for v in data.values()].unwrap()).unwrap()
print(data.into_keys().unwrap()).unwrap()
"#,
        "2 True True\nTrue True False\nTrue True True\n[\"b\", \"a\"]\nb\na\nTrue\nTrue\nTrue\nTrue\n[3, 4]\n[\"b\", \"a\"]",
    );
}

#[test]
fn affine_values_are_borrowed_and_mutable_values_project_exclusively() {
    native(
        r#"
mut data = {"a": [1].unwrap(), "b": [2].unwrap()}.unwrap()
for values in (&mut data).values():
    values.append(3).unwrap()
for values in data.values():
    print(values).unwrap()
print([*k for k in data.keys()].unwrap()).unwrap()
"#,
        "[1, 3]\n[2, 3]\n[\"a\", \"b\"]",
    );
}

#[test]
fn projections_keep_temporary_owners_alive_and_preserve_inherent_methods() {
    native(
        r#"
class Custom:
    def keys(self) -> list[i64]:
        [7].unwrap()
    def values(self) -> list[i64]:
        [8].unwrap()
for key in {1: 2}.unwrap().keys():
    print(*key).unwrap()
for value in {1: [2].unwrap()}.unwrap().values():
    print(value).unwrap()
for key in Custom().keys():
    print(key).unwrap()
for value in Custom().values():
    print(value).unwrap()
"#,
        "1\n[2]\n7\n8",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn observations_never_allocate_and_algebra_failure_preserves_source() {
    native(
        r#"
data = {"a": [1].unwrap(), "b": [2].unwrap()}.unwrap()
other = {"b", "a"}.unwrap()
empty = dict[str, list[i64]]().unwrap()
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
keys = data.keys()
same = keys == other
subset = keys.issubset(&other)
superset = keys.issuperset(&other)
disjoint = empty.keys().isdisjoint(&keys)
empty_same = empty.keys() == empty.keys()
empty_different = empty.keys() != keys
empty_subset = empty.keys().issubset(&keys)
empty_superset = empty.keys().issuperset(&empty.keys())
size = len(keys)
found = "a" in keys
mut total = 0
for key in keys:
    total += len(*key)
for values in data.values():
    total += len(values)
failed = keys.union(&other)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(same, subset, superset, disjoint, size, found, total).unwrap()
print(empty_same, empty_different, empty_subset, empty_superset).unwrap()
print(str.repr(failed).unwrap()).unwrap()
print(data).unwrap()
"#,
        "True True True True 2 True 4\nTrue True True True\nResult[set[str], AllocError].Err(AllocError.OutOfMemory)\n{\"a\": [1], \"b\": [2]}",
    );
}

#[rstest]
#[case("data.insert(3, 4).unwrap()")]
#[case("data[1] = 3")]
#[case("data.update({3: 4}.unwrap()).unwrap()")]
#[case("data.pop(1)")]
#[case("data.clear()")]
#[case("drop(data)")]
fn bound_view_prevents_mutation_or_move(#[case] mutation: &str) {
    let source = format!(
        "mut data = {{1: 2}}.unwrap()\nkeys = data.keys()\n{mutation}\nprint(len(keys)).unwrap()"
    );
    let error = support::check_source(&source).unwrap_err().to_string();
    assert!(error.contains("borrow"), "{error}");
}

#[rstest]
#[case(
    "data = {1: 2}.unwrap()\nitems = [data.keys()].unwrap()",
    "cannot be stored"
)]
#[case("data = {1: 2}.unwrap()\nitems = (data.keys(), 1)", "cannot be stored")]
#[case("class Bad:\n    keys: KeyView[i64]", "cannot be stored")]
#[case("def bad(keys: KeyView[i64]) -> ():\n    pass", "local-only")]
#[case(
    "def bad(data: &dict[i64, i64]) -> KeyView[i64]:\n    data.keys()",
    "local-only"
)]
#[case(
    "keys = {1: 2}.unwrap().keys()\nprint(len(keys)).unwrap()",
    "temporary"
)]
#[case("data = {1: 2}.unwrap()\nvalues = data.values()", "values_snapshot")]
#[case(
    "data = {1: 2}.unwrap()\nview = data.keys()\nreference = &view\ndrop(view)\nprint(len(reference)).unwrap()",
    "owned value"
)]
#[case("data = {1: 2}.unwrap()\nprint(data.keys()).unwrap()", "keys_snapshot")]
#[case(
    "data = {1: 2}.unwrap()\nprint(str.repr(data.keys()).unwrap()).unwrap()",
    "keys_snapshot"
)]
#[case(
    "data = {1: 2}.unwrap()\nview = data.keys()\nf = def [view]() -> i64:\n    len(view)",
    "cannot be captured"
)]
#[case(
    "data = {1: 2}.unwrap()\nview = data.keys()\nf = def [&view]() -> i64:\n    len(view)",
    "cannot be captured"
)]
#[case(
    "data = {1: 2}.unwrap()\nview = data.keys()\ng = (len(view) for n in range(1))",
    "references"
)]
#[case(
    "data = {1: 2}.unwrap()\nview = data.keys()\nwith spawn(def [view]() -> i64: len(view)).unwrap() as task:\n    print(task.join()).unwrap()",
    "cannot be captured"
)]
#[case(
    "def identity[T](x: T) -> T:\n    x\ndata = {1: 2}.unwrap()\nview = identity(data.keys())",
    "local-only"
)]
#[case(
    "mut data = {1: 2}.unwrap()\nview = data.keys()\nalias = view\ndata[1] = 3\nprint(len(alias)).unwrap()",
    "borrow"
)]
#[case("data = {1: 2}.unwrap()\nview = Some(data.keys())", "references")]
#[case(
    "def old(data: &dict[i64, i64]) -> Result[list[i64], AllocError]:\n    data.keys()?",
    "keys_snapshot"
)]
fn views_cannot_escape_or_be_materialised_implicitly(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[rstest]
#[case(&[], &[])]
#[case(&[], &[1, 2])]
#[case(&[1, 2], &[])]
#[case(&[1, 2], &[2, 1])]
#[case(&[1, 2], &[2, 3])]
#[case(&[1, 2], &[3, 4])]
#[case(&[1], &[1, 2])]
#[case(&[1, 2], &[1])]
fn set_operations_cover_empty_overlap_and_order(#[case] left: &[i64], #[case] right: &[i64]) {
    use std::collections::BTreeSet;
    let a: BTreeSet<_> = left.iter().copied().collect();
    let b: BTreeSet<_> = right.iter().copied().collect();
    let set = |items: &BTreeSet<i64>| {
        if items.is_empty() {
            "set[i64]().unwrap()".to_owned()
        } else {
            format!(
                "{{{}}}.unwrap()",
                items
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    };
    let dict = |items: &[i64]| {
        if items.is_empty() {
            "dict[i64, i64]().unwrap()".to_owned()
        } else {
            format!(
                "{{{}}}.unwrap()",
                items
                    .iter()
                    .map(|n| format!("{n}: {n}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    };
    for operand in ["&other", "&mapping.keys()"] {
        let mut source = format!(
            "data = {}\nmapping = {}\nother = {}\nview = data.keys()\n",
            dict(left),
            dict(right),
            set(&b)
        );
        for (method, result) in [
            ("union", a.union(&b).copied().collect()),
            ("intersection", a.intersection(&b).copied().collect()),
            ("difference", a.difference(&b).copied().collect()),
            (
                "symmetric_difference",
                a.symmetric_difference(&b).copied().collect(),
            ),
        ] {
            source.push_str(&format!(
                "print(view.{method}({operand}).unwrap() == {}).unwrap()\n",
                set(&result)
            ));
        }
        source.push_str(&format!("print(view.issubset({operand}) == {}).unwrap()\nprint(view.issuperset({operand}) == {}).unwrap()\nprint(view.isdisjoint({operand}) == {}).unwrap()\nprint(view.to_set().unwrap() == {}).unwrap()\n",if a.is_subset(&b){"True"}else{"False"},if a.is_superset(&b){"True"}else{"False"},if a.is_disjoint(&b){"True"}else{"False"},set(&a)));
        native(&source, &"True\n".repeat(8));
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn each_algebra_allocation_can_fail_without_changing_either_source() {
    for (method, size) in [
        ("union", 3),
        ("intersection", 1),
        ("difference", 1),
        ("symmetric_difference", 2),
        ("to_set", 2),
    ] {
        for budget in 0..=4 {
            let call = if method == "to_set" {
                "keys.to_set()".to_owned()
            } else {
                format!("keys.{method}(&other.keys())")
            };
            native(
                &format!(
                    r#"
data = {{("l" + "eft").unwrap(): [1].unwrap(), "same": [2].unwrap()}}.unwrap()
other = {{"same": 3, ("r" + "ight").unwrap(): 4}}.unwrap()
keys = data.keys()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = {call}
print("__test_restore_allocations__").unwrap()
match result:
    case Ok(values):
        print(len(values)).unwrap()
    case Err(error):
        print(str.repr(error).unwrap()).unwrap()
print(data).unwrap()
print(other).unwrap()
print(len(keys.to_set().unwrap())).unwrap()
"#
                ),
                &format!(
                    "{}\n{{\"left\": [1], \"same\": [2]}}\n{{\"same\": 3, \"right\": 4}}\n2",
                    if budget < 4 {
                        "AllocError.OutOfMemory".to_owned()
                    } else {
                        size.to_string()
                    }
                ),
            );
        }
    }
}
