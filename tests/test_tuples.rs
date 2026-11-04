mod support;

/// Only the fixture entrypoint owns fragment arithmetic; declarations retain their own contracts.
fn program(source: &str) -> String {
    if source.lines().any(|line| line.starts_with("def main(")) {
        return source.to_owned();
    }
    let source = support::program(source);
    let source = source.replacen(
        "def main() -> ():",
        "def main() -> Result[(), OverflowError[i64]]:",
        1,
    );
    let end = source.rfind("    pass\n").unwrap();
    format!("{}    Ok(())\n", &source[..end])
}

#[test]
fn loops_and_comprehensions_unpack_tuples_and_borrow_dictionary_items() {
    let out = run(r#"
class Point:
    x: i64
mut points = {"a": Point(1), "b": Point(2)}.unwrap()
for key, point in points.items():
    print((*key, point.x)).unwrap()
for key, point in (&mut points).items():
    point.x = point.x + 10
print([point.x for key, point in points.items() if key == "b"].unwrap()).unwrap()
print(points).unwrap()
mut counts = {"a": 1, "b": 2}.unwrap()
for key, value in &mut counts.items():
    *value = *value * 2
print([(*k, *v) for k, v in counts.items()].unwrap()).unwrap()
x = 99
print([x + y for x, y in [(1, 2), (3, 4)].unwrap()].unwrap()).unwrap()
for x, y in [(5, 6)].unwrap():
    print(x + y).unwrap()
print(x).unwrap()
"#);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "(\"a\", 1)\n(\"b\", 2)\n[12]\n{\"a\": Point(x=11), \"b\": Point(x=12)}\n[(\"a\", 2), (\"b\", 4)]\n[3, 7]\n11\n99\n"
    );
}

#[test]
fn item_loans_reject_invalidation_and_illegal_moves() {
    for source in [
        "mut d = {1: [2].unwrap()}.unwrap()\nfor k, v in d.items():\n    d.clear()\n    print(v).unwrap()",
        "mut d = {1: [2].unwrap()}.unwrap()\nfor k, v in d.items():\n    v.append(3).unwrap()",
        "mut d = {1: [2].unwrap()}.unwrap()\nfor k, v in (&mut d).items():\n    d.pop(k)\n    print(v).unwrap()",
        "d = {1: 2}.unwrap()\nfor k, v in (&mut d).items():\n    print(v).unwrap()",
    ] {
        assert!(support::check_source(&program(source)).is_err(), "{source}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn tuples_move_owned_components_without_allocating() {
    let out = run(r#"
class Resource:
    id: i64
    def __del__(self: &mut Resource) -> ():
        print(self.id).unwrap()
a = Resource(1)
b = Resource(2)
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
result = (a, b)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(result).unwrap()
"#);
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
    assert_eq!(lines, ["(Resource(id=1), Resource(id=2))", "1", "2"]);
}

#[test]
fn tuple_values_support_signatures_indexing_and_comparison() {
    let out = run(r#"
def pair(x: i64) -> (i64, str):
    (x, "answer")
p: tuple[i64, str] = pair(42)
print(p).unwrap()
print(p[0]).unwrap()
print(p[1]).unwrap()
print((1,)).unwrap()
print(p == (42, "answer")).unwrap()
print((1, "two")).unwrap()
print([(1, "one"), (2, "two")].unwrap()).unwrap()
"#);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "(42, \"answer\")\n42\nanswer\n(1,)\nTrue\n(1, \"two\")\n[(1, \"one\"), (2, \"two\")]\n"
    );
}

#[test]
fn tuple_shape_and_storage_errors_are_static() {
    for source in [
        "x: (i64, str) = (1, 2)",
        "x: (i64, i64) = (1,)",
        "x = (1, 2)\nprint(x[2]).unwrap()",
        "x = (1, 2)\ni = 1\nprint(x[i]).unwrap()",
        "x = 1\ny = (&x, 2)",
        "print(((), 1)[0]).unwrap()",
    ] {
        assert!(support::check_source(&program(source)).is_err(), "{source}");
    }
}

#[test]
fn unit_tuple_components_remain_unit_expressions() {
    let out = run("def unit() -> ():\n    ((), 1)[0]\nunit()\n_, n = ((), 3)\nprint(n).unwrap()");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n");
}

#[test]
fn annotated_unit_tuple_components_push_one_unit() {
    // Issue #207: an annotated unit field pushed its unit twice.
    let out = run(
        "t: tuple[(), i64] = ((), 7)\nprint(t[1]).unwrap()\nu: ((), (), i64) = ((), (), 8)\nprint(u[2]).unwrap()",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n8\n");
}

#[test]
fn unpacking_moves_owned_components_and_cleans_discarded_fields() {
    let out = run(r#"
class Resource:
    id: i64
    def __del__(self: &mut Resource) -> ():
        print(self.id).unwrap()
pair = (Resource(1), Resource(2))
a, _ = pair
print(a.id).unwrap()
mut x, y = (10, 20)
x, y = (y, x)
print((x, y)).unwrap()
for number, text in [(1, "one"), (2, "two")].unwrap():
    print(number).unwrap()
    print(text).unwrap()
"#);
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
        "x = ([1].unwrap(), 2)\na, b = x\nprint(x).unwrap()",
        "x, y = (1, 2, 3)",
        "x, x = (1, 2)",
    ] {
        assert!(support::check_source(&program(source)).is_err(), "{source}");
    }
}

#[test]
fn negative_tuple_indices_count_from_the_end() {
    // Issue #583: `t[-k]` resolves at compile time to `t[len - k]`, and
    // `t[-0]` is `t[0]` as in Python.
    expect(
        r#"
t = (1, "two", 3.5)
print(t[-1], t[-2], t[-3], t[-0], t[(-1)]).unwrap()
nested = ((1, 2), ("a", (True, 9)))
print(nested[-1][-1][-1], nested[0][-2], nested[-1][1][0]).unwrap()
r = &t[-2]
print(*r, len(t[-2])).unwrap()
mut m = (10, [1, 2].unwrap())
first = &mut m[-2]
*first += 5
items = &mut m[-1]
items.append(3).unwrap()
print(m, len(m[-1])).unwrap()
"#,
        "3.5 two 1 1 3.5\n9 1 True\ntwo 3\n(15, [1, 2, 3]) 3\n",
    );
}

#[test]
fn negative_tuple_index_loans_name_the_resolved_component() {
    // `m[-1]` and `m[1]` are the same component of a pair; `m[-1]` and `m[0]`
    // are distinct.
    expect(
        "mut m = ([1].unwrap(), [2].unwrap())\na = &mut m[-1]\nb = &mut m[0]\na.append(3).unwrap()\nb.append(4).unwrap()\nprint(m).unwrap()",
        "([1, 4], [2, 3])\n",
    );
    let error = support::check_error(&program(
        "mut m = ([1].unwrap(), [2].unwrap())\na = &mut m[-1]\nb = &mut m[1]\na.append(3).unwrap()\nb.append(4).unwrap()",
    ));
    assert!(error.contains("borrow"), "{error}");
}

#[test]
fn tuple_index_errors_state_the_length() {
    for (source, expected) in [
        (
            "t = (1, 2)\nprint(t[-3]).unwrap()",
            "tuple index -3 is out of range for a tuple of length 2; valid indices are 0 to 1 and -1 to -2",
        ),
        (
            "t = (1, 2)\nprint(t[2]).unwrap()",
            "tuple index 2 is out of range for a tuple of length 2",
        ),
        (
            "mut t = (1, 2)\nr = &mut t[-3]\n*r += 1",
            "tuple index -3 is out of range for a tuple of length 2",
        ),
        (
            "t = (1, 2)\nr = &t[-5]\nprint(*r).unwrap()",
            "tuple index -5 is out of range for a tuple of length 2",
        ),
        (
            "t = (1, 2)\ni = 1\nprint(t[-i]).unwrap()",
            "tuple index must be an integer literal",
        ),
        (
            "def pair() -> (i64, i64):\n    (1, 2)\nprint(pair()[-3]).unwrap()",
            "tuple index -3 is out of range for a tuple of length 2",
        ),
    ] {
        let error = support::check_error(&program(source));
        assert!(error.contains(expected), "{source}\n{error}");
    }
}

#[test]
fn nested_unpacking_binds_assigns_and_iterates_structural_components() {
    expect(
        r#"
((x, y), z) = ((1, 2), 3)
print(x + y + z).unwrap()
mut (a, (b, c)) = (4, (5, 6))
(a, (b, c)) = (b, (c, a))
print((a, b, c)).unwrap()
for (left, right), label in [((7, 8), 9)].unwrap():
    print(left + right + label).unwrap()
mut rows = [((1, 2), 3)].unwrap()
for (left, right), label in &mut rows:
    *left = *left + 10
    *right = *right + 20
    *label = *label + 30
print(rows).unwrap()
mut owner = ((10, 20), 30)
(first, second), third = &mut owner
*first = *first + 1
*second = *second + 2
*third = *third + 3
print(owner).unwrap()
"#,
        "6\n(5, 6, 4)\n24\n[((11, 22), 33)]\n((11, 22), 33)\n",
    );
}

#[test]
fn nested_unpacking_rejects_duplicate_names_shapes_and_illegal_reference_rebinding() {
    for source in [
        "(a, (b, a)) = (1, (2, 3))",
        "mut (a, (b, a)) = (1, (2, 3))",
        "(a, (b, c)) = (1, 2)",
        "(a, (b, c)) = (1, (2, 3, 4))",
        "owner = ((1, 2), 3)\nmut ((a, b), c) = &owner",
        "owner = ((1, 2), 3)\na = 1\n((a, b), c) = &owner",
        "for (a, b), a in [((1, 2), 3)].unwrap():\n    pass",
    ] {
        assert!(support::check_source(&program(source)).is_err(), "{source}");
    }
}

#[test]
fn nested_unpacking_preserves_destination_order_and_owned_cleanup() {
    expect(
        r#"
class Resource:
    id: i64
    def __del__(self: &mut Resource) -> ():
        print(self.id).unwrap()
mut index = 0
mut items = [0, 0, 0].unwrap()
(index, (items[index], items[0])) = (2, (7, 8))
print(items).unwrap()
((a, _), b) = ((Resource(1), Resource(2)), Resource(3))
print(a.id + b.id).unwrap()
for (x, y), z in (((1, 2), 3) for _ in range(1)):
    print(x + y + z).unwrap()
"#,
        "[8, 0, 7]\n2\n4\n6\n3\n1\n",
    );
}

#[test]
fn nested_borrowed_unpacking_keeps_precise_component_loans_and_ends_them_at_last_use() {
    expect(
        r#"
mut owner = ((1, 2), 3)
((first, second), _) = &mut owner
print(owner[1]).unwrap()
*first += 10
print(owner[0][0]).unwrap()
*second += 20
print(owner).unwrap()
"#,
        "3\n11\n((11, 22), 3)\n",
    );
    for source in [
        "mut owner = ((1, 2), 3)\n((first, second), _) = &mut owner\nprint(owner[0][0]).unwrap()\n*first += 1\n*second += 1",
        "mut owner = ((1, 2), 3)\n((first, second), _) = &mut owner\nprint(owner).unwrap()\n*first += 1\n*second += 1",
    ] {
        assert!(support::check_source(&program(source)).is_err(), "{source}");
    }
}

#[test]
fn nested_comprehension_and_generator_targets_bind_before_filters_and_inner_sources() {
    expect(
        r#"
def next_value(values: &mut Generator[Result[i64, OverflowError[i64]]]) -> Result[Option[i64], OverflowError[i64]]:
    match next(values):
        case Some(value):
            Ok(Some(value?))
        case Nothing:
            Ok(Nothing)
rows = [((1, 2), 3), ((4, 5), 6)].unwrap()
print([x + y + z for (x, y), z in &rows if *x > 1].unwrap()).unwrap()
print([x + y + z + n for (x, y), z in &rows for n in range(*x) if n == 0].unwrap()).unwrap()
mut owned = [((1, 2), 3)].unwrap()
print([(*x, *y, *z) for (x, y), z in &mut owned].unwrap()).unwrap()
add = def(x: i64, y: i64, z: i64) -> Result[i64, OverflowError[i64]]:
    Ok(x + y + z)
mut values = (add(x, y, z) for (x, y), z in rows if x > 1)
print(next_value(&mut values)?).unwrap()
print(next_value(&mut values)?).unwrap()
"#,
        "[15]\n[6, 15]\n[(1, 2, 3)]\nOption[i64].Some(15)\nOption[i64].Nothing\n",
    );
}

#[test]
fn nested_generator_targets_are_lazy_and_generic_comprehensions_move_fields() {
    expect(
        r#"
def next_value(values: &mut Generator[Result[i64, OverflowError[i64]]]) -> Result[Option[i64], OverflowError[i64]]:
    match next(values):
        case Some(value):
            Ok(Some(value?))
        case Nothing:
            Ok(Nothing)
def keep(n: i64) -> bool:
    print(n).unwrap()
    n > 1
def firsts[T](rows: list[((T, T), i64)]) -> Result[list[T], AllocError]:
    [first for (first, _), _ in rows]
add = def(x: i64, y: i64) -> Result[i64, OverflowError[i64]]:
    Ok(x + y)
mut pending = (add(x, y) for (x, y), _ in [((1, 2), 0), ((3, 4), 0)].unwrap() if keep(x))
print("created").unwrap()
print(next_value(&mut pending)?).unwrap()
print(next_value(&mut pending)?).unwrap()
print(firsts([(([1].unwrap(), [2].unwrap()), 0)].unwrap()).unwrap()).unwrap()
"#,
        "created\n1\n3\nOption[i64].Some(7)\nOption[i64].Nothing\n[[1]]\n",
    );
}

#[test]
fn nested_target_grouping_is_distinct_from_singleton_tuple_structure() {
    expect(
        r#"
class Resource:
    id: i64
    def __del__(self: &mut Resource) -> ():
        print(self.id).unwrap()
def unwrap_rows[T](rows: list[(T,)]) -> Result[list[T], AllocError]:
    [x for (x,) in rows]
(single,) = (8,)
mut ((first, (second))) = (1, 2)
((first, (second))) = (second, first)
(grouped) = 3
mut (mutable_grouped) = 4
mutable_grouped += 1
print((single, first, second, grouped, mutable_grouped)).unwrap()
for (number,) in [(9,)].unwrap():
    print(number).unwrap()
for (number) in range(1):
    print(number).unwrap()
(head, (left, right)) = ([1].unwrap(), [2, 3].unwrap())
print((head, left, right)).unwrap()
mut pair = ((1,), 2)
((exclusive,), _) = &mut pair
*exclusive += 10
((shared,), _) = &pair
print(*shared).unwrap()
(owned,) = (Resource(11),)
((item,), _) = ((Resource(12),), 3)
print((owned.id, item.id)).unwrap()
(_,) = (Resource(13),)
drop(item)
drop(owned)
for (_,) in [(Resource(14),)].unwrap():
    pass
print(unwrap_rows([([15].unwrap(),)].unwrap()).unwrap()).unwrap()
"#,
        "(8, 2, 1, 3, 5)\n9\n0\n([1], 2, 3)\n11\n(11, 12)\n13\n12\n11\n14\n[[15]]\n",
    );
}

#[test]
fn nested_loop_bindings_shadow_outer_names_for_each_source_kind() {
    for declaration in ["a = 99", "mut a = 99"] {
        for target in ["(a, b), c", "((a, b), c)"] {
            for source in [
                "[((1, 2), 3)].unwrap()",
                "iter([((1, 2), 3)].unwrap())",
                "&rows",
                "iter(&rows)",
                "&mut rows",
                "iter(&mut rows)",
            ] {
                expect(
                    &format!(
                        r#"
mut rows = [((1, 2), 3)].unwrap()
{declaration}
b = 88
c = 77
for {target} in {source}:
    print(a, b, c).unwrap()
print(a, b, c).unwrap()
"#
                    ),
                    "1 2 3\n99 88 77\n",
                );
            }
        }
    }
}

#[test]
fn flat_pipeline_unpacking_also_creates_fresh_loop_bindings() {
    for declaration in ["a = 99", "mut a = 99"] {
        for target in ["a, b, c", "(a, b, c)"] {
            for source in ["iter([(1, 2, 3)].unwrap())", "iter(&rows)"] {
                expect(
                    &format!(
                        r#"
rows = [(1, 2, 3)].unwrap()
{declaration}
b = 88
c = 77
for {target} in {source}:
    print(a, b, c).unwrap()
print(a, b, c).unwrap()
"#
                    ),
                    "1 2 3\n99 88 77\n",
                );
            }
        }
    }
}

#[test]
fn nested_comprehensions_and_generators_shadow_after_evaluating_their_sources() {
    for declaration in ["a = 99", "mut a = 99"] {
        expect(
            &format!(
                r#"
def increment(a: i64) -> Result[i64, OverflowError[i64]]:
    Ok(a + 1)
def values() -> Generator[Result[i64, OverflowError[i64]]]:
    mut a = 99
    for (a, b), c in [((increment(a), 2), 3)].unwrap():
        yield a
    yield Ok(a)
{declaration}
b = 88
c = 77
for (a, b), c in [((a + 1, 2), 3)].unwrap():
    print(a, b, c).unwrap()
print([a + b + c for (a, b), c in [((a + 1, 2), 3)].unwrap() if a > 0].unwrap()).unwrap()
print([a for a in range(2) for (a, b), c in [((a + 7, 8), 9)].unwrap() for n in range(1)].unwrap()).unwrap()
offset = def(a: i64) -> Result[i64, OverflowError[i64]]:
    Ok(a + 7)
print([value? for value in (a for a in range(2) for (a, b), c in [((offset(a), 8), 9)].unwrap())].unwrap()).unwrap()
print([value? for value in values()].unwrap()).unwrap()
print(a, b, c).unwrap()
"#
            ),
            "100 2 3\n[105]\n[7, 8]\n[7, 8]\n[100, 99]\n99 88 77\n",
        );
    }
    expect(
        r#"
mut a = 99
(a, b), c = ((1, 2), 3)
print(a, b, c).unwrap()
"#,
        "1 2 3\n",
    );
    assert!(support::check_source("a = 99\n((a, b), c) = &((1, 2), 3)").is_err());
}

#[test]
fn nested_loop_shadowing_preserves_affine_outer_owners() {
    for mutable in ["", "mut "] {
        for source in [
            "[((Resource(1), Resource(2)), 3)].unwrap()",
            "iter([((Resource(1), Resource(2)), 3)].unwrap())",
        ] {
            expect(
                &format!(
                    r#"
class Resource:
    id: i64
    def __del__(self: &mut Resource) -> ():
        print("drop", self.id).unwrap()
{mutable}item = Resource(90)
for (item, _), _ in {source}:
    print("inner", item.id).unwrap()
print("outer", item.id).unwrap()
"#
                ),
                "drop 2\ninner 1\ndrop 1\nouter 90\ndrop 90\n",
            );
        }
        expect(
            &format!(
                r#"
class Resource:
    id: i64
    def __del__(self: &mut Resource) -> ():
        print("drop", self.id).unwrap()
{mutable}item = Resource(90)
rows = [((Resource(1), 2), 3)].unwrap()
for (item, _), _ in &rows:
    print("inner", item.id).unwrap()
print("outer", item.id).unwrap()
"#
            ),
            "inner 1\nouter 90\ndrop 1\ndrop 90\n",
        );
    }
    assert!(
        support::check_source(
            "class Resource:\n    id: i64\nitem = Resource(1)\nfor (item, _), _ in [((item, 2), 3)].unwrap():\n    print(item.id).unwrap()\nprint(item.id).unwrap()"
        )
        .is_err()
    );
}

#[test]
fn mutable_nested_declarations_preserve_the_outer_comma_shape() {
    expect(
        r#"
mut (a, b), c = ((1, 2), 3)
a += 10
b += 20
c += 30
print(a, b, c).unwrap()
mut ((d, e), f) = ((4, 5), 6)
print(d, e, f).unwrap()
mut (g,), h = ((7,), 8)
print(g + 1, h).unwrap()
mut (((i, j), k)), l = (((9, 10), 11), 12)
print(i, j, k, l).unwrap()
mut (m), n = (13, 14)
print(m, n).unwrap()
"#,
        "11 22 33\n4 5 6\n8 8\n9 10 11 12\n13 14\n",
    );
    for source in [
        "mut (a, b), c = (1, 2, 3)",
        "mut (a,), b = (1, 2)",
        "mut ((a, b), c) = (1, 2, 3)",
        "mut (((a, b), c)), d = ((1, 2, 3), 4)",
        "mut (a, b), a = ((1, 2), 3)",
    ] {
        assert!(support::check_source(&program(source)).is_err(), "{source}");
    }
}

#[test]
fn parenthesized_pair_targets_bind_borrowed_adapter_components_directly() {
    expect(
        r#"
x = 99
y = 88
z = 77
mut rows = [(1, 2)].unwrap()
mut labels = [3].unwrap()
for (x, y), z in zip(&rows, &labels):
    print(*x, *y, *z).unwrap()
for ((x, y), z) in zip(&rows, &labels):
    print(*x, *y, *z).unwrap()
print([(*x, *y, *z) for ((x, y), z) in zip(&rows, &labels)].unwrap()).unwrap()
print([(*x, *y, *z) for ((x, y), z) in zip(&rows, &labels) for n in range(1)].unwrap()).unwrap()
for ((x, y), z) in zip(&mut rows, &mut labels):
    *x += 10
    *y += 20
    *z += 30
print(rows, labels).unwrap()
mut d = {1: (2, 3)}.unwrap()
for (k, (x, y)) in d.items():
    print(*k, *x, *y).unwrap()
print([(*k, *x, *y) for (k, (x, y)) in d.items()].unwrap()).unwrap()
print([(*k, *x, *y) for (k, (x, y)) in d.items() for n in range(1)].unwrap()).unwrap()
for (k, (x, y)) in (&mut d).items():
    *x += 10
    *y += 20
print(d).unwrap()
print(x, y, z).unwrap()
"#,
        "1 2 3\n1 2 3\n[(1, 2, 3)]\n[(1, 2, 3)]\n[(11, 22)] [33]\n1 2 3\n[(1, 2, 3)]\n[(1, 2, 3)]\n{1: (12, 23)}\n99 88 77\n",
    );
    expect(
        r#"
def values() -> Generator[Result[i64, OverflowError[i64]]]:
    rows = [(1, 2)].unwrap()
    labels = [3].unwrap()
    total = checked:
        mut sum = 0
        for ((x, y), z) in zip(&rows, &labels):
            sum += *x + *y + *z
        sum
    yield total
print([value? for value in values()].unwrap()).unwrap()
d = {1: 2}.unwrap()
for (k, v) in d.items():
    print(*k, *v).unwrap()
print([(*k, *v) for (k, v) in d.items()].unwrap()).unwrap()
"#,
        "[6]\n1 2\n[(1, 2)]\n",
    );
    for target in ["(x, y), z", "((x, y), z)"] {
        let diagnostic = support::check_diagnostic(&program(&format!(
            "def values() -> Generator[Result[i64, OverflowError[i64]]]:\n    rows = [(1, 2)].unwrap()\n    labels = [3].unwrap()\n    for {target} in zip(&rows, &labels):\n        value = checked:\n            *x + *y + *z\n        yield value\nprint([value? for value in values()].unwrap()).unwrap()"
        )));
        assert!(
            diagnostic.contains("cannot remain live across yield"),
            "{diagnostic}"
        );
    }
}

#[test]
fn nested_comprehension_and_generator_bindings_preserve_affine_outer_owners() {
    expect(
        r#"
class Resource:
    id: i64
    def __del__(self: &mut Resource) -> ():
        print("drop", self.id).unwrap()
item = Resource(90)
print([item.id for (item, _), _ in [((Resource(1), Resource(2)), 3)].unwrap()].unwrap()).unwrap()
print("outer", item.id).unwrap()
mut pending = (item.id for (item, _), _ in [((Resource(3), Resource(4)), 5)].unwrap())
print(next(pending)).unwrap()
drop(pending)
print("outer", item.id).unwrap()
"#,
        "drop 2\ndrop 1\n[1]\nouter 90\ndrop 4\nOption[i64].Some(3)\ndrop 3\nouter 90\ndrop 90\n",
    );
}

fn run(source: &str) -> std::process::Output {
    support::run(&program(source))
}

fn expect(source: &str, expected: &str) {
    support::assert_stdout(&program(source), expected);
}
