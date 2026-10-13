mod support;

#[test]
fn result_context_helper_accepts_unit_success_without_a_unit_specific_overload() {
    let out = support::run(
        r#"
class Context[E]:
    operation: str
    cause: E

def context[T, E](result: Result[T, E], operation: str) -> Result[T, Context[E]]:
    match result:
        case Ok(value):
            Ok(value)
        case Err(cause):
            Err(Context[E](operation, cause))

def main() -> Result[(), Failure]:
    flushed = flush_stdout()
    context(flushed, "flush").unwrap()
    failed: Result[(), str] = Err("disk full")
    match context(failed, "write"):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.operation)?
            print(error.cause)?
    Ok(())
"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "write\ndisk full\n");
}

#[test]
fn unit_payloads_specialize_inferred_explicit_and_first_class_functions() {
    let out = support::run(
        r#"
def keep[T, E](value: Result[T, E]) -> Result[T, E]:
    value

def optional[T](value: Option[T]) -> Option[T]:
    value

def unwrap[T, E](value: Result[T, E]) -> T:
    value.unwrap()

class Helpers:
    def keep[T](self, value: Option[T]) -> Option[T]:
        value

def main() -> Result[(), Failure]:
    success: Result[(), str] = Ok(())
    keep(success).unwrap()
    failure: Result[(), str] = Err("failed")
    match keep[(), str](failure):
        case Ok(_):
            print("unexpected")?
        case Err(message):
            print(message)?
    callback = keep[(), str]
    callback(Ok(())).unwrap()
    inferred_callback: Callable[[Result[(), str]], Result[(), str]] = keep
    inferred_callback(Ok(())).unwrap()
    unit_error: Result[i64, ()] = Ok(7)
    print(keep(unit_error).unwrap())?
    present: Option[()] = Some(())
    optional(present).unwrap()
    optional[()](Nothing).unwrap_or(())
    unwrap[(), str](Ok(()))
    helpers = Helpers()
    helpers.keep[()](Some(())).unwrap()
    print("done")?
    Ok(())
"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "failed\n7\ndone\n");
}

#[test]
fn unit_specializations_keep_parameter_storage_and_constraint_checks() {
    for (source, diagnostic) in [
        (
            "def f[T](value: T) -> T:\n    value\nf[()](())",
            "unit parameters are not supported yet",
        ),
        (
            "def f[T: IntType](value: Option[T]) -> Option[T]:\n    value\nf[()](Nothing)",
            "does not satisfy IntType",
        ),
        ("def f[T]() -> list[T]:\n    []\nf[()]()", "unit"),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{error}");
    }
}
