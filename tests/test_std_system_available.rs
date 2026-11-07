//! Portable CPU availability and caller-selected fallback policy.

mod support;

#[test]
fn available_parallelism_has_a_positive_pool_ready_result_and_caller_fallback() {
    let output = support::run_file_stdout(
        r#"import std.system
from std.system import SystemError

def fallback(value: Result[Positive[i64], SystemError]) -> Positive[i64]:
    value.unwrap_or(1)

def work() -> Result[Positive[i64], SystemError]:
    std.system.available_parallelism()

def main() -> Result[(), IoError | SpawnError[()]]:
    match std.system.available_parallelism():
        case Ok(count):
            print(count.get() > 0)?
        case Err(error):
            print(fallback(Err(error)).get() == 1)?
    print(fallback(Err(SystemError.Unsupported)).get())?
    print(fallback(Err(SystemError.Unavailable)).get())?
    print(fallback(Err(SystemError.Limit)).get())?
    print(fallback(Ok(3)).get())?
    with spawn(work).drop_job()? as task:
        print(fallback(task.join()).get() > 0)?
    Ok(())
"#,
    );
    assert_eq!(output, "True\n1\n1\n1\n3\nTrue");
    let error =
        support::check_error("def main() -> ():\n    drop(__system_available_parallelism())");
    assert!(
        error.contains("reserved for the standard library"),
        "{error}"
    );
}
#[cfg(feature = "runtime-checks")]
#[test]
fn cpu_availability_needs_no_plenty_heap_allocation() {
    let output = support::run_file_stdout(
        r#"import std.system
from std.system import SystemError

def main() -> Result[(), IoError]:
    print("__test_fail_allocations_after_0__")?
    result = std.system.available_parallelism()
    print("__test_restore_allocations__")?
    match result:
        case Ok(count):
            print(count.get() > 0)?
        case Err(SystemError.Unavailable) | Err(SystemError.Unsupported) | Err(SystemError.Limit):
            print(True)?
        case Err(_):
            print(False)?
    Ok(())
"#,
    );
    assert_eq!(output, "True");
}
