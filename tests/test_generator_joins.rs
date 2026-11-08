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
fn conditional_expressions_join_producers_and_drop_once() {
    expect(
        &with_producers(
            r#"
def returned(flag: bool) -> Generator[i64]:
    return one() if flag else two(Res(10))
def tail(flag: bool) -> Generator[i64]:
    one() if flag else two(Res(11))
def mixed(n: i64) -> Generator[i64]:
    if n == 0:
        return pick(2)
    one() if n == 1 else two(Res(12))
def inner(flag: bool) -> Generator[i64]:
    g = one() if flag else two(Res(13))
    for v in g:
        yield v
def count[T](g: Generator[T]) -> i64:
    mut n = 0
    for v in g:
        unchecked:
            n = n + 1
    n
print(list(returned(True)).unwrap()).unwrap()
print(list(returned(False)).unwrap()).unwrap()
print(list(tail(False)).unwrap()).unwrap()
for n in range(3):
    print(list(mixed(n)).unwrap()).unwrap()
print(list(inner(False)).unwrap()).unwrap()
print(count(one() if False else two(Res(14)))).unwrap()
mut g = two(Res(15)) if True else one()
print(next(g)).unwrap()
drop(g)
print("after 15").unwrap()
unstarted = one() if False else two(Res(16))
drop(unstarted)
print("after 16").unwrap()
print(list((one() if True else two(Res(17))) if False else pick(2)).unwrap()).unwrap()
"#,
        ),
        "[1]\ndrop 10\n[2, 10]\ndrop 11\n[2, 11]\n[1]\n[1]\ndrop 12\n[2, 12]\n\
         drop 13\n[2, 13]\ndrop 14\n2\nOption[i64].Some(2)\ndrop 15\nafter 15\n\
         drop 16\nafter 16\n[1]\n",
    );
}

#[test]
fn if_and_match_values_join_producers_and_drop_once() {
    expect(
        &with_producers(
            r#"
def three(a: i64, b: i64) -> Generator[i64]:
    yield a
    yield b
def matched(n: i64) -> Option[list[i64]]:
    g = match n:
        case 0:
            one()
        case 1:
            two(Res(10))
        case 2:
            return Nothing
        case _:
            three(5, 6)
    Some(list(g).unwrap())
def chosen(n: i64, flag: bool) -> list[i64]:
    g = if flag:
        match n:
            case 0:
                one()
            case _:
                two(Res(21))
    elif n > 5:
        three(n, n)
    else:
        one()
    list(g).unwrap()
def unstarted(flag: bool) -> ():
    g = if flag:
        two(Res(40))
    else:
        one()
    drop(g)
def across(skip: i64) -> Generator[i64]:
    for i in range(3):
        g = match i:
            case 0:
                one()
            case 1:
                if skip == 1:
                    continue
                two(Res(51))
            case _:
                break
        for v in g:
            yield v
        yield 0
for n in range(4):
    print(matched(n)).unwrap()
print(chosen(0, True)).unwrap()
print(chosen(1, True)).unwrap()
print(chosen(9, False)).unwrap()
print(chosen(2, False)).unwrap()
unstarted(True)
unstarted(False)
print("after unstarted").unwrap()
print(list(across(0)).unwrap()).unwrap()
print(list(across(1)).unwrap()).unwrap()
mut g = across(0)
print(next(g)).unwrap()
print(next(g)).unwrap()
print(next(g)).unwrap()
drop(g)
print("after across").unwrap()
"#,
        ),
        "Option[list[i64]].Some([1])\ndrop 10\nOption[list[i64]].Some([2, 10])\n\
         Option[list[i64]].Nothing\nOption[list[i64]].Some([5, 6])\n\
         [1]\ndrop 21\n[2, 21]\n[9, 9]\n[1]\n\
         drop 40\nafter unstarted\n\
         drop 51\n[1, 0, 2, 51, 0]\n[1, 0]\n\
         Option[i64].Some(1)\nOption[i64].Some(0)\nOption[i64].Some(2)\ndrop 51\nafter across\n",
    );
}

#[test]
fn reassignment_and_payloads_keep_one_identity() {
    // Phase guard (issue #872): delete when these positions join producers.
    let producers = r#"
def one() -> Generator[i64]:
    yield 1
def two() -> Generator[i64]:
    yield 2
"#;
    let hint = "different producers join only where a value is chosen";
    for body in [
        "o = Some(one()) if True else Some(two())\ndrop(o)\n",
        "mut g = one()\ng = two()\ndrop(g)\n",
        "def f(flag: bool) -> Option[Generator[i64]]:\n    if flag:\n        return Some(one())\n    Some(two())\ndrop(f(True))\n",
    ] {
        let error = check_error(&format!("{producers}{body}"));
        assert!(error.contains(hint), "{error}");
    }
}

#[test]
fn joined_producers_with_different_items_stay_rejected() {
    let producers = r#"
def one() -> Generator[i64]:
    yield 1
def two() -> Generator[str]:
    yield "x"
"#;
    for body in [
        "def f(flag: bool) -> Generator[i64]:\n    if flag:\n        return one()\n    two()\ndrop(f(True))\n",
        "g = match 1:\n    case 0:\n        one()\n    case _:\n        two()\ndrop(g)\n",
        "g = if True:\n    one()\nelse:\n    two()\ndrop(g)\n",
    ] {
        let error = check_error(&format!("{producers}{body}"));
        assert!(error.contains("got Generator[str]"), "{error}");
    }
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
c = sum(one() if False else wide(5, 6, 7))
drop(wide(1, 1, 1) if True else one())
m = match c:
    case 18:
        wide(1, 2, 3)
    case _:
        one()
d = sum(m)
print("__test_restore_allocations__").unwrap()
print(a, b, first, c, d).unwrap()
"#,
        "1 9 Option[i64].Some(2) 18 6",
    );
}
