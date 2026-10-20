mod support;
use support::assert_stdout as run;

#[test]
fn heterogeneous_handlers_and_branch_factories_share_a_structural_type() {
    run(
        r#"
def make(k: i64) -> Result[BoxedCallable[[i64], i64], AllocError]:
    if k > 0:
        add = def [k](v: i64) -> i64:
            k + v
        return BoxedCallable(add)
    span = range(10, 13)
    mul = def [span](v: i64) -> i64:
        span[1] * v
    BoxedCallable(mul)
def negate(n: i64) -> i64:
    -n
def invoke[F: Callable[[i64], i64]](callback: &F, n: i64) -> i64:
    callback(n)
def typed[T](callback: &BoxedCallable[[T], T], value: T) -> T:
    callback(value)
def via_mut(callback: &mut BoxedCallable[[i64], i64], value: i64) -> i64:
    callback(value)
class Holder:
    callback: BoxedCallable[[i64], i64]
enum Job:
    Run(BoxedCallable[[i64], i64])
mut handlers: dict[str, BoxedCallable[[i64], i64]] = {"add": make(3).unwrap(), "mul": make(0).unwrap()}.unwrap()
print(handlers["add"](4)).unwrap()
print(handlers["mul"](4)).unwrap()
handlers["add"] = BoxedCallable(negate).unwrap()
print(handlers["add"](4)).unwrap()
mut callbacks: list[BoxedCallable[[i64], i64]] = [make(2).unwrap(), make(0).unwrap()].unwrap()
for n in range(50):
    callbacks.append(make(n + 1).unwrap()).unwrap()
callbacks.reverse()
removed = callbacks.pop(0).unwrap()
print(invoke(&removed, 1)).unwrap()
print(typed(&removed, 2)).unwrap()
mut mutable_binding = make(1).unwrap()
print(via_mut(&mut mutable_binding, 2)).unwrap()
callback = &callbacks[0]
print(callback(1)).unwrap()
holder = Holder(make(7).unwrap())
print(holder.callback(1)).unwrap()
job = Job.Run(make(8).unwrap())
match &job:
    case Job.Run(callback):
        print(callback(1)).unwrap()
print(make(9).unwrap()(1)).unwrap()
"#,
        "7\n44\n-4\n51\n52\n3\n50\n8\n9\n10\n",
    );
}

#[test]
fn call_abi_supports_owned_arguments_unit_and_inline_results() {
    run(
        r#"
def consume(values: list[i64]) -> i64:
    values[0]
def inspect(values: &list[i64]) -> i64:
    values[0]
def pair(n: i64) -> tuple[i64, i64]:
    (n, n + 1)
def noop() -> ():
    pass
consume_box = BoxedCallable(consume).unwrap()
read_box = BoxedCallable(inspect).unwrap()
pair_box = BoxedCallable(pair).unwrap()
unit_box = BoxedCallable(noop).unwrap()
print(consume_box([4].unwrap())).unwrap()
values = [8].unwrap()
print(read_box(&values)).unwrap()
print(pair_box(10)).unwrap()
unit_box()
"#,
        "4\n8\n(10, 11)\n",
    );
}

#[test]
fn boxing_rejects_unsupported_modes_borrows_and_reference_outputs() {
    for (source, expected) in [
        (
            "n = 1\nf = def [&n]() -> i64:\n    n\nBoxedCallable(f)\n",
            "borrowed captures",
        ),
        (
            "n = 1\nf = def [mut n]() -> i64:\n    n = n + 1\n    n\nBoxedCallable(f)\n",
            "shared-call",
        ),
        (
            "n = 1\nf = def once [n]() -> i64:\n    n\nBoxedCallable(f)\n",
            "reusable",
        ),
        (
            "def identity(n: &i64) -> &i64:\n    n\nBoxedCallable(identity)\n",
            "cannot return references",
        ),
        ("BoxedCallable(1)\n", "owned Callable or closure"),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

#[test]
fn boxed_signatures_preserve_private_nominal_visibility() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("api.plenty"), "class Hidden:\n    value: i64\npub def exposed(callback: BoxedCallable[[Hidden], i64]) -> ():\n    pass\n").unwrap();
    let main = directory.path().join("main.plenty");
    std::fs::write(&main, "import api\ndef main() -> ():\n    pass\n").unwrap();
    let error = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(
        error.contains("public signature exposes private type"),
        "{error}"
    );
}

#[test]
fn erasure_preserves_move_borrow_signature_and_equality_rules() {
    let prefix = "def id(n: i64) -> i64:\n    n\nf = BoxedCallable(id).unwrap()\n";
    for (tail, expected) in [
        ("moved = f\nf(1)", "moved"),
        ("borrowed = &f\ndrop(f)\nborrowed(1)", "borrow"),
        ("copy(f)", "cannot be copied"),
        ("f(True)", "expected i64"),
        ("f()", "expects 1 arguments"),
        ("print(f == f).unwrap()", "closures do not support equality"),
        (
            "wrong: BoxedCallable[[bool], i64] = f",
            "expected BoxedCallable",
        ),
    ] {
        let error = support::check_source(&format!("{prefix}{tail}\n"))
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

#[test]
fn erased_environments_remain_ineligible_for_worker_transfer() {
    for source in [
        "sender, receiver = channel[BoxedCallable[[i64], i64]](1).unwrap()",
        r#"
def id(n: i64) -> i64:
    n
def worker(callback: &BoxedCallable[[i64], i64]) -> i64:
    callback(1)
def main() -> Result[(), AllocError | ThreadError | IoError]:
    callback = BoxedCallable(id)?
    with spawn(worker, &callback)? as task:
        task.join()
    Ok(())
"#,
        r#"
class Holder:
    callback: BoxedCallable[[i64], i64]
def id(n: i64) -> i64:
    n
def worker(holder: &Holder) -> i64:
    holder.callback(1)
def main() -> Result[(), AllocError | ThreadError | IoError]:
    holder = Holder(BoxedCallable(id)?)
    with spawn(worker, &holder)? as task:
        task.join()
    Ok(())
"#,
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(
            error.contains("boxed callable captures and effects"),
            "{error}"
        );
    }
}

#[test]
fn heterogeneous_stored_environment_replacement_and_extraction_drop_once() {
    run(
        r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
def first(n: i64) -> Result[BoxedCallable[[], i64], AllocError]:
    guard = Guard(n)
    callback = def [guard]() -> i64:
        guard.id
    BoxedCallable(callback)
def second(n: i64) -> Result[BoxedCallable[[], i64], AllocError]:
    guard = Guard(n)
    span = range(n, n + 2)
    callback = def [guard, span]() -> i64:
        guard.id + span[1]
    BoxedCallable(callback)
mut callbacks = {"a": first(1).unwrap(), "b": second(2).unwrap()}.unwrap()
callbacks["a"] = second(3).unwrap()
removed = callbacks.pop("b").unwrap()
print(removed()).unwrap()
drop(callbacks)
drop(removed)
"#,
        "1\n5\n3\n2\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_boxing_drops_capture_and_calls_and_drops_allocate_nothing() {
    run(
        r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        write_stdout("drop\n").unwrap()
        pass
def make(n: i64) -> Closure[[], i64]:
    guard = Guard(n)
    span = range(n, n + 2)
    def [guard, span]() -> i64:
        guard.id + span[1]
held = BoxedCallable(make(2)).unwrap()
pending = make(4)
print("__test_fail_allocations_after_0__").unwrap()
match BoxedCallable(pending):
    case Ok(unexpected):
        drop(unexpected)
    case Err(_):
        write_stdout("failed\n").unwrap()
        pass
value = held()
drop(held)
print("__test_restore_allocations__").unwrap()
print(value).unwrap()
"#,
        "__test_fail_allocations_after_0__\ndrop\nfailed\ndrop\n__test_restore_allocations__\n5\n",
    );
}
