//! `Shared[T]`: counted shared owners, shared access rooted at the handle,
//! `with handle.lock()`, the compile-time cycle and eligibility rules, and the
//! lock-held checks at future waits and pool shutdown.
mod support;
use support::check_file_error as rejected;

const HEADER: &str = "\
from std.atomic import Atomic, Ordering
from std.sync import CancellationToken, Mutex
from std.concurrent import ThreadPoolExecutor, Future, SubmitError
from std.fmt import FormatError

class Point:
    x: i64
    y: i64
    tags: list[str]

    def sum(self) -> i64:
        return self.x + self.y

    def shift(self: &mut Point) -> ():
        self.x = self.x + 1

class Noisy:
    label: str
    drops: Shared[Atomic[u64]]

    def __del__(self: &mut Noisy) -> ():
        self.drops.fetch_add(1, Ordering.Relaxed)
        print(\"dropped\", self.label).unwrap()

def submitted[T, F](result: Result[Future[T], SubmitError[F]]) -> Result[Future[T], str]:
    match result:
        case Ok(future):
            Ok(future)
        case Err(_):
            Err(\"submission failed\")
";

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{HEADER}\ndef main() -> Result[(), AllocError | IoError | PoolError | FormatError]:\n{body}    Ok(())\n")
}

fn output(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new("timeout")
        .arg("60s")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn rejects(source: &str, message: &str) {
    let error = rejected(source);
    assert!(error.contains(message), "{source}\n{error}");
}

fn rejects_body(body: &str, message: &str) {
    rejects(&program(body), message);
}

#[test]
fn the_last_owner_runs_the_destructor_once() {
    let source = program(
        r#"
drops = Shared[Atomic[u64]](Atomic(0))?
a = Shared(Noisy("state", drops.share()))?
b = a.share()
c = b.share()
drop(a)
print("a gone")?
drop(c)
print("c gone")?
print(b.label)?
drop(b)
print(drops.load(Ordering.Relaxed))?
"#,
    );
    assert_eq!(output(&source), "a gone\nc gone\nstate\ndropped state\n1");
}

#[test]
fn fields_methods_and_references_borrow_through_the_handle() {
    let source = program(
        r#"
h = Shared(Point(3, 4, ["a", "b"]?))?
print(h.x)?
print(h.sum())?
print(len(h.tags))?
print(h.tags[1])?
r = &*h
print(r.y)?
print(total(&*h))?
n = Shared[i64](7)?
print(*n + 1)?
boxed = Shared(Box(Point(1, 1, []?))?)?
print(boxed.x)?
"#,
    )
    .replace(
        "\ndef main()",
        "\ndef total(p: &Point) -> i64:\n    return p.x * p.y\n\ndef main()",
    );
    assert_eq!(output(&source), "3\n7\n2\nb\n4\n12\n8\n1");
}

#[test]
fn a_borrowed_handle_cannot_be_moved_or_dropped() {
    rejects_body(
        "h = Shared(Point(1, 2, []?))?\nr = &*h\ndrop(h)\nprint(r.x)?",
        "cannot move `h` while it is borrowed",
    );
    rejects_body(
        "h = Shared(Point(1, 2, []?))?\nr = &h.tags\nk = h\nprint(len(*r))?",
        "cannot move `h` while `h.tags` is borrowed",
    );
}

#[test]
fn the_content_is_shared_only() {
    let shared = "a Shared owner gives shared access only";
    for body in [
        "y = &mut *h",
        "h.x = 5",
        "h.tags.append(\"c\")?",
        "h.shift()",
        "(*h).shift()",
    ] {
        rejects_body(
            &format!("mut h = Shared(Point(1, 2, []?))?\n{body}"),
            shared,
        );
    }
    rejects_body(
        "h = Shared(Point(1, 2, []?))?\np = *h",
        "cannot move out of a Shared owner",
    );
}

#[test]
fn handles_have_no_equality_formatting_or_copy() {
    for (body, message) in [
        (
            "b = h == g",
            "values containing Shared owners do not support equality",
        ),
        (
            "q = [h]?\nb = g in q",
            "values containing Shared owners do not support equality",
        ),
        ("print(h)?", "Shared owners cannot be printed"),
        ("s = f\"{h}\"?", "Shared owners cannot be formatted"),
        ("s = str.repr(h)?", "Shared owners cannot be formatted"),
        (
            "c = copy(h)?",
            "values containing Shared owners cannot be copied; call share()",
        ),
    ] {
        rejects_body(
            &format!("h = Shared(Point(1, 2, []?))?\ng = h.share()\n{body}"),
            message,
        );
    }
    // Copyable content reads and copies through the handle.
    let source = program("h = Shared(Point(1, 2, [\"t\"]?))?\nc = copy(*h)?\nprint(c)?");
    assert_eq!(output(&source), "Point(x=1, y=2, tags=[\"t\"])");
}

#[test]
fn executors_and_futures_cannot_be_shared() {
    rejects_body(
        "s = Shared(ThreadPoolExecutor(2, 2)?)?",
        "Shared[ThreadPoolExecutor] cannot hold ThreadPoolExecutor",
    );
    rejects(
        &format!(
            "{HEADER}\nclass Holder:\n    pending: Option[Future[i64]]\n\ndef main() -> Result[(), AllocError]:\n    s = Shared(Holder(Nothing))?\n    Ok(())\n"
        ),
        "cannot hold Future[i64]",
    );
}

#[test]
fn a_share_that_could_own_itself_is_rejected() {
    let cycle =
        "so a value could hold a share of itself and never be released; link back with an index";
    rejects(
        &format!(
            "{HEADER}\nclass Node:\n    links: Mutex[list[Shared[Node]]]\n\ndef main() -> Result[(), AllocError]:\n    n = Shared(Node(Mutex[list[Shared[Node]]]([]?)))?\n    Ok(())\n"
        ),
        &format!("Shared[Node] can reach itself through Mutex[list[Shared[Node]]], {cycle}"),
    );
    rejects(
        &format!(
            "{HEADER}\nclass Inbox:\n    queue: Receiver[Shared[Inbox]]\n\ndef main() -> Result[(), AllocError | ChannelError]:\n    tx, rx = channel[Shared[Inbox]](1)?\n    n = Shared(Inbox(rx))?\n    Ok(())\n"
        ),
        &format!("Shared[Inbox] can reach itself through Receiver[Shared[Inbox]], {cycle}"),
    );
    rejects(
        &format!(
            "{HEADER}\nclass Left:\n    right: Mutex[Option[Shared[Right]]]\n\nclass Right:\n    left: Shared[Left]\n\ndef main() -> Result[(), AllocError]:\n    l = Shared(Left(Mutex[Option[Shared[Right]]](Nothing)))?\n    Ok(())\n"
        ),
        &format!("Shared[Left] can reach itself through Mutex[Option[Shared[Right]]], {cycle}"),
    );
    rejects_body(
        "callbacks = Shared(Mutex[list[BoxedCallable[[], i64]]]([]?))?",
        "can store a BoxedCallable[[], i64] through Mutex[list[BoxedCallable[[], i64]]], and its captures cannot be checked",
    );
    // An immutable DAG cannot: each value exists before any share of it.
    let source = format!(
        "{HEADER}\nclass Tree:\n    value: i64\n    kids: list[Shared[Tree]]\n\ndef main() -> Result[(), AllocError | IoError]:\n    leaf = Shared(Tree(1, []?))?\n    left = Shared(Tree(2, [leaf.share()]?))?\n    root = Tree(3, [left.share(), leaf.share()]?)\n    print(root.kids[0].kids[0].value + root.kids[1].value)?\n    Ok(())\n"
    );
    assert_eq!(output(&source), "2");
}

#[test]
fn pool_jobs_share_an_atomic_and_a_mutex() {
    let source = format!(
        r#"{HEADER}
class Stats:
    count: i64
    total: i64

def work(hits: Shared[Atomic[u64]], stats: Shared[Mutex[Stats]], rounds: i64) -> i64:
    for i in range(rounds):
        hits.fetch_add(1, Ordering.Relaxed)
        with stats.lock() as data:
            data.count = data.count + 1
            data.total = data.total + i
    rounds

def main() -> Result[(), AllocError | FutureError | IoError | PoolError | str]:
    hits = Shared[Atomic[u64]](Atomic(0))?
    stats = Shared[Mutex[Stats]](Mutex(Stats(0, 0)))?
    with ThreadPoolExecutor(4, 4)? as pool:
        mut futures: list[Future[i64]] = []?
        for _ in range(4):
            futures.append(submitted(pool.submit(work, hits.share(), stats.share(), 20000))?)?
        while True:
            match futures.pop():
                case Some(future):
                    print(future.result()?)?
                case Nothing:
                    break
    print(hits.load(Ordering.Acquire))?
    with stats.lock() as data:
        print(data.count)?
        print(data.total)?
    Ok(())
"#
    );
    assert_eq!(
        output(&source),
        "20000\n20000\n20000\n20000\n80000\n80000\n799960000"
    );
}

#[test]
fn the_last_owner_can_drop_on_a_worker() {
    let source = format!(
        r#"{HEADER}
def hold(state: Shared[Noisy], stop: CancellationToken) -> i64:
    stop.wait()
    len(state.label)

def main() -> Result[(), AllocError | FutureError | IoError | PoolError | str]:
    drops = Shared[Atomic[u64]](Atomic(0))?
    stop = CancellationToken()?
    state = Shared(Noisy("pooled", drops.share()))?
    with ThreadPoolExecutor(2, 2)? as pool:
        first = submitted(pool.submit(hold, state.share(), stop.share()))?
        second = submitted(pool.submit(hold, state.share(), stop.share()))?
        drop(state)
        print(drops.load(Ordering.Acquire))?
        stop.cancel()
        print(first.result()? + second.result()?)?
    print(drops.load(Ordering.Acquire))?
    Ok(())
"#
    );
    assert_eq!(output(&source), "0\ndropped pooled\n12\n1");
}

#[test]
fn scoped_workers_take_shares_and_borrows() {
    let source = format!(
        r#"{HEADER}
def bump(counter: Shared[Mutex[i64]], rounds: i64) -> ():
    for _ in range(rounds):
        with counter.lock() as value:
            *value = *value + 1

def peek(counter: &Shared[Mutex[i64]]) -> ():
    with counter.lock() as value:
        *value = *value + 100

def main() -> Result[(), AllocError | IoError | ThreadError]:
    counter = Shared(Mutex(0))?
    with spawn(bump, counter.share(), 10000)?, spawn(bump, counter.share(), 10000)?:
        pass
    peek(&counter)
    with counter.lock() as value:
        print(*value)?
    Ok(())
"#
    );
    assert_eq!(output(&source), "20100");
}

#[test]
fn a_nested_lock_of_the_same_type_is_rejected_when_it_could_alias() {
    let alias = "both are a `Mutex[i64]`, and a reference, shared handle, or computed index can make them one mutex";
    rejects_body(
        "a = Shared(Mutex(0))?\nb = a.share()\nwith a.lock() as x:\n    with b.lock() as y:\n        *y = 1",
        &format!("this locks `b` while an enclosing `with` block holds a lock on `a`; {alias}"),
    );
    rejects(
        &format!(
            "{HEADER}\ndef add(m: &Mutex[i64]) -> ():\n    n = Mutex(2)\n    with m.lock() as a:\n        with n.lock() as b:\n            *a = *a + *b\n\ndef main() -> ():\n    pass\n"
        ),
        &format!("this locks `n` while an enclosing `with` block holds a lock on `m`; {alias}"),
    );
    rejects_body(
        "cells = [Mutex(1), Mutex(2)]?\ni = 0\nj = 1\nwith cells[i].lock() as x, cells[j].lock() as y:\n    pass",
        &format!("this locks `cells[?]` while an enclosing `with` block holds a lock on `cells[?]`; {alias}"),
    );
    // Distinct owned places are distinct mutexes.
    let source = program(
        "a = Mutex(1)\nb = Mutex(2)\nwith a.lock() as x, b.lock() as y:\n    *x = *x + *y\ncells = [Mutex(3), Mutex(4)]?\nwith cells[0].lock() as x, cells[1].lock() as y:\n    *x = *x + *y\nprint(a.into_inner())?\nwith cells[0].lock() as x:\n    print(*x)?",
    );
    assert_eq!(output(&source), "3\n7");
    // Distinct guarded types may nest.
    let source = program(
        "a = Shared(Mutex(1))?\nb = Shared(Mutex[str](\"s\"))?\nwith a.lock() as x, b.lock() as y:\n    *x = *x + len(*y)\nwith a.lock() as x:\n    print(*x)?",
    );
    assert_eq!(output(&source), "2");
}

#[test]
fn waits_while_holding_a_lock_a_job_can_take_are_rejected() {
    let job = "
def bump(counter: Shared[Mutex[i64]]) -> i64:
    with counter.lock() as value:
        *value = *value + 1
    0
";
    let with_job = |body: &str| {
        let body = body
            .trim_matches('\n')
            .lines()
            .map(|line| format!("    {line}\n"))
            .collect::<String>();
        format!("{HEADER}{job}\ndef main() -> Result[(), AllocError | FutureError | IoError | PoolError | ThreadError | str]:\n{body}    Ok(())\n")
    };
    rejects(
        &with_job(
            "counter = Shared(Mutex(0))?\nwith ThreadPoolExecutor(2, 2)? as pool:\n    match pool.submit(bump, counter.share()):\n        case Ok(future):\n            with counter.lock() as value:\n                print(future.result()?)?\n        case Err(_):\n            pass",
        ),
        "this result() waits for a job that can lock a `Mutex[i64]` while this function holds a lock on a `Mutex[i64]`",
    );
    rejects(
        &with_job(
            "counter = Shared(Mutex(0))?\npool = ThreadPoolExecutor(2, 2)?\ndrop(pool.submit(bump, counter.share()))\nwith counter.lock() as value:\n    pool.shutdown()",
        ),
        "this shuts down a pool after a job that can lock a `Mutex[i64]` was submitted, while this function holds a lock on a `Mutex[i64]`",
    );
    rejects(
        &with_job(
            "counter = Shared(Mutex(0))?\nwith counter.lock() as value:\n    with ThreadPoolExecutor(2, 2)? as pool:\n        drop(pool.submit(bump, counter.share()))",
        ),
        "leaving its scope shuts down this pool after a job that can lock a `Mutex[i64]` was submitted",
    );
    rejects(
        &with_job(
            "counter = Shared(Mutex(0))?\nwith counter.lock() as value:\n    with spawn(bump, counter.share())? as task:\n        print(task.join())?",
        ),
        "this join waits for a task whose worker can lock a `Mutex[i64]` while this function holds a lock on a `Mutex[i64]`",
    );
    // Waiting after the lock's block ends is accepted.
    let source = with_job(
        "counter = Shared(Mutex(0))?\nwith ThreadPoolExecutor(2, 2)? as pool:\n    future = submitted(pool.submit(bump, counter.share()))?\n    with counter.lock() as value:\n        *value = *value + 10\n    print(future.result()?)?\nwith counter.lock() as value:\n    print(*value)?",
    );
    assert_eq!(output(&source), "0\n11");
}

#[test]
fn temporaries_and_misuse_have_specific_diagnostics() {
    rejects_body(
        "n = copy(*Shared(5)?)",
        "a temporary Shared owner has no place to borrow from; bind it to a name first",
    );
    rejects_body(
        "h = Shared(Mutex(0))?\nh.lock()",
        "lock() is valid only as a `with` manager",
    );
    rejects_body("h = Shared(1)?\ng = h.share(2)", "share takes no arguments");
    rejects_body("h = Shared[i64](\"text\")?", "expected i64, got str");
    rejects_body(
        "h = Shared([1, 2]?)?\nh[0] = 9",
        "a Shared owner gives shared access only",
    );
    rejects_body(
        "h = Shared(([\"a\"]?, 1))?\nx = h[0]",
        "cannot move an element out of a Shared owner; borrow it with `&handle[i]`",
    );
    rejects_body(
        "x = 1\ny = *x",
        "dereference requires a reference, a Box, or a Shared handle",
    );
    rejects_body(
        "h = Shared(Mutex(1))?\nv = h.into_inner()",
        "cannot move out of a Shared owner; other owners may still use its Mutex[i64]",
    );
    rejects_body(
        "h = Shared(Mutex(1))?\nwith h.lock() as v:\n    drop(h)",
        "cannot move `h` while it is borrowed",
    );
    rejects(
        &format!(
            "{HEADER}\ndef take[T](h: Shared[T]) -> T:\n    return *h\n\ndef main() -> Result[(), AllocError]:\n    s = take(Shared([1]?)?)\n    Ok(())\n"
        ),
        "cannot move out of a Shared owner",
    );
    rejects(
        &format!(
            "{HEADER}\ndef bump(counter: Shared[Mutex[i64]]) -> i64:\n    0\n\ndef main() -> Result[(), AllocError | PoolError]:\n    h = Shared(Mutex(1))?\n    with ThreadPoolExecutor(1, 1)? as pool:\n        with h.lock() as v:\n            drop(pool.submit(bump, h))\n    Ok(())\n"
        ),
        "cannot move `h` while it is borrowed",
    );
    // Copyable elements copy out; nested handles lock through every level.
    let source = program(
        "h = Shared(([\"a\"]?, 7))?\nprint(h[1])?\nprint(copy(h[0])?)?\nn = Shared(Shared(Mutex(1))?)?\nwith n.lock() as v:\n    *v = 5\nwith n.lock() as v:\n    print(*v)?",
    );
    assert_eq!(output(&source), "7\n[\"a\"]\n5");
}

/// `share()` and borrows allocate nothing; a failed construction drops its
/// value; the last release frees the allocation.
#[cfg(feature = "runtime-checks")]
#[test]
fn sharing_allocates_once_and_releases_everything() {
    let source = program(
        r#"
drops = Shared[Atomic[u64]](Atomic(0))?
words = Shared([f"word {n}"? for n in range(2000)]?)?
print("__test_begin_no_allocations__")?
more = words.share()
first = len(more[0])
drop(more)
print("__test_end_no_allocations__")?
print(first)?
print("__test_fail_allocations_after_0__")?
match Shared(Counted(drops.share())):
    case Ok(_):
        print("allocated")?
    case Err(_):
        print("failed")?
print("__test_restore_allocations__")?
print(drops.load(Ordering.Relaxed))?
drop(words)
print("__test_small_live_heap__")?
"#,
    )
    .replace(
        "\ndef main()",
        "\nclass Counted:\n    drops: Shared[Atomic[u64]]\n\n    def __del__(self: &mut Counted) -> ():\n        self.drops.fetch_add(1, Ordering.Relaxed)\n        pass\n\ndef main()",
    );
    assert_eq!(output(&source), "6\nfailed\n1");
}
