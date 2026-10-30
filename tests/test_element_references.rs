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
match counts.get_mut("a"):
    case Some(v):
        *v = 4
    case Nothing:
        pass
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
        "mut a = [1, 2].unwrap()\ni = 1\nr = &mut a[0]\ns = &mut a[i]\nprint(r).unwrap()\nprint(s).unwrap()",
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

/// Distinct nonnegative integer literals name distinct list elements, so
/// their loans coexist. Reads and writes of another literal index do too.
#[test]
fn distinct_literal_list_indices_are_disjoint() {
    support::assert_stdout(
        r#"
def add_into(target: &mut list[i64], source: &list[i64]) -> Result[(), AllocError]:
    for value in source:
        target.append(*value)?
    Ok(())

def first_two(values: &mut list[i64]) -> ():
    a = &mut values[0]
    b = &values[1]
    *a = *a + *b

def main() -> Result[(), AllocError | IoError]:
    mut buffers = [[1, 2]?, [3]?]?
    add_into(&mut buffers[1], &buffers[0])?
    front = &mut buffers[0]
    buffers[1].append(4)?
    front.append(9)?
    print(buffers)?
    mut grid = [[1, 2]?, [3, 4]?]?
    top = &mut grid[0][1]
    bottom = &grid[1][1]
    *top = *bottom
    print(grid)?
    mut xs = [1, 2, 3]?
    a = &mut xs[0]
    xs[1] = 7
    xs[2] += 10
    print(xs[2])?
    *a = 5
    print(xs)?
    first_two(&mut xs)
    print(xs)?
    for i in range(2):
        left = &mut xs[0]
        right = &mut xs[1]
        *left = *left + i
        *right = *right + i
    print(xs)?
    Ok(())
"#,
        "[[1, 2, 9], [3, 1, 2, 4]]\n[[1, 4], [3, 4]]\n13\n[5, 7, 13]\n[12, 7, 13]\n[13, 8, 13]\n",
    );
}

/// Everything that is not two distinct literal indices still borrows the
/// whole list, and any whole-list access conflicts with an element loan.
#[test]
fn unproven_indices_and_whole_list_access_conflict() {
    let cases = [
        (
            "a = &mut xs[0]\nb = &mut xs[0]\n*a = 1\n*b = 2",
            "`xs[0]` while it is exclusively borrowed",
        ),
        (
            "a = &mut xs[0]\nb = &xs[0]\n*a = 1\nprint(b).unwrap()",
            "`xs[0]` while it is exclusively borrowed",
        ),
        (
            "i = 1\na = &mut xs[0]\nb = &mut xs[i]\n*a = 1\n*b = 2",
            "`xs` while `xs[0]` is exclusively borrowed",
        ),
        (
            "a = &mut xs[2]\nb = &mut xs[-1]\n*a = 1\n*b = 2",
            "`xs` while `xs[2]` is exclusively borrowed",
        ),
        (
            "a = &mut xs[0]\nxs.append(4).unwrap()\n*a = 1",
            "`xs` while `xs[0]` is exclusively borrowed",
        ),
        (
            "a = &xs[0]\nxs.clear()\nprint(a).unwrap()",
            "`xs` while `xs[0]` is borrowed",
        ),
        (
            "a = &xs[0]\nxs.pop()\nprint(a).unwrap()",
            "`xs` while `xs[0]` is borrowed",
        ),
        (
            "a = &xs[0]\nxs = [4].unwrap()\nprint(a).unwrap()",
            "assign to `xs` while `xs[0]` is borrowed",
        ),
        (
            "a = &xs[0]\ndrop(xs)\nprint(a).unwrap()",
            "use of moved binding `xs`",
        ),
        (
            "a = &mut xs[0]\nprint(len(xs)).unwrap()\n*a = 1",
            "read or borrow `xs` while `xs[0]` is exclusively borrowed",
        ),
        (
            "a = &mut xs[0]\ntake(&xs)\n*a = 1",
            "read or borrow `xs` while `xs[0]` is exclusively borrowed",
        ),
        (
            "a = &mut xs[0]\nfor x in &xs:\n    print(x).unwrap()\n*a = 1",
            "`xs` while `xs[0]` is exclusively borrowed",
        ),
        (
            "mut d = {1: 2, 3: 4}.unwrap()\na = d.get_mut(1)\nb = d.get_mut(3)\nprint(str.repr(a).unwrap()).unwrap()",
            "`d` while it is exclusively borrowed",
        ),
        (
            "a = &mut xs[0]\nfor i in range(2):\n    b = &mut xs[0]\n    *b = i\n*a = 1",
            "`xs[0]` while it is exclusively borrowed",
        ),
        (
            "mut a = &mut xs[0]\nb = &mut xs[1]\n*a = 1\n*b = 2",
            "`xs[1]` while `xs` is exclusively borrowed",
        ),
    ];
    for (body, expected) in cases {
        let source = format!(
            "def take(values: &list[i64]) -> ():\n    pass\nmut xs = [1, 2, 3].unwrap()\n{body}"
        );
        let error = support::check_diagnostic(&source);
        assert!(error.contains(expected), "{source}\n{error}");
    }
}
