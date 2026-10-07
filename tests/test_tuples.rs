mod support;

#[test]
fn loops_and_comprehensions_unpack_tuples_and_borrow_dictionary_items() {
    let out = support::run(
        r#"
class Point:
    x: i64
mut points = {"a": Point(1), "b": Point(2)}
for key, point in points.items():
    print((key, point.x))
for key, point in (&mut points).items():
    point.x = point.x + 10
print([point.x for key, point in points.items() if key == "b"])
print(points)
mut counts = {"a": 1, "b": 2}
for key, value in &mut counts.items():
    *value = *value * 2
print([(k, v) for k, v in counts.items()])
x = 99
print([x + y for x, y in [(1, 2), (3, 4)]])
for x, y in [(5, 6)]:
    print(x + y)
print(x)
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
        "mut d = {1: [2]}\nfor k, v in d.items():\n    d.clear()\n    print(v)",
        "mut d = {1: [2]}\nfor k, v in d.items():\n    v.append(3)",
        "mut d = {1: [2]}\nfor k, v in (&mut d).items():\n    d.pop(k)\n    print(v)",
        "d = {1: 2}\nfor k, v in (&mut d).items():\n    print(v)",
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
        print("__test_restore_allocations__")
        print(self.id)
a = Resource(1)
b = Resource(2)
print("__test_fail_allocations_after_0__")
result = try (a, b)
print("__test_restore_allocations__")
print(result)
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
    (x, "answer")
p: tuple[i64, str] = pair(42)
print(p)
print(p[0])
print(p[1])
print((1,))
print(p == (42, "answer"))
print(try (1, "two"))
print([(1, "one"), (2, "two")])
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
        "x = (1, 2)\nprint(x[2])",
        "x = (1, 2)\ni = 1\nprint(x[i])",
        "x = 1\ny = (&x, 2)",
        "print(((), 1)[0])",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}

#[test]
fn unit_tuple_components_remain_unit_expressions() {
    let out = support::run("def unit() -> ():\n    ((), 1)[0]\nunit()\n_, n = ((), 3)\nprint(n)");
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
        print(self.id)
pair = (Resource(1), Resource(2))
a, _ = pair
print(a.id)
mut x, y = (10, 20)
x, y = (y, x)
print((x, y))
for number, text in [(1, "one"), (2, "two")]:
    print(number)
    print(text)
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
        "x = ([1], 2)\na, b = x\nprint(x)",
        "x, y = (1, 2, 3)",
        "x, x = (1, 2)",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}
