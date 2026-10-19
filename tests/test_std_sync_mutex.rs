//! `std.sync.Mutex`: lexically scoped locks, unlock on every exit, and the
//! compile-time checks for waits that could never finish.
mod support;
use support::check_file_error as rejected;

const HEADER: &str = "from std.sync import Mutex\n";

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{HEADER}def main() -> Result[(), Failure]:\n{body}    Ok(())\n")
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
        .trim_end()
        .to_string()
}

fn rejects(source: &str, message: &str) {
    let error = rejected(source);
    assert!(error.contains(message), "{source}\n{error}");
}

fn rejects_body(body: &str, message: &str) {
    rejects(&program(body), message);
}

#[test]
fn workers_update_a_compound_record_together() {
    let source = format!(
        r#"{HEADER}
class Stats:
    count: i64
    total: i64
    torn: bool

def record(stats: &Mutex[Stats], value: i64) -> ():
    for _ in range(100000):
        with stats.lock() as data:
            data.count = data.count + 1
            data.total = data.total + value

def check(stats: &Mutex[Stats], rounds: i64) -> ():
    for _ in range(rounds):
        with stats.lock() as data:
            # Every worker adds 3 or 4 per count; the sum of both always
            # lies between those bounds when observed under the lock.
            if data.total < 3 * data.count or data.total > 4 * data.count:
                data.torn = True

def main() -> Result[(), Failure]:
    stats = Mutex[Stats](Stats(0, 0, False))
    with spawn(record, &stats, 3)?, spawn(record, &stats, 4)?, spawn(record, &stats, 3)?, spawn(record, &stats, 4)?, spawn(check, &stats, 50000)?:
        check(&stats, 50000)
    final = stats.into_inner()
    print(final.count)?
    print(final.total)?
    print(final.torn)?
    Ok(())
"#
    );
    assert_eq!(output(&source), "400000\n1400000\nFalse");
}

#[test]
fn every_exit_unlocks() {
    let source = format!(
        r#"{HEADER}
def early(m: &Mutex[i64]) -> i64:
    with m.lock() as v:
        *v = *v + 1
        if *v > 0:
            return *v
    0

def failing(m: &Mutex[i64]) -> Result[i64, str]:
    with m.lock() as v:
        *v = *v + 1
        bad: Result[i64, str] = Err("stopped")
        bad?
    Ok(0)

def main() -> Result[(), Failure]:
    m = Mutex(0)
    print(early(&m))?
    match failing(&m):
        case Ok(_):
            pass
        case Err(error):
            print(error)?
    for i in range(5):
        with m.lock() as v:
            *v = *v + 1
            if i == 1:
                continue
            if i == 3:
                break
    with m.lock() as v:
        print(*v)?
    mut i = 0
    while True:
        i = i + 1
        with m.lock() as v:
            *v = *v + 10
            if i == 2:
                continue
            if i == 4:
                break
    with m.lock():
        pass
    print(m.into_inner())?
    Ok(())
"#
    );
    assert_eq!(output(&source), "1\nstopped\n6\n46");
}

#[test]
fn mutexes_live_in_ordinary_owners() {
    let source = format!(
        r#"{HEADER}
class Noisy:
    label: str

    def __del__(self: &mut Noisy) -> ():
        print("dropped").unwrap()

class Pair:
    items: Mutex[list[i64]]
    size: Mutex[i64]

def put[T](m: &Mutex[list[T]], value: T) -> Result[(), AllocError]:
    with m.lock() as values:
        values.append(value)?
    Ok(())

def main() -> Result[(), Failure]:
    shared = Pair(Mutex([1]?), Mutex(0))
    put(&shared.items, 5)?
    with shared.items.lock() as items, shared.size.lock() as size:
        *size = len(*items)
    with shared.size.lock() as size:
        print(*size)?
    boxed = Box(Mutex(7))?
    with (*boxed).lock() as v:
        *v = *v * 2
    with (*boxed).lock() as v:
        print(*v)?
    cells = [Mutex(1), Mutex(2)]?
    with cells[1].lock() as v:
        *v = 10
    for i in range(2):
        with cells[i].lock() as v:
            print(*v)?
    moved = Mutex[str]("text")
    again = moved
    print(again.into_inner())?
    noisy = Mutex(Noisy("kept"))
    inner = noisy.into_inner()
    print(inner.label)?
    Ok(())
"#
    );
    assert_eq!(output(&source), "2\n14\n1\n10\ntext\nkept\ndropped");
}

#[test]
fn borrowed_mutexes_lock_through_references() {
    let source = format!(
        r#"{HEADER}
def bump(m: &mut Mutex[i64]) -> ():
    with m.lock() as v:
        *v = *v + 1
    with (*m).lock() as v:
        *v = *v + 1

def main() -> Result[(), Failure]:
    mut m = Mutex(1)
    bump(&mut m)
    m = Mutex(10)
    bump(&mut m)
    print(m.into_inner())?
    Ok(())
"#
    );
    assert_eq!(output(&source), "12");
}

#[test]
fn spawning_outside_the_lock_is_accepted() {
    let source = format!(
        r#"{HEADER}
def work(m: &Mutex[i64]) -> ():
    with m.lock() as v:
        *v = *v + 1

def main() -> Result[(), Failure]:
    m = Mutex(1)
    with spawn(work, &m)? as task:
        with m.lock() as v:
            *v = *v + 1
        task.join()
    print(m.into_inner())?
    Ok(())
"#
    );
    assert_eq!(output(&source), "3");
}

#[test]
fn locks_nest_and_workers_start_inside_them() {
    let source = format!(
        r#"{HEADER}
def double(value: i64) -> i64:
    value * 2

def main() -> Result[(), Failure]:
    nested = Mutex(Mutex(3))
    with nested.lock() as inner:
        with inner.lock() as value:
            *value = *value + 1
    counter = Mutex(5)
    with counter.lock() as value:
        with spawn(double, *value)? as task:
            *value = task.join()
    print(nested.into_inner().into_inner())?
    print(counter.into_inner())?
    Ok(())
"#
    );
    assert_eq!(output(&source), "4\n10");
}

/// Locking allocates nothing, and `into_inner` moves a heap value out
/// without leaking or releasing it twice.
#[cfg(feature = "runtime-checks")]
#[test]
fn locks_allocate_nothing_and_into_inner_keeps_the_heap_balanced() {
    let source = format!(
        r#"{HEADER}
def main() -> Result[(), Failure]:
    words = Mutex([f"word {{n}}"? for n in range(2000)]?)
    print("__test_begin_no_allocations__")?
    with words.lock() as values:
        values.reverse()
    print("__test_end_no_allocations__")?
    values = words.into_inner()
    print(values[0])?
    drop(values)
    print("__test_small_live_heap__")?
    Ok(())
"#
    );
    let visible: Vec<_> = output(&source)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .map(str::to_owned)
        .collect();
    assert_eq!(visible, ["word 1999"]);
}

#[test]
fn import_aliases_name_the_same_type() {
    let source = r#"import std.sync as sync
from std.sync import Mutex as Guarded

def main() -> Result[(), Failure]:
    a = sync.Mutex[i64](1)
    b: Guarded[i64] = Guarded(2)
    mut extra = 0
    with b.lock() as y:
        extra = *y
    with a.lock() as x:
        *x = *x + extra
    print(a.into_inner())?
    Ok(())
"#;
    assert_eq!(output(source), "3");
    rejects(
        "def main() -> ():\n    m = Mutex(1)\n",
        "unknown function `Mutex`",
    );
}

#[test]
fn relocking_in_the_same_block_is_rejected() {
    let relock = "would wait for itself forever; use the existing binding, or release the lock before relocking";
    rejects_body(
        "m = Mutex(1)\nwith m.lock() as a:\n    with m.lock() as b:\n        pass",
        &format!("`m` is already locked by an enclosing `with` block, so this lock {relock}"),
    );
    rejects_body(
        "m = Mutex(1)\nwith m.lock() as a, m.lock():\n    pass",
        "`m` is already locked",
    );
    rejects(
        &format!(
            "{HEADER}class Pair:\n    a: Mutex[i64]\n\ndef f(p: &Pair) -> ():\n    with p.a.lock():\n        with (*p).a.lock():\n            pass\n\ndef main() -> ():\n    pass\n"
        ),
        "`p.a` is already locked",
    );
}

#[test]
fn joining_while_holding_a_reachable_lock_is_rejected() {
    let worker =
        "def work(m: &Mutex[i64]) -> ():\n    with m.lock() as v:\n        *v = *v + 1\n\n";
    let hang = "a task whose worker can lock a `Mutex[i64]` while this function holds a lock on a `Mutex[i64]`: the worker could wait for that lock, and this join, forever";
    rejects(
        &format!(
            "{HEADER}{worker}def main() -> Result[(), Failure]:\n    m = Mutex(1)\n    with m.lock() as a:\n        with spawn(work, &m)?:\n            pass\n    Ok(())\n"
        ),
        &format!("leaving this `with` block joins {hang}"),
    );
    rejects(
        &format!(
            "{HEADER}{worker}def main() -> Result[(), Failure]:\n    m = Mutex(1)\n    with spawn(work, &m)? as task:\n        with m.lock() as a:\n            task.join()\n    Ok(())\n"
        ),
        &format!("this join waits for {hang}"),
    );
    rejects(
        &format!(
            "{HEADER}{worker}def main() -> Result[(), Failure]:\n    m = Mutex(1)\n    with m.lock() as a:\n        with spawn(work, &m)?:\n            pass\n    Ok(())\n"
        ),
        "note: the lock is acquired here; end the lock's `with` block before this one, or start the task outside it",
    );
    rejects(
        &format!(
            "{HEADER}{worker}def main() -> Result[(), Failure]:\n    m = Mutex(1)\n    with m.lock() as v, spawn(work, &m)?:\n        pass\n    Ok(())\n"
        ),
        &format!("leaving this `with` block joins {hang}"),
    );
}

#[test]
fn yield_inside_a_lock_is_rejected() {
    rejects(
        &format!(
            "{HEADER}def values() -> Generator[i64]:\n    m = Mutex(1)\n    with m.lock() as v:\n        yield *v\n\ndef main() -> ():\n    pass\n"
        ),
        "yield inside a mutex lock block would keep the lock held while the generator is suspended; release the lock before yielding",
    );
}

#[test]
fn lock_is_only_a_with_manager() {
    let form = "lock() is valid only as a `with` manager: `with mutex.lock() as value:`";
    rejects_body("m = Mutex(1)\nm.lock()", form);
    rejects_body("x = Mutex(1).lock()", form);
    rejects(
        &format!(
            "{HEADER}def make() -> Mutex[i64]:\n    Mutex(1)\n\ndef main() -> ():\n    with make().lock() as v:\n        pass\n"
        ),
        "lock() requires a named mutex or a field of one; bind the mutex to a name first",
    );
    rejects_body(
        "m = Mutex(1)\nwith m.lock(1) as v:\n    pass",
        "lock takes no arguments",
    );
    rejects_body(
        "m = Mutex(1)\nm.get()",
        "Mutex has no method `get`; it supports `with mutex.lock() as value:` and into_inner()",
    );
}

#[test]
fn the_locked_reference_stays_in_its_block() {
    rejects_body(
        "m = Mutex(1)\nwith m.lock() as v:\n    kept = [v]?",
        "references cannot be stored in collections",
    );
    rejects(
        &format!(
            "{HEADER}def keep(m: &mut Mutex[i64]) -> &mut i64:\n    with m.lock() as v:\n        return v\n\ndef main() -> ():\n    pass\n"
        ),
        "returned reference must originate from the reference parameter",
    );
    rejects_body(
        "m = Mutex(1)\nwith m.lock() as v:\n    f = def() -> i64:\n        *v\n    f()",
        "anonymous functions cannot capture `v`",
    );
}

#[test]
fn a_locked_mutex_cannot_move() {
    rejects_body(
        "m = Mutex(1)\nwith m.lock() as v:\n    n = m",
        "conflicting borrow: cannot move `m` while it is borrowed",
    );
    rejects_body(
        "m = Mutex(1)\nwith m.lock() as v:\n    m.into_inner()",
        "conflicting borrow: cannot move `m` while it is borrowed",
    );
    rejects(
        &format!(
            "{HEADER}def f(m: &Mutex[i64]) -> i64:\n    m.into_inner()\n\ndef main() -> ():\n    pass\n"
        ),
        "into_inner consumes an owned mutex, not a reference",
    );
}

#[test]
fn mutexes_have_no_value_operations() {
    rejects_body(
        "m = Mutex(1)\nprint(m == m)?",
        "values containing mutexes do not support equality",
    );
    rejects_body("m = Mutex(1)\nprint(m)?", "mutexes cannot be printed");
    rejects_body(
        "m = Mutex(1)\nprint(f\"{m}\"?)?",
        "mutexes cannot be formatted",
    );
    rejects_body("m = Mutex(1)\nn = copy(m)", "cannot be copied");
    rejects_body(
        "m = Mutex(1)\nprint(m.value)?",
        "a Mutex's fields are private; reach its value with `with mutex.lock() as value:`",
    );
}
