//! Every callable form passes wherever a callback is accepted (rule 6,
//! backlog B93; issues #617, #619), copyable borrowed items in `find` and
//! `reduce`, and diagnostics that suggest `*x` or `iter(source)` (rule 4,
//! backlog B91; issues #621, #622).
mod support;
use support::{assert_stdout, check_diagnostic};

#[test]
fn pipelines_accept_stored_capture_free_closures() {
    assert_stdout(
        r#"
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3]?
    big = (def(x: &i64) -> bool: *x > 1)
    tenfold = (def(x: &i64) -> i64: *x * 10)
    add = (def(a: i64, b: i64) -> i64: a + b)
    print(iter(&xs).filter(big).count(), iter(&xs).filter(big).count())?
    print(iter(&xs).map(tenfold).fold(0, add), iter(&xs).map(tenfold).reduce(add))?
    print(iter(&xs).any(big), iter(&xs).all(big), iter(&xs).find(big))?
    Ok(())
"#,
        "2 2\n60 Option[i64].Some(60)\nTrue False Option[i64].Some(2)\n",
    );
}

#[test]
fn pipelines_accept_callable_parameters_elements_and_boxed_callables() {
    assert_stdout(
        r#"
def count_if(xs: &list[i64], p: Callable[[&i64], bool]) -> i64:
    iter(xs).filter(p).count()

def big(x: &i64) -> bool:
    *x > 1

def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3]?
    print(count_if(&xs, big), count_if(&xs, def(x: &i64) -> bool: *x > 2))?
    handlers = [big]?
    print(iter(&xs).filter(handlers[0]).count())?
    positive = BoxedCallable(def(x: &i64) -> bool: *x > 0)?
    print(iter(&xs).filter(positive).count())?
    Ok(())
"#,
        "2 1\n2\n3\n",
    );
}

#[test]
fn pipeline_callback_rejections_name_the_form() {
    let error = check_diagnostic(
        "xs = [1, 2].unwrap()\nz = 1\nh = def [z](x: &i64) -> bool:\n    *x > z\nprint(iter(&xs).filter(&h).count()).unwrap()",
    );
    assert!(
        error.contains("pipeline callbacks are passed by value, not by reference"),
        "{error}"
    );
    let error = check_diagnostic(
        "xs = [1, 2].unwrap()\nh = def once [](x: &i64) -> bool:\n    *x > 1\nprint(iter(&xs).filter(h).count()).unwrap()",
    );
    assert!(
        error.contains("a one-shot closure cannot be a pipeline callback"),
        "{error}"
    );
}

#[test]
fn sort_by_key_accepts_closure_literals() {
    assert_stdout(
        r#"
def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 5, 3]?
    xs.sort_by_key(def(x: &i64) -> i64: -*x)
    print(xs)?
    pivot = 6
    xs.sort_by_key(def [pivot](x: &i64) -> i64: (*x - pivot) * (*x - pivot), True)
    print(xs)?
    Ok(())
"#,
        "[5, 3, 1]\n[1, 3, 5]\n",
    );
}

#[test]
fn mut_borrows_of_named_functions_pass_to_mut_callback_parameters() {
    assert_stdout(
        r#"
def apply[F: Callable[[i64], i64]](f: &mut F, value: i64) -> i64:
    f(value)

def inc(x: i64) -> i64:
    x + 1

def main() -> Result[(), IoError]:
    print(apply(&mut inc, 1), apply(&mut inc, 2))?
    first = &mut inc
    second = &mut inc
    print(first(10), second(20))?
    mut total = 0
    mut running = def [mut total](x: i64) -> i64:
        total += x
        total
    print(apply(&mut running, 2), apply(&mut running, 3))?
    Ok(())
"#,
        "2 3\n11 21\n2 5\n",
    );
}

#[test]
fn shared_callback_diagnostic_names_the_mut_capture() {
    let error = check_diagnostic(
        "def apply[F: Callable[[i64], i64]](f: &F, value: i64) -> i64:\n    f(value)\nmut total = 0\nrunning = def [mut total](x: i64) -> i64:\n    total += x\n    total\nprint(apply(&running, 2)).unwrap()",
    );
    assert!(
        error.contains("cannot call this mutable closure through a shared reference: it writes its capture `total`"),
        "{error}"
    );
}

#[test]
fn find_and_reduce_copy_copyable_borrowed_items() {
    assert_stdout(
        r#"
def big(x: &i64) -> bool:
    *x > 1

def add(a: i64, b: i64) -> i64:
    a + b

def longer(a: str, b: str) -> str:
    if len(b) > len(a):
        b
    else:
        a

def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3]?
    empty: list[i64] = []?
    print(iter(&xs).find(big), iter(&empty).find(big))?
    print(iter(&xs).reduce(add), iter(&empty).reduce(add))?
    names = ["a", "ccc", "bb"]?
    print(iter(&names).reduce(longer), iter(&names).find(def(s: &str) -> bool: len(*s) == 2))?
    mut ys = [4, 5]?
    print(iter(&mut ys).reduce(add))?
    Ok(())
"#,
        "Option[i64].Some(2) Option[i64].Nothing\nOption[i64].Some(6) Option[i64].Nothing\nOption[str].Some(\"ccc\") Option[str].Some(\"bb\")\nOption[i64].Some(9)\n",
    );
}

#[test]
fn find_still_rejects_borrowed_affine_items() {
    let error = check_diagnostic(
        "rows = [[1].unwrap()].unwrap()\nprint(iter(&rows).find(def(r: &list[i64]) -> bool: True)).unwrap()",
    );
    assert!(
        error.contains("references cannot be stored in a find result"),
        "{error}"
    );
}

#[test]
fn borrowed_scalar_diagnostics_suggest_dereferencing() {
    let error = check_diagnostic("xs = [1, 2].unwrap()\nys = [x for x in &xs].unwrap()");
    assert!(
        error.contains("references cannot be stored in collections; store a copy with `*x`"),
        "{error}"
    );
    let error = check_diagnostic(
        "def inc(x: i64) -> i64:\n    x + 1\nxs = [1, 2].unwrap()\nfor x in &xs:\n    inc(x)",
    );
    assert!(
        error.contains("expected i64, got &i64; copy the value with `*`"),
        "{error}"
    );
}

#[test]
fn iterator_methods_on_sources_suggest_iter() {
    let prelude =
        "xs = [1, 2].unwrap()\nmut d: dict[i64, i64] = {}.unwrap()\nd.insert(1, 2).unwrap()\n";
    for (call, expected) in [
        (
            "range(3).count()",
            "`count` is an iterator method, and a range is not an iterator; start a pipeline with `iter`: `iter(range(...)).count()`",
        ),
        (
            "xs.count()",
            "a list is not an iterator; start a pipeline with `iter`: `iter(&xs).count()`",
        ),
        (
            "xs.map(def(x: &i64) -> i64: *x).count()",
            "`map` is an iterator method, and a list is not an iterator; start a pipeline with `iter`: `iter(&xs).map(...)`",
        ),
        ("d.keys().count()", "`iter(d.keys()).count()`"),
        ("d.values().count()", "`iter(d.values()).count()`"),
        (
            "reversed(&xs).map(def(x: &i64) -> i64: *x).count()",
            "`iter(reversed(...)).map(...)`",
        ),
    ] {
        let error = check_diagnostic(&format!("{prelude}print({call}).unwrap()"));
        assert!(error.contains(expected), "{call}\n{error}");
    }
}

#[test]
fn list_and_text_search_methods_are_unchanged() {
    assert_stdout(
        r#"
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 2]?
    print(xs.count(2), xs.find(2), "aba".count("a"), "aba".find("b"))?
    Ok(())
"#,
        "2 Option[i64].Some(1) 2 Option[i64].Some(1)\n",
    );
}
