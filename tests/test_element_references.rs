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
