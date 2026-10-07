mod support;

#[test]
fn loops_and_comprehensions_unpack_tuples_and_borrow_dictionary_items() {
    let out = support::run(
        r#"
class Point:
    x: i64
mut points = {"a": Point(1).unwrap(), "b": Point(2).unwrap()}.unwrap()
for key, point in points.items():
    print((key, point.x).unwrap()).unwrap()
for key, point in (&mut points).items():
    point.x = point.x + 10
print([point.x for key, point in points.items() if key == "b"].unwrap()).unwrap()
print(points).unwrap()
mut counts = {"a": 1, "b": 2}.unwrap()
for key, value in &mut counts.items():
    *value = *value * 2
print([(k, v).unwrap() for k, v in counts.items()].unwrap()).unwrap()
x = 99
print([x + y for x, y in [(1, 2).unwrap(), (3, 4).unwrap()].unwrap()].unwrap()).unwrap()
for x, y in [(5, 6).unwrap()].unwrap():
    print(x + y).unwrap()
print(x).unwrap()
"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "(\"a\", 1)\n(\"b\", 2)\n[12]\n{\"a\": Point(x=11), \"b\": Point(x=12)}\n[(\"a\", 2), (\"b\", 4)]\n[3, 7]\n11\n99\n");
}

#[test]
fn item_loans_reject_invalidation_and_illegal_moves() {
    for source in [
        "mut d = {1: [2].unwrap()}.unwrap()\nfor k, v in d.items():\n    d.clear()\n    print(v).unwrap()",
        "mut d = {1: [2].unwrap()}.unwrap()\nfor k, v in d.items():\n    v.append(3).unwrap()",
        "mut d = {1: [2].unwrap()}.unwrap()\nfor k, v in (&mut d).items():\n    d.pop(k)\n    print(v).unwrap()",
        "d = {1: 2}.unwrap()\nfor k, v in (&mut d).items():\n    print(v).unwrap()",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn checked_tuple_failure_releases_evaluated_owned_components() {
    let out = support::run(
        r#"
class Resource:
    id: i64
    def __del__(self: &mut Resource) -> ():
        print("__test_restore_allocations__").unwrap()
        print(self.id).unwrap()
a = Resource(1).unwrap()
b = Resource(2).unwrap()
print("__test_fail_allocations_after_0__").unwrap()
result = (a, b)
print("__test_restore_allocations__").unwrap()
print(result).unwrap()
"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let lines = text
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>();
    assert_eq!(
        lines,
        [
            "1",
            "2",
            "Result[tuple[Resource, Resource], AllocError].Err(AllocError.OutOfMemory)"
        ]
    );
}

#[test]
fn tuple_values_support_signatures_indexing_comparison_and_checked_construction() {
    let out = support::run(
        r#"
def pair(x: i64) -> (i64, str):
    (x, "answer").unwrap()
p: tuple[i64, str] = pair(42)
print(p).unwrap()
print(p[0]).unwrap()
print(p[1]).unwrap()
print((1,).unwrap()).unwrap()
print(p == (42, "answer").unwrap()).unwrap()
print((1, "two")).unwrap()
print([(1, "one").unwrap(), (2, "two").unwrap()].unwrap()).unwrap()
"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "(42, \"answer\")\n42\nanswer\n(1,)\nTrue\nResult[tuple[i64, str], AllocError].Ok((1, \"two\"))\n[(1, \"one\"), (2, \"two\")]\n");
}

#[test]
fn tuple_shape_and_storage_errors_are_static() {
    for source in [
        "x: (i64, str) = (1, 2)",
        "x: (i64, i64) = (1,)",
        "x = (1, 2).unwrap()\nprint(x[2]).unwrap()",
        "x = (1, 2).unwrap()\ni = 1\nprint(x[i]).unwrap()",
        "x = 1\ny = (&x, 2).unwrap()",
        "print(((), 1).unwrap()[0]).unwrap()",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}

#[test]
fn unit_tuple_components_remain_unit_expressions() {
    let out = support::run("def unit() -> ():\n    ((), 1).unwrap()[0]\nunit()\n_, n = ((), 3).unwrap()\nprint(n).unwrap()");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n");
}

#[test]
fn unpacking_moves_owned_components_and_cleans_discarded_fields() {
    let out = support::run(
        r#"
class Resource:
    id: i64
    def __del__(self: &mut Resource) -> ():
        print(self.id).unwrap()
pair = (Resource(1).unwrap(), Resource(2).unwrap()).unwrap()
a, _ = pair
print(a.id).unwrap()
mut x, y = (10, 20).unwrap()
x, y = (y, x).unwrap()
print((x, y).unwrap()).unwrap()
for number, text in [(1, "one").unwrap(), (2, "two").unwrap()].unwrap():
    print(number).unwrap()
    print(text).unwrap()
"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "2\n1\n(20, 10)\n1\none\n2\ntwo\n1\n"
    );
    for source in [
        "x = ([1].unwrap(), 2).unwrap()\na, b = x\nprint(x).unwrap()",
        "x, y = (1, 2, 3)",
        "x, x = (1, 2)",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}
