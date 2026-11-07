//! Issue #793: a worker, or pool job, that holds an endpoint but cannot wait
//! on its queue does not block a join or shutdown while the joining function
//! keeps the opposite endpoint. Only `send_nowait`, `recv_nowait`, and nowait
//! selection are nonblocking; helpers, destructors, started workers, and
//! calls through values count.
mod support;

const NOTIFY: &str = "def notify(sender: Sender[i64]) -> ():
    for n in range(3):
        match sender.send_nowait(n):
            case Ok(_):
                pass
            case Err(_):
                return
";

fn runs(source: &str, expected: &str) {
    let source = format!("{NOTIFY}{source}");
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("nowait-test");
    support::compile_source_to_executable(&source, &executable)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    let output = support::output_within(
        &mut std::process::Command::new(executable),
        b"",
        std::time::Duration::from_secs(15),
    );
    assert!(output.status.success(), "{source}: {output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

fn rejected(source: &str, expected: &str) {
    let source = format!("{NOTIFY}{source}");
    let error = support::check_source(&source).unwrap_err().to_string();
    assert!(error.contains(expected), "{source}\n{error}");
}

#[test]
fn nowait_workers_join_while_the_receiver_survives() {
    runs(
        "def main() -> Result[(), AllocError | IoError | RecvError | SpawnError[()]]:
    sender, receiver = channel[i64](1)?
    with spawn(notify, sender.share()).drop_job()? as task:
        task.join()
    with spawn(def once [sender]() -> (): notify(sender)).unwrap():
        pass
    print(receiver.recv()?)?
    Ok(())
",
        "0\n",
    );
}

#[test]
fn blocking_sends_a_nowait_worker_reaches_are_rejected() {
    const SEND: &str = "could block the worker's send";
    let main = "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    with spawn(worker, sender).unwrap():
        pass
";
    for worker in [
        // A helper's blocking send.
        "def forward(sender: &Sender[i64]) -> ():
    drop(sender.send(1))
def worker(sender: Sender[i64]) -> ():
    notify(sender.share())
    forward(&sender)
",
        // A timeout bounds the wait, but the bound can be arbitrarily long.
        "def worker(sender: Sender[i64]) -> ():
    drop(sender.send_timeout_ms(1, 10))
",
        // A started worker is joined before this one returns.
        "def produce(sender: Sender[i64]) -> ():
    drop(sender.send(1))
def worker(sender: Sender[i64]) -> ():
    with spawn(produce, sender).unwrap():
        pass
",
        // The callee of a value is unknown.
        "def nothing() -> ():
    pass
def call(sender: Sender[i64], callback: Callable[[], ()]) -> ():
    callback()
    notify(sender)
def worker(sender: Sender[i64]) -> ():
    call(sender, nothing)
",
    ] {
        rejected(&format!("{worker}{main}"), SEND);
    }
    rejected(
        "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    with spawn(def once [sender]() -> (): drop(sender.send(1))).unwrap():
        pass
",
        SEND,
    );
}

#[test]
fn destructors_a_worker_can_run_count_as_its_waits() {
    let flush = "class Flush:
    sender: Sender[i64]
    def __del__(self: &mut Flush) -> ():
        drop(self.sender.send(0))
";
    // The worker drops its argument.
    rejected(
        &format!(
            "{flush}def worker(flush: Flush) -> ():
    drop(flush.sender.send_nowait(1))
def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    with spawn(worker, Flush(sender)).unwrap():
        pass
"
        ),
        "could block the worker's send",
    );
    // The worker drops a received message.
    rejected(
        &format!(
            "{flush}def worker(flushes: Receiver[Flush]) -> ():
    drop(flushes.recv_nowait())
def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    queue, flushes = channel[Flush](1).unwrap()
    queue.send(Flush(sender)).unwrap()
    drop(queue)
    with spawn(worker, flushes).unwrap():
        pass
"
        ),
        "could block the worker's send",
    );
}

#[test]
fn receiving_workers_wait_only_for_blocking_receives() {
    let main = "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    sender.send(1).unwrap()
    with spawn(worker, receiver).unwrap():
        pass
";
    for worker in [
        "def worker(receiver: Receiver[i64]) -> ():
    drop(receiver.recv_nowait())
",
        "def worker(receiver: Receiver[i64]) -> ():
    other, spare = channel[i64](1).unwrap()
    drop(select_recv_nowait(&receiver, &spare))
",
    ] {
        support::check_source(&format!("{NOTIFY}{worker}{main}"))
            .unwrap_or_else(|e| panic!("{worker}\n{e}"));
    }
    for worker in [
        "def wait(receiver: &Receiver[i64]) -> ():
    drop(receiver.recv())
def worker(receiver: Receiver[i64]) -> ():
    drop(receiver.recv_nowait())
    wait(&receiver)
",
        "def worker(receiver: Receiver[i64]) -> ():
    drop(receiver.recv_timeout_ms(10))
",
        "def worker(receiver: Receiver[i64]) -> ():
    other, spare = channel[i64](1).unwrap()
    drop(select_recv(&receiver, &spare))
",
    ] {
        rejected(
            &format!("{worker}{main}"),
            "could block the worker's receive",
        );
    }
}

#[test]
fn pool_jobs_that_cannot_wait_allow_live_receivers() {
    runs(
        "def main() -> Result[(), AllocError | FutureError | IoError | PoolError | RecvError]:
    sender, receiver = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(notify, sender.share()).unwrap()
        future.result()?
        later = pool.submit(notify, sender).unwrap()
    print(receiver.recv()?)?
    Ok(())
",
        "0\n",
    );
    for (wait, expected) in [
        (
            "        future.result().unwrap()\n",
            "could block the job's send",
        ),
        ("        pass\n", "could block the job's send"),
    ] {
        rejected(
            &format!(
                "def forward(sender: Sender[i64]) -> ():
    notify(sender.share())
    drop(sender.send(1))
def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    with ThreadPoolExecutor(1, 1).unwrap() as pool:
        future = pool.submit(forward, sender).unwrap()
{wait}"
            ),
            expected,
        );
    }
}
