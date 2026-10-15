mod support;

fn runs(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("channel-test");
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
fn bounded_queue_preserves_fifo_errors_and_failed_message_ownership() {
    runs(r#"
sender, receiver = channel[list[i64]](1).unwrap()
print(str.repr(receiver.recv_nowait()).unwrap()).unwrap()
sender.send([1, 2].unwrap()).unwrap()
match sender.send_nowait([3].unwrap()):
    case Ok(_):
        print("wrong").unwrap()
    case Err(error):
        match error:
            case SendError.Full(value):
                print(value).unwrap()
            case SendError.Disconnected(value):
                print("wrong").unwrap()
print(receiver.recv().unwrap()).unwrap()
sender.send([4].unwrap()).unwrap()
drop(sender)
print(receiver.recv().unwrap()).unwrap()
print(str.repr(receiver.recv()).unwrap()).unwrap()
other, destination = channel[i64](1).unwrap()
drop(destination)
print(str.repr(other.send(42)).unwrap()).unwrap()
print(str.repr(other.send_nowait(43)).unwrap()).unwrap()
"#, "Result[list[i64], RecvError].Err(RecvError.Empty)\n[3]\n[1, 2]\n[4]\nResult[list[i64], RecvError].Err(RecvError.Disconnected)\nResult[(), SendError[i64]].Err(SendError[i64].Disconnected(42))\nResult[(), SendError[i64]].Err(SendError[i64].Disconnected(43))\n");
}

#[test]
fn consuming_producer_and_shared_receivers_transfer_all_messages() {
    runs(
        r#"
def produce(sender: Sender[i64], start: i64) -> Result[(), Failure]:
    for i in range(start, start + 100):
        sender.send(i)?
    Ok(())
def collect(receiver: Receiver[i64]) -> i64:
    mut total = 0
    while True:
        match receiver.recv():
            case Ok(n):
                total = total + n
            case Err(_):
                return total
    total
sender, receiver = channel[i64](2).unwrap()
second_sender = sender.share()
second_receiver = receiver.share()
first = def once [sender]() -> Result[(), Failure]:
    produce(sender, 0)
second = def once [second_sender]() -> Result[(), Failure]:
    produce(second_sender, 100)
consumer = def once [second_receiver]() -> i64:
    collect(second_receiver)
with spawn(first).unwrap() as a, spawn(second).unwrap() as b, spawn(consumer).unwrap() as c:
    total = collect(receiver)
    a.join().unwrap()
    b.join().unwrap()
    print(total + c.join()).unwrap()
"#,
        "19900\n",
    );
}

#[test]
fn last_endpoint_drop_wakes_blocked_peers() {
    runs(r#"
sender, receiver = channel[i64](1).unwrap()
sender.send(1).unwrap()
job = def once [sender]() -> Result[(), SendError[i64]]:
    sender.send(2)
with spawn(job).unwrap() as task:
    drop(receiver)
    print(str.repr(task.join()).unwrap()).unwrap()
other, destination = channel[i64](1).unwrap()
waiting = def once [destination]() -> Result[i64, RecvError]:
    destination.recv()
with spawn(waiting).unwrap() as task:
    drop(other)
    print(str.repr(task.join()).unwrap()).unwrap()
"#, "Result[(), SendError[i64]].Err(SendError[i64].Disconnected(2))\nResult[i64, RecvError].Err(RecvError.Disconnected)\n");
}

#[test]
fn channel_generics_and_stored_handles_preserve_explicit_sharing() {
    runs(r#"
class Mailbox[T]:
    sender: Sender[T]
def deliver[T](sender: &Sender[T], message: T) -> Result[(), SendError[T]]:
    sender.send(message)
sender, receiver = channel[str](2).unwrap()
mailbox = Mailbox(sender)
other = mailbox.sender.share()
deliver(&other, "hello").unwrap()
drop(other)
print(receiver.recv().unwrap()).unwrap()
print(str.repr(receiver.recv_nowait()).unwrap()).unwrap()
drop(mailbox)
print(str.repr(receiver.recv()).unwrap()).unwrap()
"#, "hello\nResult[str, RecvError].Err(RecvError.Empty)\nResult[str, RecvError].Err(RecvError.Disconnected)\n");
}

#[test]
fn channel_messages_reject_uncertified_effects_and_endpoint_copy_or_equality() {
    for (source, expected) in [
        ("sender, receiver = channel[Callable[[], i64]](1).unwrap()\n", "indirect callable"),
        ("sender, receiver = channel[&i64](1).unwrap()\n", "wholly owned"),
        ("sender, receiver = channel[i64](1).unwrap()\nother = copy(sender)\n", "cannot be copied"),
        ("sender, receiver = channel[i64](1).unwrap()\nprint(sender == sender).unwrap()\n", "equality"),
        ("sender, receiver = channel[i64](1).unwrap()\nvalues = [sender].unwrap()\nprint(values == values).unwrap()\n", "equality"),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
    // Native handles are owned values; see design/facilities/03-concurrency/effects.md.
    support::check_source("sender, receiver = channel[File](1).unwrap()\n").unwrap();
}

#[cfg(feature = "runtime-checks")]
#[test]
fn construction_failure_is_recoverable_and_queue_operations_do_not_allocate() {
    runs(r#"
print(str.repr(channel[i64](0)).unwrap()).unwrap()
print(str.repr(channel[i64](18446744073709551615u64)).unwrap()).unwrap()
print("__test_fail_allocations_after_0__").unwrap()
first = channel[i64](2)
print("__test_restore_allocations__").unwrap()
print(str.repr(first).unwrap()).unwrap()
# The endpoint pair is inline; only the shared queue allocates.
print("__test_fail_allocations_after_1__").unwrap()
second = channel[i64](2)
print("__test_restore_allocations__").unwrap()
print(str.repr(second).unwrap()).unwrap()
sender, receiver = channel[range[i64]](1).unwrap()
span = range(10, 15)
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
other = sender.share()
sender.send(span).unwrap()
answer = receiver.recv().unwrap()[3]
drop(sender)
drop(other)
closed = receiver.recv()
drop(receiver)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(answer).unwrap()
print(str.repr(closed).unwrap()).unwrap()
"#, "Result[tuple[Sender[i64], Receiver[i64]], ChannelError].Err(ChannelError.InvalidCapacity)\nResult[tuple[Sender[i64], Receiver[i64]], ChannelError].Err(ChannelError.Allocation(AllocError.CapacityOverflow))\n__test_fail_allocations_after_0__\n__test_restore_allocations__\nResult[tuple[Sender[i64], Receiver[i64]], ChannelError].Err(ChannelError.Allocation(AllocError.OutOfMemory))\n__test_fail_allocations_after_1__\n__test_restore_allocations__\nResult[tuple[Sender[i64], Receiver[i64]], ChannelError].Ok((<sender>, <receiver>))\n__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n13\nResult[range, RecvError].Err(RecvError.Disconnected)\n");
}

#[test]
fn queued_destructors_run_outside_the_lock_and_do_not_release_messages_twice() {
    runs(
        r#"
class Guard:
    id: i64
    feedback: Sender[i64]
    def __del__(self) -> ():
        self.feedback.send_nowait(self.id).unwrap()
        pass
feedback, report = channel[i64](4).unwrap()
sender, receiver = channel[Guard](1).unwrap()
sender.send(Guard(1, feedback.share())).unwrap()
match sender.send_nowait(Guard(2, feedback.share())):
    case Ok(_):
        print("wrong").unwrap()
    case Err(error):
        drop(error)
drop(receiver)
drop(sender)
drop(feedback)
print(report.recv().unwrap()).unwrap()
print(report.recv().unwrap()).unwrap()
print(str.repr(report.recv()).unwrap()).unwrap()
"#,
        "2\n1\nResult[i64, RecvError].Err(RecvError.Disconnected)\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn large_closure_messages_relocate_and_failed_sends_return_the_original_job() {
    runs(r#"
def exercise[F: OnceCallable[[], i64]](first: F, second: F) -> i64:
    sender, receiver = channel[F](1).unwrap()
    print("__test_begin_no_allocations__").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    sender.send(first).unwrap()
    mut answer = 0
    match sender.send_nowait(second):
        case Ok(_):
            pass
        case Err(error):
            match error:
                case SendError.Full(job):
                    answer = job()
                case SendError.Disconnected(job):
                    answer = job()
    job = receiver.recv().unwrap()
    drop(sender)
    drop(receiver)
    answer = answer + job()
    print("__test_restore_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
    answer
def make(n: i64) -> OnceClosure[[], i64]:
    a = range(n, n + 5)
    b = range(n + 10, n + 15)
    def once [a, b]() -> i64:
        a[2] + b[2]
print(exercise(make(0), make(100))).unwrap()
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n228\n");
}

#[test]
fn channel_messages_do_not_hide_bad_destructors_or_bad_callback_bodies() {
    for source in [
        r#"
def seven() -> i64:
    7
class Guard:
    n: i64
    def __del__(self) -> ():
        callback: Callable[[], i64] = seven
        callback()
        pass
sender, receiver = channel[Guard](1).unwrap()
"#,
        r#"
def queue[F](job: F) -> ():
    sender, receiver = channel[F](1).unwrap()
    sender.send(job).unwrap()
def seven() -> i64:
    7
n = 1
job = def [n]() -> i64:
    callback: Callable[[], i64] = seven
    n + callback()
queue(job)
"#,
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(
            error.contains("channel") && error.contains("indirect callable"),
            "{error}"
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_producer_start_drops_its_endpoint_and_successful_peers_finish() {
    runs(r#"
def start[F: OnceCallable[[], ()]](producer: F, receiver: Receiver[i64]) -> Result[(), Failure]:
    consumer = def once [receiver]() -> Result[i64, RecvError]:
        receiver.recv()
    with spawn(consumer)? as task, spawn(producer)?:
        task.join()?
    Ok(())
sender, receiver = channel[i64](1).unwrap()
producer = def once [sender]() -> ():
    sender.send(42).unwrap()
    pass
print("__test_one_thread_start__").unwrap()
print(str.repr(start(producer, receiver)).unwrap()).unwrap()
print("__test_restore_thread_starts__").unwrap()
"#, "__test_one_thread_start__\nResult[(), Failure].Err(Failure.Unspecified)\n__test_restore_thread_starts__\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn native_worker_sends_and_waits_with_allocation_disabled() {
    runs(r#"
sender, receiver = channel[i64](1).unwrap()
ready, started = channel[i64](1).unwrap()
sender.send(1).unwrap()
producer = def once [sender, ready]() -> ():
    print("__test_begin_no_allocations__").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    ready.send(1).unwrap()
    sender.send(2).unwrap()
    print("__test_restore_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
with spawn(producer).unwrap() as task:
    started.recv().unwrap()
    first = receiver.recv().unwrap()
    second = receiver.recv().unwrap()
    drop(started)
    drop(receiver)
    task.join()
    print(first + second).unwrap()
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n3\n");
}

#[test]
fn concrete_closure_messages_cross_workers_and_generic_sharing_is_inferred() {
    runs(
        r#"
def pass_endpoint[T](sender: Sender[T]) -> Sender[T]:
    sender
def exercise[F: OnceCallable[[], i64]](callback: F) -> i64:
    sender, receiver = channel[F](1).unwrap()
    producer = def once [sender, callback]() -> ():
        sender.send(callback).unwrap()
        pass
    with spawn(producer).unwrap(), receiver as inbox:
        received = inbox.recv().unwrap()
        return received()
span = range(30, 40)
callback = def once [span]() -> i64:
    span[2]
print(exercise(callback)).unwrap()
sender, receiver = channel[i64](1).unwrap()
other = pass_endpoint(sender.share())
other.send(42).unwrap()
print(receiver.recv().unwrap()).unwrap()
"#,
        "32\n42\n",
    );
}

#[test]
fn recursive_messages_cannot_keep_their_own_receiver_alive() {
    let source = "class Message:\n    receiver: Receiver[Message]\nsender, receiver = channel[Message](1).unwrap()\nsender.send(Message(receiver)).unwrap()\n";
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains("retain its own receiver"), "{error}");
    let source = "class A:\n    receiver: Receiver[B]\nclass B:\n    receiver: Receiver[A]\nsender, receiver = channel[A](1).unwrap()\n";
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains("retain its own receiver"), "{error}");
    runs(
        r#"
class Node:
    value: i64
    next: Option[Box[Node]]
    reply: Sender[i64]
reply, answers = channel[i64](1).unwrap()
sender, receiver = channel[Node](1).unwrap()
sender.send(Node(42, Nothing, reply)).unwrap()
node = receiver.recv().unwrap()
node.reply.send(node.value).unwrap()
print(answers.recv().unwrap()).unwrap()
class Event:
    reply: Sender[Event]
events, listener = channel[Event](1).unwrap()
events.send(Event(events.share())).unwrap()
drop(events)
drop(listener)
"#,
        "42\n",
    );
}

#[test]
fn endpoint_contexts_disconnect_before_join_on_early_return_and_propagation() {
    runs(r#"
def producer(sender: Sender[i64]) -> Result[(), Failure]:
    for n in range(10000):
        sender.send(n)?
    Ok(())
def stop() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    job = def once [sender]() -> Result[(), Failure]:
        producer(sender)
    with spawn(job)? as task, receiver as inbox:
        inbox.recv()?
        return Err(Failure.Unspecified)
def failed() -> Result[(), Failure]:
    Err(Failure.Unspecified)
def propagate() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    job = def once [sender]() -> Result[(), Failure]:
        producer(sender)
    with spawn(job)? as task, receiver as inbox:
        inbox.recv()?
        failed()?
    Ok(())
print(str.repr(stop()).unwrap()).unwrap()
print(str.repr(propagate()).unwrap()).unwrap()
sender, receiver = channel[i64](1).unwrap()
with sender as outbox:
    outbox.send(42).unwrap()
print(receiver.recv().unwrap()).unwrap()
print(str.repr(receiver.recv()).unwrap()).unwrap()
"#, "Result[(), Failure].Err(Failure.Unspecified)\nResult[(), Failure].Err(Failure.Unspecified)\n42\nResult[i64, RecvError].Err(RecvError.Disconnected)\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn endpoint_guard_shuts_down_an_earlier_producer_when_a_later_start_fails() {
    runs(r#"
def attempt() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    first = def once [sender]() -> Result[(), Failure]:
        for n in range(10000):
            sender.send(n)?
        Ok(())
    second = def once () -> ():
        pass
    with spawn(first)? as a, receiver as inbox, spawn(second)? as b:
        pass
    Ok(())
print("__test_one_thread_start__").unwrap()
print(str.repr(attempt()).unwrap()).unwrap()
print("__test_restore_thread_starts__").unwrap()
"#, "__test_one_thread_start__\nResult[(), Failure].Err(Failure.Unspecified)\n__test_restore_thread_starts__\n");
}

#[test]
fn endpoint_guards_disconnect_on_loop_control_exits() {
    runs(
        r#"
def main() -> Result[(), Failure]:
    for i in range(3):
        sender, receiver = channel[i64](1)?
        job = def once [sender]() -> Result[(), Failure]:
            for n in range(10000):
                sender.send(n)?
            Ok(())
        with spawn(job)?, receiver as inbox:
            inbox.recv()?
            if i == 0:
                continue
            break
    print("done")?
    Ok(())
"#,
        "done\n",
    );
}

/// A worker that sends until the queue disconnects; `main` starts on line 8.
const PRODUCE: &str = "def produce(sender: Sender[i64]) -> ():
    for n in range(10000):
        match sender.send(n):
            case Ok(_):
                pass
            case Err(_):
                return
";

/// The diagnostic for a join that a surviving receiver could block forever.
fn blocked_join(body: &str) -> String {
    let error = support::check_source(&format!("{PRODUCE}{body}"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("could block the worker's send"), "{error}");
    error
}

#[test]
fn receiver_outside_the_task_block_cannot_reach_the_automatic_join() {
    // Issue #12: without the check this program hangs in the automatic join.
    let error = blocked_join(
        "def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    with spawn(produce, sender)?:
        print(receiver.recv()?)?
    Ok(())
",
    );
    assert_eq!(
        error,
        "10:10: leaving this `with` block joins a task whose worker holds `Sender[i64]` while \
         `receiver` still holds `Receiver[i64]`: a full queue could block the worker's send, and \
         this join, forever\n  \
         9:5: note: `receiver` holds `Receiver[i64]`; drop it before leaving the block, or guard \
         it directly after the task: `with spawn(...)?, receiver as inbox:`\n  \
         11:15: note: `?` leaves the block here"
    );
}

#[test]
fn receiver_guard_order_decides_whether_it_outlives_the_join() {
    runs(
        &format!(
            "{PRODUCE}def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    with spawn(produce, sender)?, receiver as inbox:
        print(inbox.recv()?)?
    print(\"done\")?
    Ok(())
"
        ),
        "0\ndone\n",
    );
    // Contexts exit in reverse order: this guard outlives the task.
    let error = blocked_join(
        "def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    with receiver as inbox, spawn(produce, sender)?:
        print(inbox.recv()?)?
    Ok(())
",
    );
    assert!(
        error.starts_with("10:29: leaving this `with` block"),
        "{error}"
    );
    assert!(
        error.contains("10:10: note: `receiver` holds `Receiver[i64]`"),
        "{error}"
    );
}

#[test]
fn every_exit_from_a_task_block_is_checked_for_surviving_receivers() {
    // Falling off the end of the block names no exit.
    let error = blocked_join(
        "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    with spawn(produce, sender).unwrap():
        pass
",
    );
    assert!(!error.contains("leaves the block here"), "{error}");
    for (exit, line) in [("return Ok(())", 13), ("break", 13), ("continue", 13)] {
        let keyword = exit.split(' ').next().unwrap();
        let error = blocked_join(&format!(
            "def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1).unwrap()
    for i in range(1):
        with spawn(produce, sender.share()).unwrap():
            drop(receiver.share())
            {exit}
    Ok(())
"
        ));
        assert!(
            error.contains(&format!(
                "{line}:13: note: `{keyword}` leaves the block here"
            )),
            "{error}"
        );
    }
    // The receiver is gone at the end of the block, but not when `?` leaves it.
    let error = blocked_join(
        "def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    with spawn(produce, sender)? as task:
        print(receiver.recv()?)?
        drop(receiver)
        task.join()
    Ok(())
",
    );
    assert!(
        error.contains("11:15: note: `?` leaves the block here"),
        "{error}"
    );
    // A later start that fails leaves before the receiver is guarded.
    let error = blocked_join(
        "def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    other = sender.share()
    with spawn(produce, sender)?, spawn(produce, other)?, receiver as inbox:
        print(inbox.recv()?)?
    Ok(())
",
    );
    assert!(
        error.contains("11:35: note: `?` leaves the block here"),
        "{error}"
    );
}

#[test]
fn every_surviving_owner_of_a_matching_receiver_is_reported() {
    // A shared handle outside the guard keeps the queue connected.
    let error = blocked_join(
        "def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    spare = receiver.share()
    with spawn(produce, sender)?, receiver as inbox:
        print(inbox.recv()?)?
    Ok(())
",
    );
    assert!(error.contains("while `spare` still holds"), "{error}");
    assert!(
        error.contains("10:5: note: `spare` holds `Receiver[i64]`"),
        "{error}"
    );
    let error = blocked_join(
        "class Hub:
    inbox: Receiver[i64]
def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    spare = receiver.share()
    hub = Hub(receiver.share())
    handles = [receiver.share()]?
    reader = def once [receiver]() -> ():
        pass
    with spawn(produce, sender).unwrap():
        pass
    Ok(())
",
    );
    assert!(
        error.contains("note: `spare` holds `Receiver[i64]`"),
        "{error}"
    );
    for owner in ["hub", "handles", "reader"] {
        assert!(
            error.contains(&format!("note: `{owner}` also holds `Receiver[i64]`")),
            "{error}"
        );
    }
    // A drop on one branch leaves the receiver alive on the other.
    let error = blocked_join(
        "def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    early = len(\"x\") == 1
    with spawn(produce, sender).unwrap():
        if early:
            drop(receiver)
    Ok(())
",
    );
    assert!(
        error.contains("`receiver` holds `Receiver[i64]` on some paths"),
        "{error}"
    );
    // A borrowed receiver belongs to a caller, which this function cannot end.
    for body in [
        "def run(sender: Sender[i64], inbox: &Receiver[i64]) -> ():
    with spawn(produce, sender).unwrap():
        pass
",
        "class Hub:
    receiver: Receiver[i64]
    def run(self, sender: Sender[i64]) -> ():
        with spawn(produce, sender).unwrap():
            pass
",
        "def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    job = def once [sender, &receiver]() -> ():
        with spawn(produce, sender).unwrap():
            pass
    job()
    Ok(())
",
    ] {
        let error = blocked_join(body);
        assert!(
            error.contains("its owner outlives this call, so take the receiver by value"),
            "{error}"
        );
    }
}

#[test]
fn explicit_joins_wait_only_after_matching_receivers_are_gone() {
    // Draining to disconnection does not make the guard's handle disappear.
    let error = blocked_join(
        "def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    with spawn(produce, sender)? as task, receiver as inbox:
        print(inbox.recv()?)?
        task.join()
    Ok(())
",
    );
    assert_eq!(
        error,
        "12:9: this join waits for a task whose worker holds `Sender[i64]` while `receiver` \
         still holds `Receiver[i64]`: a full queue could block the worker's send, and this join, \
         forever\n  \
         10:43: note: `receiver` holds `Receiver[i64]`; drop it, or end its `with` block, before \
         joining\n  \
         10:10: note: the task starts here"
    );
    runs(
        &format!(
            "{PRODUCE}def count(sender: Sender[i64]) -> i64:
    produce(sender)
    7
def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    with spawn(count, sender)? as task:
        with receiver as inbox:
            print(inbox.recv()?)?
        print(task.join())?
    left, right = channel[i64](1)?
    with spawn(count, left).unwrap() as task:
        print(right.recv().unwrap()).unwrap()
        drop(right)
        print(task.join()).unwrap()
    partial, last = channel[i64](1)?
    with spawn(count, partial)? as task:
        with last as inbox:
            print(inbox.recv()?)?
        if len(\"x\") == 1:
            print(task.join())?
    Ok(())
"
        ),
        "0\n7\n0\n7\n0\n7\n",
    );
}

#[test]
fn nested_task_blocks_check_each_join_against_live_receivers() {
    let error = blocked_join(
        "def idle() -> ():
    pass
def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    with spawn(idle)?:
        with spawn(produce, sender)?:
            print(receiver.recv()?)?
    Ok(())
",
    );
    assert!(
        error.starts_with("13:14: leaving this `with` block"),
        "{error}"
    );
    // The outer producer is joined after the inner guard has dropped the receiver.
    runs(
        &format!(
            "{PRODUCE}def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    other = sender.share()
    with spawn(produce, sender)?:
        with spawn(produce, other).unwrap(), receiver as inbox:
            print(inbox.recv()?)?
    print(\"done\")?
    Ok(())
"
        ),
        "0\ndone\n",
    );
}

#[test]
fn joins_without_a_matching_live_receiver_are_accepted() {
    runs(
        &format!(
            "{PRODUCE}def consume(receiver: Receiver[i64]) -> i64:
    mut count = 0
    while True:
        match receiver.recv():
            case Ok(_):
                count = count + 1
            case Err(_):
                return count
    count
def idle(n: i64) -> i64:
    n + 1
def drain(sender: Sender[i64], receiver: Receiver[i64]) -> Result[(), Failure]:
    with spawn(produce, sender)?, receiver as inbox:
        print(inbox.recv()?)?
    Ok(())
def main() -> Result[(), Failure]:
    # A receiver for another message type cannot release this worker.
    sender, receiver = channel[i64](1)?
    names, labels = channel[str](1)?
    with spawn(produce, sender)?, receiver as inbox:
        print(inbox.recv()?)?
    names.send(\"label\")?
    print(labels.recv()?)?
    # This worker holds no sender.
    with spawn(idle, 1)? as task:
        names.send(\"idle\")?
        print(task.join())?
    # The receiver belongs to another worker, which ends at disconnection.
    first, queued = channel[i64](1)?
    second = first.share()
    # A failed first producer start must not join while second is still alive.
    with spawn(consume, queued)? as consumer, spawn(produce, first).unwrap(), spawn(produce, second)?:
        print(consumer.join())?
    # A helper that owns the receiver ends it before the outer join.
    outer, handed = channel[i64](1)?
    inner = outer.share()
    with spawn(produce, outer)?:
        drain(inner, handed)?
    Ok(())
"
        ),
        "0\nlabel\n2\n20000\n0\n",
    );
}

#[test]
fn receivers_are_matched_by_message_type_not_by_queue() {
    // None of these programs can hang. The rule matches endpoints by message
    // type and does not follow how many messages a worker sends, so it rejects
    // them; the reference lists them as its known false positives.
    for body in [
        // An independent queue of the same message type.
        "def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    unrelated, other = channel[i64](1)?
    with spawn(produce, sender)?, receiver as inbox:
        print(inbox.recv()?)?
    Ok(())
",
        // The parent receives everything the worker sends.
        "def once_only(sender: Sender[i64]) -> ():
    sender.send(1).unwrap()
def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    with spawn(once_only, sender).unwrap():
        receiver.recv().unwrap()
",
        // The worker never waits for queue space.
        "def attempt(sender: Sender[i64]) -> ():
    match sender.send_nowait(1):
        case Ok(_):
            pass
        case Err(_):
            pass
def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    with spawn(attempt, sender).unwrap():
        pass
",
    ] {
        blocked_join(body);
    }
}
