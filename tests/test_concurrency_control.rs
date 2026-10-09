mod support;

fn runs(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("concurrency-control-test");
    support::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    let output = std::process::Command::new("timeout")
        .arg("15s")
        .arg(executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "status {:?}; stdout {}; stderr {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn shared_cancellation_wakes_running_work_without_forcing_termination() {
    runs(
        r#"
def work(stop: CancellationToken, ready: Sender[i64]) -> i64:
    ready.send(1).unwrap()
    stop.wait()
    if stop.is_cancelled():
        return 42
    0
token = CancellationToken().unwrap()
temporary = token.share()
drop(temporary)
print(token.is_cancelled()).unwrap()
print(token.wait_timeout(0)).unwrap()
print(token.wait_timeout(1)).unwrap()
ready, started = channel[i64](1).unwrap()
with ThreadPoolExecutor(1, 1).unwrap() as pool:
    future = pool.submit(work, token.share(), ready).unwrap()
    started.recv().unwrap()
    print(future.cancel()).unwrap()
    token.cancel()
    token.cancel()
    print(future.result().unwrap()).unwrap()
print(token.wait_timeout(0)).unwrap()
token.wait()
print(token.is_cancelled()).unwrap()
"#,
        "False\nFalse\nFalse\nFalse\n42\nTrue\nTrue\n",
    );
}

#[test]
fn tokens_move_through_ordinary_owners_and_scoped_threads() {
    runs(
        r#"
class Stop:
    token: CancellationToken
token = CancellationToken().unwrap()
stored = [Stop(token.share()).unwrap()].unwrap()
sender, receiver = channel[CancellationToken](1).unwrap()
sender.send(stored[0].token.share()).unwrap()
worker_token = receiver.recv().unwrap()
job = def once [worker_token]() -> bool:
    worker_token.wait()
    worker_token.is_cancelled()
with spawn(job).unwrap() as task:
    token.cancel()
    print(task.join()).unwrap()
print(stored[0].token.is_cancelled()).unwrap()
"#,
        "True\nTrue\n",
    );
}

#[test]
fn future_timeout_preserves_its_result_and_recognizes_cancellation() {
    runs(
        r#"
def block(ready: Sender[i64], gate: Receiver[i64]) -> i64:
    ready.send(1).unwrap()
    gate.recv().unwrap()
def value() -> i64:
    9
ready, started = channel[i64](1).unwrap()
signal, gate = channel[i64](1).unwrap()
with ThreadPoolExecutor(1, 1).unwrap() as pool:
    running = pool.submit(block, ready, gate).unwrap()
    started.recv().unwrap()
    print(running.wait_timeout(0)).unwrap()
    print(running.wait_timeout(1)).unwrap()
    pending = pool.submit(value).unwrap()
    print(pending.cancel()).unwrap()
    print(pending.wait_timeout(0)).unwrap()
    print(pending.result()).unwrap()
    signal.send(7).unwrap()
    print(running.wait_timeout(10000)).unwrap()
    print(running.wait_timeout(18446744073709551615u64)).unwrap()
    print(running.result().unwrap()).unwrap()
"#,
        "False\nFalse\nTrue\nTrue\nResult[i64, FutureError].Err(FutureError.Cancelled)\nTrue\nTrue\n7\n",
    );
}

#[test]
fn timed_channel_operations_preserve_unsent_values_and_prioritize_readiness() {
    runs(
        r#"
sender, receiver = channel[list[i64]](1).unwrap()
print(receiver.recv_timeout(0)).unwrap()
sender.send_timeout([1].unwrap(), 0).unwrap()
match sender.send_timeout([2, 3].unwrap(), 1):
    case Ok(_):
        print("wrong").unwrap()
    case Err(error):
        match error:
            case SendTimeoutError[list[i64]].TimedOut(value):
                print(value).unwrap()
            case SendTimeoutError[list[i64]].Disconnected(value):
                print("wrong").unwrap()
print(receiver.recv_timeout(0).unwrap()).unwrap()
sender.send_timeout([4].unwrap(), 0).unwrap()
drop(sender)
print(receiver.recv_timeout(0).unwrap()).unwrap()
print(receiver.recv_timeout(0)).unwrap()
other, destination = channel[i64](1).unwrap()
drop(destination)
print(other.send_timeout(42, 0)).unwrap()
"#,
        "Result[list[i64], RecvTimeoutError].Err(RecvTimeoutError.TimedOut)\n[2, 3]\n[1]\n[4]\nResult[list[i64], RecvTimeoutError].Err(RecvTimeoutError.Disconnected)\nResult[(), SendTimeoutError[i64]].Err(SendTimeoutError[i64].Disconnected(42))\n",
    );
}

#[test]
fn timed_send_and_receive_make_progress_between_threads() {
    runs(
        r#"
sender, receiver = channel[range[i64]](1).unwrap()
sender.send(range(3)).unwrap()
job = def once [sender]() -> Result[(), SendTimeoutError[range[i64]]]:
    sender.send_timeout(range(5, 8), 10000)
with spawn(job).unwrap() as task:
    print(receiver.recv_timeout(10000).unwrap()[2]).unwrap()
    print(receiver.recv_timeout(10000).unwrap()[2]).unwrap()
    task.join().unwrap()
"#,
        "2\n7\n",
    );
}

#[test]
fn selection_transfers_one_typed_message_and_ignores_a_drained_disconnected_peer() {
    runs(
        r#"
def describe(value: Selected[i64, str]) -> str:
    match value:
        case Selected[i64, str].First(n):
            if n == 7:
                return "number"
            "wrong"
        case Selected[i64, str].Second(text):
            text
left, first = channel[i64](2).unwrap()
right, second = channel[str](1).unwrap()
print(select_recv_nowait(&first, &second)).unwrap()
print(select_recv_timeout(&first, &second, 0)).unwrap()
left.send(7).unwrap()
right.send("text").unwrap()
print(describe(select_recv(&first, &second).unwrap())).unwrap()
drop(left)
print(describe(select_recv_timeout(&first, &second, 0).unwrap())).unwrap()
print(select_recv_nowait(&first, &second)).unwrap()
drop(right)
print(select_recv(&first, &second)).unwrap()
"#,
        "Result[Selected[i64, str], SelectError].Err(SelectError.Empty)\nResult[Selected[i64, str], SelectError].Err(SelectError.TimedOut)\nnumber\ntext\nResult[Selected[i64, str], SelectError].Err(SelectError.Empty)\nResult[Selected[i64, str], SelectError].Err(SelectError.Disconnected)\n",
    );
}

#[test]
fn selection_waits_for_owned_inline_values_and_accepts_shared_aliases() {
    runs(
        r#"
left, first = channel[range[i64]](1).unwrap()
right, second = channel[list[i64]](1).unwrap()
job = def once [right]() -> Result[(), Failure]:
    right.send([8, 9]?)?
    Ok(())
with spawn(job).unwrap() as task:
    match select_recv_timeout(&first, &second, 10000).unwrap():
        case Selected[range[i64], list[i64]].First(_):
            print("wrong").unwrap()
        case Selected[range[i64], list[i64]].Second(values):
            print(values).unwrap()
    task.join().unwrap()
alias = first.share()
left.send(range(10, 13)).unwrap()
match select_recv(&first, &alias).unwrap():
    case Selected[range[i64], range[i64]].First(values):
        print(values[2]).unwrap()
    case Selected[range[i64], range[i64]].Second(_):
        print("wrong").unwrap()
print(first.recv_nowait()).unwrap()
"#,
        "[8, 9]\n12\nResult[range, RecvError].Err(RecvError.Empty)\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn cancellation_and_selection_have_no_per_operation_allocation() {
    runs(
        r#"
def selected_number(value: Selected[range[i64], i64]) -> i64:
    match value:
        case Selected[range[i64], i64].First(values):
            values[2]
        case Selected[range[i64], i64].Second(value):
            value
print("__test_fail_allocations_after_0__").unwrap()
failed = CancellationToken()
print("__test_restore_allocations__").unwrap()
match failed:
    case Ok(_):
        print("wrong").unwrap()
    case Err(error):
        print(error).unwrap()
token = CancellationToken().unwrap()
left, first = channel[range[i64]](1).unwrap()
right, second = channel[i64](1).unwrap()
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
other = token.share()
other.cancel()
stopped = token.wait_timeout(0)
left.send_timeout(range(20, 23), 0).unwrap()
selected = select_recv_timeout(&first, &second, 0).unwrap()
answer = selected_number(selected)
empty = second.recv_timeout(0)
drop(other)
drop(token)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(stopped).unwrap()
print(answer).unwrap()
print(empty).unwrap()
"#,
        "__test_fail_allocations_after_0__\n__test_restore_allocations__\nAllocError.OutOfMemory\n__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\nTrue\n22\nResult[i64, RecvTimeoutError].Err(RecvTimeoutError.TimedOut)\n",
    );
}

#[test]
fn selection_types_infer_through_generic_functions_and_forwarded_borrows() {
    runs(
        r#"
def receive[A, B](first: &Receiver[A], second: &Receiver[B]) -> Result[Selected[A, B], SelectError]:
    select_recv_nowait(first, second)
def which[A, B](value: Selected[A, B]) -> i64:
    match value:
        case Selected[A, B].First(_):
            1
        case Selected[A, B].Second(_):
            2
def recover[T](error: SendTimeoutError[T]) -> T:
    match error:
        case SendTimeoutError[T].Disconnected(value):
            value
        case SendTimeoutError[T].TimedOut(value):
            value
left, first = channel[range[i64]](1).unwrap()
right, second = channel[str](1).unwrap()
left.send(range(3)).unwrap()
print(which(receive(&first, &second).unwrap())).unwrap()
right.send("hello").unwrap()
print(which(receive(&first, &second).unwrap())).unwrap()
left.send(range(3)).unwrap()
match left.send_timeout(range(7, 10), 0):
    case Ok(_):
        print("wrong").unwrap()
    case Err(error):
        print(recover(error)[2]).unwrap()
"#,
        "1\n2\n9\n",
    );
}

#[test]
fn new_operations_enforce_affine_handles_and_typed_arguments() {
    for (source, expected) in [
        (
            "token = CancellationToken().unwrap()\nother = copy(token)\n",
            "cannot be copied",
        ),
        (
            "token = CancellationToken().unwrap()\nprint(token == token).unwrap()\n",
            "equality",
        ),
        (
            "token = CancellationToken().unwrap()\nmoved = token\ntoken.cancel()\n",
            "moved",
        ),
        (
            "token = CancellationToken().unwrap()\nbudget = 1.0f64\ntoken.wait_timeout(budget)\n",
            "u64",
        ),
        (
            "sender, receiver = channel[i64](1).unwrap()\nselect_recv(&sender, &receiver)\n",
            "Receiver",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}
