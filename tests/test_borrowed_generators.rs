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
