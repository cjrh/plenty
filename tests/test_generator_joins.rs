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
fn reassignment_joins_producers_and_drops_each_frame_once() {
    expect(
        &with_producers(
            r#"
def replaced() -> ():
    mut g = two(Res(50))
    print(next(g)).unwrap()
    g = one()
    print(list(g).unwrap()).unwrap()
def looped() -> ():
    mut g = one()
    for i in range(3):
        if i == 1:
            g = two(Res(52))
        elif i == 2:
            g = choose(True)
    print(list(g).unwrap()).unwrap()
replaced()
looped()
"#,
        ),
        "Option[i64].Some(2)\ndrop 50\n[1]\ndrop 52\n[1]\n",
    );
}

#[test]
fn option_and_result_payloads_built_where_chosen_join_producers() {
    expect(
        &with_producers(
            r#"
def show(o: Option[Generator[i64]]) -> ():
    match o:
        case Some(g):
            print(list(g).unwrap()).unwrap()
        case Nothing:
            print("none").unwrap()
def factory(flag: bool) -> Option[Generator[i64]]:
    if flag:
        return Some(one())
    Some(two(Res(60)))
def fallible(n: i64) -> Result[Option[Generator[i64]], str]:
    if n < 0:
        return Err("negative")
    if n == 0:
        return Ok(Some(one()))
    Ok(Some(two(Res(n))))
def main_body(flag: bool) -> ():
    show(Some(one()) if flag else Some(two(Res(61))))
    x = if flag:
        Some(two(Res(62)))
    else:
        Some(one())
    show(x)
    y = match 1:
        case 0:
            Some(one())
        case _:
            Some(two(Res(63)))
    show(y)
    mut z = Some(one())
    z = Some(two(Res(64)))
    show(z)
    show(factory(True))
    show(factory(False))
    match fallible(65):
        case Ok(o):
            show(o)
        case Err(e):
            print(e).unwrap()
    match fallible(-1):
        case Ok(o):
            show(o)
        case Err(e):
            print(e).unwrap()
main_body(True)
"#,
        ),
        "[1]\ndrop 62\n[2, 62]\ndrop 63\n[2, 63]\ndrop 64\n[2, 64]\n[1]\ndrop 60\n[2, 60]\ndrop 65\n[2, 65]\nnegative\n",
    );
}

#[test]
fn nested_payload_choices_join_with_the_wider_outer_choice() {
    // An inner choice's recorded join is narrower than the outer one's: each
    // value is built with their join, not reported as already built.
    expect(
        &with_producers(
            r#"
def show(o: Option[Generator[i64]]) -> ():
    match o:
        case Some(g):
            print(list(g).unwrap()).unwrap()
        case Nothing:
            print("none").unwrap()
def factory(k: i64) -> Option[Generator[i64]]:
    if k == 0:
        return Some(one())
    Some(two(Res(70))) if k == 1 else Some(choose(True))
def main_body() -> ():
    for k in range(3):
        matched = match k % 2:
            case 0:
                match k:
                    case 0:
                        Some(one())
                    case _:
                        Some(choose(True))
            case _:
                match k:
                    case 1:
                        Some(two(Res(71)))
                    case _:
                        Nothing
        show(matched)
        block = if k == 0:
            Some(one())
        else:
            if k == 1:
                Some(two(Res(72)))
            else:
                Some(choose(True))
        show(block)
        show((Some(one()) if k == 0 else Some(two(Res(73)))) if k < 2 else Some(choose(True)))
        show(factory(k))
main_body()
"#,
        ),
        "[1]\n[1]\n[1]\n[1]\ndrop 71\n[2, 71]\ndrop 72\n[2, 72]\ndrop 73\n[2, 73]\ndrop 70\n[2, 70]\n[1]\n[1]\n[1]\n[1]\n",
    );
}

#[test]
fn annotated_locals_join_producers_on_reassignment() {
    // An annotation names only the item type; reassignment still joins.
    expect(
        &with_producers(
            r#"
def main_body() -> ():
    mut g: Generator[i64] = one()
    g = two(Res(80))
    print(list(g).unwrap()).unwrap()
    mut o: Option[Generator[i64]] = Some(one())
    o = Some(two(Res(81)))
    match o:
        case Some(h):
            print(list(h).unwrap()).unwrap()
        case Nothing:
            pass
main_body()
"#,
        ),
        "drop 80\n[2, 80]\ndrop 81\n[2, 81]\n",
    );
}

#[test]
fn many_join_sites_in_one_function_need_one_extra_pass() {
    // Each pass records every join it meets and lowering repeats in a loop,
    // so thousands of sites neither recurse nor repeat per site.
    let mut body = String::from("def many(c: bool) -> ():\n    mut g = one()\n");
    for _ in 0..3000 {
        body.push_str("    drop(Some(one()) if c else Some(two(Res(1))))\n");
        body.push_str("    g = choose(c)\n    g = two(Res(2))\n");
    }
    body.push_str("    print(list(g).unwrap()).unwrap()\nmany(True)\n");
    let output = support::run_stdout(&with_producers(&body));
    assert!(output.ends_with("drop 2\n[2, 2]\n"), "{output}");
}

#[test]
fn an_already_built_payload_keeps_its_producer() {
    // Phase guard (issue #900): delete when built payloads join.
    let error = check_error(&with_producers(
        r#"
def maybe() -> Option[Generator[i64]]:
    Some(one())
o = maybe() if True else Some(two(Res(1)))
drop(o)
"#,
    ));
    assert!(
        error.contains("already built with one generator producer"),
        "{error}"
    );
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
