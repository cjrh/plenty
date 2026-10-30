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
def main() -> Result[(), AllocError | IoError | Domain]:
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
def main() -> Result[(), IoError | ParseError]:
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
enum MapperError:
    Unavailable
def mapper(fail: bool) -> Result[OnceClosure[[i64], i64], MapperError]:
    print("mapper").unwrap()
    if fail:
        return Err(MapperError.Unavailable)
    Ok(def once (value: i64) -> i64: value)
def attempt() -> Result[(), MapperError]:
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
        (
            "def f() -> Result[i64, IoError]:\n    i64.parse(\"bad\")?\n    Ok(0)\n",
            "identical Result error types",
        ),
        (
            "def wrong(error: IoError) -> i64:\n    0\ndef f() -> ():\n    drop(map_error(i64.parse(\"bad\"), wrong))\n",
            "ParseError",
        ),
    ] {
        let source = format!(
            "from std.error import map_error\n{body}def main() -> ():\n    f()\n    pass\n"
        );
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
def main() -> Result[(), AllocError | IoError]:
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
def main() -> Result[(), IoError | ThreadError]:
    with spawn(worker)? as task:
        print(task.join()?)?
    Ok(())
"#,
        "42\n",
    );
}

fn check_error(source: &str) -> String {
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("main.plenty");
    std::fs::write(&file, source).unwrap();
    plenty::check_file(&file, None).unwrap_err().to_string()
}

#[test]
fn result_method_maps_with_variant_constructor_values() {
    assert_output(
        r#"
enum AppError:
    Io(IoError)
    Parse(ParseError)
enum Wrapped[T]:
    One(T)
def parsed(text: &str) -> Result[i64, AppError]:
    Ok(i64.parse(text).map_error(AppError.Parse)? + 1)
def main() -> Result[(), AppError | IoError | AllocError]:
    print("hello").map_error(AppError.Io)?
    print(parsed("41")?)?
    failed = i64.parse("bad")
    match failed.map_error(AppError.Parse):
        case Err(AppError.Parse(cause)):
            print(cause == ParseError.Invalid)?
        case _:
            print("unexpected")?
    make = AppError.Parse
    print(make(ParseError.Invalid), make == AppError.Parse)?
    typed: Callable[[i64], Wrapped[i64]] = Wrapped[i64].One
    print(typed(5))?
    kept: Result[str, i64] = Ok("kept")
    print(str.repr(kept.map_error(Wrapped[i64].One))?)?
    Ok(())
"#,
        "hello\n42\nTrue\nAppError.Parse(ParseError.Invalid) True\nWrapped[i64].One(5)\nResult[str, Wrapped[i64]].Ok(\"kept\")\n",
    );
}

#[test]
fn result_method_accepts_closures_with_extra_payloads_and_captures() {
    assert_output(
        r#"
enum AppError:
    Io(str, IoError)
def label(cause: IoError) -> AppError:
    AppError.Io("printing greeting", cause)
def main() -> Result[(), AppError]:
    print("one").map_error(label)?
    operation = "printing again"
    print("two").map_error(
        def [operation](cause: IoError) -> AppError:
            AppError.Io(operation, cause)
    )?
    match i64.parse("bad").map_error(def(cause: ParseError) -> str: "parse failed"):
        case Err(message):
            print(message).map_error(label)?
        case Ok(_):
            pass
    Ok(())
"#,
        "one\ntwo\nparse failed\n",
    );
}

#[test]
fn result_method_moves_affine_values_and_drops_unused_mappers_once() {
    assert_output(
        r#"
class Resource:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
enum Wrapped:
    Held(Resource)
def receiver() -> Result[Resource, Resource]:
    print("receiver").unwrap()
    Ok(Resource(40))
def mapper(owned: Resource) -> OnceClosure[[Resource], Resource]:
    print("mapper").unwrap()
    def once [owned](error: Resource) -> Resource:
        print("wrong").unwrap()
        drop(error)
        owned
def main() -> ():
    failed: Result[i64, Resource] = Err(Resource(1))
    drop(failed.map_error(Wrapped.Held))
    owned = Resource(10)
    convert = def once [owned](error: Resource) -> Resource:
        print("called").unwrap()
        drop(error)
        owned
    rejected: Result[i64, Resource] = Err(Resource(20))
    drop(rejected.map_error(convert))
    result = receiver().map_error(mapper(Resource(30)))
    print("forwarded").unwrap()
    drop(result)
"#,
        "1\ncalled\n20\n10\nreceiver\nmapper\n30\nforwarded\n40\n",
    );
}

#[test]
fn result_method_rejects_wrong_receivers_and_mappers() {
    let prelude = "enum E:\n    Io(IoError)\n    Two(str, IoError)\ndef wrong(error: ParseError) -> E:\n    E.Io(IoError.InvalidInput)\ndef good(error: IoError) -> E:\n    E.Io(error)\n";
    for (body, expected) in [
        (
            "    print(\"x\").map_error(wrong)?\n",
            "map_error mapper must take the error `IoError` and return the new error type; got a callable taking (ParseError) and returning E",
        ),
        (
            "    print(\"x\").map_error(E.Two)?\n",
            "supply the other payloads in a closure",
        ),
        (
            "    drop(print(\"x\").map_error(def(cause: IoError) -> (): drop(cause)))\n",
            "returning ()",
        ),
        (
            "    print(\"x\").map_error(&good)?\n",
            "map_error takes an owned mapper",
        ),
        (
            "    print(\"x\").map_error(good, good)?\n",
            "map_error takes one mapper, got 2 arguments",
        ),
        (
            "    result = print(\"x\")\n    (&result).map_error(good)?\n",
            "map_error consumes its Result",
        ),
        (
            "    value = 5\n    value.map_error(good)?\n",
            "map_error requires a Result, got i64",
        ),
        (
            "    make = Wrapped.One\n",
            "generic enum `Wrapped` needs type arguments here, such as `Wrapped[i64].One`",
        ),
    ] {
        let source = format!(
            "{prelude}enum Wrapped[T]:\n    One(T)\ndef f() -> Result[(), E]:\n{body}    Ok(())\ndef main() -> ():\n    drop(f())\n"
        );
        let error = check_error(&source);
        assert!(error.contains(expected), "{source}\n{error}");
    }
    // Propagation is unchanged: `?` never selects a variant.
    let error = check_error(
        "enum E:\n    Io(IoError)\ndef f() -> Result[(), E]:\n    print(\"x\")?\n    Ok(())\ndef main() -> ():\n    drop(f())\n",
    );
    assert!(error.contains("identical Result error types"), "{error}");
}

#[test]
fn inherent_map_error_methods_still_take_precedence() {
    assert_output(
        r#"
class Counter:
    count: i64
    def map_error(self, extra: i64) -> i64:
        self.count + extra
def make() -> Counter:
    Counter(1)
def main() -> Result[(), IoError]:
    counter = Counter(2)
    print(counter.map_error(1), make().map_error(3))?
    Ok(())
"#,
        "3 4\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn result_method_and_constructor_values_allocate_nothing() {
    assert_output(
        r#"
enum AppError:
    Parse(ParseError)
def main() -> Result[(), AllocError | IoError]:
    success: Result[list[i64], ParseError] = Ok([3, 4]?)
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    kept = success.map_error(AppError.Parse)
    failed = i64.parse("bad").map_error(AppError.Parse)
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    match kept:
        case Ok(values):
            print(values)?
        case Err(_):
            print("unexpected")?
    match failed:
        case Err(AppError.Parse(cause)):
            print(cause == ParseError.Invalid)?
        case Ok(_):
            print("unexpected")?
    Ok(())
"#,
        "[3, 4]\nTrue\n",
    );
}
