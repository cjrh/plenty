//! Type arguments of generic function and method calls inferred from a
//! concrete expected result type (issue #873).

mod support;

fn run(source: &str, expected: &str) {
    let output = support::run(source);
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

const HELPERS: &str = r#"
def empty[T]() -> Result[list[T], AllocError]:
    []

def none[T]() -> Option[T]:
    Nothing

def same[T](value: T) -> T:
    value

def repeat[T](value: T, count: i64) -> Result[list[T], AllocError]:
    mut out: list[T] = []?
    for _ in range(count):
        out.append(value)?
    Ok(out)
"#;

fn with_helpers(main: &str) -> String {
    format!("{HELPERS}\n{main}")
}

#[test]
fn issue_873_reproduction() {
    run(
        r#"
def empty[T]() -> Result[list[T], AllocError]:
    []
def main() -> Result[(), AllocError]:
    xs: list[u8] = empty()?
    drop(xs)
    Ok(())
"#,
        "",
    );
}

#[test]
fn every_kind_of_expectation_supplies_the_arguments() {
    run(
        &with_helpers(
            r#"
type Bytes = list[u8]

class Holder:
    items: list[u16]

def count(values: list[i8]) -> i64:
    len(values)

def returned() -> Result[list[u32], AllocError]:
    empty()

def returned_early() -> Result[Bytes, AllocError]:
    return empty()

def wrapped() -> Result[Option[list[i16]], AllocError]:
    Ok(Some(empty()?))

def main() -> Result[(), AllocError | IoError]:
    local: list[u8] = empty()?
    aliased: Bytes = empty()?
    absent: Option[str] = none()
    holder = Holder(empty()?)
    print(local, aliased, absent, holder.items, count(empty()?))?
    print(returned()?, returned_early()?, wrapped()?)?
    Ok(())
"#,
        ),
        "[] [] Option[str].Nothing [] 0\n[] [] Option[list[i16]].Some([])\n",
    );
}

#[test]
fn determined_positions_type_their_arguments() {
    run(
        &with_helpers(
            r#"
def main() -> Result[(), AllocError | IoError]:
    small: u8 = same(200)
    copies: list[u8] = repeat(7, 2)?
    nested: list[u8] = same(empty()?)
    print(small, copies, nested)?
    Ok(())
"#,
        ),
        "200 [7, 7] []\n",
    );
}

#[test]
fn generic_methods_take_the_expectation() {
    run(
        r#"
class Number:
    value: i64

    def convert[T: IntType](self) -> T:
        T(self.value)

    def several[T](self) -> Result[list[T], AllocError]:
        []

def main() -> Result[(), AllocError | IoError]:
    number = Number(7)
    small: u8 = number.convert()
    names: list[str] = number.several()?
    wide: i32 = Number(9).convert()
    print(small, names, wide)?
    Ok(())
"#,
        "7 [] 9\n",
    );
}

#[test]
fn argument_and_result_evidence_combine() {
    run(
        r#"
enum Invalid:
    Rejected

def tagged[A, B](tag: A) -> Result[(A, list[B]), AllocError]:
    Ok((tag, []?))

def fail[T, E](error: E) -> Result[T, E]:
    Err(error)

def first[T](values: &list[T]) -> Option[&T]:
    if len(values) == 0:
        return Nothing
    Some(&values[0])

def attempt() -> Result[i64, Invalid | AllocError]:
    value: i64 = fail(Invalid.Rejected)?
    Ok(value)

def main() -> Result[(), AllocError | IoError]:
    pair: (str, list[u8]) = tagged("a")?
    values = [4, 5]?
    head: Option[&i64] = first(&values)
    print(pair, head)?
    match attempt():
        case Err(Invalid.Rejected):
            print("rejected")?
        case _:
            print("other")?
    Ok(())
"#,
        "(\"a\", []) Option[&i64].Some(4)\nrejected\n",
    );
}

#[test]
fn disagreeing_evidence_is_reported() {
    reject(
        &with_helpers("def main() -> ():\n    n = 1\n    small: u8 = same(n)\n    drop(small)\n"),
        "type parameter `T` for `same` is i64 from its arguments but u8 from the expected result type",
    );
    reject(
        &with_helpers("def main() -> ():\n    small: u8 = same(1i64)\n    drop(small)\n"),
        "is i64 from its arguments but u8 from the expected result type",
    );
    reject(
        &with_helpers(
            "def main() -> Result[(), AllocError]:\n    xs: list[u8] = empty[i64]()?\n    drop(xs)\n    Ok(())\n",
        ),
        "expected list[u8], got list[i64]",
    );
}

#[test]
fn calls_without_exact_evidence_still_need_explicit_arguments() {
    let message = "cannot infer type parameter `T` for `none` from its arguments";
    reject(
        &with_helpers("def main() -> ():\n    x = none()\n    drop(x)\n"),
        message,
    );
    // A member value widens into an anonymous sum, so the sum names no member.
    reject(
        &with_helpers("def main() -> ():\n    x: Option[u8] | str = none()\n    drop(x)\n"),
        message,
    );
    reject(
        &with_helpers("def main() -> ():\n    x: i64 | str = none()\n    drop(x)\n"),
        message,
    );
}

#[test]
fn ownership_and_borrows_are_unchanged() {
    reject(
        &with_helpers(
            "def main() -> Result[(), AllocError | IoError]:\n    a = [1]?\n    b: list[i64] = same(a)\n    print(a)?\n    drop(b)\n    Ok(())\n",
        ),
        "moved",
    );
    reject(
        r#"
def view[T](value: &T) -> &T:
    value

def bad() -> &i64:
    n = 1
    view(&n)
"#,
        "reference parameter",
    );
}

#[test]
fn propagated_errors_are_not_evidence() {
    // `?` converts the operand's error into the function's error type, so
    // only the operand's payload is evidence for its type arguments.
    run(
        r#"
def passthrough[T, E](r: Result[T, E]) -> Result[T, E]:
    r

def check[E](error: E) -> Result[i64, E]:
    Err(error)

def fails[T, E](error: E) -> Result[T, E]:
    Err(error)

def erased() -> Result[i64, Failure]:
    r: Result[i64, str] = Ok(5)
    v: i64 = passthrough(r)?
    Ok(v)

def checked() -> Result[i64, Failure]:
    v: i64 = check("bad")?
    Ok(v)

def payload() -> Result[u8, Failure]:
    v: u8 = fails("oops")?
    Ok(v)

def widened() -> Result[u8, str | AllocError]:
    v: u8 = fails("wide")?
    Ok(v)

def report(result: Result[i64, Failure]) -> Result[(), IoError]:
    match result:
        case Ok(v):
            print(v)?
        case Err(_):
            print("err")?
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    report(erased())?
    report(checked())?
    match payload():
        case Ok(v):
            print(v)?
        case Err(_):
            print("payload err")?
    match widened():
        case Err(e):
            print(e)?
        case Ok(_):
            print("ok")?
    Ok(())
"#,
        "5\nerr\npayload err\n\"wide\"\n",
    );
    reject(
        r#"
def fails[T, E]() -> Result[T, E]:
    panic("unreachable")

def attempt() -> Result[i64, IoError]:
    v: i64 = fails()?
    Ok(v)

def main() -> ():
    drop(attempt())
"#,
        "cannot infer type parameter `E` for `fails`",
    );
}

#[test]
fn bound_conflicts_name_the_expected_result() {
    reject(
        r#"
def apply[T, F: Callable[[], T]](f: &F) -> T:
    f()

def three() -> i64:
    3

def main() -> ():
    x: u8 = apply(&three)
    drop(x)
"#,
        "type parameter `T` for `apply` is i64 from its arguments but u8 from the expected result type",
    );
}

#[test]
fn generic_methods_of_unannotated_generic_class_receivers() {
    // The receiver's class instance is created by the call itself, after the
    // expectation is recorded.
    run(
        r#"
class Holder[T]:
    value: T

    def get[U: IntType](self) -> U:
        U(7)

def main() -> Result[(), AllocError | IoError]:
    b = Holder(1)
    a: u8 = b.get()
    print(a)?
    Ok(())
"#,
        "7\n",
    );
}

#[test]
fn only_the_direct_operand_of_try_ignores_its_error() {
    // `wrap`'s parameter, not `?`, gives `mkerr()` its expected type, so its
    // error is evidence as it is without `?`.
    run(
        r#"
def mkerr[E]() -> Result[i64, E]:
    Ok(4)

def wrap(r: Result[i64, str]) -> Result[i64, str]:
    r

def passthrough[T, E](r: Result[T, E]) -> Result[T, E]:
    r

def direct() -> Result[i64, str]:
    a: Result[i64, str] = wrap(mkerr())
    v: i64 = wrap(mkerr())?
    drop(a?)
    Ok(v)

def arms(c: bool) -> Result[i64, Failure]:
    r: Result[i64, str] = Ok(5)
    v: i64 = (passthrough(r) if c else passthrough(r))?
    Ok(v)

def main() -> Result[(), AllocError | IoError]:
    match direct():
        case Ok(v):
            print(v)?
        case Err(e):
            print(e)?
    match arms(True):
        case Ok(v):
            print(v)?
        case Err(_):
            print("err")?
    Ok(())
"#,
        "4\n5\n",
    );
}
