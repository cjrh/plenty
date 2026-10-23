//! A `match` block as the value of a binding, an assignment, or a `return`
//! (issue #118).

mod support;
use std::process::Command;

fn run(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("program");
    support::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    let output = Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

fn reject(source: &str, message: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(message), "expected {message}, got {error}");
}

const SHAPE: &str = r#"
enum Shape:
    Dot
    Circle(i64)
    Named(str, list[i64])
"#;

const NOISY: &str = r#"
class Noisy:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()

enum Held:
    Empty
    One(Noisy)
    Two(Noisy, Noisy)
"#;

#[test]
fn issue_118_reproduction_binds_the_selected_case() {
    run(
        r#"
def f() -> i64:
    7

def main() -> Result[(), IoError]:
    x = match f():
        case 7:
            1
        case _:
            0
    print(x)?
    Ok(())
"#,
        "1\n",
    );
}

#[test]
fn binding_forms_accept_a_match_block() {
    run(
        &format!(
            r#"{SHAPE}
def size(shape: Shape) -> i64:
    total: i64 = match shape:
        case Shape.Dot:
            0
        case Shape.Circle(radius):
            radius * 2
        case Shape.Named(name, values):
            mut sum = len(name)
            for value in values:
                sum += value
            sum
    total + 1

def main() -> Result[(), AllocError | IoError]:
    print(size(Shape.Dot))?
    print(size(Shape.Circle(4)))?
    print(size(Shape.Named("abc", [1, 2]?)))?
    mut label = "unset"
    label = match size(Shape.Dot):
        case 1:
            "one"
        case _:
            "other"
    print(label)?
    mut count = match label:
        case "one":
            10
        case _:
            20
    count += 1
    print(count)?
    Ok(())
"#
        ),
        "1\n9\n7\none\n11\n",
    );
}

#[test]
fn annotation_supplies_the_type_each_case_needs() {
    run(
        r#"
def main() -> Result[(), AllocError | IoError]:
    for n in [1, 2]?:
        found: Option[u8] = match n:
            case 1:
                Some(200)
            case _:
                Nothing
        print(found)?
    Ok(())
"#,
        "Option[u8].Some(200)\nOption[u8].Nothing\n",
    );
}

#[test]
fn return_accepts_a_match_block() {
    run(
        r#"
def name(n: i64) -> str:
    if n < 0:
        return match n:
            case -1:
                "minus one"
            case _:
                "negative"
    return match n:
        case 0:
            "zero"
        case _:
            "many"

def main() -> Result[(), IoError]:
    print(name(-1))?
    print(name(-5))?
    print(name(0))?
    print(name(3))?
    Ok(())
"#,
        "minus one\nnegative\nzero\nmany\n",
    );
}

#[test]
fn return_match_makes_each_case_a_return() {
    // Five million frames overflow the stack unless the call in the last
    // case is a tail call, as a call in any other `return` is.
    run(
        r#"
def count(n: i64, total: i64) -> i64:
    return match n:
        case 0:
            total
        case _:
            count(n - 1, total + 1)

def report(n: i64) -> ():
    return match n:
        case 0:
            print("zero").unwrap()
        case 1:
            if n == 1:
                print("one").unwrap()
        case _:
            unused = n

def main() -> Result[(), IoError]:
    print(count(5000000, 0))?
    report(0)
    report(1)
    report(2)
    Ok(())
"#,
        "5000000\nzero\none\n",
    );
    run(
        &format!(
            r#"{NOISY}
def make(flag: bool) -> Noisy:
    outer = Noisy("outer")
    return match flag:
        case True:
            inner = Noisy("inner")
            Noisy("made")
        case False:
            outer

def main() -> ():
    a = make(True)
    print("--").unwrap()
    b = make(False)
    print("end").unwrap()
"#
        ),
        "inner\nouter\n--\nend\nouter\nmade\n",
    );
    reject(
        r#"
def f(n: i64) -> i64:
    return match n:
        case 0:
            pass
        case _:
            1
"#,
        "expected i64, got ()",
    );
    reject(
        r#"
def f(n: i64) -> i64:
    return match n:
        case _:
            1
    2
"#,
        "unreachable statement after a control-flow exit",
    );
    reject(
        r#"
def walk(n: i64) -> Generator[i64]:
    yield n
    return match n:
        case _:
            1
"#,
        "a generator may only use bare return",
    );
}

#[test]
fn return_match_follows_the_returned_reference_rules() {
    run(
        r#"
class Pair:
    left: i64
    right: i64

enum Either:
    First(Pair)
    Second(Pair)

def side(pair: &Pair, left: bool) -> &i64:
    return match left:
        case True:
            &pair.left
        case False:
            &pair.right

def inner(value: &Either) -> &i64:
    return match value:
        case Either.First(pair):
            &pair.left
        case Either.Second(pair):
            side(pair, False)

def main() -> Result[(), IoError]:
    pair = Pair(1, 2)
    print(*side(&pair, True))?
    print(*side(&pair, False))?
    print(*inner(&Either.First(Pair(3, 4))))?
    print(*inner(&Either.Second(Pair(5, 6))))?
    Ok(())
"#,
        "1\n2\n3\n6\n",
    );
    reject(
        r#"
def side(left: bool, other: &i64) -> &i64:
    local = 3
    return match left:
        case True:
            &local
        case False:
            other
"#,
        "returned reference must originate from the first reference parameter",
    );
}

#[test]
fn match_value_works_inside_a_resource_scope() {
    run(
        &format!(
            r#"{NOISY}
class Scope:
    name: str
    def __enter__(self: &mut Scope) -> i64:
        print("enter").unwrap()
        3
    def __exit__(self: &mut Scope) -> ():
        print("exit").unwrap()

def scoped(flag: bool) -> i64:
    with Scope("scope") as base:
        value = match flag:
            case True:
                local = Noisy("local")
                moved = Noisy("moved")
                kept = match base:
                    case 3:
                        moved
                    case _:
                        return -1
                print("inner bound").unwrap()
                base + len(kept.name)
            case False:
                return 0
        print(value).unwrap()
    1

def main() -> ():
    print(scoped(True)).unwrap()
    print("--").unwrap()
    print(scoped(False)).unwrap()
"#
        ),
        "enter\ninner bound\nmoved\nlocal\n8\nexit\n1\n--\nenter\nexit\n0\n",
    );
}

#[test]
fn element_field_and_dereference_assignments_accept_a_match_block() {
    run(
        r#"
class Counter:
    count: i64
    label: str
    def __init__(self, seed: i64) -> ():
        self.count = match seed:
            case 0:
                100
            case _:
                seed
        self.label = "new"

def bump(target: &mut i64, by: bool) -> ():
    *target = match by:
        case True:
            *target + 1
        case False:
            *target

def main() -> Result[(), AllocError | IoError]:
    mut items = [1, 2]?
    items[0] = match items[1]:
        case 2:
            9
        case _:
            8
    print(items)?
    mut counter = Counter(0)
    counter.label = match counter.count:
        case 100:
            "default"
        case _:
            "seeded"
    print(counter.count)?
    print(counter.label)?
    mut n = 4
    bump(&mut n, True)
    bump(&mut n, False)
    print(n)?
    Ok(())
"#,
        "[9, 2]\n100\ndefault\n5\n",
    );
}

#[test]
fn diverging_cases_contribute_no_value() {
    run(
        &format!(
            r#"{SHAPE}
def parse(text: str) -> Result[i64, str]:
    match text:
        case "one":
            Ok(1)
        case "two":
            Ok(2)
        case _:
            Err("unknown")

def doubled(shape: Shape, text: str) -> Result[i64, str]:
    base = match shape:
        case Shape.Dot:
            return Ok(-1)
        case Shape.Circle(radius):
            radius
        case Shape.Named(_, _):
            parse(text)?
    Ok(base * 2)

def show(result: Result[i64, str]) -> Result[(), IoError]:
    match result:
        case Ok(value):
            print(value)?
        case Err(message):
            print(message)?
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    show(doubled(Shape.Dot, "one"))?
    show(doubled(Shape.Circle(3), "one"))?
    show(doubled(Shape.Named("n", []?), "two"))?
    show(doubled(Shape.Named("n", []?), "three"))?
    mut total = 0
    for n in [1, 2, 3, 4, 5]?:
        step = match n:
            case 2:
                continue
            case 4:
                break
            case _:
                n * 10
        total += step
    print(total)?
    Ok(())
"#
        ),
        "-1\n6\n4\nunknown\n40\n",
    );
}

#[test]
fn owned_match_moves_payloads_into_the_value() {
    run(
        &format!(
            r#"{SHAPE}
def label(shape: Shape) -> str:
    text = match shape:
        case Shape.Named(name, _):
            name
        case _ as other:
            match other:
                case Shape.Dot:
                    "dot"
                case _:
                    "circle"
    text

def numbers(shape: Shape) -> Result[list[i64], AllocError]:
    values: list[i64] = match shape:
        case Shape.Named(_, values):
            values
        case Shape.Circle(radius):
            [radius]?
        case Shape.Dot:
            []?
    Ok(values)

def main() -> Result[(), AllocError | IoError]:
    print(label(Shape.Named("abcdefghij", [1]?)))?
    print(label(Shape.Dot))?
    print(label(Shape.Circle(1)))?
    print(numbers(Shape.Named("n", [4, 5]?))?)?
    print(numbers(Shape.Circle(7))?)?
    print(numbers(Shape.Dot)?)?
    Ok(())
"#
        ),
        "abcdefghij\ndot\ncircle\n[4, 5]\n[7]\n[]\n",
    );
}

#[test]
fn consumed_scrutinee_cannot_be_used_afterwards() {
    reject(
        &format!(
            r#"{SHAPE}
def main() -> Result[(), AllocError | IoError]:
    shape = Shape.Named("n", [1]?)
    text = match shape:
        case Shape.Named(name, _):
            name
        case _:
            "other"
    print(text)?
    match shape:
        case _:
            pass
    Ok(())
"#
        ),
        "moved",
    );
}

#[test]
fn borrowed_match_leaves_the_scrutinee_usable() {
    run(
        &format!(
            r#"{SHAPE}
def main() -> Result[(), IoError]:
    shape = Shape.Circle(6)
    radius = match &shape:
        case Shape.Circle(radius):
            *radius
        case _:
            0
    print(radius)?
    again = match shape:
        case Shape.Circle(radius):
            radius + 1
        case _:
            0
    print(again)?
    Ok(())
"#
        ),
        "6\n7\n",
    );
}

#[test]
fn case_locals_and_unbound_payloads_are_destroyed_before_the_binding() {
    run(
        &format!(
            r#"{NOISY}
def keep(held: Held) -> Noisy:
    kept = match held:
        case Held.Empty:
            Noisy("made")
        case Held.One(only):
            scratch = Noisy("scratch")
            only
        case Held.Two(first, _):
            first
    print("bound").unwrap()
    kept

def main() -> ():
    a = keep(Held.Empty)
    print("--").unwrap()
    b = keep(Held.One(Noisy("one")))
    print("--").unwrap()
    c = keep(Held.Two(Noisy("first"), Noisy("second")))
    print("end").unwrap()
"#
        ),
        "bound\n--\nscratch\nbound\n--\nsecond\nbound\nend\nfirst\none\nmade\n",
    );
}

#[test]
fn exits_from_a_case_destroy_what_the_function_owns() {
    run(
        &format!(
            r#"{NOISY}
def fail(flag: bool) -> Result[i64, str]:
    if flag:
        return Err("failed")
    Ok(1)

def pick(held: Held, flag: bool) -> Result[i64, str]:
    outer = Noisy("outer")
    value = match held:
        case Held.Empty:
            return Ok(0)
        case Held.One(only):
            inner = Noisy("inner")
            fail(flag)?
        case Held.Two(_, _):
            2
    print("continued").unwrap()
    Ok(value)

def show(result: Result[i64, str]) -> ():
    match result:
        case Ok(value):
            print(value).unwrap()
        case Err(message):
            print(message).unwrap()

def main() -> ():
    show(pick(Held.Empty, False))
    print("--").unwrap()
    show(pick(Held.One(Noisy("one")), True))
    print("--").unwrap()
    show(pick(Held.One(Noisy("one")), False))
    print("--").unwrap()
    show(pick(Held.Two(Noisy("first"), Noisy("second")), False))
"#
        ),
        "outer\n0\n--\ninner\none\nouter\nfailed\n--\ninner\none\ncontinued\nouter\n1\n--\nsecond\nfirst\ncontinued\nouter\n2\n",
    );
}

#[test]
fn replaced_owner_is_destroyed_after_the_match_runs() {
    run(
        &format!(
            r#"{NOISY}
def main() -> ():
    mut current = Noisy("old")
    for n in [1, 2].unwrap():
        current = match n:
            case 1:
                print("choosing").unwrap()
                Noisy("first")
            case _:
                Noisy("second")
    print("end").unwrap()
"#
        ),
        "choosing\nold\nfirst\nend\nsecond\n",
    );
}

#[test]
fn nested_blocks_supply_a_case_value() {
    run(
        r#"
def grade(score: i64, bonus: bool) -> str:
    text = match bonus:
        case True:
            if score > 90:
                "top"
            elif score > 50:
                "pass"
            else:
                "fail"
        case False:
            inner = match score:
                case 100:
                    "perfect"
                case _:
                    "plain"
            inner
    text

def main() -> Result[(), IoError]:
    print(grade(95, True))?
    print(grade(60, True))?
    print(grade(10, True))?
    print(grade(100, False))?
    print(grade(1, False))?
    Ok(())
"#,
        "top\npass\nfail\nperfect\nplain\n",
    );
}

#[test]
fn generic_functions_closures_and_generators_accept_a_match_block() {
    run(
        r#"
def choose[T](flag: bool, yes: T, no: T) -> T:
    picked = match flag:
        case True:
            yes
        case False:
            no
    picked

def steps(limit: i64) -> Generator[i64]:
    for n in range(limit):
        scaled = match n:
            case 1:
                yield 100
                -1
            case _:
                n
        yield scaled

def main() -> Result[(), IoError]:
    print(choose(True, 1, 2))?
    print(choose(False, "a", "b"))?
    mut total = 0
    mut add = def[&mut total](n: i64) -> ():
        total = match n:
            case 0:
                total
            case _:
                total + n
    add(0)
    add(5)
    print(total)?
    for value in steps(3):
        print(value)?
    Ok(())
"#,
        "1\nb\n5\n0\n100\n-1\n2\n",
    );
}

#[test]
fn cases_must_agree_on_the_value_type() {
    reject(
        r#"
def main() -> Result[(), AllocError]:
    n = 1
    x = match n:
        case 1:
            1
        case _:
            "other"
    Ok(())
"#,
        "expected",
    );
    reject(
        r#"
def main() -> Result[(), AllocError]:
    n = 1
    x: str = match n:
        case 1:
            1
        case _:
            2
    Ok(())
"#,
        "expected",
    );
    reject(
        r#"
def f(n: i64) -> str:
    return match n:
        case 1:
            1
        case _:
            2
"#,
        "expected",
    );
}

#[test]
fn match_value_keeps_exhaustiveness_and_result_rules() {
    reject(
        r#"
def main() -> Result[(), AllocError]:
    flag = True
    x = match flag:
        case True:
            1
    Ok(())
"#,
        "missing False",
    );
    reject(
        r#"
def fallible() -> Result[i64, str]:
    Ok(1)

def main() -> Result[(), AllocError]:
    flag = True
    x = match flag:
        case True:
            fallible()
            1
        case False:
            2
    Ok(())
"#,
        "cannot implicitly discard a Result",
    );
}

#[test]
fn match_value_needs_a_continuing_case_with_a_value() {
    reject(
        r#"
def f(flag: bool) -> i64:
    x = match flag:
        case True:
            return 1
        case False:
            return 2
    x
"#,
        "every case of this `match` exits, so it has no value",
    );
    reject(
        r#"
def main() -> Result[(), AllocError]:
    flag = True
    x = match flag:
        case True:
            pass
        case False:
            pass
    Ok(())
"#,
        "expected a value, got ()",
    );
}

#[test]
fn match_value_cannot_produce_a_reference() {
    reject(
        &format!(
            r#"{SHAPE}
def main() -> Result[(), IoError]:
    shape = Shape.Circle(6)
    radius = match &shape:
        case Shape.Circle(radius):
            radius
        case _:
            return Ok(())
    print(*radius)?
    Ok(())
"#
        ),
        "a `match` block used as a value cannot produce a reference",
    );
}

#[test]
fn match_block_is_rejected_inside_an_expression() {
    const POSITION: &str = "a `match` block is a value only as the whole right side";
    reject(
        r#"
def main() -> Result[(), IoError]:
    n = 1
    print(match n:
        case _:
            1
    )?
    Ok(())
"#,
        POSITION,
    );
    reject(
        r#"
def main() -> Result[(), AllocError]:
    mut n = 1
    n += match n:
        case _:
            1
    Ok(())
"#,
        POSITION,
    );
    reject(
        r#"
def main() -> Result[(), AllocError]:
    n = 1
    a, b = match n:
        case _:
            (1, 2)
    Ok(())
"#,
        POSITION,
    );
    reject(
        r#"
def main() -> Result[(), AllocError]:
    n = 1
    x = 1 + match n:
        case _:
            1
    Ok(())
"#,
        POSITION,
    );
}
