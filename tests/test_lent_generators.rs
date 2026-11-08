//! Loops over a lent generator (`for value in &mut g`) advance the lender's
//! frame in place and keep its remaining progress (issue #872).
mod support;
use support::{assert_stdout as expect, check_error};

const NUMBERS: &str = r#"
def numbers(stop: i64) -> Generator[i64]:
    mut value = 0
    while value < stop:
        yield value
        # value < stop <= i64.MAX
        unchecked:
            value = value + 1
"#;

fn with_numbers(body: &str) -> String {
    format!("{NUMBERS}{body}")
}

#[test]
fn break_keeps_progress_for_next_loops_and_owned_consumers() {
    expect(
        &with_numbers(
            r#"
def take_two(g: &mut Generator[i64]) -> ():
    mut seen = 0
    for v in g:
        print(v).unwrap()
        unchecked:
            seen = seen + 1
        if seen == 2:
            break
mut g = numbers(7)
for v in &mut g:
    print(v).unwrap()
    if v == 1:
        break
take_two(&mut g)
print(next(g)).unwrap()
for v in &mut g:
    print(v).unwrap()
print(next(&mut g)).unwrap()
for v in g:
    print(v).unwrap()
"#,
        ),
        "0\n1\n2\n3\nOption[i64].Some(4)\n5\n6\nOption[i64].Nothing\n",
    );
}

#[test]
fn return_and_propagation_leave_the_rest_in_a_generic_borrower() {
    expect(
        &with_numbers(
            r#"
def first_over[T](g: &mut Generator[T], limit: T) -> Option[T]:
    for v in g:
        if v > limit:
            return Some(v)
    Nothing
def odd(v: i64) -> Result[i64, str]:
    if v % 2 == 1:
        return Err("odd")
    Ok(v)
def evens(g: &mut Generator[i64]) -> Result[i64, str]:
    mut count = 0
    for v in g:
        odd(v)?
        unchecked:
            count = count + 1
    Ok(count)
mut g = numbers(10)
print(first_over(&mut g, 1)).unwrap()
match evens(&mut g):
    case Ok(n):
        print(n).unwrap()
    case Err(e):
        print(e).unwrap()
print(next(g)).unwrap()
"#,
        ),
        "Option[i64].Some(2)\nodd\nOption[i64].Some(4)\n",
    );
}

#[test]
fn comprehensions_enumerate_and_zip_take_a_lent_generator() {
    expect(
        &with_numbers(
            r#"
mut g = numbers(6)
for i, v in enumerate(&mut g):
    if i == 1:
        break
evens = [v for v in &mut g if v % 2 == 0].unwrap()
print(evens).unwrap()
print(next(g)).unwrap()
# zip takes 2 from `h` before finding `pair` exhausted.
mut h = numbers(5)
pair = [10, 20].unwrap()
for a, b in zip(&mut h, &pair):
    print(a).unwrap()
print(next(h)).unwrap()
# As the last input, `k` is not advanced once `pair` ends.
mut k = numbers(5)
for b, a in zip(&pair, &mut k):
    print(a).unwrap()
print(next(k)).unwrap()
"#,
        ),
        "[2, 4]\nOption[i64].Nothing\n0\n1\nOption[i64].Some(3)\n0\n1\nOption[i64].Some(2)\n",
    );
}

#[test]
fn the_lender_owns_suspended_locals_after_break() {
    // The loop exit keeps the frame; the owner's scope end drops it once.
    expect(
        r#"
class Resource:
    n: i64
    def __del__(self: &mut Resource) -> ():
        print(self.n).unwrap()
def values(resource: Resource) -> Generator[i64]:
    local = Resource(20)
    yield resource.n
    yield local.n
def run() -> ():
    mut g = values(Resource(1))
    for v in &mut g:
        print(v).unwrap()
        break
    print("after loop").unwrap()
run()
print("after owner").unwrap()
"#,
        "1\nafter loop\n20\n1\nafter owner\n",
    );
}

#[test]
fn lent_loops_add_no_allocation() {
    expect(
        r#"
def labels() -> Generator[Option[i64]]:
    yield Some(1)
    yield Nothing
    yield Some(3)
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
mut g = labels()
mut found = 0
for item in &mut g:
    match item:
        case Some(n):
            found = n
        case Nothing:
            break
last = next(g)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(found).unwrap()
print(last).unwrap()
"#,
        "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n1\nOption[Option[i64]].Some(Option[i64].Some(3))\n",
    );
}

#[test]
fn lent_generator_loops_reject_conflicts_and_non_advancing_sources() {
    let cases = [
        (
            "mut g = numbers(3)\nfor v in &mut g:\n    drop(next(&mut g))",
            "conflicting borrow",
        ),
        (
            "g = numbers(3)\nfor v in &mut g:\n    pass",
            "mutable borrowing requires a mut binding",
        ),
        (
            "mut g = numbers(3)\nfor v in &g:\n    pass",
            "a shared generator reference cannot be advanced; lend it as `&mut` to loop over it",
        ),
        (
            "def f(g: &Generator[i64]) -> ():\n    for v in g:\n        pass\nmut g = numbers(3)\nf(&g)",
            "a shared generator reference cannot be advanced",
        ),
        (
            "mut g = numbers(3)\nfor v in reversed(&mut g):\n    pass",
            "reversed requires a list",
        ),
        // A loan cannot cross `yield` (B12); the owning loop or `next` works.
        (
            "def outer() -> Generator[i64]:\n    mut g = numbers(3)\n    for v in &mut g:\n        yield v\ndrop(outer())",
            "cannot remain live across yield",
        ),
    ];
    for (body, expected) in cases {
        let error = check_error(&with_numbers(body));
        assert!(error.contains(expected), "{body}\n{error}");
    }
}

#[test]
fn different_producers_name_the_frame_identity() {
    let error = check_error(
        r#"
def one() -> Generator[i64]:
    yield 1
def two() -> Generator[i64]:
    yield 2
mut source = one()
source = two()
drop(source)
"#,
    );
    assert!(
        error.contains(
            "expected Generator[i64] from `one`, got Generator[i64] from `two`; different producers join only in a conditional expression"
        ),
        "{error}"
    );
}
