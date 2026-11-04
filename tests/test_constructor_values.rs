//! Constructor values obey ordinary positional callable signatures (issue #618).
mod support;
use support::{assert_stdout, check_diagnostic};

#[test]
fn variants_work_in_pipelines_and_borrowed_callback_positions() {
    assert_stdout(
        r#"
enum Wrapped:
    Value(i64)
def apply[F: Callable[[i64], Wrapped]](f: &F) -> Wrapped:
    f(7)
def main() -> Result[(), AllocError | IoError]:
    wrapped: list[Wrapped] = iter(range(3)).map(Wrapped.Value).collect()?
    print(wrapped)?
    print(apply(&Wrapped.Value))?
    options: list[Option[i64]] = iter(range(2)).map(Option[i64].Some).collect()?
    results: list[Result[i64, str]] = iter(range(2)).map(Result[i64, str].Ok).collect()?
    print(options, results)?
    Ok(())
"#,
        "[Wrapped.Value(0), Wrapped.Value(1), Wrapped.Value(2)]\nWrapped.Value(7)\n[Option[i64].Some(0), Option[i64].Some(1)] [Result[i64, str].Ok(0), Result[i64, str].Ok(1)]\n",
    );
}

#[test]
fn classes_and_generic_class_instances_are_positional_function_values() {
    assert_stdout(
        r#"
class Point:
    x: i64
    y: i64
class Cell[T]:
    value: T
class Fallible[T]:
    value: T
    def __init__(self: &mut Fallible[T], value: T) -> Result[(), AllocError]:
        self.value = value
        Ok(())
def make[F: Callable[[i64, i64], Point]](f: &F) -> Point:
    f(3, 4)
def generic(f: Callable[[i64], Cell[i64]]) -> Cell[i64]:
    f(9)
def main() -> Result[(), AllocError | IoError]:
    point = Point
    print(point(1, 2), make(&Point))?
    cells: list[Cell[i64]] = iter(range(2)).map(Cell[i64]).collect()?
    print(cells)?
    cell: Callable[[str], Cell[str]] = Cell
    print(cell("hello"), generic(Cell))?
    create: Callable[[i64], Result[Fallible[i64], AllocError]] = Fallible
    print(create(8)?)?
    Ok(())
"#,
        "Point(x=1, y=2) Point(x=3, y=4)\n[Cell[i64](value=0), Cell[i64](value=1)]\nCell[str](value=\"hello\") Cell[i64](value=9)\nFallible[i64](value=8)\n",
    );
}

#[test]
fn expected_callable_specializes_prelude_and_generic_variant_values() {
    assert_stdout(
        r#"
enum Wrapped[T]:
    Value(T)
def apply[F: Callable[[i64], Option[i64]]](f: &F) -> Option[i64]:
    f(6)
def apply_mut[F: Callable[[i64], Option[i64]]](f: &mut F) -> Option[i64]:
    f(9)
def wrap(f: Callable[[i64], Wrapped[i64]]) -> Wrapped[i64]:
    f(7)
def success() -> Callable[[i64], Result[i64, str]]:
    Ok
def main() -> Result[(), AllocError | IoError]:
    some: Callable[[i64], Option[i64]] = Some
    error: Callable[[str], Result[i64, str]] = Err
    print(some(5), apply(&Some), wrap(Wrapped.Value))?
    print(str.repr(success()(8))?, str.repr(error("bad"))?, apply_mut(&mut Some))?
    Ok(())
"#,
        "Option[i64].Some(5) Option[i64].Some(6) Wrapped[i64].Value(7)\nResult[i64, str].Ok(8) Result[i64, str].Err(\"bad\") Option[i64].Some(9)\n",
    );
}

#[test]
fn constructor_values_preserve_affine_payloads_and_cleanup() {
    assert_stdout(
        r#"
class Resource:
    value: i64
    def __del__(self) -> ():
        print(self.value).unwrap()
def main() -> Result[(), IoError]:
    make = Resource
    wrap = Option[Resource].Some
    value = wrap(make(12))
    drop(value)
    print("done")?
    Ok(())
"#,
        "12\ndone\n",
    );
}

#[test]
fn constructor_values_enforce_the_existing_layout_depth_limit() {
    let mut source = String::from("class C0:\n    value: i64\n");
    for level in 1..64 {
        source.push_str(&format!("class C{level}:\n    value: C{}\n", level - 1));
    }
    source.push_str("def main() -> ():\n    constructor = C63\n");
    let error = check_diagnostic(&source);
    assert!(
        error.contains("type nesting exceeds the implementation limit of 64"),
        "{error}"
    );
}

#[test]
fn constructors_need_context_and_keep_signature_checks() {
    for (source, expected) in [
        (
            "f = Some",
            "requires an expected Callable type or an explicit instance",
        ),
        (
            "f: Callable[[i64], Option[str]] = Some",
            "expected Callable[[i64], Option[str]]",
        ),
        (
            "f: Callable[[i64], i64] = Ok",
            "does not produce the expected Callable result",
        ),
        (
            "enum E:\n    Empty\nf: Callable[[], E] = E.Empty",
            "expected Callable[[], E], got E",
        ),
        (
            "class Point:\n    x: i64\nf = Point\nx = f(x=1)",
            "named arguments require a statically resolved",
        ),
    ] {
        let error = check_diagnostic(source);
        assert!(
            error.contains(expected),
            "expected {expected:?}, got {error}"
        );
    }
}

#[test]
fn constructor_values_keep_private_initializers_private() {
    for (declaration, expression, annotation) in [
        ("pub class Secret:\n    value: i64\n", "Secret", ""),
        ("pub class Secret[T]:\n    value: T\n", "Secret[i64]", ""),
        (
            "pub class Secret[T]:\n    value: T\n",
            "Secret",
            ": Callable[[i64], Secret[i64]]",
        ),
    ] {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("lib.plenty"), declaration).unwrap();
        let main = workspace.path().join("main.plenty");
        std::fs::write(
            &main,
            format!(
                "from lib import Secret\ndef main() -> ():\n    make{annotation} = {expression}\n"
            ),
        )
        .unwrap();
        let error = plenty::check_file(&main, None).unwrap_err().to_string();
        assert!(error.contains("__new__` is private"), "{error}");
    }
}

#[test]
fn constructor_names_follow_local_shadowing_and_reference_lifetimes() {
    assert_stdout(
        r#"
class Point:
    value: i64
def main() -> Result[(), IoError]:
    Point = (def(value: i64) -> Result[i64, OverflowError[i64]]: Ok(value + 1))
    print(Point(4).unwrap())?
    mut value = 7
    wrap: Callable[[&i64], Option[&i64]] = Some
    wrapped = wrap(&value)
    match wrapped:
        case Some(found):
            print(*found)?
        case Nothing:
            pass
    value = 8
    print(value)?
    Ok(())
"#,
        "5\n7\n8\n",
    );
    let error = check_diagnostic(
        "def bad(outer: &i64) -> Option[&i64]:\n    value = 7\n    wrap: Callable[[&i64], Option[&i64]] = Some\n    wrap(&value)",
    );
    assert!(error.contains("reference"), "{error}");
}

#[test]
fn fallible_constructor_values_preserve_failed_init_cleanup() {
    assert_stdout(
        r#"
class Resource:
    value: i64
    def __del__(self) -> ():
        print(self.value).unwrap()
class Fallible:
    value: Resource
    def __init__(self: &mut Fallible, value: Resource) -> Result[(), AllocError]:
        self.value = value
        Err(AllocError.OutOfMemory)
def main() -> Result[(), IoError]:
    make = Resource
    fail = Fallible
    match fail(make(12)):
        case Err(AllocError.OutOfMemory):
            print("failed")?
        case _:
            print("unexpected")?
    Ok(())
"#,
        "12\nfailed\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn constructor_values_do_not_allocate() {
    support::assert_visible_stdout(
        r#"
class Value:
    value: i64
def main() -> Result[(), IoError]:
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    make = Value
    wrap = Option[Value].Some
    drop(wrap(make(11)))
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    print("done")?
    Ok(())
"#,
        "done\n",
    );
}

#[test]
fn generic_constructor_values_preserve_alias_and_affine_union_errors() {
    assert_stdout(
        r#"
class Guard:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
enum Problem:
    Owned(Guard)
type ConstructorError = Problem | OverflowError[i64]
class Cell[T, E]:
    value: T
    def __init__(self, value: T, error: Option[E]) -> Result[(), E | OverflowError[i64]]:
        self.value = value
        match error:
            case Some(error):
                return Err(error)
            case Nothing:
                pass
        Ok(())
def main() -> Result[(), IoError]:
    make: Callable[[Guard, Option[Problem]], Result[Cell[Guard, Problem], ConstructorError]] = Cell
    result = make(Guard("field"), Some(Problem.Owned(Guard("error"))))
    print("returned")?
    drop(result)
    value = Cell[Guard, Problem].new(Guard("success"), Nothing).unwrap()
    print(value.value.name)?
    drop(value)
    Ok(())
"#,
        "field\nreturned\nerror\nsuccess\nsuccess\n",
    );
}

#[test]
fn constructor_factory_resolves_whole_result_and_unit_aliases() {
    assert_stdout(
        r#"
type Ready = ()
type InitResult = Result[(), str]
class Plain[T]:
    value: T
    def __init__(self, value: T) -> Ready:
        self.value = value
class Checked[T]:
    value: T
    def __init__(self, value: T, fail: bool = False) -> InitResult:
        if fail:
            return Err("rejected")
        self.value = value
        Ok(())
def main() -> Result[(), IoError]:
    plain: Callable[[i64], Plain[i64]] = Plain
    make: Callable[[i64, bool], Result[Checked[i64], str]] = Checked
    print(plain(1), Plain(value=True))?
    print(make(2, False).unwrap(), Checked(value=3).unwrap(), Checked[i64].new(4).unwrap())?
    print(Checked[i64](5, True).is_err())?
    Ok(())
"#,
        "Plain[i64](value=1) Plain[bool](value=True)\nChecked[i64](value=2) Checked[i64](value=3) Checked[i64](value=4)\nTrue\n",
    );
}
