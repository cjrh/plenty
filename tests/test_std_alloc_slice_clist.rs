use super::test_std_alloc::assert_stdout;
use super::test_std_alloc_set_algebra::POLICY;
use crate::test_support::check_error;

#[test]
fn selected_mixed_columns_follow_slice_bounds_and_keep_row_bits() {
    assert_stdout(
        r#"import std.alloc as alloc

class Row:
    flag: bool
    short: i16
    wide: u64
    number: f32

def main() -> Result[(), AllocError | IoError | RangeError]:
    out = alloc.system()
    mut rows = clist[Row]()
    rows.append(Row(True, -32768i16, u64.MAX, -0.0f32)).unwrap()
    rows.append(Row(False, 7i16, 8u64, 1.25f32)).unwrap()
    rows.append(Row(True, -2i16, 6u64, 2.5f32)).unwrap()
    k = -2
    copy = out.slice_clist[Row](&rows, alloc.Unset.Unset, step=NonZero.try_from(k)?)?
    print(copy[0].short, copy[-1].flag, copy[-1].wide, copy[-1].number)?
    print(out.slice_clist(&rows, i64.MIN, i64.MAX)? == rows)?
    empty = out.slice_clist(&rows, stop=-1, step=-1)?
    print(empty.len())?
    rows[0].short = 0i16
    print(copy[-1].short, rows[0].short)?
    Ok(())
"#,
        "-2 True 18446744073709551615 -0.0\nTrue\n0\n-32768 0\n",
    );
}

#[test]
fn selected_empty_retains_provider_through_worker_growth_and_drop() {
    assert_stdout(
        &format!(
            r#"{POLICY}
class Item:
    value: i64

def make() -> Result[clist[Item], AllocError]:
    rows = clist[Item]()
    owner = budget("worker", 2, AllocError.CapacityOverflow)
    owner.slice_clist(&rows)

def work(values: clist[Item]) -> Result[i64, IoError | AllocError]:
    mut items = values
    print(items.len(), items.capacity())?
    items.append(Item(9)).unwrap()
    match items.reserve(100):
        case Err(error):
            print(error, items[-1].value)?
        case Ok(_):
            pass
    # Bracket slicing produces an ordinary owner, not a child of the provider.
    mut independent = items[::-1]?
    independent.reserve(100)?
    independent[0].value = 3
    print(items.remove(0).unwrap().value, items.len())?
    Ok(independent[0].value)

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    with spawn(work, make()?).drop_job()? as job:
        print(job.join()?)?
    Ok(())
"#
        ),
        "0 0\nrefused worker\nAllocError.CapacityOverflow 9\n9 0\nworker 0 2\n3\n",
    );
}

#[test]
fn refused_slice_preserves_the_source_and_never_reuses_its_provider() {
    assert_stdout(
        &format!(
            r#"{POLICY}
class Row:
    value: i64

def main() -> Result[(), AllocError | IoError]:
    mut rows = clist[Row]()
    rows.append(Row(11)).unwrap()
    a = budget("source", 1)
    source = a.slice_clist(&rows)?
    out = budget("output", 0)
    match out.slice_clist(&source):
        case Err(error):
            print(error, source[0].value)?
        case Ok(value):
            drop(value)
    print(out.slice_clist(&source, stop=0).is_err())?
    drop(source)
    Ok(())
"#
        ),
        "refused output\nAllocError.OutOfMemory 11\nrefused output\nTrue\noutput 0 0\nsource 0 1\n",
    );
}

#[test]
fn slice_clist_obeys_caller_row_privacy_even_with_cached_specializations() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("model.plenty"),
        r#"import std.alloc as alloc
pub class Row:
    private_value: i64
pub def make(out: &alloc.Allocator) -> Result[clist[Row], AllocError]:
    rows = clist[Row]()
    out.slice_clist(&rows, step=-1)
"#,
    )
    .unwrap();
    let main = dir.path().join("main.plenty");
    let source = "from model import make, Row\nimport std.alloc as alloc\ndef main() -> Result[(), AllocError]:\n    out = alloc.system()\n    rows = make(&out)?\n    STATEMENT\n    Ok(())\n";
    std::fs::write(&main, source.replace("STATEMENT", "drop(rows)")).unwrap();
    plenty::check_file(&main, None).unwrap();
    for call in [
        "out.slice_clist(&rows, step=-1)",
        "out.slice_clist[Row](&rows, 0, 1, 1)",
    ] {
        std::fs::write(&main, source.replace("STATEMENT", &format!("drop({call})"))).unwrap();
        let err = plenty::check_file(&main, None).unwrap_err().to_string();
        assert!(err.contains("private"), "{err}");
    }
}

#[test]
fn slice_clist_keeps_the_existing_step_actor_and_intrinsic_boundaries() {
    let source = "import std.alloc as alloc\nclass Row:\n    n: i32\ndef use_slice(out: &alloc.Allocator, rows: &clist[Row]) -> ():\n    drop(out.slice_clist(rows, step=STEP))\ndef main() -> ():\n    pass\n";
    for (step, diagnostic) in [("0", "cannot be zero"), ("rows.len()", "NonZero[i64]")] {
        let error = check_error(&source.replace("STEP", step));
        assert!(error.contains(diagnostic), "{error}");
    }
    let error = check_error(&format!(
        "import std.actor as actors\n{}",
        source
            .replace("STEP", "1")
            .replace("    pass", "    drop(actors.heaps_live())")
    ));
    assert!(
        error.contains("retained allocator use cannot be combined"),
        "{error}"
    );
    let error =
        check_error("def main() -> ():\n    drop(__allocator_clist_slice[i64](0, 0, 0, 0, 1))\n");
    assert!(
        error.contains("reserved for the standard library"),
        "{error}"
    );
}

#[test]
fn selected_clist_does_not_relax_schema_or_worker_borrow_rules() {
    let source = r#"import std.alloc as alloc
class Row:
    n: i64

def work(out: &alloc.Allocator, rows: &clist[Row]) -> Result[clist[Row], AllocError]:
    out.slice_clist(rows)

def main() -> ():
    out = alloc.system()
    mut rows = clist[Row]()
    with spawn(work, &out, &rows).drop_job().unwrap() as job:
        rows.clear()
        drop(job.join())
"#;
    let err = check_error(source);
    assert!(
        err.contains("cannot modify or exclusively borrow `rows`"),
        "{err}"
    );
    let err = check_error(&source.replace("n: i64", "n: str"));
    assert!(err.contains("primitive scalar fields"), "{err}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn selected_columns_gather_without_ordinary_storage_or_scratch() {
    assert_stdout(
        r#"import std.alloc as alloc
pub class Preallocated:
    blocks: list[alloc.ByteBlock]
    pub def allocate(self: &mut Preallocated, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        self.blocks.pop().ok_or(AllocError.OutOfMemory)
    pub def release(self: &mut Preallocated, storage: alloc.ByteBlock) -> ():
        drop(storage)
class Row:
    small: i8
    value: f64

def main() -> Result[(), AllocError | IoError]:
    root = alloc.system()
    layout = alloc.layout(1024, 64).unwrap()
    blocks = [root.block(&layout)?, root.block(&layout)?]?
    out = alloc.allocator(&root, Preallocated(blocks)).unwrap()
    mut rows = clist[Row]()
    rows.append(Row(1i8, 3.25)).unwrap()
    rows.append(Row(2i8, 4.5)).unwrap()
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    mut result = out.slice_clist(&rows, step=-1)?
    result.reserve(16)?
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    print(result[0].small, result[1].value)?
    Ok(())
"#,
        "__test_fail_allocations_after_0__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_restore_allocations__\n2 3.25\n",
    );
}

// Compile only. Policy can wait for the caller's lock or for a FIFO peer.
const JOIN_POLICY: &str = r#"import std.alloc as alloc
from std.sync import Mutex

pub class Policy:
    root: alloc.Allocator
    guard: Shared[Mutex[i64]]
    pub def allocate(self: &mut Policy, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        with self.guard.lock():
            return self.root.block(request)
    pub def release(self: &mut Policy, storage: alloc.ByteBlock) -> ():
        with self.guard.lock():
            drop(open("provider.fifo"))
            drop(storage)

class Row:
    n: i64

class Bundle:
    saved: Option[Box[clist[Row]]]

WORKER

def main() -> ():
    guard = Shared(Mutex(0)).unwrap()
    root = alloc.system()
    out = PROVIDER
    rows = clist[Row]()
    mut picked = OWNER
    CALLER
"#;

fn check_join(
    worker: &str,
    caller: &str,
    selected: bool,
    registered: bool,
    expected: Option<&str>,
) {
    let owner = if selected {
        "out.slice_clist(&rows).unwrap()"
    } else {
        "clist[Row]()"
    };
    check_provider_join(worker, caller, owner, registered, expected);
}

pub(super) fn check_provider_join(
    worker: &str,
    caller: &str,
    owner: &str,
    registered: bool,
    expected: Option<&str>,
) {
    let program = JOIN_POLICY
        .replace("WORKER", worker)
        .replace("CALLER", caller)
        .replace(
            "PROVIDER",
            if registered {
                "alloc.allocator(&root, Policy(root.share(), guard.share())).unwrap()"
            } else {
                "root.share()"
            },
        )
        .replace("OWNER", owner);
    let result = plenty::check_source(&program);
    match expected {
        Some(message) => {
            let err = result.expect_err(&program).to_string();
            assert!(err.contains(message), "{program}\n{err}");
        }
        None => result.unwrap_or_else(|err| panic!("{program}\n{err}")),
    }
}

#[test]
fn selected_clist_worker_joins_account_for_retained_provider_state() {
    const LOCK: &str = "worker can lock a `Mutex[i64]`";
    let join = "with spawn(work, ARG).drop_job().unwrap() as task:\n        with guard.lock():\n            task.join()";
    // The last owner can be cleaned up implicitly inside a nested payload.
    for (worker, arg) in [
        (
            "def work(rows: clist[Row]) -> ():\n    drop(rows)",
            "picked",
        ),
        (
            "def work(bundle: Bundle) -> ():\n    pass",
            "Bundle(Some(Box(picked).unwrap()))",
        ),
        (
            "def work(rows: &mut clist[Row]) -> ():\n    drop(rows.reserve(32))",
            "&mut picked",
        ),
    ] {
        for (selected, registered) in [(false, true), (true, false), (true, true)] {
            check_join(
                worker,
                &join.replace("ARG", arg),
                selected,
                registered,
                (selected && registered).then_some(LOCK),
            );
        }
    }
    // Allocation itself can wait, before a clist has crossed the boundary.
    let factory = "def build(out: &alloc.Allocator) -> ():\n    rows = clist[Row]()\n    drop(out.slice_clist(&rows))\ndef work(out: &alloc.Allocator) -> ():\n    build(out)";
    check_join(
        factory,
        &join.replace("ARG", "&out"),
        false,
        true,
        Some(LOCK),
    );
    check_join(
        factory,
        &join.replace("ARG", "&out").replace(
            "with guard.lock():",
            "other = Mutex(True)\n        with other.lock():",
        ),
        true,
        true,
        None,
    );
    // An allocator argument alone doesn't make a provider's locks reachable.
    check_join(
        "def work(out: &alloc.Allocator) -> ():\n    drop(out.share())",
        &join.replace("ARG", "&out"),
        true,
        true,
        None,
    );
    check_join(
        "def work(rows: &clist[Row]) -> ():\n    drop(rows[0].n)",
        &join.replace("ARG", "&picked"),
        true,
        true,
        None,
    );
}

#[test]
fn selected_clist_callbacks_change_worker_waits_but_borrowed_reads_do_not() {
    let join = "sender, receiver = channel[i64](1).unwrap()\n    with spawn(work, sender, ARG).drop_job().unwrap():\n        pass";
    check_join(
        "def work(sender: Sender[i64], out: &alloc.Allocator) -> ():\n    rows = clist[Row]()\n    drop(out.slice_clist(&rows))",
        &join.replace("ARG", "&out"),
        false,
        true,
        Some("could block the worker's send"),
    );
    for (param, body, arg, waits) in [
        (
            "rows: &mut clist[Row]",
            "drop(rows.append(Row(1)))",
            "&mut picked",
            true,
        ),
        (
            "rows: &mut clist[Row]",
            "drop(rows.reserve(1))",
            "&mut picked",
            true,
        ),
        ("rows: clist[Row]", "drop(rows)", "picked", true),
        (
            "bundle: Bundle",
            "pass",
            "Bundle(Some(Box(picked).unwrap()))",
            true,
        ),
        (
            "bundle: &mut Bundle",
            "bundle.saved = Nothing",
            "&mut bundle",
            true,
        ),
        ("rows: &clist[Row]", "drop(rows.len())", "&picked", false),
    ] {
        let worker = format!("def work(sender: Sender[i64], {param}) -> ():\n    {body}");
        let mut caller = join.replace("ARG", arg);
        if arg == "&mut bundle" {
            caller = format!("mut bundle = Bundle(Some(Box(picked).unwrap()))\n    {caller}");
        }
        for (selected, registered) in [(false, true), (true, false), (true, true)] {
            check_join(
                &worker,
                &caller,
                selected,
                registered,
                (waits && selected && registered).then_some("could block the worker's send"),
            );
        }
    }
}

#[test]
fn selected_clist_pool_jobs_record_callback_waits_and_hidden_mutexes() {
    let factory = "def build(out: &alloc.Allocator) -> ():\n    rows = clist[Row]()\n    drop(out.slice_clist(&rows))\ndef work(out: alloc.Allocator) -> ():\n    build(&out)";
    let future = "with ThreadPoolExecutor(1, 1).unwrap() as pool:\n        future = pool.submit(work, ARG).unwrap()\n        with guard.lock():\n            drop(future.result())";
    let lock = "this result() waits for a job that can lock a `Mutex[i64]`";
    check_join(
        factory,
        &future.replace("ARG", "out"),
        false,
        true,
        Some(lock),
    );
    let worker = "def work(bundle: Bundle) -> ():\n    pass";
    let arg = "Bundle(Some(Box(picked).unwrap()))";
    check_join(worker, &future.replace("ARG", arg), true, true, Some(lock));
    check_join(
        worker,
        &future
            .replace("ARG", arg)
            .replace("drop(future.result())", "pool.shutdown()"),
        true,
        true,
        Some("a job that can lock a `Mutex[i64]` was submitted"),
    );
    let helper = "def helper(bundle: Bundle) -> ():\n    with ThreadPoolExecutor(1, 1).unwrap() as pool:\n        drop(pool.submit(work, bundle))";
    check_join(
        &format!("{worker}\n{helper}"),
        &format!("with guard.lock():\n        helper({arg})"),
        true,
        true,
        Some("a job that can lock a `Mutex[i64]`"),
    );
    let sender = "def work(sender: Sender[i64], bundle: Bundle) -> ():\n    pass";
    let caller = format!(
        "sender, receiver = channel[i64](1).unwrap()\n    with ThreadPoolExecutor(1, 1).unwrap() as pool:\n        drop(pool.submit(work, sender, {arg}))"
    );
    check_join(
        sender,
        &caller,
        true,
        true,
        Some("could block the job's send"),
    );
    check_join(sender, &caller, false, true, None);
}

#[test]
fn indirect_selected_clist_construction_and_data_only_controls() {
    let worker = "def target(out: &alloc.Allocator) -> ():\n    BODY\ndef work(f: Callable[[&alloc.Allocator], ()], out: &alloc.Allocator) -> ():\n    f(out)";
    let caller = "with spawn(work, target, &out).drop_job().unwrap() as task:\n        with guard.lock():\n            task.join()";
    check_join(
        &worker.replace(
            "BODY",
            "rows = clist[Row]()\n    drop(out.slice_clist(&rows))",
        ),
        caller,
        true,
        true,
        Some("worker can lock a `Mutex[i64]`"),
    );
    check_join(
        &worker.replace("BODY", "drop(out.share())"),
        caller,
        true,
        true,
        None,
    );
}

#[test]
fn selected_clist_opaque_pool_calls_distinguish_owning_inputs_and_results() {
    let body = "def work(bundle: Bundle) -> ():\n    pass\ndef callback(SIGNATURE) -> RESULT:\n    BODY\ndef helper(f: Callable[[TYPES], RESULT], submitter: Submitter, bundle: Bundle, values: clist[Row]) -> ():\n    mut rows = values\n    drop(f(ARG))\n    drop(submitter.submit_nowait(work, bundle).drop_job())";
    let caller = "with ThreadPoolExecutor(1, 1).unwrap() as pool:\n        with guard.lock():\n            helper(callback, pool.submitter(), Bundle(Some(Box(picked).unwrap())), rows)";
    for (params, types, result, code, arg, capable) in [
        (
            "input: clist[Row]",
            "clist[Row]",
            "()",
            "drop(input)",
            "rows",
            true,
        ),
        ("", "", "clist[Row]", "clist[Row]()", "", true),
        (
            "input: &clist[Row]",
            "&clist[Row]",
            "()",
            "drop(input.len())",
            "&rows",
            false,
        ),
        (
            "input: &mut clist[Row]",
            "&mut clist[Row]",
            "()",
            "drop(input.reserve(5))",
            "&mut rows",
            true,
        ),
        ("", "", "()", "pass", "", false),
    ] {
        let helper = body
            .replace("SIGNATURE", params)
            .replace("TYPES", types)
            .replace("RESULT", result)
            .replace("BODY", code)
            .replace("ARG", arg);
        for selected in [false, true] {
            check_join(
                &helper,
                caller,
                selected,
                true,
                (selected && capable)
                    .then_some("this call to `helper` is assumed capable of shutting down a pool"),
            );
        }
    }
}

// Only the outer Allocator crosses the worker boundary. Its hidden state owns
// the selected clist; the *inner* provider owns the lock used at clist drop.
const HOLDING: &str = r#"pub class Holding:
    root: alloc.Allocator
    bundle: Bundle
    pub def allocate(self: &mut Holding, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        self.root.block(request)
    pub def release(self: &mut Holding, storage: alloc.ByteBlock) -> ():
        drop(storage)

class Wrapped:
    value: Option[Box[alloc.Allocator]]
"#;
const OUTER: &str = "outer = alloc.allocator(&root, Holding(root.share(), Bundle(Some(Box(picked).unwrap())))).unwrap()";

#[test]
fn final_allocator_release_at_scoped_join_includes_hidden_clist_cleanup() {
    let join = "with spawn(work, ARG).drop_job().unwrap() as task:\n        with guard.lock():\n            task.join()";
    let lock = "worker can lock a `Mutex[i64]`";
    for (signature, body, arg, final_release) in [
        ("owner: alloc.Allocator", "drop(owner)", "outer", true),
        (
            "wrapped: Wrapped",
            "pass",
            "Wrapped(Some(Box(outer).unwrap()))",
            true,
        ),
        (
            "owner: &alloc.Allocator",
            "drop(owner.share())",
            "&outer",
            false,
        ),
    ] {
        let worker = format!("{HOLDING}\ndef work({signature}) -> ():\n    {body}");
        let caller = format!("{OUTER}\n    {}", join.replace("ARG", arg));
        for selected in [false, true] {
            check_join(
                &worker,
                &caller,
                selected,
                true,
                (selected && final_release).then_some(lock),
            );
        }
    }
    // Borrowing the carrier does not pin an allocator that the worker can
    // receive or replace. Neither carrier itself contains a Mutex[i64].
    for (signature, body, setup, arg) in [
        (
            "rx: &Receiver[alloc.Allocator]",
            "drop(rx.recv_nowait())",
            "tx, rx = channel[alloc.Allocator](1).unwrap()\n    tx.send(outer).unwrap()\n    drop(tx)",
            "&rx",
        ),
        (
            "slot: &Mutex[Wrapped]",
            "with slot.lock() as wrapped:\n        wrapped.value = Nothing",
            "slot = Mutex(Wrapped(Some(Box(outer).unwrap())))",
            "&slot",
        ),
    ] {
        let worker = format!("{HOLDING}\ndef work({signature}) -> ():\n    {body}");
        let caller = format!("{OUTER}\n    {setup}\n    {}", join.replace("ARG", arg));
        check_join(&worker, &caller, true, true, Some(lock));
    }
    let owning = format!("{HOLDING}\ndef work(owner: alloc.Allocator) -> ():\n    pass");
    // Producing selected text doesn't activate an ordinary clist in Holding.
    let text_only = format!(
        "drop(out.slice_text(\"selected text\"))\n    {OUTER}\n    {}",
        join.replace("ARG", "outer")
    );
    check_join(&owning, &text_only, false, true, None);
    // A producer alone doesn't make owning Allocator cleanup a clist release:
    // the only *registered* state is Policy, not Holding.
    check_join(&owning, &join.replace("ARG", "out"), true, true, None);
}

#[test]
fn final_allocator_release_in_pool_job_is_visible_at_result_and_shutdown() {
    let worker = format!("{HOLDING}\ndef work(wrapped: Wrapped) -> ():\n    pass");
    for (wait, diagnostic) in [
        (
            "drop(future.result())",
            "this result() waits for a job that can lock a `Mutex[i64]`",
        ),
        (
            "pool.shutdown()",
            "a job that can lock a `Mutex[i64]` was submitted",
        ),
    ] {
        let caller = format!(
            "{OUTER}\n    with ThreadPoolExecutor(1, 1).unwrap() as pool:\n        future = pool.submit(work, Wrapped(Some(Box(outer).unwrap()))).unwrap()\n        with guard.lock():\n            {wait}"
        );
        check_join(&worker, &caller, true, true, Some(diagnostic));
    }
}
