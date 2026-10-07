//! Native propagation, ownership cleanup, and allocation-free standard sums.
mod support;
use rstest::rstest;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{source}\n{:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected,
        "{source}"
    );
}

#[test]
fn result_and_option_success_failure_and_changed_success_type() {
    native(r#"
def number(ok: bool) -> Result[i64, str]:
    Ok(21) if ok else Err("failed")
def doubled(ok: bool) -> Result[str, str]:
    n = number(ok)? * 2
    print(n).unwrap()
    Ok("done")
def optional(ok: bool) -> Option[i64]:
    Some(21) if ok else Nothing
def changed(ok: bool) -> Option[str]:
    print(optional(ok)?).unwrap()
    Some("done")
print(doubled(True)).unwrap()
print(doubled(False)).unwrap()
print(changed(True)).unwrap()
print(changed(False)).unwrap()
"#, "42\nResult[str, str].Ok(\"done\")\nResult[str, str].Err(\"failed\")\n21\nOption[str].Some(\"done\")\nOption[str].Nothing\n");
}

#[test]
fn nested_postfix_unit_returns_and_comprehensions() {
    native(r#"
class Point:
    x: i64
def unwrap() -> Option[i64]:
    value: Option[Option[list[i64]]] = Some(Some([4, 5].unwrap()))
    Some(value??[1])
def done(ok: bool) -> Result[(), str]:
    Ok(()) if ok else Err("stop")
def unit(ok: bool) -> Result[i64, str]:
    done(ok)?
    Ok(9)
def item(n: i64) -> Result[i64, str]:
    Ok(n * n) if n < 3 else Err("large")
def collect(n: i64) -> Result[list[i64], str]:
    Ok([item(i)? for i in range(n)].unwrap())
def field() -> Option[i64]:
    Some(Some(Point(7).unwrap())?.x)
print(unwrap()).unwrap()
print(unit(True)).unwrap()
print(unit(False)).unwrap()
print(collect(3)).unwrap()
print(collect(5)).unwrap()
print(field()).unwrap()
"#, "Option[i64].Some(5)\nResult[i64, str].Ok(9)\nResult[i64, str].Err(\"stop\")\nResult[list[i64], str].Ok([0, 1, 4])\nResult[list[i64], str].Err(\"large\")\nOption[i64].Some(7)\n");
}

#[test]
fn failure_drops_earlier_arguments_and_locals_once_and_skips_later_arguments() {
    native(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
def fail() -> Result[i64, Guard]:
    Err(Guard("error").unwrap())
def later() -> i64:
    print("must not run").unwrap()
    3
def consume(g: Guard, a: i64, b: i64) -> i64:
    a + b
def work() -> Result[i64, Guard]:
    first = Guard("first").unwrap()
    second = Guard("second").unwrap()
    Ok(consume(Guard("argument").unwrap(), fail()?, later()))
match work():
    case Ok(_):
        pass
    case Err(error):
        print("caught").unwrap()
        drop(error)
"#,
        "argument\nsecond\nfirst\ncaught\nerror\n",
    );
}

#[test]
fn inline_sums_survive_references_storage_copies_and_generator_suspension() {
    native(r#"
class Holder:
    value: Option[list[i64]]
def replace(value: &mut Option[list[i64]]) -> ():
    *value = Some([9].unwrap())
def values(x: Result[Option[f64], str]) -> Generator[Result[Option[f64], str]]:
    yield x
    yield Err("end")
mut h = Holder(Some([1, 2].unwrap())).unwrap()
replace(&mut h.value)
print(h).unwrap()
mut items: list[Result[Option[u64], str]] = [Ok(Some(18446744073709551615u64)), Err("bad")].unwrap()
print(copy(items).unwrap()).unwrap()
print(items[0]).unwrap()
for value in values(Ok(Some(-0.0))).unwrap():
    print(value).unwrap()
mut it = values(Ok(Nothing)).unwrap()
print(next(it)).unwrap()
drop(it)
"#, "Holder(value=Option[list[i64]].Some([9]))\n[Result[Option[u64], str].Ok(Option[u64].Some(18446744073709551615)), Result[Option[u64], str].Err(\"bad\")]\nResult[Option[u64], str].Ok(Option[u64].Some(18446744073709551615))\nResult[Option[f64], str].Ok(Option[f64].Some(-0.0))\nResult[Option[f64], str].Err(\"end\")\nOption[Result[Option[f64], str]].Some(Result[Option[f64], str].Ok(Option[f64].Nothing))\n");
}

#[test]
fn propagation_obeys_short_circuiting_loop_conditions_and_unit_errors() {
    native(r#"
def flag() -> Result[bool, ()]:
    print("flag").unwrap()
    Err(())
def short() -> Result[bool, ()]:
    Ok(False and flag()?)
def looping() -> Result[(), ()]:
    while flag()?:
        print("unreachable").unwrap()
    Ok(())
def filter_items() -> Result[list[i64], ()]:
    Ok([n for n in range(3) if flag()?].unwrap())
print(short()).unwrap()
print(looping()).unwrap()
print(filter_items()).unwrap()
"#, "Result[bool, ()].Ok(False)\nflag\nResult[(), ()].Err(())\nflag\nResult[list[i64], ()].Err(())\n");
}

#[test]
fn all_sixty_four_tag_bits_survive_calls_and_unwrapping() {
    let ty = "T64";
    let aliases = (1..=64).fold("type T0 = u64\n".to_owned(), |mut text, n| {
        text.push_str(&format!("type T{n} = Option[T{}]\n", n - 1));
        text
    });
    let present = format!(
        "{}18446744073709551615u64{}",
        "Some(".repeat(64),
        ")".repeat(64)
    );
    let absent = format!("{}Nothing{}", "Some(".repeat(63), ")".repeat(63));
    native(
        &format!(
            r#"
{aliases}
def unroll(value: {ty}) -> Option[u64]:
    Some(value{})
def main() -> ():
    a: {ty} = {present}
    b: {ty} = {absent}
    print(unroll(a)).unwrap()
    print(unroll(b)).unwrap()
"#,
            "?".repeat(64)
        ),
        "Option[u64].Some(18446744073709551615)\nOption[u64].Nothing\n",
    );
}

#[test]
fn wrapping_existing_owned_payload_and_propagating_it_does_not_allocate() {
    native(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
def wrap(guard: Guard) -> Result[i64, Guard]:
    Err(guard)
def propagate(guard: Guard) -> Result[(), Guard]:
    wrap(guard)?
    Ok(())
def main() -> ():
    guard = Guard("dropped").unwrap()
    print("__test_begin_no_allocations__").unwrap()
    result = propagate(guard)
    match result:
        case Ok(_):
            pass
        case Err(guard):
            drop(guard)
    print("__test_end_no_allocations__").unwrap()
"#,
        "__test_begin_no_allocations__\ndropped\n__test_end_no_allocations__\n",
    );
}

#[test]
fn constructing_copying_comparing_and_propagating_sums_never_allocate() {
    // runtime-checks counts allocation attempts, not merely live allocations.
    native(r#"
def number(ok: bool) -> Result[u64, i64]:
    Ok(18446744073709551615u64) if ok else Err(-42)
def nested(ok: bool) -> Result[Option[u64], i64]:
    Ok(Some(number(ok)?))
def absent() -> Option[u64]:
    missing: Option[i64] = Nothing
    missing?
    Some(1u64)
def recurse(n: i64, value: Result[Option[u64], i64]) -> Result[Option[u64], i64]:
    if n == 0:
        return value
    recurse(n - 1, value)
print("__test_begin_no_allocations__").unwrap()
good = recurse(10000, nested(True))
bad = nested(False)
missing = absent()
same = good == copy(good).unwrap()
unit: Result[(), ()] = Ok(())
match good:
    case Ok(value):
        match value:
            case Some(n):
                copy(n).unwrap()
            case Nothing:
                pass
    case Err(_):
        pass
print("__test_end_no_allocations__").unwrap()
print(good).unwrap()
print(bad).unwrap()
print(missing).unwrap()
print(same).unwrap()
"#, "__test_begin_no_allocations__\n__test_end_no_allocations__\nResult[Option[u64], i64].Ok(Option[u64].Some(18446744073709551615))\nResult[Option[u64], i64].Err(-42)\nOption[u64].Nothing\nTrue\n");
}

#[test]
fn generator_next_and_stored_sum_reads_add_no_allocations() {
    native(r#"
def values() -> Generator[i64]:
    yield 42
def main() -> ():
    mut it = values().unwrap()
    stored = [Some(7)].unwrap()
    print("__test_begin_no_allocations__").unwrap()
    first = next(it)
    end = next(it)
    again = next(it)
    loaded = stored[0]
    same = first == Some(42)
    print("__test_end_no_allocations__").unwrap()
    print(first).unwrap()
    print(end).unwrap()
    print(again).unwrap()
    print(loaded).unwrap()
    print(same).unwrap()
"#, "__test_begin_no_allocations__\n__test_end_no_allocations__\nOption[i64].Some(42)\nOption[i64].Nothing\nOption[i64].Nothing\nOption[i64].Some(7)\nTrue\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_instrumentation_detects_a_temporary_allocation() {
    let output = support::run("print(\"__test_begin_no_allocations__\").unwrap()\nlen((\"a\" + \"b\").unwrap())\nprint(\"__test_end_no_allocations__\").unwrap()");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected allocation"));
}

#[rstest]
#[case(
    "def f() -> i64:\n    Some(1)?\n    0",
    "requires a function returning Result or Option"
)]
#[case(
    "def f() -> Option[i64]:\n    Some(1?)",
    "requires a Result or Option operand"
)]
#[case(
    "def f() -> Result[i64, str]:\n    Ok(Some(1)?)",
    "same Result or Option family"
)]
#[case(
    "def f() -> Result[i64, str]:\n    x: Result[i64, i64] = Err(1)\n    Ok(x?)",
    "identical Result error types"
)]
#[case(
    "enum E:\n    Value(i64)\ndef f() -> Option[i64]:\n    Some(E.Value(1).unwrap()?)",
    "same Result or Option family"
)]
#[case(
    "def f() -> Generator[i64]:\n    yield Some(1)?",
    "not supported in generators"
)]
#[case("def f() -> Result[i64, str]:\n    x: Result[list[i64], str] = Ok([1].unwrap())\n    values = x?\n    print(x).unwrap()\n    Ok(1)", "moved")]
fn rejects_invalid_propagation(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
