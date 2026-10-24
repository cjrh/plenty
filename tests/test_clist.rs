//! Packed columns preserve scalar bits, owner loans and failed append inputs.
mod support;
use rstest::rstest;
use support::assert_visible_stdout as native;

#[test]
fn nested_owner_indices_are_evaluated_in_order_and_only_once() {
    native(
        r#"
class Point:
    x: i64
def outer() -> i64:
    print("owner").unwrap()
    0
def inner() -> i64:
    print("row").unwrap()
    0
def rhs() -> i64:
    print("rhs").unwrap()
    3
mut rows = clist[Point]()
rows.append(Point(4)).unwrap()
mut groups = [rows].unwrap()
print(groups[outer()][inner()].x).unwrap()
print(groups[outer()].copy_row(inner()).unwrap().x).unwrap()
groups[outer()][inner()].x = rhs()
groups[outer()][inner()].x += rhs()
print(groups[0][0].x).unwrap()
mut wrapped = (clist[Point](), 7)
wrapped[0].append(Point(8)).unwrap()
wrapped[0][0].x += 2
print(wrapped[0][0].x).unwrap()
shared = Shared(groups).unwrap()
print(shared[outer()][inner()].x).unwrap()
"#,
        "owner\nrow\n4\nowner\nrow\n4\nrhs\nowner\nrow\nowner\nrow\nrhs\n6\n10\nowner\nrow\n6",
    );
}

#[test]
fn primitive_columns_rows_and_pipelines() {
    native(
        r#"
class Point:
    x: i64
    y: f64
def add(total: i64, x: i64) -> i64:
    total + x
mut points = clist[Point]()
print(len(points), points.capacity()).unwrap()
points.reserve(10).unwrap()
for i in range(10):
    points.append(Point(i, f64(i))).unwrap()
points[-1].x = 40
print(len(points), points[0].x, points[-1].x, points[-2].y).unwrap()
print(points.copy_row(2), points.copy_row(-1), points.copy_row(10)).unwrap()
print(iter(points.column(Point.x)).fold(0, add)).unwrap()
mut total = 0
for x, y in zip(points.column(Point.x), points.column(Point.y)):
    total += x + i64(y)
print(total).unwrap()
"#,
        "0 0\n10 0 40 8.0\nOption[Point].Some(Point(x=2, y=2.0)) Option[Point].Some(Point(x=40, y=9.0)) Option[Point].Nothing\n76\n121",
    );
}

#[test]
fn packed_widths_and_float_bits_roundtrip() {
    native(
        r#"
class Values:
    flag: bool
    a: i8
    b: u8
    c: i16
    d: u16
    e: i32
    f: u32
    g: i64
    h: u64
    i: f32
    j: f64
mut rows = clist[Values].with_capacity(1).unwrap()
rows.append(Values(True, -128i8, 255u8, -32768i16, 65535u16, -2147483648i32, 4294967295u32, -9223372036854775808, 18446744073709551615u64, -0.0f32, -0.0)).unwrap()
print(rows.copy_row(0)).unwrap()
for x in rows.column(Values.i):
    print(x).unwrap()
for x in rows.column(Values.h):
    print(x).unwrap()
for x in rows.column(Values.a):
    print(x).unwrap()
"#,
        "Option[Values].Some(Values(flag=True, a=-128, b=255, c=-32768, d=65535, e=-2147483648, f=4294967295, g=-9223372036854775808, h=18446744073709551615, i=-0.0, j=-0.0))\n-0.0\n18446744073709551615\n-128",
    );
}

#[rstest]
#[case("items.append(Point(3)).unwrap()", "borrow")]
#[case("items.reserve(100).unwrap()", "borrow")]
#[case("items[0].x = 10", "borrow")]
#[case("moved = items", "borrow")]
fn column_plan_protects_the_whole_owner(#[case] mutation: &str, #[case] expected: &str) {
    let error = support::check_source(&format!(
        r#"
class Point:
    x: i64
mut items = clist[Point]()
items.append(Point(1)).unwrap()
values = items.column(Point.x)
{mutation}
print(values.count()).unwrap()
"#
    ))
    .unwrap_err()
    .to_string();
    assert!(error.contains(expected), "{error}");
}

#[rstest]
#[case("class Point:\n    x: str", "primitive scalar")]
#[case(
    "class Point:\n    x: i64\n    def __del__(self: &mut Point) -> ():\n        ()",
    "destructor"
)]
fn unsupported_rows_are_diagnosed(#[case] schema: &str, #[case] expected: &str) {
    let error = support::check_source(&format!("{schema}\nitems = clist[Point]()"))
        .unwrap_err()
        .to_string();
    assert!(error.contains(expected), "{error}");
}

#[test]
fn empty_and_out_of_bounds_rows() {
    native(
        r#"
class Point:
    x: i64
items = clist[Point]()
print(items.copy_row(0), items.copy_row(-1), items.column(Point.x).count()).unwrap()
"#,
        "Option[Point].Nothing Option[Point].Nothing 0",
    );
    let output = support::run(
        "class Point:\n    x: i64\nitems = clist[Point]()\nprint(items[0].x).unwrap()",
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("clist index out of bounds"));
}

#[test]
fn projected_enum_owner_preserves_tags_and_row_updates_evaluate_once() {
    native(
        r#"
class Point:
    x: i64
def build() -> clist[Point]:
    clist[Point]()
def grow(items: &mut clist[Point]) -> ():
    items.append(Point(7)).unwrap()
def position() -> i64:
    print("index").unwrap()
    0
def increment() -> i64:
    print("value").unwrap()
    2
mut wrapped = Some(build())
match &mut wrapped:
    case Some(items):
        grow(&mut *items)
    case Nothing:
        ()
match wrapped:
    case Some(items):
        mut owned = items
        owned[position()].x += increment()
        owned[position()].x = increment()
        print(owned[0].x).unwrap()
    case Nothing:
        print("lost tag").unwrap()
"#,
        "index\nvalue\nvalue\nindex\n2",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_append_preserves_row_and_reserved_work_does_not_allocate() {
    native(
        r#"
class Point:
    x: i64
    y: i64
print("__test_fail_allocations_after_0__").unwrap()
mut items = clist[Point]()
failed = items.append(Point(21, 42))
print("__test_restore_allocations__").unwrap()
print(len(items)).unwrap()
match failed:
    case Err(AppendError.OutOfMemory(row)):
        print(row).unwrap()
        items.reserve(1).unwrap()
        print("__test_fail_allocations_after_0__").unwrap()
        items.append(row).unwrap()
        copied = items.copy_row(0)
        count = items.column(Point.x).count()
        print("__test_restore_allocations__").unwrap()
        print(copied, count).unwrap()
    case _:
        print("wrong outcome").unwrap()
"#,
        "0\nPoint(x=21, y=42)\nOption[Point].Some(Point(x=21, y=42)) 1",
    );
}

#[test]
fn column_plans_cannot_escape_into_generator_storage() {
    let error = support::check_source(
        r#"
class Point:
    x: i64
items = clist[Point]()
escaped = items.column(Point.x).generator()
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("borrowed state cannot be suspended"),
        "{error}"
    );
}

// B76: relax this conservative boundary only with generator-frame loan support.
#[test]
fn column_iteration_in_generator_functions_is_explicitly_rejected() {
    let error = support::check_source(
        r#"
class Point:
    x: i64
def values() -> Generator[i64]:
    items = clist[Point]()
    for value in items.column(Point.x):
        yield value
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("column iteration is not supported inside generator functions"),
        "{error}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_growth_and_overflow_leave_existing_columns_unchanged() {
    native(
        r#"
class Point:
    x: i64
    y: u8
mut items = clist[Point].with_capacity(8).unwrap()
for i in range(8):
    items.append(Point(i, u8(i))).unwrap()
print("__test_fail_allocations_after_0__").unwrap()
failed = items.append(Point(8, 255u8))
print("__test_restore_allocations__").unwrap()
print(len(items), items.capacity(), items[-1].x, items[-1].y).unwrap()
print(str.repr(items.reserve(-1)).unwrap(), str.repr(items.reserve(9223372036854775807)).unwrap()).unwrap()
match failed:
    case Err(AppendError.OutOfMemory(row)):
        items.append(row).unwrap()
    case _:
        print("wrong outcome").unwrap()
print(items.copy_row(8), len(items)).unwrap()
"#,
        "8 8 7 7\nResult[(), AllocError].Err(AllocError.CapacityOverflow) Result[(), AllocError].Err(AllocError.CapacityOverflow)\nOption[Point].Some(Point(x=8, y=255)) 9",
    );
}

#[test]
fn generic_rows_and_stored_owners_keep_type_identity() {
    native(
        r#"
class Cell[T]:
    value: T
class Store:
    rows: clist[Cell[i64]]
def count[T](rows: &clist[T]) -> i64:
    len(rows)
mut store = Store(clist[Cell[i64]]())
store.rows.append(Cell[i64](7)).unwrap()
print(count(&store.rows), store.rows.copy_row(0).unwrap().value).unwrap()
owners = [store].unwrap()
print(len(owners)).unwrap()
"#,
        "1 7\n1",
    );
}

#[test]
fn column_selectors_and_copy_rows_obey_module_privacy() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("model.plenty"),
        "pub class Point:\n    x: i64\npub def make() -> clist[Point]:\n    clist[Point]()\n",
    )
    .unwrap();
    for operation in [
        "clist[Point]()",
        "items.copy_row(0)",
        "items.column(Point.x).count()",
        "items[0].x",
    ] {
        std::fs::write(dir.path().join("main.plenty"), format!("from model import Point, make\ndef main() -> ():\n    items = make()\n    value = {operation}\n")).unwrap();
        let error = plenty::check_file(&dir.path().join("main.plenty"), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("private"), "{operation}: {error}");
    }
}

#[test]
fn clist_aliases_equality_and_nested_nan_comparisons() {
    native(
        r#"
class Point:
    x: f64
type Points = clist[Point]
class Holder:
    points: Points
mut a = Points()
mut b = Points.new()
print(a == b).unwrap()
a.append(Point(1.0)).unwrap()
b.append(Point(1.0)).unwrap()
print(a == b).unwrap()
a[0].x = f64.NAN
print(a == a).unwrap()
holder = Holder(a)
print(holder == holder).unwrap()
"#,
        "True\nTrue\nFalse\nFalse",
    );
}

#[rstest]
#[case("&items[0].x", "packed clist fields cannot be borrowed")]
#[case("items[0]", "rows are not contiguous")]
#[case("items.column(Point.y)", "unknown field")]
fn unsupported_projection_diagnostics(#[case] expression: &str, #[case] message: &str) {
    let error = support::check_source(&format!(
        "class Point:\n    x: i64\nitems = clist[Point]()\nvalue = {expression}"
    ))
    .unwrap_err()
    .to_string();
    assert!(error.contains(message), "{error}");
}
