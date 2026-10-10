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
mut values = [1, 2].unwrap()
a = &values[0]
b = &values[-1]
print(a).unwrap()
print(b).unwrap()
r = &mut values[0]
*r = 7
print(first(&values)).unwrap()
mut points = [Point(3)].unwrap()
p = &mut points[0].x
*p = 9
print(points).unwrap()
mut counts = {"a": 1}.unwrap()
v = &mut counts["a"]
*v = 4
print(counts).unwrap()
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
mut points = [Point(1), Point(2)].unwrap()
found = find(&points, 2)
print(found.x).unwrap()
points.append(Point(3)).unwrap()
print(len(points)).unwrap()
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
        "mut a = [1].unwrap()\nr = &a[0]\na.append(2).unwrap()\nprint(r).unwrap()",
        "mut a = [1].unwrap()\nr = &a[0]\ndrop(a)\nprint(r).unwrap()",
        "mut a = [1, 2].unwrap()\nr = &mut a[0]\ns = &mut a[1]\nprint(r).unwrap()\nprint(s).unwrap()",
        "a = [1].unwrap()\nr = &mut a[0]",
        "mut a = {1: 2}.unwrap()\nr = &a[1]\na.pop(1)\nprint(r).unwrap()",
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
        print(self.x).unwrap()
def bump(points: &mut list[Point]) -> ():
    for point in points:
        point.x = point.x + 10
mut points = [Point(1), Point(2)].unwrap()
for point in &points:
    print(point.x).unwrap()
bump(&mut points)
for point in &mut points:
    if point.x == 11:
        continue
    point.x = 30
    break
print(points).unwrap()
mut numbers = [1, 2].unwrap()
for number in &mut numbers:
    *number = *number + 3
print(numbers).unwrap()
print([point.x for point in &points].unwrap()).unwrap()
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
        "mut a = [[1].unwrap()].unwrap()\nfor item in &a:\n    a.append([2].unwrap()).unwrap()\n    print(item).unwrap()",
        "mut a = [[1].unwrap()].unwrap()\nfor item in &mut a:\n    drop(a)",
        "a = [[1].unwrap()].unwrap()\nfor item in &a:\n    item.append(2).unwrap()",
        "a = [[1].unwrap()].unwrap()\nfor item in &a:\n    owned: list[i64] = *item",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}
