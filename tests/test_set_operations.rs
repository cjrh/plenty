//! Set operations observe their operands and preserve explicit ownership.
mod support;
use rstest::rstest;

#[test]
fn difference_update_borrows_inputs_and_preserves_storage() {
    native(
        r#"
def remove(target: &mut set[str], blocked: &set[str]) -> ():
    target.difference_update(blocked)
class Store:
    members: set[str]
mut store = Store({("é" + "").unwrap(), "remove", "keep"}.unwrap())
blocked = {"é", "remove", "other"}.unwrap()
remove(&mut store.members, &blocked)
print(len(store.members)).unwrap()
print("keep" in store.members).unwrap()
print("é" in store.members).unwrap()
print(len(blocked)).unwrap()
store.members.difference_update(set[str]().unwrap())
print(len(store.members)).unwrap()
store.members.difference_update({"keep"}.unwrap())
print(len(store.members)).unwrap()
class Custom:
    def difference_update(self) -> i64:
        42
print(Custom().difference_update()).unwrap()
"#,
        "1\nTrue\nFalse\n3\n1\n0\n42",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn difference_update_repairs_hashes_and_can_empty_without_allocating() {
    native(
        r#"
mut target = {n for n in range(100)}.unwrap()
blocked = {n for n in range(0, 100, 2)}.unwrap()
all = {n for n in range(100)}.unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
target.difference_update(blocked)
a = len(target)
b = 98 in target
c = 99 in target
target.difference_update(all)
d = len(target)
target.add(101).unwrap()
e = 101 in target
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(a).unwrap()
print(b).unwrap()
print(c).unwrap()
print(d).unwrap()
print(e).unwrap()
print(len(blocked)).unwrap()
"#,
        "50\nFalse\nTrue\n0\nTrue\n50",
    );
}

#[test]
fn intersection_update_filters_fields_and_borrows_its_source() {
    native(
        r#"
def keep(target: &mut set[str], allowed: &set[str]) -> ():
    target.intersection_update(allowed)
class Store:
    members: set[str]
mut store = Store({("é" + "").unwrap(), "remove", "keep"}.unwrap())
allowed = {"é", "keep", "extra"}.unwrap()
keep(&mut store.members, &allowed)
print(len(store.members)).unwrap()
print("é" in store.members and "keep" in store.members).unwrap()
print("remove" in store.members).unwrap()
print(len(allowed)).unwrap()
store.members.intersection_update(set[str]().unwrap())
print(len(store.members)).unwrap()
class Custom:
    def intersection_update(self) -> i64:
        42
print(Custom().intersection_update()).unwrap()
"#,
        "2\nTrue\nFalse\n3\n0\n42",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn intersection_update_reuses_capacity_and_repairs_probe_chains() {
    native(
        r#"
mut target = {n for n in range(100)}.unwrap()
allowed = {n for n in range(0, 100, 2)}.unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
target.intersection_update(allowed)
target.add(101).unwrap()
a = 98 in target
b = 99 in target
c = 101 in target
d = len(target)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(a).unwrap()
print(b).unwrap()
print(c).unwrap()
print(d).unwrap()
print(len(allowed)).unwrap()
"#,
        "True\nFalse\nTrue\n51\n50",
    );
}

#[rstest]
#[case("a = {1}.unwrap()\na.intersection_update({1}.unwrap())", "immutable")]
#[case("mut a = {1}.unwrap()\na.intersection_update(a)", "borrow")]
#[case(
    "mut a = {1}.unwrap()\nr = &a\na.intersection_update({1}.unwrap())\nprint(r).unwrap()",
    "borrow"
)]
#[case(
    "mut a = {1}.unwrap()\na.intersection_update({1u8}.unwrap())",
    "expected set[i64]"
)]
#[case(
    "mut a = {1}.unwrap()\na.intersection_update()",
    "requires one set argument"
)]
#[case(
    "{1}.unwrap().intersection_update({1}.unwrap())",
    "requires a mutable set"
)]
fn invalid_intersection_update(
    #[case] source: &str,
    #[case] expected: &str,
    #[values("intersection_update", "difference_update")] method: &str,
) {
    let source = source.replace("intersection_update", method);
    let error = support::check_source(&source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[test]
fn union_borrows_sources_and_returns_an_independent_owner() {
    native(
        r#"
def combine(a: &set[str], b: &set[str]) -> Result[set[str], AllocError]:
    a.union(b)
def unwrap(result: Result[set[str], AllocError]) -> set[str]:
    match result:
        case Ok(value):
            value
        case Err(error):
            set[str]().unwrap()
mut a = {("é" + "").unwrap(), "left"}.unwrap()
b = {"é", "right"}.unwrap()
mut output = unwrap(combine(&a, &b))
a.clear()
print(len(output)).unwrap()
print("é" in output).unwrap()
print("left" in output and "right" in output).unwrap()
print(output.discard("left")).unwrap()
print(len(b)).unwrap()
print(len(a)).unwrap()
match {True}.unwrap().union({False}.unwrap()):
    case Ok(value):
        print(len(value)).unwrap()
    case Err(error):
        print(-1).unwrap()
match set[u8]().unwrap().union(set[u8]().unwrap()):
    case Ok(value):
        print(len(value)).unwrap()
    case Err(error):
        print(-1).unwrap()
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
#[case(4)]
fn algebra_recovers_from_each_storage_failure(
    #[case] budget: usize,
    #[values(("union", 3), ("intersection", 1), ("difference", 1), ("symmetric_difference", 2))]
    operation: (&str, i64),
) {
    let (method, length) = operation;
    let source = format!(
        r#"
a = {{("é" + "").unwrap(), "left"}}.unwrap()
b = {{"é", "right"}}.unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = a.{method}(b)
print("__test_restore_allocations__").unwrap()
match result:
    case Ok(value):
        print(len(value)).unwrap()
    case Err(error):
        print(-1).unwrap()
print(len(a)).unwrap()
print(len(b)).unwrap()
print("left" in a and "right" in b).unwrap()
"#
    );
    native(
        &source,
        &format!("{}\n2\n2\nTrue", if budget == 4 { length } else { -1 }),
    );
}

#[rstest]
#[case(
    "print({1}.unwrap().union({1u8}.unwrap())).unwrap()",
    "expected set[i64]"
)]
#[case("print({1}.unwrap().union()).unwrap()", "requires one set argument")]
#[case(
    "print([1].unwrap().union({1}.unwrap())).unwrap()",
    "requires a set receiver"
)]
fn invalid_union(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[rstest]
#[case("union", 150, 100)]
#[case("intersection", 50, 100)]
#[case("difference", 50, 0)]
#[case("symmetric_difference", 100, 0)]
fn algebra_membership_and_self_aliases(
    #[case] method: &str,
    #[case] length: i64,
    #[case] self_length: i64,
) {
    native(
        &format!(
            r#"
def operation(a: &set[i64], b: &set[i64]) -> Result[set[i64], AllocError]:
    a.{method}(b)
a = {{n for n in range(100)}}.unwrap()
b = {{n for n in range(50, 150)}}.unwrap()
match operation(&a, &b):
    case Ok(values):
        print(len(values)).unwrap()
        print(75 in values).unwrap()
        print(0 in values).unwrap()
        print(149 in values).unwrap()
    case Err(error):
        print(-1).unwrap()
match a.{method}(a):
    case Ok(values):
        print(len(values)).unwrap()
    case Err(error):
        print(-1).unwrap()
print(len(a)).unwrap()
print(len(b)).unwrap()
"#
        ),
        &format!(
            "{length}\n{}\n{self_length}\n100\n100",
            match method {
                "union" => "True\nTrue\nTrue",
                "intersection" => "True\nFalse\nFalse",
                "difference" => "False\nTrue\nFalse",
                _ => "False\nTrue\nTrue",
            }
        ),
    );
}

#[test]
fn intersection_of_disjoint_sets_needs_only_an_owner_header() {
    native(
        r#"
a = {1}.unwrap()
b = {2}.unwrap()
print("__test_fail_allocations_after_1__").unwrap()
result = a.intersection(b)
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
print(str.repr(a.intersection(set[i64]().unwrap())).unwrap()).unwrap()
"#,
        "Result[set[i64], AllocError].Ok(set())\nResult[set[i64], AllocError].Ok(set())",
    );
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
a = {("é" + "").unwrap(), "🙂"}.unwrap()
b = {"é", "🙂", "extra"}.unwrap()
print(check(&a, &b)).unwrap()
print(a.issubset(a)).unwrap()
print(a.isdisjoint(a)).unwrap()
print(a.isdisjoint({"other"}.unwrap())).unwrap()
print(b.issubset(a)).unwrap()
print(set[i64]().unwrap().issubset({1}.unwrap())).unwrap()
print(set[i64]().unwrap().isdisjoint(set[i64]().unwrap())).unwrap()
print({True}.unwrap().issubset({True, False}.unwrap())).unwrap()
print({1u8}.unwrap().issuperset({1u8}.unwrap())).unwrap()
print(len(a)).unwrap()
class Custom:
    def issubset(self) -> i64:
        42
print(Custom().issubset()).unwrap()
"#,
        "True\nTrue\nFalse\nTrue\nFalse\nTrue\nTrue\nTrue\nTrue\n2\n42",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn relations_do_not_allocate() {
    native(
        r#"
a = {n for n in range(100)}.unwrap()
b = {n for n in range(50)}.unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
x = a.issuperset(b)
y = b.issubset(a)
z = a.isdisjoint(b)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(x).unwrap()
print(y).unwrap()
print(z).unwrap()
"#,
        "True\nTrue\nFalse",
    );
}

#[rstest]
#[case(
    "print({1}.unwrap().issubset({1u8}.unwrap())).unwrap()",
    "expected set[i64]"
)]
#[case(
    "print({1}.unwrap().isdisjoint()).unwrap()",
    "requires one set argument"
)]
#[case(
    "print([1].unwrap().issuperset({1}.unwrap())).unwrap()",
    "requires a set receiver"
)]
#[case(
    "mut a = {1}.unwrap()\nr = &mut a\nprint(a.issubset(a)).unwrap()\nr.add(2).unwrap()",
    "borrow"
)]
fn invalid_relations(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[test]
fn structural_enum_keys_survive_growth_removal_and_algebra() {
    native(
        r#"
enum Key:
    Empty
    Id(i64)
    Name(str)
    Pair(i64, str)
mut keys: set[Key] = set().unwrap()
mut values: dict[Key, i64] = {}.unwrap()
for n in range(200):
    keys.add(Key.Pair(n, "value")).unwrap()
    values.insert(Key.Pair(n, "value"), n).unwrap()
keys.add(Key.Empty).unwrap()
keys.add(Key.Id(1)).unwrap()
keys.add(Key.Name("value")).unwrap()
keys.add(Key.Pair(1, ("val" + "ue").unwrap())).unwrap()
print(len(keys), values[Key.Pair(199, "value")]).unwrap()
for n in range(100):
    keys.discard(Key.Pair(n, "value"))
print(len(keys), Key.Pair(100, "value") in keys).unwrap()
other = {Key.Empty, Key.Id(1)}.unwrap()
print(len(keys.intersection(other).unwrap())).unwrap()
print(len(keys.difference(other).unwrap())).unwrap()
print(len(keys.union(other).unwrap())).unwrap()
print(len(keys.symmetric_difference(other).unwrap())).unwrap()
keys.update(other).unwrap()
print(len(keys)).unwrap()
"#,
        "203 199\n103 True\n2\n101\n103\n101\n103\n",
    );
}

#[test]
fn nested_and_generic_enum_keys_keep_tags_and_payload_identity() {
    native(
        r#"
enum Color:
    Red
    Blue
enum Key[T]:
    Empty
    First(T)
    Second(T)
mut keys: set[Key[(Color, str)]] = set().unwrap()
keys.add(Key[(Color, str)].First((Color.Red, "x"))).unwrap()
keys.add(Key[(Color, str)].Second((Color.Red, "x"))).unwrap()
keys.add(Key[(Color, str)].First((Color.Blue, "x"))).unwrap()
keys.add(Key[(Color, str)].First((Color.Red, ("x" + "").unwrap()))).unwrap()
print(len(keys)).unwrap()
colors = {Color.Red, Color.Blue, Color.Red}.unwrap()
print(len(colors)).unwrap()
options: dict[Option[i64], i64] = {Nothing: 1, Some(2): 3}.unwrap()
empty: Option[i64] = Nothing
print(options[empty], options[Some(2)]).unwrap()
"#,
        "3\n2\n1 3\n",
    );
}

#[test]
fn unsupported_enum_payloads_remain_rejected_as_keys() {
    // Anonymous sums remain a separate B74 design decision.
    for declaration in [
        "enum Key:\n    Value(f64)\n",
        "enum Key:\n    Value(list[i64])\n",
        "enum Key:\n    End\n    Next(Box[Key])\n",
        "type Key = i64 | str\n",
    ] {
        let source = format!(
            "{declaration}\ndef main() -> Result[(), AllocError]:\n    keys: set[Key] = set()?\n    Ok(())\n"
        );
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains("set elements must be"), "{error}");
    }
}

#[test]
fn structural_class_keys_support_owned_nested_and_copy_classes() {
    support::assert_stdout(
        r#"
class Name:
    text: str
class Key:
    name: Name
    number: i64
@copy
class Small:
    number: i64
    flag: bool
enum Choice:
    Named(Key)
    Small(Small)
mut keys: set[Key] = set().unwrap()
for n in range(80):
    keys.add(Key(Name(str.from(n).unwrap()), n)).unwrap()
probe = Key(Name("42"), 42)
print(probe in keys, len(keys), probe.name.text).unwrap()
keys.add(Key(Name(("4" + "2").unwrap()), 42)).unwrap()
keys.discard(&probe)
print(probe in keys, len(keys), probe.name.text).unwrap()
choices = {Choice.Named(Key(Name("a"), 1)), Choice.Named(Key(Name("a"), 1)), Choice.Small(Small(1, True))}.unwrap()
print(len(choices), Choice.Named(Key(Name("a"), 1)) in choices).unwrap()
"#,
        "True 80 42\nFalse 79 42\n2 True\n",
    );
}

#[test]
fn structural_class_keys_algebra_copies_independent_owners() {
    support::assert_stdout(
        r#"
class Key:
    text: str
    n: i64
mut left = {Key("same", 1), Key("left", 2)}.unwrap()
right = {Key("same", 1), Key("right", 3)}.unwrap()
union = left.union(&right).unwrap()
intersection = left.intersection(&right).unwrap()
difference = left.difference(&right).unwrap()
symmetric = left.symmetric_difference(&right).unwrap()
print(left.issubset(&union), left.isdisjoint(&difference), len(intersection), len(difference), len(symmetric)).unwrap()
left.intersection_update(&right)
print(len(left)).unwrap()
drop(left)
drop(right)
print(Key("same", 1) in union, Key("right", 3) in union).unwrap()
for item in union:
    mut key = item
    key.n += 10
    print(key.text, key.n).unwrap()
print(Key("same", 1) in intersection, Key("left", 2) in difference, Key("right", 3) in symmetric).unwrap()
"#,
        "True False 1 1 2\n1\nTrue True\nsame 11\nleft 12\nright 13\nTrue True True\n",
    );
}

#[test]
fn structural_class_keys_reject_ineligible_fields_and_cleanup() {
    for declaration in [
        "class Key:\n    value: f64\n",
        "class Key:\n    value: list[i64]\n",
        "class Key:\n    value: Box[i64]\n",
        "class Key:\n    value: i64\n    def __del__(self: &mut Key) -> ():\n        pass\n",
    ] {
        let source = format!(
            "{declaration}\ndef main() -> Result[(), AllocError]:\n    keys: set[Key] = set()?\n    Ok(())\n"
        );
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains("set elements must be"), "{error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn structural_class_keys_allocation_failure_preserves_sources() {
    for budget in 0..=4 {
        for operation in [
            "union",
            "intersection",
            "difference",
            "symmetric_difference",
        ] {
            native(
                &format!(
                    r#"
class Key:
    text: str
    n: i64
left = {{Key(("same" + "").unwrap(), 1), Key(("left" + "").unwrap(), 2)}}.unwrap()
right = {{Key(("same" + "").unwrap(), 1), Key(("right" + "").unwrap(), 3)}}.unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = left.{operation}(&right)
print("__test_restore_allocations__").unwrap()
print(result.is_ok()).unwrap()
print(Key("left", 2) in left, Key("right", 3) in right).unwrap()
drop(left)
drop(right)
match result:
    case Ok(keys):
        for key in &keys:
            print(len(key.text) > 0).unwrap()
    case Err(error):
        pass
"#
                ),
                &format!(
                    "{}\nTrue True{}",
                    if budget == 4 { "True" } else { "False" },
                    if budget == 4 {
                        "\nTrue".repeat(match operation {
                            "union" => 3,
                            "symmetric_difference" => 2,
                            _ => 1,
                        })
                    } else {
                        String::new()
                    }
                ),
            );
        }
    }
}

#[test]
fn enum_bitsets_preserve_views_copying_and_set_operations() {
    native(
        r#"
enum E:
    A
    B
    C
mut left = {E.C, E.A}.unwrap()
right = {E.B, E.C}.unwrap()
union = left.union(&right).unwrap()
intersection = left.intersection(&right).unwrap()
difference = left.difference(&right).unwrap()
symmetric = left.symmetric_difference(&right).unwrap()
print(len(union), len(intersection), len(difference), len(symmetric)).unwrap()
print(left.issubset(&union), union.issuperset(&right), difference.isdisjoint(&right)).unwrap()
cloned = copy(left).unwrap()
print(cloned == left).unwrap()
left.intersection_update(&right)
print(left == intersection).unwrap()
left.update({E.A, E.B}.unwrap()).unwrap()
left.difference_update(&right)
print(left == difference).unwrap()
map = {E.B: 2, E.A: 1}.unwrap()
view = map.keys()
print(len(view.union(&right).unwrap()), len(right.intersection(&view).unwrap())).unwrap()
print(view.to_set().unwrap() == {E.A, E.B}.unwrap()).unwrap()
mut mixed = copy(union).unwrap()
mixed.intersection_update(&view)
print(len(mixed)).unwrap()
mixed.difference_update(&view)
print(len(mixed)).unwrap()
for value in &union:
    match value:
        case E.A:
            print("A").unwrap()
        case E.B:
            print("B").unwrap()
        case E.C:
            print("C").unwrap()
left.clear()
print(len(left), len(cloned)).unwrap()
"#,
        "3 1 1 2\nTrue True True\nTrue\nTrue\nTrue\n3 1\nTrue\n2\n0\nA\nB\nC\n0 2",
    );
}

#[test]
fn enum_bitsets_handle_one_64_and_65_variants() {
    for size in [1, 63, 64, 65] {
        let variants = (0..size).map(|i| format!("    V{i}\n")).collect::<String>();
        let values = (0..size)
            .rev()
            .map(|i| format!("E.V{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        native(
            &format!(
                r#"
enum E:
{variants}
values = [{values}].unwrap()
mut members = {{value for value in &values}}.unwrap()
print(len(members)).unwrap()
print(E.V0 in members, E.V{last} in members).unwrap()
print(members.discard(E.V{last}), E.V{last} in members).unwrap()
members.add(E.V{last}).unwrap()
print(len(members), len(copy(members).unwrap())).unwrap()
"#,
                last = size - 1
            )
            .replace(
                "{value for value in &values}",
                "{*value for value in &values}",
            ),
            &format!("{size}\nTrue True\nTrue False\n{size} {size}"),
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn enum_bitsets_recover_owner_failure_and_mutate_without_allocating() {
    for budget in 0..=1 {
        native(
            &format!(
                r#"
enum E:
    A
    B
    C
mut left = {{E.A}}.unwrap()
right = {{E.B}}.unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = left.union(&right)
print("__test_restore_allocations__").unwrap()
print(result.is_ok(), len(left), len(right)).unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
left.add(E.B).unwrap()
left.add(E.C).unwrap()
left.reserve(100).unwrap()
left.update(right).unwrap()
left.discard(E.A)
count = len(left)
left.clear()
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(count, len(left)).unwrap()
"#
            ),
            &format!("{} 1 1\n2 0", if budget == 0 { "False" } else { "True" }),
        );
    }
}

#[test]
fn enum_bitsets_survive_owned_iteration_and_nested_copying() {
    native(
        r#"
enum E:
    A
    B
def score(value: &E) -> i64:
    match value:
        case E.A:
            1
        case E.B:
            2
original = [{E.B, E.A}.unwrap()].unwrap()
mut total = 0
for member in copy(original[0]).unwrap():
    total += score(&member)
print(total, len(original[0])).unwrap()
cloned = copy(original).unwrap()
drop(original)
print(len(cloned[0]), cloned[0] == {E.A, E.B}.unwrap()).unwrap()
collected = [*member for member in &cloned[0]].unwrap()
print(len(collected)).unwrap()
"#,
        "3 2\n2 True\n2",
    );
}
