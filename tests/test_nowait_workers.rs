//! Issue #793: a worker, or pool job, that holds an endpoint but cannot wait
//! for anything does not block a join or shutdown while the joining function
//! keeps the opposite endpoint. Only `send_nowait`, `recv_nowait`, nowait
//! selection, and `submit_nowait` never wait; any other wait counts, on any
//! queue, lock, token, readiness, child process, file or socket transfer, or
//! foreign call, through helpers, destructors, started workers, and calls
//! through values. Standard-stream reads and writes do not count.
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
    let source = format!("from std.sync import Mutex\n{NOTIFY}{source}");
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

#[test]
fn a_worker_that_waits_on_anything_else_keeps_the_endpoint_rule() {
    // The worker's own queue operations never wait, but it can wait for a
    // third thread that is itself blocked on the function's receiver.
    const SEND: &str = "could block the worker's send";
    for worker in [
        // Another queue: its producer could be sending on `sender`'s queue.
        "def worker(sender: Sender[i64], inbox: Receiver[bool]) -> ():
    notify(sender)
    drop(inbox.recv())
",
        // A lock: its holder could be sending on `sender`'s queue.
        "def worker(sender: Sender[i64], inbox: Receiver[bool]) -> ():
    notify(sender)
    drop(inbox.recv_nowait())
    cell = Mutex[i64](0)
    with cell.lock() as value:
        *value = 1
",
        // A blocking submit waits for space in a pool's queue.
        "def worker(sender: Sender[i64], inbox: Receiver[bool]) -> ():
    drop(inbox.recv_nowait())
    with ThreadPoolExecutor(1, 1).unwrap() as pool:
        drop(pool.submit(notify, sender))
",
    ] {
        rejected(
            &format!(
                "{worker}def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    outbox, inbox = channel[bool](1).unwrap()
    with spawn(worker, sender, inbox).unwrap():
        pass
"
            ),
            SEND,
        );
    }
    // A pool job that waits on another queue keeps the shutdown rule.
    rejected(
        "def forward(sender: Sender[i64], inbox: Receiver[bool]) -> ():
    notify(sender)
    drop(inbox.recv())
def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    outbox, inbox = channel[bool](1).unwrap()
    with ThreadPoolExecutor(1, 1).unwrap() as pool:
        future = pool.submit(def once [sender, inbox]() -> (): forward(sender, inbox)).unwrap()
",
        "could block the job's send",
    );
}

/// Waits that end only when another thread acts, or that the checker cannot
/// see into, in a worker that holds a sender and a token.
const OTHER_WAITS: [&str; 5] = [
    // A cancellation token wait: its canceller could be blocked on the queue.
    "    token.wait()
",
    "    drop(token.wait_timeout_ms(10))
",
    // `cancelled_before` waits on the token with a deadline.
    "    never = CancellationToken().unwrap()
    deadline = deadline_after(&monotonic_now(), &seconds(1000)).unwrap()
    drop(cancelled_before(&token, &deadline, &never))
",
    // A readiness wait: the writer could be blocked on the queue.
    "    drop(wait(&file, Interest.Read, &deadline_at(&monotonic_now()), &token))
",
    // A child process can wait on a pipe that this program drains.
    "    match command(\"true\"):
        case Ok(found):
            match found.spawn():
                case Ok(started):
                    mut child = started
                    drop(child.wait())
                case Err(_):
                    pass
        case Err(_):
            pass
",
];

const OTHER_IMPORTS: &str = "from std.process import command
from std.ready import Interest, wait, wait_now
from std.sync import cancelled_before
from std.time import deadline_after, deadline_at, monotonic_now, seconds
";

#[test]
fn token_readiness_and_process_waits_count_for_workers_and_jobs() {
    const START: &str = "    with spawn(worker, sender, file, token).unwrap():
        pass
";
    const SUBMIT: &str = "    with ThreadPoolExecutor(1, 1).unwrap() as pool:
        future = pool.submit(def once [sender, file, token]() -> (): worker(sender, file, token)).unwrap()
        future.result().unwrap()
";
    let program = |body: &str, wait: &str| {
        format!(
            "{OTHER_IMPORTS}{NOTIFY}def worker(sender: Sender[i64], file: File, token: CancellationToken) -> ():
    notify(sender)
{body}def start(file: File, token: CancellationToken) -> ():
    sender, receiver = channel[i64](1).unwrap()
{wait}def main() -> ():
    pass
"
        )
    };
    for (wait, expected) in [
        (START, "could block the worker's send"),
        (SUBMIT, "could block the job's send"),
    ] {
        // A readiness test that does not wait keeps the worker nonblocking.
        let source = program(
            "    drop(wait_now(&file, Interest.Read))\n    drop(token.is_cancelled())\n",
            wait,
        );
        support::check_source(&source).unwrap_or_else(|e| panic!("{source}\n{e}"));
        for body in OTHER_WAITS {
            let source = program(body, wait);
            let error = support::check_source(&source).unwrap_err().to_string();
            assert!(error.contains(expected), "{source}\n{error}");
        }
    }
}

#[test]
fn file_and_socket_transfers_count_but_standard_streams_do_not() {
    // A third thread can block on the queue before it writes the byte the
    // worker reads; the join then never returns (review of local PR #1039).
    rejected(
        "import std.bytes
from std.net import TcpStream, connect, listen, loopback_v4
from std.time import deadline_after, monotonic_now, seconds
def waiter(sender: Sender[i64], stream: TcpStream) -> ():
    notify(sender)
    mut inbound = stream
    mut buffer = std.bytes.from_text(\"y\").unwrap()
    drop(inbound.read(&mut buffer, 0, 1))
def filler(sender: Sender[i64], stream: TcpStream) -> ():
    drop(sender.send(1))
    drop(sender.send(2))
    mut outbound = stream
    drop(outbound.write(b\"x\"))
def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    sender, receiver = channel[i64](1)?
    listener = listen(&loopback_v4(0u16), 1).unwrap()
    address = listener.local_address().unwrap()
    never = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &seconds(10)).unwrap()
    client = connect(&address, &deadline, &never).unwrap()
    server = listener.accept(&deadline, &never).unwrap()
    with spawn(filler, sender.share(), client).drop_job()?:
        with spawn(waiter, sender, server).unwrap() as task:
            task.join()
        drop(receiver)
    print(\"done\")?
    Ok(())
",
        "could block the worker's send",
    );
    // Any file can be a pipe or FIFO whose writer is another thread.
    for transfer in [
        "    mut buffer = std.bytes.from_text(\"y\").unwrap()
    drop(stream.read(&mut buffer, 0, 1))
",
        "    drop(stream.write(b\"x\"))
",
    ] {
        rejected(
            &format!(
                "import std.bytes
from std.io import FileStream
def worker(sender: Sender[i64], file: FileStream) -> ():
    notify(sender)
    mut stream = file
{transfer}def start(file: FileStream) -> ():
    sender, receiver = channel[i64](1).unwrap()
    with spawn(worker, sender, file).unwrap():
        pass
def main() -> ():
    pass
"
            ),
            "could block the worker's send",
        );
    }
    rejected(
        "from std.io import FileStream
def forward(sender: Sender[i64], file: FileStream) -> ():
    notify(sender)
    mut stream = file
    drop(stream.write(b\"x\"))
def start(file: FileStream) -> ():
    sender, receiver = channel[i64](1).unwrap()
    with ThreadPoolExecutor(1, 1).unwrap() as pool:
        future = pool.submit(def once [sender, file]() -> (): forward(sender, file)).unwrap()
def main() -> ():
    pass
",
        "could block the job's send",
    );
    // A standard stream's other end is outside the program.
    runs(
        "def worker(sender: Sender[i64]) -> Result[(), IoError]:
    notify(sender)
    print(\"sent\")?
    Ok(())
def main() -> Result[(), AllocError | IoError | RecvError | SpawnError[()]]:
    sender, receiver = channel[i64](1)?
    with spawn(worker, sender).drop_job()?:
        pass
    print(receiver.recv()?)?
    Ok(())
",
        "sent\n0\n",
    );
}

#[test]
fn whole_file_and_database_operations_count() {
    // Opening a FIFO by path waits for its other end, which a thread blocked
    // on the queue could be about to open (review of local PR #1039).
    rejected(
        "def waiter(sender: Sender[i64]) -> ():
    notify(sender)
    drop(read_text(\"fifo\"))
def filler(sender: Sender[i64]) -> ():
    drop(sender.send(1))
    drop(sender.send(2))
    drop(write_text(\"fifo\", \"x\"))
def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    sender, receiver = channel[i64](1)?
    with spawn(filler, sender.share()).drop_job()?:
        with spawn(waiter, sender).unwrap() as task:
            task.join()
        drop(receiver)
    Ok(())
",
        "could block the worker's send",
    );
    // A busy timeout waits for another connection's lock, which a thread
    // blocked on the queue could hold.
    rejected(
        "import std.db.sqlite as sqlite
from std.time import seconds
def waiter(sender: Sender[i64]) -> ():
    notify(sender)
    mut db = sqlite.connect(\"lock.db\", sqlite.Mode.Create, 65536).unwrap()
    db.set_busy_timeout(&seconds(1000)).unwrap()
    drop(db.execute(\"INSERT INTO t VALUES (2)\"))
def filler(sender: Sender[i64]) -> ():
    mut db = sqlite.connect(\"lock.db\", sqlite.Mode.Create, 65536).unwrap()
    db.execute(\"BEGIN IMMEDIATE\").unwrap()
    drop(sender.send(1))
    drop(sender.send(2))
    db.execute(\"COMMIT\").unwrap()
def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    sender, receiver = channel[i64](1)?
    with spawn(filler, sender.share()).drop_job()?:
        with spawn(waiter, sender).unwrap() as task:
            task.join()
        drop(receiver)
    Ok(())
",
        "could block the worker's send",
    );
    // `std.io.open_path` opens without `O_NONBLOCK`, in a pool job too.
    rejected(
        "import std.io
import std.path
def forward(sender: Sender[i64]) -> ():
    notify(sender)
    fifo = std.path.from_text(\"fifo\").unwrap()
    drop(std.io.open_path(&fifo, \"r\"))
def start() -> ():
    sender, receiver = channel[i64](1).unwrap()
    with ThreadPoolExecutor(1, 1).unwrap() as pool:
        future = pool.submit(def once [sender]() -> (): forward(sender)).unwrap()
def main() -> ():
    pass
",
        "could block the job's send",
    );
}

#[test]
fn handle_operations_that_never_block_keep_a_worker_nonblocking() {
    // Each operation here is in `never_blocks`; removing an entry fails this
    // test. The selector is made by the caller because `selector` converts
    // errors through a function value, which counts as a wait.
    let source = format!(
        "import std.fs
import std.path
from std.fs.lock import Mode, acquire_now
from std.ready import Interest, Selector, selector
{NOTIFY}def worker(sender: Sender[i64], root: std.fs.Root, events: Selector) -> ():
    notify(sender)
    name = std.path.from_text(\"data\").unwrap()
    match root.read_file(&name):
        case Ok(stream):
            mut watched = events
            drop(watched.watch(&stream.file, 1u64, Interest.Read))
        case Err(_):
            pass
    drop(root.create_file(&std.path.from_text(\"new\").unwrap()))
    drop(acquire_now(&std.path.from_text(\"lock\").unwrap(), Mode.Exclusive))

def main() -> Result[(), AllocError | IoError | RecvError | SpawnError[()]]:
    root = std.fs.root(&std.path.from_text(\".\")?)?
    watched = selector(4).unwrap()
    sender, receiver = channel[i64](1)?
    with spawn(worker, sender, root, watched).drop_job()?:
        pass
    print(receiver.recv()?)?
    Ok(())
"
    );
    support::check_source(&source).unwrap_or_else(|e| panic!("{source}\n{e}"));
}
