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
