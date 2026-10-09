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
print(receiver.recv_nowait()).unwrap()
sender.send([1, 2].unwrap()).unwrap()
match sender.send_nowait([3].unwrap()):
    case Ok(_):
        print("wrong").unwrap()
    case Err(error):
        match error:
            case SendError[list[i64]].Full(value):
                print(value).unwrap()
            case SendError[list[i64]].Disconnected(value):
                print("wrong").unwrap()
print(receiver.recv().unwrap()).unwrap()
sender.send([4].unwrap()).unwrap()
drop(sender)
print(receiver.recv().unwrap()).unwrap()
print(receiver.recv()).unwrap()
other, destination = channel[i64](1).unwrap()
drop(destination)
print(other.send(42)).unwrap()
print(other.send_nowait(43)).unwrap()
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
    print(task.join()).unwrap()
other, destination = channel[i64](1).unwrap()
waiting = def once [destination]() -> Result[i64, RecvError]:
    destination.recv()
with spawn(waiting).unwrap() as task:
    drop(other)
    print(task.join()).unwrap()
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
mailbox = Mailbox(sender).unwrap()
other = mailbox.sender.share()
deliver(&other, "hello").unwrap()
drop(other)
print(receiver.recv().unwrap()).unwrap()
print(receiver.recv_nowait()).unwrap()
drop(mailbox)
print(receiver.recv()).unwrap()
"#, "hello\nResult[str, RecvError].Err(RecvError.Empty)\nResult[str, RecvError].Err(RecvError.Disconnected)\n");
}

#[test]
fn channel_messages_reject_uncertified_effects_and_endpoint_copy_or_equality() {
    for (source, expected) in [
        ("sender, receiver = channel[File](1).unwrap()\n", "cross-thread"),
        ("sender, receiver = channel[Callable[[], i64]](1).unwrap()\n", "indirect callable"),
        ("sender, receiver = channel[&i64](1).unwrap()\n", "wholly owned"),
        ("sender, receiver = channel[i64](1).unwrap()\nother = copy(sender)\n", "cannot be copied"),
        ("sender, receiver = channel[i64](1).unwrap()\nprint(sender == sender).unwrap()\n", "equality"),
        ("sender, receiver = channel[i64](1).unwrap()\nvalues = [sender].unwrap()\nprint(values == values).unwrap()\n", "equality"),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn construction_failure_is_recoverable_and_queue_operations_do_not_allocate() {
    runs(r#"
print(channel[i64](0)).unwrap()
print(channel[i64](18446744073709551615u64)).unwrap()
print("__test_fail_allocations_after_0__").unwrap()
first = channel[i64](2)
print("__test_restore_allocations__").unwrap()
print(first).unwrap()
print("__test_fail_allocations_after_1__").unwrap()
second = channel[i64](2)
print("__test_restore_allocations__").unwrap()
print(second).unwrap()
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
print(closed).unwrap()
"#, "Result[tuple[Sender[i64], Receiver[i64]], ChannelError].Err(ChannelError.InvalidCapacity)\nResult[tuple[Sender[i64], Receiver[i64]], ChannelError].Err(ChannelError.Allocation(AllocError.CapacityOverflow))\n__test_fail_allocations_after_0__\n__test_restore_allocations__\nResult[tuple[Sender[i64], Receiver[i64]], ChannelError].Err(ChannelError.Allocation(AllocError.OutOfMemory))\n__test_fail_allocations_after_1__\n__test_restore_allocations__\nResult[tuple[Sender[i64], Receiver[i64]], ChannelError].Err(ChannelError.Allocation(AllocError.OutOfMemory))\n__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n13\nResult[range, RecvError].Err(RecvError.Disconnected)\n");
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
sender.send(Guard(1, feedback.share()).unwrap()).unwrap()
match sender.send_nowait(Guard(2, feedback.share()).unwrap()):
    case Ok(_):
        print("wrong").unwrap()
    case Err(error):
        drop(error)
drop(receiver)
drop(sender)
drop(feedback)
print(report.recv().unwrap()).unwrap()
print(report.recv().unwrap()).unwrap()
print(report.recv()).unwrap()
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
                case SendError[F].Full(job):
                    answer = job()
                case SendError[F].Disconnected(job):
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
class Guard:
    n: i64
    def __del__(self) -> ():
        file = open("unused", "r").unwrap()
        drop(file)
sender, receiver = channel[Guard](1).unwrap()
"#,
        r#"
def queue[F](job: F) -> ():
    sender, receiver = channel[F](1).unwrap()
    sender.send(job).unwrap()
n = 1
job = def [n]() -> i64:
    file = open("unused", "r").unwrap()
    n
queue(job)
"#,
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(
            error.contains("channel") && error.contains("File"),
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
print(start(producer, receiver)).unwrap()
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
    with spawn(producer).unwrap() as task:
        received = receiver.recv().unwrap()
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
    let source = "class Message:\n    receiver: Receiver[Message]\nsender, receiver = channel[Message](1).unwrap()\nsender.send(Message(receiver).unwrap()).unwrap()\n";
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains("retain its own receiver"), "{error}");
    let source = "class A:\n    receiver: Receiver[B]\nclass B:\n    receiver: Receiver[A]\nsender, receiver = channel[A](1).unwrap()\n";
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains("retain its own receiver"), "{error}");
    runs(
        r#"
class Node:
    value: i64
    next: Option[Node]
    reply: Sender[i64]
reply, answers = channel[i64](1).unwrap()
sender, receiver = channel[Node](1).unwrap()
sender.send(Node(42, Nothing, reply).unwrap()).unwrap()
node = receiver.recv().unwrap()
node.reply.send(node.value).unwrap()
print(answers.recv().unwrap()).unwrap()
class Event:
    reply: Sender[Event]
events, listener = channel[Event](1).unwrap()
events.send(Event(events.share()).unwrap()).unwrap()
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
print(stop()).unwrap()
print(propagate()).unwrap()
sender, receiver = channel[i64](1).unwrap()
with sender as outbox:
    outbox.send(42).unwrap()
print(receiver.recv().unwrap()).unwrap()
print(receiver.recv()).unwrap()
"#, "Result[(), Failure].Err(Failure.Unspecified)\nResult[(), Failure].Err(Failure.Unspecified)\n42\nResult[i64, RecvError].Err(RecvError.Disconnected)\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn endpoint_guard_shuts_down_an_earlier_producer_when_a_later_start_fails() {
    runs(r#"
def attempt() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    other = sender.share()
    first = def once [sender]() -> Result[(), Failure]:
        for n in range(10000):
            sender.send(n)?
        Ok(())
    second = def once [other]() -> Result[(), Failure]:
        other.send(42)?
        Ok(())
    with spawn(first)? as a, receiver as inbox, spawn(second)? as b:
        pass
    Ok(())
print("__test_one_thread_start__").unwrap()
print(attempt()).unwrap()
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
