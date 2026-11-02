//! Closed-world thin callable effects remain inspectable on workers (issue #661).
mod support;
use support::{assert_stdout, check_diagnostic};

#[test]
fn workers_map_errors_with_variant_values_in_both_mapping_forms() {
    assert_stdout(
        r#"
import std.bytes
from std.error import map_error
enum Domain:
    Allocation(AllocError)
    Parse(ParseError)
def worker() -> Result[i64, Domain]:
    buffer = std.bytes.empty().map_error(Domain.Allocation)?
    drop(buffer)
    value = map_error(i64.parse("41"), Domain.Parse)?
    Ok(value + 1)
def main() -> Result[(), IoError | Domain | SpawnError[()]]:
    with spawn(worker).drop_job()? as task:
        print(task.join()?)?
    Ok(())
"#,
        "42\n",
    );
}

#[test]
fn callable_targets_survive_branches_aggregates_forwarders_and_worker_results() {
    assert_stdout(
        r#"
class Handler:
    callback: Callable[[i64], i64]
def add(value: i64) -> i64:
    value + 1
def twice(value: i64) -> i64:
    value * 2
def choose(flag: bool) -> Callable[[i64], i64]:
    add if flag else twice
def forward(callback: Callable[[i64], i64]) -> Callable[[i64], i64]:
    callback
def worker(handler: &Handler) -> i64:
    forward(handler.callback)(20)
def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    callbacks = [choose(True), choose(False)]?
    for callback in &callbacks:
        handler = Handler(*callback)
        with spawn(worker, &handler).drop_job()? as task:
            print(task.join())?
    with spawn(choose, False).drop_job()? as task:
        print(task.join()(7))?
    sender, receiver = channel[Callable[[i64], i64]](1)?
    sender.send(add).unwrap()
    print(receiver.recv().unwrap()(9))?
    Ok(())
"#,
        "21\n40\n14\n10\n",
    );
}

#[test]
fn callable_signature_cycles_through_returned_classes_terminate() {
    assert_stdout(
        r#"
class Node:
    callback: Callable[[], Node]
def make() -> Node:
    Node(make)
def worker(node: Node) -> i64:
    next = node.callback()
    drop(next)
    7
def main() -> Result[(), IoError | SpawnError[()]]:
    with spawn(worker, make()).drop_job()? as task:
        print(task.join())?
    Ok(())
"#,
        "7\n",
    );
}

#[test]
fn all_targets_are_checked_including_main_thread_only_addresses() {
    let declarations = r#"
import std.env
def safe() -> i64:
    7
def unsafe_callback() -> i64:
    path = std.env.current_dir().unwrap()
    std.env.set_current_dir(&path).unwrap()
    9
def forward(callback: Callable[[], i64]) -> Callable[[], i64]:
    callback
def worker(callback: Callable[[], i64]) -> i64:
    callback()
"#;
    for setup in [
        "callback = unsafe_callback if True else safe",
        "callbacks = [safe, unsafe_callback].unwrap()\n    callback = callbacks[0]",
        "callback = forward(unsafe_callback)",
        "unrelated = unsafe_callback\n    callback = safe",
    ] {
        let source = format!(
            "{declarations}\ndef main() -> ():\n    {setup}\n    with spawn(worker, callback).unwrap():\n        pass\n"
        );
        let error = check_diagnostic(&source);
        assert!(
            error.contains("changing the working directory"),
            "{source}\n{error}"
        );
        assert!(error.contains("unsafe_callback"), "{error}");
    }
}

#[test]
fn boxed_callables_and_absent_target_sets_remain_uncertified() {
    let error = check_diagnostic("sender, receiver = channel[Callable[[], i64]](1).unwrap()");
    assert!(error.contains("no statically known targets"), "{error}");
    let error = check_diagnostic(
        "def safe() -> i64:\n    7\ndef worker(callback: BoxedCallable[[], i64]) -> i64:\n    callback()\nwith spawn(worker, BoxedCallable(safe).unwrap()).unwrap():\n    pass",
    );
    assert!(
        error.contains("boxed callable captures and effects"),
        "{error}"
    );
}

#[test]
fn foreign_targets_are_inspected_and_foreign_abis_cannot_forge_callable_values() {
    let workspace = tempfile::tempdir().unwrap();
    let native = workspace.path().join("native.plentyi");
    let main = workspace.path().join("main.plenty");
    std::fs::write(&native, "pub extern def raw() -> i64 = \"external_raw\"\n").unwrap();
    std::fs::write(&main, "from native import raw\ndef worker(callback: Callable[[], i64]) -> i64:\n    callback()\ndef main() -> ():\n    with spawn(worker, raw).unwrap():\n        pass\n").unwrap();
    let error = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(
        error.contains("foreign calls have no worker-thread effect contract"),
        "{error}"
    );
    for (declarations, expected) in [
        (
            "pub extern def raw() -> Callable[[], i64] = \"external_raw\"\n",
            "no supported C ABI return representation",
        ),
        (
            "pub class Holder:\n    callback: Callable[[], i64]\npub extern def raw() -> Holder = \"external_raw\"\n",
            "no supported C ABI return representation",
        ),
    ] {
        std::fs::write(&native, declarations).unwrap();
        let error = plenty::check_file(&main, None).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}
