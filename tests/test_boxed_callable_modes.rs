//! `BoxedMutCallable` and `BoxedOnceCallable` (issue #871).
mod support;

fn run(source: &str, expected: &str) {
    support::assert_stdout(source, expected);
}

#[test]
fn mutable_boxes_keep_distinct_state_per_factory_call() {
    run(
        r#"
def counter(start: i64, step: i64) -> Result[BoxedMutCallable[[], Result[i64, OverflowError[i64]]], AllocError]:
    tick = def [mut start, step]() -> Result[i64, OverflowError[i64]]:
        start = start + step
        Ok(start)
    BoxedMutCallable(tick)

def doubler() -> Result[BoxedMutCallable[[], Result[i64, OverflowError[i64]]], AllocError]:
    mut value = 1
    tick = def [mut value]() -> Result[i64, OverflowError[i64]]:
        value = value * 2
        Ok(value)
    BoxedMutCallable(tick)

def fixed() -> Result[i64, OverflowError[i64]]:
    Ok(7)

def run_all(callbacks: &mut list[BoxedMutCallable[[], Result[i64, OverflowError[i64]]]]) -> Result[(), IoError | OverflowError[i64]]:
    for callback in callbacks:
        print(callback()?)?
    Ok(())

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    shared = def [](n: i64) -> Result[i64, OverflowError[i64]]:
        Ok(n)
    mut callbacks = [counter(0, 1)?, counter(0, 1)?, doubler()?, BoxedMutCallable(fixed)?]?
    run_all(&mut callbacks)?
    run_all(&mut callbacks)?
    mut first = callbacks.pop(0).unwrap()
    print(first()?)?
    mut reuse = BoxedMutCallable(shared)?
    print(reuse(5)?)?
    xs = [1, 2]?
    double = def(n: &i64) -> i64:
        *n
    mapper = BoxedMutCallable(double)?
    ys: list[i64] = iter(&xs).map(mapper).collect()?
    print(ys)?
    Ok(())
"#,
        "1\n1\n2\n7\n2\n2\n4\n7\n3\n5\n[1, 2]\n",
    );
}

#[test]
fn one_shot_boxes_run_each_job_once_and_free_it() {
    run(
        r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        write_stdout("drop ").unwrap()
        print(self.id).unwrap()

def consume(guard: Guard) -> i64:
    guard.id

def report() -> i64:
    100

def job(id: i64) -> Result[BoxedOnceCallable[[], i64], AllocError]:
    guard = Guard(id)
    run = def once [guard]() -> i64:
        consume(guard)
    BoxedOnceCallable(run)

def reusable(id: i64) -> Result[BoxedOnceCallable[[], i64], AllocError]:
    guard = Guard(id)
    run = def [guard]() -> i64:
        guard.id
    BoxedOnceCallable(run)

def main() -> Result[(), AllocError | IoError]:
    mut jobs = [job(1)?, reusable(2)?, BoxedOnceCallable(report)?, job(3)?]?
    unused = jobs.pop().unwrap()
    for next in jobs:
        print(next())?
    print(0)?
    drop(unused)
    print(job(4)?())?
    Ok(())
"#,
        "drop 1\n1\ndrop 2\n2\n100\n0\ndrop 3\ndrop 4\n4\n",
    );
}

#[test]
fn one_shot_boxes_take_owned_arguments_and_return_inline_values() {
    run(
        r#"
def total(values: list[i64], extra: i64) -> Result[tuple[i64, i64], AllocError | OverflowError[i64]]:
    mut sum = extra
    for value in &values:
        sum = sum + value
    Ok((sum, len(values)))

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    prefix = [1, 2]?
    joined = def once [prefix](values: list[i64], extra: i64) -> Result[tuple[i64, i64], AllocError | OverflowError[i64]]:
        mut all = prefix
        for value in values:
            all.append(value)?
        total(all, extra)
    named = BoxedOnceCallable(total)?
    print(named([4, 5]?, 1)?)?
    boxed = BoxedOnceCallable(joined)?
    print(boxed([3]?, 10)?)?
    Ok(())
"#,
        "(10, 2)\n(16, 3)\n",
    );
}

#[test]
fn modes_reject_calls_their_contract_does_not_grant() {
    let counter =
        "mut n = 0\ntick = def [mut n]() -> i64:\n    unchecked:\n        n += 1\n    n\n";
    let once = "g = [1].unwrap()\nrun = def once [g]() -> i64:\n    g[0]\n";
    for (source, expected) in [
        (
            format!("{counter}BoxedCallable(tick).unwrap()\n"),
            "use BoxedMutCallable",
        ),
        (
            format!("{once}BoxedCallable(run).unwrap()\n"),
            "use BoxedOnceCallable",
        ),
        (
            format!("{once}BoxedMutCallable(run).unwrap()\n"),
            "use BoxedOnceCallable",
        ),
        (
            format!("{counter}boxed = BoxedMutCallable(tick).unwrap()\nview = &boxed\nview()\n"),
            "mutable",
        ),
        (
            format!("{once}boxed = BoxedOnceCallable(run).unwrap()\nboxed()\nboxed()\n"),
            "moved",
        ),
        (
            format!("{once}boxed = BoxedOnceCallable(run).unwrap()\nview = &boxed\nview()\n"),
            "must be owned",
        ),
        (
            format!("{once}jobs = [BoxedOnceCallable(run).unwrap()].unwrap()\njobs[0]()\n"),
            "cannot move out of indexed storage",
        ),
        (
            "n = 1\nf = def [&n]() -> i64:\n    n\nBoxedOnceCallable(f).unwrap()\n".to_owned(),
            "borrowed captures",
        ),
        (
            "n = 1\nf = def [&n]() -> i64:\n    n\nBoxedMutCallable(f).unwrap()\n".to_owned(),
            "borrowed captures",
        ),
        (
            "def identity(n: &i64) -> &i64:\n    n\nBoxedOnceCallable(identity).unwrap()\n"
                .to_owned(),
            "cannot return references",
        ),
        (
            format!("def call[F: Callable[[], i64]](f: &F) -> i64:\n    f()\n{once}boxed = BoxedOnceCallable(run).unwrap()\ncall(&boxed)\n"),
            "a one-shot callback is consumed by its call",
        ),
        (
            "def key(n: &i64) -> i64:\n    *n\nmut xs = [2, 1].unwrap()\nk = BoxedMutCallable(key).unwrap()\nxs.sort_by_key(k)\n".to_owned(),
            "requires a shared-call key; use a BoxedCallable, not a BoxedMutCallable",
        ),
        (
            "def key(n: &i64) -> i64:\n    *n\nxs = [2, 1].unwrap()\nk = BoxedOnceCallable(key).unwrap()\nys: list[i64] = iter(&xs).map(k).collect().unwrap()\n".to_owned(),
            "a BoxedOnceCallable cannot be a pipeline callback",
        ),
        (
            "def one() -> i64:\n    1\nf: BoxedMutCallable[[], i64] = BoxedCallable(one).unwrap()\n"
                .to_owned(),
            "expected BoxedMutCallable",
        ),
    ] {
        let error = support::check_source(&source)
            .expect_err(&source)
            .to_string();
        assert!(error.contains(expected), "{expected}: {error}\n{source}");
    }
}

#[test]
fn erased_modes_remain_ineligible_for_worker_transfer() {
    for ty in ["BoxedMutCallable", "BoxedOnceCallable"] {
        let source = format!(
            "def main() -> Result[(), AllocError]:\n    sender, receiver = channel[{ty}[[i64], i64]](1)?\n    Ok(())\n"
        );
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(
            error.contains("boxed callable captures and effects"),
            "{ty}: {error}"
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_mode_boxing_drops_the_callback_and_calls_allocate_nothing() {
    run(
        r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        print("drop").unwrap()

def make(n: i64) -> OnceClosure[[], i64]:
    guard = Guard(n)
    def once [guard]() -> i64:
        guard.id

def main() -> Result[(), AllocError | IoError]:
    held = BoxedOnceCallable(make(2))?
    mut counter = 0
    tick = def [mut counter]() -> i64:
        unchecked:
            counter += 1
        counter
    mut ticks = BoxedMutCallable(tick)?
    pending = make(4)
    print("__test_fail_allocations_after_0__")?
    match BoxedOnceCallable(pending):
        case Ok(unexpected):
            drop(unexpected)
        case Err(_):
            write_stdout("failed\n")?
    value = held()
    first = ticks()
    count = ticks()
    print("__test_restore_allocations__")?
    print(value)?
    print(first)?
    print(count)?
    Ok(())
"#,
        "__test_fail_allocations_after_0__\ndrop\nfailed\ndrop\n__test_restore_allocations__\n2\n1\n2\n",
    );
}

#[test]
fn producer_mismatch_suggests_the_matching_erased_owner() {
    for (capture, expected) in [
        (
            "[n]",
            "box each as BoxedCallable[[], i64] with `BoxedCallable(closure)?`",
        ),
        ("[mut n]", "box each as BoxedMutCallable[[], i64]"),
        ("once [n]", "box each as BoxedOnceCallable[[], i64]"),
        (
            "[&n]",
            "pass each to a generic `F: Callable[...]` parameter",
        ),
    ] {
        let source = format!(
            "mut n = 1\nf = def {capture}() -> i64:\n    n\nmut g = def {capture}() -> i64:\n    n\ng = f\n"
        );
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(
            error.contains("each closure expression has its own environment type")
                && error.contains(expected),
            "{capture}: {error}"
        );
    }
}
