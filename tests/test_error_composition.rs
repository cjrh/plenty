use std::process::Command;

fn assert_output(source: &str, expected: &str) {
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("main.plenty");
    let binary = workspace.path().join("program");
    std::fs::write(&file, source).unwrap();
    plenty::compile_file_to_executable(&file, &binary, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = Command::new(binary).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let visible: String = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(visible, expected);
}

#[test]
fn maps_only_errors_and_preserves_success_payloads() {
    assert_output(
        r#"
from std.error import map_error
enum Domain:
    Parse(ParseError)
def classify(error: ParseError) -> Domain:
    print("mapped").unwrap()
    Domain.Parse(error)
def parsed(text: &str) -> Result[i64, Domain]:
    Ok(map_error(i64.parse(text), classify)? + 1)
def main() -> Result[(), Failure]:
    print(parsed("41")?)?
    match parsed("bad"):
        case Ok(_):
            print("unexpected")?
        case Err(Domain.Parse(cause)):
            print(cause == ParseError.Invalid)?
    kept: Result[list[i64], ParseError] = Ok([7, 8]?)
    print(map_error(kept, classify)?)?
    unit: Result[(), ParseError] = Ok(())
    map_error(unit, classify)?
    Ok(())
"#,
        "42\nmapped\nTrue\n[7, 8]\n",
    );
}

#[test]
fn consumes_affine_errors_and_drops_used_and_unused_captures_once() {
    assert_output(
        r#"
from std.error import map_error
class Resource:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
def main() -> ():
    owned = Resource(10)
    convert = def once [owned](error: Resource) -> Resource:
        print("called").unwrap()
        drop(error)
        owned
    failed: Result[i64, Resource] = Err(Resource(20))
    mapped = map_error(failed, convert)
    drop(mapped)
    unused = Resource(30)
    skipped = def once [unused](error: Resource) -> Resource:
        print("wrong").unwrap()
        drop(error)
        unused
    success: Result[Resource, Resource] = Ok(Resource(40))
    result = map_error(success, skipped)
    print("forwarded").unwrap()
    drop(result)
"#,
        "called\n20\n10\n30\nforwarded\n40\n",
    );
}

#[test]
fn accepts_mutable_reusable_callbacks_and_does_not_flatten_results() {
    assert_output(
        r#"
from std.error import map_error
def main() -> Result[(), Failure]:
    count = 10
    mapper = def [mut count](error: ParseError) -> Result[i64, ParseError]:
        count += 1
        Ok(count)
    result: Result[i64, Result[i64, ParseError]] = map_error(i64.parse("bad"), mapper)
    match result:
        case Ok(_):
            print("unexpected")?
        case Err(inner):
            print(inner?)?
    Ok(())
"#,
        "11\n",
    );
}

#[test]
fn expressions_run_once_in_order_and_failed_mapper_construction_cleans_result() {
    assert_output(
        r#"
from std.error import map_error
class Resource:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
def result() -> Result[Resource, i64]:
    print("result").unwrap()
    Ok(Resource(42))
def mapper(fail: bool) -> Result[OnceClosure[[i64], i64], Failure]:
    print("mapper").unwrap()
    if fail:
        return Err(Failure.Unspecified)
    Ok(def once (value: i64) -> i64: value)
def attempt() -> Result[(), Failure]:
    drop(map_error(result(), mapper(True)?))
    Ok(())
def main() -> ():
    drop(attempt())
"#,
        "result\nmapper\n42\n",
    );
}

#[test]
fn mapping_does_not_change_propagation_or_accept_wrong_callbacks() {
    for (body, expected) in [
        ("def f() -> Result[i64, IoError]:\n    i64.parse(\"bad\")?\n    Ok(0)\n", "identical Result error types"),
        ("def wrong(error: IoError) -> i64:\n    0\ndef f() -> ():\n    drop(map_error(i64.parse(\"bad\"), wrong))\n", "ParseError"),
    ] {
        let source = format!("from std.error import map_error\n{body}def main() -> ():\n    f()\n    pass\n");
        let workspace = tempfile::tempdir().unwrap();
        let file = workspace.path().join("main.plenty");
        std::fs::write(&file, source).unwrap();
        let error = plenty::check_file(&file, None).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn mapping_allocates_nothing_and_callback_allocation_failure_stays_explicit() {
    assert_output(
        r#"
from std.error import map_error
def classify(error: ParseError) -> IoError:
    IoError.InvalidInput
def enriched(error: ParseError) -> Result[str, AllocError]:
    "context for ".concat("parse error")
def main() -> Result[(), Failure]:
    success: Result[list[i64], ParseError] = Ok([3, 4]?)
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    kept = map_error(success, classify)
    failed = map_error(i64.parse("bad"), classify)
    print("__test_end_no_allocations__")?
    nested = map_error(i64.parse("bad"), enriched)
    print("__test_restore_allocations__")?
    print(kept?)?
    match failed:
        case Ok(_):
            print(False)?
        case Err(error):
            print(error == IoError.InvalidInput)?
    match nested:
        case Err(Err(AllocError.OutOfMemory)):
            print("allocation kept")?
        case _:
            print("unexpected")?
    Ok(())
"#,
        "[3, 4]\nTrue\nallocation kept\n",
    );
}

#[test]
fn worker_mapping_uses_a_concrete_callback_with_checked_effects() {
    assert_output(
        r#"
from std.error import map_error
def classify(cause: ParseError) -> IoError:
    IoError.InvalidInput
def worker() -> Result[i64, IoError]:
    map_error(i64.parse("42"), def once (cause: ParseError) -> IoError: classify(cause))
def main() -> Result[(), Failure]:
    with spawn(worker)? as task:
        print(task.join()?)?
    Ok(())
"#,
        "42\n",
    );
}
