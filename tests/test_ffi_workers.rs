//! `@thread_safe` foreign declarations on worker threads (issue #772): the
//! annotation certifies calls, including from destructors, and everything
//! else stays rejected. `@thread_movable` and `@thread_shareable` opaque
//! declarations (issue #833) say which native values may cross.

mod support;
use std::path::Path;

/// Scalar POSIX calls; `close` releases the owner's descriptor in `__del__`.
const ANNOTATED: &str = r#"@thread_safe
pub extern def pid() -> i32 = "getpid"
@thread_safe
pub extern def dup(fd: i32) -> i32 = "dup"
@thread_safe
pub extern def close(fd: i32) -> i32 = "close"

pub class Fd:
    pub fd: i32

    def __del__(self: &mut Fd) -> ():
        if self.fd >= 0:
            print(close(self.fd)).unwrap()

pub def open_stdout() -> Fd:
    Fd(dup(1))
"#;

fn write_interface(dir: &Path, interface: &str) {
    support::write_file(&dir.join("native.plentyi"), interface);
}

fn run_with(interface: &str, source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    write_interface(dir.path(), interface);
    let output = std::process::Command::new(support::compile_file(dir.path(), source))
        .output()
        .unwrap();
    let output = support::assert_success(output);
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .map(|line| format!("{line}\n"))
        .collect()
}

fn check_with(interface: &str, source: &str) -> Result<(), String> {
    let dir = tempfile::tempdir().unwrap();
    write_interface(dir.path(), interface);
    let path = support::write_main(dir.path(), source);
    plenty::check_file(&path, None).map_err(|error| error.to_string())
}

fn rejected(interface: &str, source: &str, expected: &[&str]) {
    let error = check_with(interface, source).expect_err(source);
    for part in expected {
        assert!(error.contains(part), "expected {part:?}\n{source}\n{error}");
    }
}

/// `ANNOTATED` with the annotation removed from the declaration of `symbol`.
fn without(symbol: &str) -> String {
    ANNOTATED.replace(
        &format!("@thread_safe\npub extern def {symbol}"),
        &format!("pub extern def {symbol}"),
    )
}

#[test]
fn annotated_calls_and_destructors_run_on_spawned_and_pool_workers() {
    let source = r#"import native
from native import Fd

def same(main_pid: i32) -> bool:
    native.pid() == main_pid

def own(fd: Fd) -> i32:
    fd.fd

def main() -> Result[(), AllocError | IoError | SpawnError[()] | PoolError | SubmitError[()] | FutureError]:
    parent = native.pid()
    with spawn(same, parent).drop_job()? as task:
        print(task.join())?
    owner = native.open_stdout()
    number = owner.fd
    # The worker drops the owner, so its destructor closes the descriptor there.
    with spawn(own, owner).drop_job()? as task:
        print(task.join() == number)?
    print(native.close(number))?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(same, parent).drop_job()?
        print(future.result()?)?
    Ok(())
"#;
    assert_eq!(run_with(ANNOTATED, source), "True\n0\nTrue\n-1\nTrue\n");
}

#[test]
fn the_last_shared_holder_closes_once() {
    let source = r#"import native
from native import Fd

def hold(fd: Shared[Fd]) -> bool:
    fd.fd >= 0

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    owner = Shared(native.open_stdout())?
    with spawn(hold, owner.share()).drop_job()? as first, spawn(hold, owner).drop_job()? as second:
        print(first.join() and second.join())?
    print("joined")?
    Ok(())
"#;
    assert_eq!(run_with(ANNOTATED, source), "0\nTrue\njoined\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn a_failed_start_closes_the_moved_owner_once_on_the_parent() {
    let source = r#"import native
from native import Fd

def own(fd: Fd) -> i32:
    print("unexpected worker").unwrap()
    fd.fd

def start(owner: Fd) -> Result[(), SpawnError[()]]:
    with spawn(own, owner).drop_job()?:
        pass
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    owner = native.open_stdout()
    number = owner.fd
    print("__test_fail_thread_starts__")?
    started = start(owner)
    print("__test_restore_thread_starts__")?
    if started.is_err():
        print("failed")?
    print(native.close(number))?
    Ok(())
"#;
    assert_eq!(run_with(ANNOTATED, source), "0\nfailed\n-1\n");
}

#[test]
fn unannotated_calls_are_rejected_directly_transitively_and_in_destructors() {
    let direct = r#"import native
def work() -> i32:
    native.pid()
def main() -> Result[(), SpawnError[()]]:
    with spawn(work).drop_job()?:
        pass
    Ok(())
"#;
    rejected(
        &without("pid"),
        direct,
        &[
            "worker `work` reaches `native.pid`",
            "C function `getpid` has no worker-thread effect contract",
            "`@thread_safe`",
        ],
    );
    let helper = r#"import native
def middle() -> i32:
    native.pid()
def work() -> i32:
    middle()
def main() -> Result[(), SpawnError[()]]:
    with spawn(work).drop_job()?:
        pass
    Ok(())
"#;
    rejected(
        &without("pid"),
        helper,
        &["reaches `native.pid` through `middle`", "`getpid`"],
    );
    // The worker never calls C itself; dropping the moved owner would.
    let destructor = r#"import native
from native import Fd
def own(fd: Fd) -> i32:
    fd.fd
def main() -> Result[(), SpawnError[()]]:
    with spawn(own, native.open_stdout()).drop_job()?:
        pass
    Ok(())
"#;
    rejected(
        &without("close"),
        destructor,
        &[
            "reaches `native.close` through `native.Fd.__del__`",
            "`close`",
        ],
    );
    // A shared owner's last drop can happen on any holder's thread.
    let shared = r#"import native
from native import Fd
def hold(fd: Shared[Fd]) -> ():
    pass
def main() -> Result[(), AllocError | SpawnError[()]]:
    owner = Shared(native.open_stdout())?
    with spawn(hold, owner.share()).drop_job()?:
        pass
    Ok(())
"#;
    rejected(
        &without("close"),
        shared,
        &["`native.Fd.__del__`", "`close`"],
    );
    let channel = r#"import native
from native import Fd
def main() -> Result[(), AllocError]:
    sender, receiver = channel[Fd](1)?
    Ok(())
"#;
    rejected(
        &without("close"),
        channel,
        &["channel message", "`native.Fd.__del__`"],
    );
    let pool = r#"import native
def work(value: i32) -> bool:
    native.pid() > value
def main() -> Result[(), PoolError | SubmitError[()] | FutureError]:
    with ThreadPoolExecutor(1, 1)? as pool:
        drop(pool.submit(work, 1i32).drop_job()?.result()?)
    Ok(())
"#;
    rejected(&without("pid"), pool, &["executor worker", "`getpid`"]);
}

#[test]
fn indirect_targets_are_certified_per_declaration() {
    // Every address of `Callable[[], i32]` is a candidate, including the
    // unannotated `getpid` wrapper the worker never names.
    let source = r#"import native
def call(callback: Callable[[], i32]) -> i32:
    callback()
def main() -> Result[(), SpawnError[()]]:
    with spawn(call, native.pid).drop_job()?:
        pass
    Ok(())
"#;
    check_with(ANNOTATED, source).unwrap();
    rejected(&without("pid"), source, &["`getpid`"]);
}

#[test]
fn http_handlers_follow_the_same_contract() {
    let source = r#"import native
import std.http
import std.http.server
import std.net
from std.http.server import Exchange
from std.net import TcpStream
from std.sync import CancellationToken

pub enum Route:
    Home

pub class App:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), AllocError | std.http.HttpError]:
        if native.pid() > 0i32:
            exchange.send_text(200, "home")?
        Ok(())

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | AllocError]:
    mut table = std.http.server.router[Route]()?
    table.add("GET", "/", Route.Home)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    stop = CancellationToken()?
    stop.cancel()
    config = std.http.server.config()
    drop(std.http.server.serve(listener, &table, &App(), &config, &stop)?)
    Ok(())
"#;
    check_with(ANNOTATED, source).unwrap();
    rejected(&without("pid"), source, &["`getpid`"]);
}

/// A heap block owned through an opaque pointer, with the contract `CONTRACT`
/// on its declaration. `Block.__del__` releases it with `free`.
const BLOCKS: &str = r#"CONTRACT
pub opaque Raw
@thread_safe
pub extern def malloc(size: u64) -> Raw = "malloc"
@thread_safe
pub extern def free(raw: Raw) -> () = "free"

pub class Block:
    raw: Raw

    def __del__(self: &mut Block) -> ():
        free(self.raw)
        print("freed").unwrap()

    pub def ok(self) -> bool:
        not self.raw.is_null()

pub def block() -> Block:
    Block(malloc(16u64))
"#;

fn blocks(contract: &str) -> String {
    BLOCKS.replace("CONTRACT", contract)
}

#[test]
fn movable_owners_move_and_drop_on_workers() {
    let source = r#"import native
from native import Block

def own(block: Block) -> bool:
    block.ok()

def local() -> bool:
    native.block().ok()

def receive(receiver: Receiver[Block]) -> bool:
    receiver.recv().unwrap().ok()

def main() -> Result[(), AllocError | IoError | SpawnError[()] | SendError[Block]]:
    # The worker drops the moved owner, so `free` runs there.
    with spawn(own, native.block()).drop_job()? as task:
        print(task.join())?
    print("moved")?
    with spawn(local).drop_job()? as task:
        print(task.join())?
    print("local")?
    sender, receiver = channel[Block](1)?
    sender.send(native.block())?
    drop(sender)
    with spawn(receive, receiver).drop_job()? as task:
        print(task.join())?
    print("sent")?
    Ok(())
"#;
    assert_eq!(
        run_with(&blocks("@thread_movable"), source),
        "freed\nTrue\nmoved\nfreed\nTrue\nlocal\nfreed\nTrue\nsent\n"
    );
}

#[test]
fn a_mutex_lets_workers_share_a_movable_owner() {
    let source = r#"import native
from native import Block
from std.sync import Mutex

def use(block: Shared[Mutex[Block]]) -> bool:
    mut ok = False
    with block.lock() as owner:
        ok = owner.ok()
    ok

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    guarded = Shared(Mutex(native.block()))?
    with spawn(use, guarded.share()).drop_job()? as first, spawn(use, guarded).drop_job()? as second:
        print(first.join() and second.join())?
    print("joined")?
    Ok(())
"#;
    assert_eq!(
        run_with(&blocks("@thread_movable"), source),
        "freed\nTrue\njoined\n"
    );
}

#[test]
fn shareable_values_cross_by_copy_and_share() {
    let source = r#"import native
from native import Block, Raw

def hold(block: Shared[Block]) -> bool:
    block.ok()

def copied(raw: Raw) -> bool:
    not raw.is_null()

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    owner = Shared(native.block())?
    with spawn(hold, owner.share()).drop_job()? as first, spawn(hold, owner).drop_job()? as second:
        print(first.join() and second.join())?
    print("joined")?
    raw = native.malloc(8u64)
    with spawn(copied, raw).drop_job()? as task:
        print(task.join())?
    native.free(raw)
    Ok(())
"#;
    assert_eq!(
        run_with(&blocks("@thread_shareable"), source),
        "freed\nTrue\njoined\nTrue\n"
    );
}

#[test]
fn opaque_values_need_a_contract_for_their_crossing() {
    let local = r#"import native
def work() -> bool:
    native.block().ok()
def main() -> Result[(), SpawnError[()]]:
    with spawn(work).drop_job()?:
        pass
    Ok(())
"#;
    rejected(
        &blocks(""),
        local,
        &[
            "worker `work`",
            "native.Raw has no thread contract",
            "`@thread_movable` or `@thread_shareable`",
        ],
    );
    let movable = blocks("@thread_movable");
    let not_shareable = "is `@thread_movable`, not `@thread_shareable`";
    let bare = r#"import native
from native import Raw
def work(raw: Raw) -> ():
    pass
def main() -> Result[(), SpawnError[()]]:
    with spawn(work, native.malloc(8u64)).drop_job()?:
        pass
    Ok(())
"#;
    rejected(&movable, bare, &[not_shareable]);
    let shared = r#"import native
from native import Block
def hold(block: Shared[Block]) -> ():
    pass
def main() -> Result[(), AllocError | SpawnError[()]]:
    owner = Shared(native.block())?
    with spawn(hold, owner).drop_job()?:
        pass
    Ok(())
"#;
    rejected(&movable, shared, &[not_shareable]);
    let message = r#"import native
from native import Raw
def main() -> Result[(), AllocError]:
    sender, receiver = channel[Raw](1)?
    Ok(())
"#;
    rejected(&movable, message, &["channel message", not_shareable]);
    // A copy leaves the original pointer behind on the sending thread.
    let copy = format!("{movable}\n@copy\npub class Handle:\n    pub raw: Raw\n");
    let copied = r#"import native
from native import Handle
def work(handle: Handle) -> ():
    pass
def main() -> Result[(), SpawnError[()]]:
    with spawn(work, Handle(native.malloc(8u64))).drop_job()?:
        pass
    Ok(())
"#;
    rejected(&copy, copied, &[not_shareable]);
}

#[test]
fn contracts_are_declared_once_and_consistently() {
    let main = "import native\ndef main() -> ():\n    pass\n";
    for (interface, expected) in [
        (
            "@thread_movable\n@thread_shareable\nopaque Raw\n",
            "`@thread_shareable` already includes `@thread_movable`",
        ),
        (
            "@thread_movable\nextern def pid() -> i32 = \"getpid\"\n",
            "`@thread_movable` applies only to `opaque` declarations",
        ),
        (
            "@thread_shareable\nclass Box:\n    value: i64\n",
            "`@thread_shareable` applies only to `opaque` declarations",
        ),
        // The release function an owner's `__del__` calls is one of these.
        (
            "@thread_movable\nopaque Raw\nextern def free(raw: Raw) -> () = \"free\"\n",
            "C function taking or returning `native.Raw` must be `@thread_safe`",
        ),
        (
            "@thread_shareable\nopaque Raw\nextern def fill(out: &mut Raw) -> i32 = \"fill\"\n",
            "must be `@thread_safe`",
        ),
    ] {
        rejected(interface, main, &[expected]);
    }
    // Unmarked opaque types keep their unrestricted single-thread bindings.
    check_with(
        "opaque Raw\nextern def free(raw: Raw) -> () = \"free\"\n",
        main,
    )
    .unwrap();
}

#[test]
fn the_annotation_has_one_meaning_and_one_place() {
    let main = "import native\ndef main() -> ():\n    pass\n";
    for (interface, expected) in [
        (
            "@thread_safe\nopaque Raw\n",
            "`@thread_safe` applies only to `extern def` declarations; an `opaque` type takes `@thread_movable` or `@thread_shareable`",
        ),
        (
            "@thread_safe\nclass Box:\n    value: i64\n",
            "`@thread_safe` applies only to `extern def` declarations",
        ),
        (
            "@thread_safe\ndef helper() -> i64:\n    1\n",
            "`@thread_safe` applies only to `extern def` declarations",
        ),
        (
            "@copy\nextern def pid() -> i32 = \"getpid\"\n",
            "`@copy` applies only to class declarations",
        ),
        (
            "opaque Function\n@thread_safe\nextern def invoke(function: Function) -> i32 = function\n",
            "`@thread_safe` requires a C symbol",
        ),
        (
            "@thread_safe\nextern def a() -> i32 = \"getpid\"\nextern def b() -> i32 = \"getpid\"\n",
            "conflicting `@thread_safe` declarations for symbol `getpid`",
        ),
        (
            "@thread_safe\n@thread_safe\nextern def a() -> i32 = \"getpid\"\n",
            "duplicate annotation `@thread_safe`",
        ),
    ] {
        rejected(interface, main, &[expected]);
    }
    // Agreeing duplicate declarations remain valid.
    check_with(
        "@thread_safe\nextern def a() -> i32 = \"getpid\"\n@thread_safe\nextern def b() -> i32 = \"getpid\"\n",
        main,
    )
    .unwrap();
}
