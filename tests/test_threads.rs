mod support;
use support::assert_stdout as runs;

#[test]
fn named_workers_move_owned_inputs_and_keep_borrowed_loans_scoped() {
    runs(
        r#"
class Item:
    value: i64
    def __del__(self) -> ():
        print(self.value).unwrap()
def worker(item: Item, values: list[i64], output: &mut i64) -> list[i64]:
    *output = item.value + len(values)
    values
def apply[F: Callable[[], i64]](job: F) -> i64:
    job()
def main() -> Result[(), AllocError | IoError | ThreadError]:
    mut output = 0
    with spawn(worker, Item(7), [1, 2]?, &mut output)? as task:
        print(task.join())?
    print(output)?
    number = 42
    job = def [number]() -> i64:
        number
    with spawn(apply, job)? as task:
        print(task.join())?
    Ok(())
"#,
        "7\n[1, 2]\n9\n42\n",
    );
    let error = support::check_source(
        r#"
def worker(values: list[i64]) -> i64:
    len(values)
values = [1].unwrap()
with spawn(worker, values).unwrap():
    pass
print(values).unwrap()
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("moved"), "{error}");
    let error = support::check_source(
        r#"
def worker(values: list[i64], output: &mut i64) -> ():
    *output = len(values)
mut output = 0
with spawn(worker, [1].unwrap(), &mut output).unwrap():
    print(output).unwrap()
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("conflicting borrow"), "{error}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_named_start_drops_each_moved_argument_once_without_running_worker() {
    runs(r#"
class Item:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
def worker(item: Item, values: list[Item], count: &mut i64) -> ():
    *count = *count + 1
    print("unexpected worker").unwrap()
def attempt(count: &mut i64) -> Result[(), ThreadError]:
    with spawn(worker, Item("item"), [Item("nested")].unwrap(), count)?:
        pass
    Ok(())
def apply[F: Callable[[], i64]](job: F) -> i64:
    job()
def attempt_closure() -> Result[(), ThreadError]:
    item = Item("capture")
    job = def [item]() -> i64:
        len(item.label)
    with spawn(apply, job)?:
        pass
    Ok(())
def main() -> Result[(), AllocError | IoError]:
    mut count = 0
    print("__test_fail_thread_starts__")?
    print(str.repr(attempt(&mut count))?)?
    print(str.repr(attempt_closure())?)?
    print("__test_restore_thread_starts__")?
    print(count)?
    Ok(())
"#, "__test_fail_thread_starts__\nnested\nitem\nResult[(), ThreadError].Err(ThreadError.System(11))\ncapture\nResult[(), ThreadError].Err(ThreadError.System(11))\n__test_restore_thread_starts__\n0\n");
}

#[test]
fn named_workers_join_once_and_transfer_owned_results() {
    runs(
        r#"
def squares(n: i64) -> Result[list[i64], AllocError]:
    [i * i for i in range(n)]
def main() -> Result[(), AllocError | IoError | ThreadError]:
    with spawn(squares, 4)? as first, spawn(squares, 3)? as second:
        print(first.join()?)?
        print(second.join()?)?
    Ok(())
"#,
        "[0, 1, 4, 9]\n[0, 1, 4]\n",
    );
}

#[test]
fn shared_and_exclusive_borrows_remain_live_until_scope_exit() {
    runs(
        r#"
def sum_values(values: &list[i64]) -> i64:
    mut total = 0
    for n in values:
        total = total + n
    total
def bump(value: &mut i64) -> ():
    *value = *value + 1
def main() -> Result[(), AllocError | IoError | ThreadError]:
    values = [2, 3, 4]?
    mut n = 7
    with spawn(sum_values, &values)? as a, spawn(sum_values, &values)? as b:
        print(a.join() + b.join())?
        with spawn(bump, &mut n)?:
            print(len(values))?
    print(n)?
    Ok(())
"#,
        "18\n3\n8\n",
    );
}

#[test]
fn borrowed_closures_keep_capture_dependencies_alive() {
    runs(
        r#"
def main() -> Result[(), AllocError | IoError | ThreadError]:
    mut values = [1, 2]?
    mut job = def [&mut values]() -> Result[i64, AllocError]:
        values.append(3)?
        Ok(len(values))
    with spawn(&mut job)? as task:
        print(task.join()?)?
    print(values)?
    Ok(())
"#,
        "3\n[1, 2, 3]\n",
    );
}

#[test]
fn race_diagnostics_point_at_the_use_and_the_spawn_scope() {
    let header = "def worker(value: &mut i64) -> i64:\n    *value = 9\n    *value\ndef main() -> Result[(), IoError | ThreadError]:\n    mut value = 1\n";
    // The thread holds its borrow until the scoped join, even after an
    // explicit join, so every note names the `with` block and not the join.
    let scoped = "\n  6:29: note: the exclusive borrow of `value` starts here\n  6:10: note: the borrow is held until the end of this `with` block";
    for (body, primary) in [
        ("with spawn(worker, &mut value)? as task:\n        print(value)?", "7:15: conflicting borrow: cannot read `value` while it is exclusively borrowed"),
        ("with spawn(worker, &mut value)? as task:\n        value = 3", "7:9: conflicting borrow: cannot assign to `value` while it is exclusively borrowed"),
        ("with spawn(worker, &mut value)? as task:\n        print(task.join())?\n        print(value)?", "8:15: conflicting borrow: cannot read `value` while it is exclusively borrowed"),
        ("with spawn(worker, &mut value)? as task:\n        with spawn(worker, &mut value)? as other:\n            pass", "7:33: conflicting borrow: cannot modify or exclusively borrow `value` while it is exclusively borrowed"),
    ] {
        let source = format!("{header}    {body}\n    Ok(())\n");
        let error = plenty::check_source(&source).unwrap_err().to_string();
        assert_eq!(error, format!("{primary}{scoped}"), "{source}");
    }
    let source = format!("{header}    with spawn(worker, &mut value)? as task:\n        print(task.join())?\n        print(task.join())?\n    Ok(())\n");
    let error = plenty::check_source(&source).unwrap_err().to_string();
    assert_eq!(
        error,
        "8:15: use of moved binding `task`\n  7:15: note: `task` is moved here"
    );
    // The borrow ends with the block, not with the enclosing function.
    let source = format!("{header}    with spawn(worker, &mut value)? as task:\n        pass\n    value = 3\n    print(value)?\n    Ok(())\n");
    if let Err(error) = plenty::check_source(&source) {
        panic!("{source}\n{error}");
    }
}

#[test]
fn rejects_races_escaping_handles_and_double_joins() {
    let header = "def worker(value: &mut i64) -> i64:\n    *value = 9\n    *value\ndef main() -> Result[(), IoError | ThreadError]:\n    mut value = 1\n";
    for (body, expected) in [
        ("with spawn(worker, &mut value)? as task:\n        print(value)?", "conflicting borrow"),
        ("with spawn(worker, &mut value)? as task:\n        value = 3", "conflicting borrow"),
        ("with spawn(worker, &mut value)? as task:\n        with spawn(worker, &mut value)? as other:\n            pass", "conflicting borrow"),
        ("with spawn(worker, &mut value)? as task:\n        print(task.join())?\n        print(task.join())?", "moved"),
        ("with spawn(worker, &mut value)? as task:\n        escaped = task", "cannot escape"),
        ("with spawn(worker, &mut value)? as task:\n        escaped = &task", "cannot be borrowed"),
        ("with spawn(worker, &mut value)? as task:\n        print(task)?", "cannot be borrowed"),
    ] {
        let source = format!("{header}    {body}\n    Ok(())\n");
        let error = plenty::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(expected), "{source}\n{error}");
    }
}

#[test]
fn generics_recursive_owners_and_projected_references_cross_thread_boundary() {
    runs(
        r#"
class Node:
    value: i64
    next: Option[Box[Node]]
def total(node: &Node) -> i64:
    match &node.next:
        case Some(rest):
            node.value + total(rest)
        case Nothing:
            node.value
def add[T: IntType](value: &mut T, amount: T) -> ():
    *value = *value + amount
def main() -> Result[(), AllocError | IoError | ThreadError]:
    node = Node(2, Some(Box(Node(3, Nothing))?))
    with spawn(total, &node)? as task:
        print(task.join())?
    mut nested: Option[Result[u8, i32]] = Some(Ok(4u8))
    match &mut nested:
        case Some(result):
            match result:
                case Ok(number):
                    with spawn(add, number, 3u8)?:
                        pass
                case Err(_):
                    pass
        case Nothing:
            pass
    print(nested)?
    Ok(())
"#,
        "5\nOption[Result[u8, i32]].Some(Result[u8, i32].Ok(7))\n",
    );
}

#[test]
fn results_keep_application_errors_and_native_layouts() {
    runs(
        r#"
def fail() -> Result[i64, str]:
    Err("application")
def span() -> Option[range[u8]]:
    Some(range[u8](4))
def float_value(n: f32) -> f32:
    n + 0.5f32
def nested(n: i64) -> Result[i64, ThreadError]:
    if n == 0:
        return Ok(1)
    with spawn(nested, n - 1)? as task:
        return task.join()
def main() -> Result[(), IoError | ThreadError]:
    with spawn(fail)? as task:
        print(str.repr(task.join()).unwrap())?
    with spawn(span)? as task:
        saved = task.join()
        print(saved)?
    with spawn(float_value, 1.5f32)? as task:
        print(task.join())?
    with spawn(nested, 3)? as task:
        print(task.join()?)?
    Ok(())
"#,
        "Result[i64, str].Err(\"application\")\nOption[range[u8]].Some(range(0, 4, 1))\n2.0\n1\n",
    );
}

#[test]
fn all_normal_exits_join_before_reclaiming_borrowed_storage() {
    runs(
        r#"
def update(n: &mut i64) -> ():
    *n = *n + 1
def early(n: &mut i64) -> Result[(), ThreadError]:
    with spawn(update, n)?:
        return Ok(())
def failed(n: &mut i64) -> Result[(), ThreadError]:
    with spawn(update, n)?:
        error: Result[(), ThreadError] = Err(ThreadError.System(1))
        error?
    Ok(())
def main() -> Result[(), IoError | ThreadError]:
    mut n = 0
    early(&mut n)?
    print(str.repr(failed(&mut n)).unwrap())?
    for i in range(5):
        with spawn(update, &mut n)?:
            if i == 2:
                break
            continue
    print(n)?
    Ok(())
"#,
        "Result[(), ThreadError].Err(ThreadError.System(1))\n5\n",
    );
}

#[test]
fn rejects_worker_effects_in_helpers_destructors_and_stored_data() {
    for (source, expected) in [
        (
            r#"
def seven() -> i64:
    7
class Resource:
    value: i64
    def __del__(self: &mut Resource) -> ():
        callback: Callable[[], i64] = seven
        callback()
        pass
def worker(value: &Resource) -> i64:
    value.value
def main() -> Result[(), ThreadError]:
    value = Resource(1)
    with spawn(worker, &value)?:
        pass
    Ok(())
"#,
            "indirect callable effects",
        ),
        (
            r#"
def identity(n: i64) -> i64:
    n
def worker(callback: Callable[[i64], i64]) -> i64:
    callback(1)
def main() -> Result[(), ThreadError]:
    with spawn(worker, identity)?:
        pass
    Ok(())
"#,
            "indirect callable effects",
        ),
        (
            r#"
def worker(data: &i64) -> &i64:
    data
def main() -> Result[(), ThreadError]:
    data = 1
    with spawn(worker, &data)?:
        pass
    Ok(())
"#,
            "worker results cannot contain",
        ),
    ] {
        let error = plenty::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{source}\n{error}");
    }
}

/// A rejection reached through a generic function names that function by its
/// definition and lists the functions between the worker and it (issue #146).
#[test]
fn worker_rejections_name_generic_functions_and_the_call_path() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("main.plenty");
    std::fs::write(
        &path,
        r#"
from std.error import map_error
def wrap(cause: ParseError) -> str:
    return "bad"
class Parser:
    pub def parse(self, text: &str) -> Result[i64, str]:
        map_error(i64.parse(text), wrap)
def outer(text: str) -> Result[(), str]:
    drop(Parser().parse(&text))
    Ok(())
def work(text: str) -> Result[(), str]:
    outer(text)
def main() -> Result[(), ThreadError]:
    with spawn(work, "12")?:
        pass
    Ok(())
"#,
    )
    .unwrap();
    let error = plenty::check_file(&path, None).unwrap_err().to_string();
    assert!(
        error.contains(
            "worker `work` reaches the generic function at std/error.plenty:7:5 \
             through `outer` -> `Parser.parse`: indirect callable effects"
        ),
        "{error}"
    );
}

/// A type rejected in a callee's signature or locals names that callee and
/// the call path, like an inspect-time rejection (issue #185). A type in the
/// worker's own body is named by the worker alone.
#[test]
fn worker_type_rejections_name_the_callee_and_the_call_path() {
    for (source, expected) in [
        (
            r#"
def values() -> Generator[i64]:
    yield 1
def helper() -> i64:
    numbers = values()
    7
def middle() -> i64:
    helper()
def work() -> i64:
    middle()
def main() -> Result[(), ThreadError]:
    with spawn(work)?:
        pass
    Ok(())
"#,
            "worker `work` reaches `helper` through `middle`: \
             generator worker eligibility is not implemented yet",
        ),
        (
            r#"
def values() -> Generator[i64]:
    yield 1
def work() -> i64:
    numbers = values()
    7
def main() -> Result[(), ThreadError]:
    with spawn(work)?:
        pass
    Ok(())
"#,
            "worker `work`: generator worker eligibility is not implemented yet",
        ),
    ] {
        let error = plenty::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{source}\n{error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_start_preserves_borrowed_job_and_joins_prior_tasks() {
    runs(r#"
def update(n: &mut i64) -> ():
    *n = *n + 10
def attempt(a: &mut i64, b: &mut i64) -> Result[(), ThreadError]:
    with spawn(update, a)?, spawn(update, b)?:
        pass
    Ok(())
def main() -> Result[(), IoError]:
    mut a = 1
    mut b = 2
    print("__test_one_thread_start__")?
    print(str.repr(attempt(&mut a, &mut b)).unwrap())?
    print("__test_restore_thread_starts__")?
    print(a)?
    print(b)?
    print(str.repr(attempt(&mut a, &mut b)).unwrap())?
    print(a)?
    print(b)?
    Ok(())
"#, "__test_one_thread_start__\nResult[(), ThreadError].Err(ThreadError.System(11))\n__test_restore_thread_starts__\n11\n2\nResult[(), ThreadError].Ok(())\n21\n12\n");
}

#[test]
fn imported_helpers_cannot_hide_uncertified_foreign_effects() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("native.plentyi"),
        "pub extern def foreign() -> i64 = \"external\"\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("helper.plenty"),
        "import native\npub def work() -> i64:\n    native.foreign()\n",
    )
    .unwrap();
    let path = root.path().join("main.plenty");
    std::fs::write(&path, "import helper\ndef main() -> Result[(), ThreadError]:\n    with spawn(helper.work)?:\n        pass\n    Ok(())\n").unwrap();
    let error = plenty::check_file(&path, None).unwrap_err().to_string();
    assert!(
        error.contains("foreign calls have no worker-thread effect contract"),
        "{error}"
    );
    assert!(error.contains("native.foreign"), "{error}");
}

#[test]
fn branch_joins_and_unclaimed_results_drop_exactly_once() {
    runs(
        r#"
class Resource:
    name: str
    def __del__(self: &mut Resource) -> ():
        print(self.name).unwrap()
def make(name: str) -> Result[Resource, AllocError]:
    Ok(Resource(name))
def run(join: bool) -> Result[(), AllocError | IoError | ThreadError]:
    with spawn(make, "finished")? as task:
        if join:
            resource = task.join()?
            print(resource.name)?
    Ok(())
def main() -> Result[(), AllocError | IoError | ThreadError]:
    run(True)?
    run(False)?
    with spawn(make, "discarded").unwrap():
        pass
    Ok(())
"#,
        "finished\nfinished\nfinished\ndiscarded\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn task_bookkeeping_does_not_allocate_and_failed_closure_is_reusable() {
    runs(r#"
def attempt(job: &mut Closure[[], i64]) -> Result[i64, ThreadError]:
    with spawn(&mut job)? as task:
        return Ok(task.join())
def bump(value: &mut i64) -> ():
    *value = *value + 1
def main() -> Result[(), IoError | ThreadError]:
    mut value = 0
    print("__test_begin_no_allocations__")?
    print("__test_fail_allocations_after_0__")?
    with spawn(bump, &mut value)?:
        pass
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(value)?
    mut job = def [&mut value]() -> i64:
        value = value + 10
        value
    print("__test_fail_thread_starts__")?
    print(str.repr(attempt(&mut job)).unwrap())?
    print("__test_restore_thread_starts__")?
    print(str.repr(attempt(&mut job)).unwrap())?
    print(value)?
    Ok(())
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n1\n__test_fail_thread_starts__\nResult[i64, ThreadError].Err(ThreadError.System(11))\n__test_restore_thread_starts__\nResult[i64, ThreadError].Ok(11)\n11\n");
}

#[test]
fn shared_closures_and_disjoint_mutable_fields_are_checked() {
    runs(
        r#"
class Pair:
    left: i64
    right: i64
def bump(n: &mut i64) -> ():
    *n = *n + 1
def main() -> Result[(), IoError | ThreadError]:
    text = "hello"
    job = def [&text]() -> str:
        text
    with spawn(&job)? as a, spawn(&job)? as b:
        print(a.join())?
        print(b.join())?
    mut pair = Pair(1, 2)
    with spawn(bump, &mut pair.left)?, spawn(bump, &mut pair.right)?:
        pass
    print(pair.left + pair.right)?
    Ok(())
"#,
        "hello\nhello\n5\n",
    );
}

#[test]
fn closure_loans_block_parent_access_until_exit_and_tasks_cannot_suspend() {
    for access in ["print(values)?", "values.append(3)?", "drop(job)"] {
        let source = format!(
            r#"
def main() -> Result[(), AllocError | IoError | ThreadError]:
    mut values = [1]?
    mut job = def [&mut values]() -> Result[(), AllocError]:
        values.append(2)
    with spawn(&mut job)? as task:
        {access}
    Ok(())
"#
        );
        let error = plenty::check_source(&source).unwrap_err().to_string();
        assert!(error.contains("conflicting borrow"), "{source}\n{error}");
    }
    let error = support::check_source(
        r#"
def work() -> i64:
    1
def suspended() -> Generator[i64]:
    with spawn(work).unwrap() as task:
        yield task.join()
pass
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("yield inside with"), "{error}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn worker_allocation_failure_is_local_and_preserves_parent_owners() {
    runs(r#"
def work() -> Result[list[i64], AllocError]:
    print("__test_fail_allocations_after_0__").unwrap()
    [1, 2, 3]
def main() -> Result[(), AllocError | IoError | ThreadError]:
    values = [7]?
    with spawn(work)? as task:
        print(str.repr(task.join()).unwrap())?
    print(values)?
    print([4, 5]?)?
    Ok(())
"#, "__test_fail_allocations_after_0__\nResult[list[i64], AllocError].Err(AllocError.OutOfMemory)\n[7]\n[4, 5]\n");
}

#[test]
fn task_cleanup_precedes_enclosing_context_exit() {
    runs(
        r#"
class Counter:
    value: i64
    def __enter__(self: &mut Counter) -> &mut Counter:
        &mut self
    def __exit__(self: &mut Counter) -> ():
        print(self.value).unwrap()
def bump(counter: &mut Counter) -> ():
    counter.value = counter.value + 1
def run() -> Result[(), ThreadError]:
    with Counter(5) as counter:
        with spawn(bump, counter)?:
            return Ok(())
def main() -> Result[(), ThreadError]:
    run()?
    Ok(())
"#,
        "6\n",
    );
}

#[test]
fn joins_reject_receivers_that_could_leave_a_sending_worker_blocked() {
    // Issue #12. A sender reaches the worker as an argument, a capture, a
    // field, a borrow, or a queued message; `main` starts on line 8.
    let workers = "class Outbox:
    sender: Sender[i64]
def owned(sender: Sender[i64]) -> ():
    sender.send(1).unwrap()
    sender.send(2).unwrap()
def lend(sender: &Sender[i64]) -> ():
    owned(sender.share())
";
    for start in [
        "spawn(owned, sender)",
        "spawn(def once [sender]() -> (): owned(sender))",
        "spawn(def once [outbox]() -> (): owned(outbox.sender.share()))",
        "spawn(lend, &sender)",
        "spawn(def once [replies]() -> (): owned(replies.recv().unwrap()))",
    ] {
        let source = format!(
            "{workers}def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    outbox = Outbox(sender.share())
    paths, replies = channel[Sender[i64]](1).unwrap()
    paths.send(sender.share()).unwrap()
    with {start}.unwrap():
        pass
"
        );
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(
            error.starts_with("13:10: leaving this `with` block joins a task whose worker holds `Sender[i64]` while `receiver` still holds `Receiver[i64]`"),
            "{start}: {error}"
        );
    }
    let joined = |body: &str| {
        support::check_source(&format!(
            "{workers}def idle() -> ():
    pass
def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
{body}"
        ))
    };
    // A worker without a sender never waits for this receiver.
    joined("    with spawn(idle).unwrap() as task:\n        task.join()\n    drop(sender)\n")
        .unwrap();
    // A task joined on every path is not joined again when the block ends.
    joined("    with spawn(owned, sender).unwrap() as task:\n        drop(receiver)\n        task.join()\n").unwrap();
    // Joined on one path only: the end of the block still joins on the other.
    let error = joined(
        "    with spawn(owned, sender).unwrap() as task:
        if len(\"x\") == 1:
            drop(receiver)
            task.join()
",
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.starts_with("12:10: leaving this `with` block joins")
            && error.contains("`receiver` holds `Receiver[i64]` on some paths"),
        "{error}"
    );
    // An inner task is joined first, while the outer block's receiver is alive.
    let error = joined(
        "    with spawn(idle).unwrap(), receiver as inbox:
        with spawn(owned, sender).unwrap():
            pass
",
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.starts_with("13:14: leaving this `with` block joins"),
        "{error}"
    );
}

#[test]
fn start_errors_widen_into_declared_error_sums() {
    runs(
        r#"
def worker() -> ():
    pass
def add(n: &mut i64) -> i64:
    *n = *n + 1
    *n
def main() -> Result[(), ThreadError | IoError]:
    mut n = 1
    with spawn(worker)?:
        print("started")?
    mut job = def [mut n]() -> i64:
        n + 5
    with spawn(&mut job)? as a, spawn(add, &mut n)? as b:
        print(a.join())?
        print(b.join())?
    Ok(())
"#,
        "started\n6\n2\n",
    );
    let error = support::check_source(
        "def worker() -> ():\n    pass\ndef main() -> Result[(), IoError | AllocError]:\n    with spawn(worker)?:\n        pass\n    Ok(())\n",
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains(
            "spawn propagation requires a Result whose error accepts ThreadError; add it to the declared error, such as Result[_, AllocError | IoError | ThreadError]"
        ),
        "{error}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_start_in_error_sum_drops_arguments_and_joins_prior_tasks() {
    runs(r#"
class Item:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
def update(n: &mut i64) -> ():
    *n = *n + 10
def take(item: Item, values: list[Item]) -> ():
    print("unexpected worker").unwrap()
def attempt(a: &mut i64, b: &mut i64) -> Result[(), ThreadError | IoError]:
    with spawn(update, a)?, spawn(take, Item("item"), [Item("nested")].unwrap())?, spawn(update, b)?:
        print("unexpected body")?
    Ok(())
def main() -> Result[(), AllocError | IoError]:
    mut a = 1
    mut b = 2
    print("__test_one_thread_start__")?
    match attempt(&mut a, &mut b):
        case Err(error):
            match error:
                case start: ThreadError:
                    print(str.repr(start)?)?
                case _:
                    print("unexpected error")?
        case Ok(_):
            print("unexpected start")?
    print("__test_restore_thread_starts__")?
    print(a)?
    print(b)?
    Ok(())
"#, "__test_one_thread_start__\nnested\nitem\nThreadError.System(11)\n__test_restore_thread_starts__\n11\n2\n");
}
