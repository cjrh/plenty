//! Generators whose frames borrow through reference and view parameters
//! (issue #899): a call result carries the argument loans, so the caller
//! cannot change the borrowed data while the generator may still run.

mod support;

use support::{assert_stdout, check_error};

const EVENS: &str = r#"
def evens(xs: &list[i64]) -> Generator[i64]:
    for x in xs:
        if *x % 2 == 0:
            yield *x
"#;

fn with_evens(body: &str) -> String {
    format!("{EVENS}\n{body}")
}

#[test]
fn a_yielding_generator_reads_through_a_reference_parameter() {
    assert_stdout(
        &with_evens(
            r#"
def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3, 4]?
    mut g = evens(&xs)
    print(next(g))?
    for v in g:
        print(v)?
    xs.append(6)?
    print(evens(&xs).count())?
    Ok(())
"#,
        ),
        "Option[i64].Some(2)\n4\n3\n",
    );
}

#[test]
fn changing_the_borrowed_data_while_the_generator_is_live_is_rejected() {
    for mutation in ["xs.append(5)?", "xs.clear()", "drop(xs)"] {
        let error = check_error(&with_evens(&format!(
            r#"
def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3, 4]?
    mut g = evens(&xs)
    print(next(g))?
    {mutation}
    print(next(g))?
    Ok(())
"#
        )));
        assert!(error.contains("borrowed"), "{mutation}: {error}");
    }
}

#[test]
fn the_loan_ends_with_the_generator_last_use() {
    assert_stdout(
        &with_evens(
            r#"
def main() -> Result[(), AllocError | IoError]:
    mut xs = [2, 4, 6]?
    for v in evens(&xs):
        if v == 4:
            break
        print(v)?
    xs.clear()
    print(len(xs))?
    Ok(())
"#,
        ),
        "2\n0\n",
    );
}

#[test]
fn every_reference_argument_stays_borrowed() {
    let source = r#"
def above(xs: &list[i64], limit: &i64) -> Generator[i64]:
    for x in xs:
        if *x > *limit:
            yield *x

def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3]?
    mut n = 1
    g = above(&xs, &n)
    STEP
    for v in g:
        print(v)?
    Ok(())
"#;
    assert_stdout(&source.replace("STEP", "pass"), "2\n3\n");
    let error = check_error(&source.replace("STEP", "n = 5"));
    assert!(error.contains("`n`"), "{error}");
}

#[test]
fn a_mutable_reference_parameter_lends_exclusively() {
    let source = r#"
def drain(xs: &mut list[i64]) -> Generator[i64]:
    while len(*xs) > 0:
        match xs.pop():
            case Some(v):
                yield v
            case Nothing:
                pass

def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3]?
    g = drain(&mut xs)
    STEP
    for v in g:
        print(v)?
    print(len(xs))?
    Ok(())
"#;
    assert_stdout(&source.replace("STEP", "pass"), "3\n2\n1\n0\n");
    let error = check_error(&source.replace("STEP", "print(len(xs))?"));
    assert!(error.contains("`xs`"), "{error}");
}

#[test]
fn a_function_returns_a_generator_borrowing_its_reference_parameter() {
    let source = r#"
def evens(xs: &list[i64]) -> Generator[i64]:
    return iter(xs).filter(def(x: &i64) -> bool: *x % 2 == 0).map(def(x: &i64) -> i64: *x).generator()

def firsts(xs: &[i64]) -> Generator[i64]:
    p = iter(xs).take(2).map(def(x: &i64) -> i64: *x)
    p.generator()

def wrapped(xs: &list[i64]) -> Generator[i64]:
    evens(xs)

def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3, 4, 6]?
    g = wrapped(&xs)
    STEP
    for v in g:
        print(v)?
    xs.append(8)?
    for v in firsts(&xs[3:]):
        print(v)?
    Ok(())
"#;
    assert_stdout(&source.replace("STEP", "pass"), "2\n4\n6\n4\n6\n");
    let error = check_error(&source.replace("STEP", "xs.append(7)?"));
    assert!(error.contains("`xs`"), "{error}");
}

#[test]
fn a_pipeline_over_a_borrowing_generator_converts_back_to_a_generator() {
    assert_stdout(
        &with_evens(
            r#"
def negate(v: i64) -> i64:
    unchecked:
        0 - v

def negated(xs: &list[i64]) -> Generator[i64]:
    return evens(xs).map(negate).generator()

def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3, 4]?
    for v in negated(&xs):
        print(v)?
    Ok(())
"#,
        ),
        "-2\n-4\n",
    );
}

#[test]
fn a_returned_generator_cannot_borrow_the_function_own_values() {
    for body in [
        "    xs = [1, 2].unwrap()\n    return evens(&xs)",
        "    return evens(&own)",
        "    xs = [1, 2].unwrap()\n    return iter(&xs).map(def(x: &i64) -> i64: *x).generator()",
        "    xs = [1, 2].unwrap()\n    r = &xs\n    return iter(r).map(def(x: &i64) -> i64: *x).generator()",
    ] {
        let error = check_error(&with_evens(&format!(
            "def leak(own: list[i64]) -> Generator[i64]:\n{body}\n"
        )));
        assert!(
            error.contains("may borrow only through the function's reference parameters")
                || error.contains("borrowed state cannot be suspended"),
            "{body}: {error}"
        );
    }
}

#[test]
fn a_borrowing_generator_cannot_be_stored_where_its_loan_would_be_lost() {
    for (statement, message) in [
        (
            "o = Some(evens(&xs))",
            "references cannot be stored in enum payloads",
        ),
        (
            "t = (evens(&xs), 1)",
            "generators cannot be stored in tuples",
        ),
        (
            "g = evens(&xs)\n    f = def [g]() -> i64:\n        0",
            "this type cannot be captured yet",
        ),
    ] {
        let error = check_error(&with_evens(&format!(
            "def main() -> Result[(), AllocError]:\n    xs = [1, 2]?\n    {statement}\n    Ok(())\n"
        )));
        assert!(error.contains(message), "{statement}: {error}");
    }
    let error = check_error(&with_evens(
        "def wrap(xs: &list[i64]) -> Option[Generator[i64]]:\n    Some(evens(xs))\n",
    ));
    assert!(
        error.contains("references cannot be stored in enum payloads"),
        "{error}"
    );
}

#[test]
fn a_borrowing_generator_passed_by_value_is_consumed_by_the_callee() {
    assert_stdout(
        &with_evens(
            r#"
def total(g: Generator[i64]) -> i64:
    mut sum = 0
    for v in g:
        unchecked:
            sum += v
    sum

def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3, 4]?
    print(total(evens(&xs)))?
    xs.clear()
    Ok(())
"#,
        ),
        "6\n",
    );
}

// B12: a generator parameter that is itself a borrowing generator, a carried
// reference, or a lent pipeline has no loan the new frame could keep.
// Delete when generator frames carry such loans.
#[test]
fn a_generator_cannot_take_a_borrowing_generator_parameter_yet() {
    let error = check_error(&with_evens(
        "def doubled(g: Generator[i64]) -> Generator[i64]:\n    for v in g:\n        yield v\n\ndef main() -> Result[(), AllocError]:\n    xs = [1]?\n    for v in doubled(evens(&xs)):\n        pass\n    Ok(())\n",
    ));
    assert!(
        error.contains("generators cannot capture references"),
        "{error}"
    );
}

// Review of local PR #1134: a conditional choosing between borrowing
// generators keeps every arm's loans, as it does for references.
#[test]
fn a_conditional_frame_keeps_both_arms_borrowed() {
    for choice in [
        "mut g = evens(&xs) if len(xs) > 5 else evens(&ys)",
        "mut g = if len(xs) > 5:\n        evens(&xs)\n    else:\n        evens(&ys)",
    ] {
        let error = check_error(&with_evens(&format!(
            r#"
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2]?
    ys = [2, 4]?
    {choice}
    print(next(g))?
    drop(ys)
    print(next(g))?
    Ok(())
"#
        )));
        assert!(
            error.contains("cannot move `ys` while it is borrowed"),
            "{choice}: {error}"
        );
    }
    assert_stdout(
        &with_evens(
            r#"
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2]?
    ys = [2, 4]?
    mut g = evens(&xs) if len(xs) > 5 else evens(&ys)
    for v in g:
        print(v)?
    drop(ys)
    Ok(())
"#,
        ),
        "2\n4\n",
    );
}

#[test]
fn a_returned_conditional_frame_checks_every_arm() {
    const TWO: &str = r#"
def two(a: &list[i64], b: &list[i64]) -> Generator[i64]:
    for x in a:
        yield *x
    for x in b:
        yield *x
"#;
    let error = check_error(&format!(
        r#"{TWO}
def bad(xs: &list[i64]) -> Generator[i64]:
    ys = [1].unwrap()
    two(xs, xs) if len(xs) > 9 else two(&ys, &ys)

def main() -> ():
    pass
"#
    ));
    assert!(error.contains("may borrow only through"), "{error}");
    assert_stdout(
        &format!(
            r#"{TWO}
def either(xs: &list[i64], ys: &list[i64]) -> Generator[i64]:
    two(xs, xs) if len(xs) > 9 else two(ys, xs)

def main() -> Result[(), AllocError | IoError]:
    xs = [1]?
    ys = [2]?
    print(either(&xs, &ys).count())?
    Ok(())
"#
        ),
        "2\n",
    );
}

// Review of local PR #1134: a frame passed by value keeps its loans until
// the call returns, so a later argument cannot move or exclusively borrow
// the data it reads.
#[test]
fn a_frame_argument_keeps_its_loans_for_the_call() {
    let consumers = r#"
def consume(g: Generator[i64], xs: list[i64]) -> Result[(), AllocError | IoError]:
    drop(xs)
    for v in g:
        print(v)?
    Ok(())

def grow(g: Generator[i64], xs: &mut list[i64]) -> Result[(), AllocError | IoError]:
    for v in g:
        xs.append(v)?
    Ok(())
"#;
    for (call, expected) in [
        (
            "consume(evens(&xs), xs)?",
            "cannot move `xs` while it is borrowed",
        ),
        (
            "g = evens(&xs)\n    consume(g, xs)?",
            "cannot move `xs` while it is borrowed",
        ),
        (
            "grow(evens(&xs), &mut xs)?",
            "cannot modify or exclusively borrow `xs` while it is borrowed",
        ),
    ] {
        let error = check_error(&with_evens(&format!(
            r#"{consumers}
def main() -> Result[(), AllocError | IoError]:
    mut xs = [2, 4]?
    {call}
    Ok(())
"#
        )));
        assert!(error.contains(expected), "{call}: {error}");
    }
}

#[test]
fn a_conflict_on_a_frame_binding_does_not_suggest_dropping_mut() {
    let error = check_error(&with_evens(
        r#"
def grow(xs: &mut list[i64]) -> Generator[i64]:
    for x in xs:
        yield *x

def main() -> Result[(), AllocError | IoError]:
    mut xs = [2]?
    mut w = grow(&mut xs)
    print(next(w))?
    mut b = evens(&xs)
    print(next(b))?
    print(next(w))?
    Ok(())
"#,
    ));
    assert!(error.contains("conflicting borrow"), "{error}");
    assert!(!error.contains("without `mut`"), "{error}");
}

const NUMS: &str = r#"
def nums(n: i64) -> Generator[i64]:
    for i in range(n):
        yield i
"#;

// Review of local PR #1134, round 2: an arm producing an owned generator
// contributes no loan, in a conditional as in a `match` value, while the
// borrowing arm still keeps its owner borrowed.
#[test]
fn an_owned_arm_joins_a_borrowing_arm() {
    assert_stdout(
        &with_evens(&format!(
            r#"{NUMS}
def main() -> Result[(), AllocError | IoError]:
    ys = [2, 4]?
    k = len(ys)
    mut g = nums(2) if k > 9 else evens(&ys)
    mut h = evens(&ys) if k > 9 else nums(2)
    for v in g:
        print(v)?
    for v in h:
        print(v)?
    Ok(())
"#
        )),
        "2\n4\n0\n1\n",
    );
    let error = check_error(&with_evens(&format!(
        r#"{NUMS}
def main() -> Result[(), AllocError | IoError]:
    ys = [2, 4]?
    mut g = nums(2) if len(ys) > 9 else evens(&ys)
    drop(ys)
    print(next(g))?
    Ok(())
"#
    )));
    assert!(error.contains("cannot move `ys` while it is borrowed"), "{error}");
}

#[test]
fn a_returned_choice_accepts_an_owned_arm() {
    assert_stdout(
        &with_evens(&format!(
            r#"{NUMS}
def pick(xs: &list[i64], k: i64) -> Generator[i64]:
    match k:
        case 0:
            nums(3)
        case _:
            evens(xs)

def either(xs: &list[i64], k: i64) -> Generator[i64]:
    evens(xs) if k > 0 else nums(3)

def main() -> Result[(), AllocError | IoError]:
    ys = [2, 4]?
    for v in pick(&ys, 0):
        print(v)?
    for v in pick(&ys, 1):
        print(v)?
    print(either(&ys, 0).count())?
    Ok(())
"#
        )),
        "0\n1\n2\n2\n4\n3\n",
    );
}
