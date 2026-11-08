//! A factory may return different producers with one item type; its output is
//! an inline frame sized for the largest member (issue #872).
mod support;
use support::{assert_stdout as expect, check_error};

const PRODUCERS: &str = r#"
class Res:
    id: i64
    def __del__(self) -> ():
        print("drop", self.id).unwrap()
def one() -> Generator[i64]:
    yield 1
def two(r: Res) -> Generator[i64]:
    yield 2
    yield r.id
def choose(flag: bool) -> Generator[i64]:
    if flag:
        return one()
    two(Res(30))
def pick(n: i64) -> Generator[i64]:
    match n:
        case 0:
            one()
        case 1:
            two(Res(31))
        case _:
            one()
def either(flag: bool) -> Generator[i64]:
    if flag:
        one()
    else:
        two(Res(32))
"#;

fn with_producers(body: &str) -> String {
    format!("{PRODUCERS}{body}")
}

#[test]
fn return_and_tail_if_and_match_join_producers() {
    expect(
        &with_producers(
            r#"
for v in choose(True):
    print(v).unwrap()
for v in choose(False):
    print(v).unwrap()
print(list(pick(0)).unwrap()).unwrap()
print(list(pick(1)).unwrap()).unwrap()
print(list(either(True)).unwrap()).unwrap()
print(list(either(False)).unwrap()).unwrap()
"#,
        ),
        "1\n2\n30\ndrop 30\n[1]\ndrop 31\n[2, 31]\n[1]\ndrop 32\n[2, 32]\n",
    );
}

#[test]
fn joined_frames_drop_owned_locals_once_whether_unstarted_or_suspended() {
    expect(
        &with_producers(
            r#"
unstarted = choose(False)
print("unstarted").unwrap()
drop(unstarted)
mut g = choose(False)
print(next(g)).unwrap()
print("suspended").unwrap()
drop(g)
mut h = pick(1)
for v in &mut h:
    print("lent", v).unwrap()
    break
print(next(h)).unwrap()
print(next(h)).unwrap()
print("end").unwrap()
"#,
        ),
        "unstarted\ndrop 30\nOption[i64].Some(2)\nsuspended\ndrop 30\nlent 2\n\
         Option[i64].Some(31)\ndrop 31\nOption[i64].Nothing\nend\n",
    );
}

#[test]
fn nested_generic_and_generator_expression_members_flatten() {
    expect(
        r#"
def one[T](x: T) -> Generator[T]:
    yield x
def two[T](x: T, y: T) -> Generator[T]:
    yield x
    yield y
def pick[T](flag: bool, x: T, y: T) -> Generator[T]:
    if flag:
        return one(x)
    two(x, y)
def wider(n: i64) -> Generator[i64]:
    match n:
        case 0:
            pick(True, 1, 2)
        case 1:
            (v for v in range(3))
        case _:
            two(7, 8)
def outer(n: i64) -> Generator[i64]:
    return wider(n)
for v in pick(False, "a", "b"):
    print(v).unwrap()
print(list(outer(0)).unwrap()).unwrap()
print(list(outer(1)).unwrap()).unwrap()
print(list(outer(2)).unwrap()).unwrap()
"#,
        "a\nb\n[1]\n[0, 1, 2]\n[7, 8]\n",
    );
}

#[test]
fn joined_frames_feed_pipelines_wrappers_and_other_generators() {
    expect(
        &with_producers(
            r#"
def doubled(flag: bool) -> Generator[i64]:
    for v in choose(flag):
        unchecked:
            yield v * 2
def double(x: i64) -> i64:
    unchecked:
        x * 2
def add(a: i64, b: i64) -> i64:
    unchecked:
        a + b
def wrapped(g: Generator[i64]) -> Option[Generator[i64]]:
    Some(g)
print(list(doubled(False)).unwrap()).unwrap()
print(choose(False).map(double).fold(0, add)).unwrap()
match wrapped(choose(False)):
    case Some(g):
        print(list(g).unwrap()).unwrap()
    case Nothing:
        pass
mut g = choose(True)
g = choose(False)
print([v for v in g].unwrap()).unwrap()
for (v, i) in zip(choose(False), range(5)):
    print(v, i).unwrap()
"#,
        ),
        "drop 30\n[4, 60]\ndrop 30\n64\ndrop 30\n[2, 30]\ndrop 30\n[2, 30]\n2 0\n30 1\ndrop 30\n",
    );
}

#[test]
fn branch_values_reassignment_and_payloads_keep_one_identity() {
    // Phase guard (issue #872): delete when these positions join producers.
    let producers = r#"
def one() -> Generator[i64]:
    yield 1
def two() -> Generator[i64]:
    yield 2
"#;
    let hint = "a variable or branch value holds one producer's inline frame";
    for body in [
        "def f(flag: bool) -> Generator[i64]:\n    g = one() if flag else two()\n    g\ndrop(f(True))\n",
        "mut g = one()\ng = two()\ndrop(g)\n",
        "def f(flag: bool) -> Option[Generator[i64]]:\n    if flag:\n        return Some(one())\n    Some(two())\ndrop(f(True))\n",
    ] {
        let error = check_error(&format!("{producers}{body}"));
        assert!(error.contains(hint), "{error}");
    }
}

#[test]
fn joined_producers_with_different_items_stay_rejected() {
    let error = check_error(
        r#"
def one() -> Generator[i64]:
    yield 1
def two() -> Generator[str]:
    yield "x"
def f(flag: bool) -> Generator[i64]:
    if flag:
        return one()
    two()
drop(f(True))
"#,
    );
    assert!(error.contains("got Generator[str]"), "{error}");
}

#[test]
fn a_factory_joining_its_own_consumer_has_no_finite_frame() {
    let error = check_error(
        r#"
def one() -> Generator[i64]:
    yield 1
def a() -> Generator[i64]:
    for v in f(False):
        yield v
def f(flag: bool) -> Generator[i64]:
    if flag:
        return a()
    one()
drop(f(True))
"#,
    );
    assert!(
        error.contains("recursive inline generator layout"),
        "{error}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn joining_and_running_frames_allocates_nothing() {
    support::assert_filtered_stdout(
        r#"
def one() -> Generator[i64]:
    yield 1
def wide(a: i64, b: i64, c: i64) -> Generator[i64]:
    yield a
    yield b
    yield c
def choose(flag: bool) -> Generator[i64]:
    if flag:
        return one()
    wide(2, 3, 4)
def sum(g: Generator[i64]) -> i64:
    mut total = 0
    for v in g:
        unchecked:
            total = total + v
    total
print("__test_fail_allocations_after_0__").unwrap()
a = sum(choose(True))
b = sum(choose(False))
mut g = choose(False)
first = next(g)
drop(g)
drop(choose(False))
print("__test_restore_allocations__").unwrap()
print(a, b, first).unwrap()
"#,
        "1 9 Option[i64].Some(2)",
    );
}
