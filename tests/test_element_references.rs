//! Element loans protect backing storage from invalidation.
mod support;

#[test]
fn elements_can_be_shared_mutated_projected_and_returned() {
    let out = support::run(
        r#"
class Point:
    x: i64
def first(values: &list[i64]) -> &i64:
    &values[0]
mut values = [1, 2]
a = &values[0]
b = &values[-1]
print(a)
print(b)
r = &mut values[0]
*r = 7
print(first(&values))
mut points = [Point(3)]
p = &mut points[0].x
*p = 9
print(points)
mut counts = {"a": 1}
v = &mut counts["a"]
*v = 4
print(counts)
"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "1\n2\n7\n[Point(x=9)]\n{\"a\": 4}\n"
    );
}

#[test]
fn a_borrowed_loop_can_return_an_element_from_its_parameter() {
    let out = support::run(
        r#"
class Point:
    x: i64
def find(points: &list[Point], wanted: i64) -> &Point:
    for point in points:
        if point.x == wanted:
            return point
    &points[0]
mut points = [Point(1), Point(2)]
found = find(&points, 2)
print(found.x)
points.append(Point(3))
print(len(points))
"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n3\n");
}

#[test]
fn element_loans_reject_invalidating_access() {
    for source in [
        "mut a = [1]\nr = &a[0]\na.append(2)\nprint(r)",
        "mut a = [1]\nr = &a[0]\ndrop(a)\nprint(r)",
        "mut a = [1, 2]\nr = &mut a[0]\ns = &mut a[1]\nprint(r)\nprint(s)",
        "a = [1]\nr = &mut a[0]",
        "mut a = {1: 2}\nr = &a[1]\na.pop(1)\nprint(r)",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}

#[test]
fn borrowed_loops_preserve_owners_and_allow_element_mutation() {
    let out = support::run(
        r#"
class Point:
    x: i64
    def __del__(self: &mut Point) -> ():
        print(self.x)
def bump(points: &mut list[Point]) -> ():
    for point in points:
        point.x = point.x + 10
mut points = [Point(1), Point(2)]
for point in &points:
    print(point.x)
bump(&mut points)
for point in &mut points:
    if point.x == 11:
        continue
    point.x = 30
    break
print(points)
mut numbers = [1, 2]
for number in &mut numbers:
    *number = *number + 3
print(numbers)
print([point.x for point in &points])
"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "1\n2\n[Point(x=11), Point(x=30)]\n[4, 5]\n[11, 30]\n11\n30\n"
    );
}

#[test]
fn borrowed_loops_protect_storage_and_owned_elements() {
    for source in [
        "mut a = [[1]]\nfor item in &a:\n    a.append([2])\n    print(item)",
        "mut a = [[1]]\nfor item in &mut a:\n    drop(a)",
        "a = [[1]]\nfor item in &a:\n    item.append(2)",
        "a = [[1]]\nfor item in &a:\n    owned: list[i64] = *item",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}
