mod support;

const CONSUME: &str = "def consume(receiver: Receiver[i64]) -> ():
    match receiver.recv():
        case Ok(_):
            pass
        case Err(_):
            pass
";

fn rejected(body: &str) -> String {
    let error = support::check_source(&format!("{CONSUME}{body}"))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("could block the worker's receive"),
        "{error}"
    );
    error
}

#[test]
fn receiving_joins_report_surviving_senders() {
    let error = rejected(
        "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    job = def once [receiver]() -> ():
        consume(receiver)
    with spawn(job).unwrap() as task:
        pass
    drop(sender)
",
    );
    assert!(error.contains("leaving this `with` block joins a task whose worker holds `Receiver[i64]` while `sender` still holds `Sender[i64]`"), "{error}");
    assert!(
        error.contains("`with spawn(...)?, sender as outbox:`"),
        "{error}"
    );
    let error = rejected(
        "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    with spawn(consume, receiver).unwrap() as task, sender as outbox:
        task.join()
",
    );
    assert!(
        error.contains("this join waits for a task whose worker holds `Receiver[i64]`"),
        "{error}"
    );
    assert!(error.contains("the task starts here"), "{error}");
    assert!(
        error.contains("drop it, or end its `with` block, before joining"),
        "{error}"
    );
}

#[test]
fn suggested_sender_guards_avoid_existing_bindings_and_type_check() {
    for (sender, task, bindings, expected) in [
        ("outbox", "task", "", "outbox_2"),
        ("sender", "outbox", "", "outbox_2"),
        (
            "sender",
            "task",
            "    outbox = 0\n    outbox_2 = 0\n",
            "outbox_3",
        ),
    ] {
        let body = format!(
            "def main() -> ():
{bindings}    {sender}, receiver = channel[i64](1).unwrap()
    with spawn(consume, receiver).unwrap() as {task}:
        {sender}.send(1).unwrap()
"
        );
        let error = rejected(&body);
        let prefix = format!("`with spawn(...)?, {sender} as ");
        let suggested = error
            .split_once(&prefix)
            .unwrap()
            .1
            .split_once(":`")
            .unwrap()
            .0;
        assert_eq!(suggested, expected, "{error}");
        let fixed = body
            .replace(
                &format!("as {task}:"),
                &format!("as {task}, {sender} as {suggested}:"),
            )
            .replace(
                &format!("        {sender}.send"),
                &format!("        {suggested}.send"),
            );
        support::check_source(&format!("{CONSUME}{fixed}"))
            .unwrap_or_else(|error| panic!("{fixed}\n{error}"));
    }
}

#[test]
fn suggested_guards_avoid_parameters_and_bindings_inside_the_task_block() {
    let body = "def run(outbox: i64) -> ():
    sender, receiver = channel[i64](1).unwrap()
    outbox_2 = 0
    with spawn(consume, receiver).unwrap() as task:
        outbox_3 = 0
def main() -> ():
    run(0)
";
    let error = rejected(body);
    assert!(error.contains("sender as outbox_4:"), "{error}");
    let fixed = body.replace("as task:", "as task, sender as outbox_4:");
    support::check_source(&format!("{CONSUME}{fixed}")).unwrap();
}

#[test]
fn suggested_receiver_guards_also_avoid_existing_bindings() {
    let source = "def produce(sender: Sender[i64]) -> ():
    drop(sender.send(1))
def main() -> ():
    sender, inbox = channel[i64](1).unwrap()
    inbox_2 = 0
    with spawn(produce, sender).unwrap() as task:
        drop(inbox.recv())
";
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains("could block the worker's send"), "{error}");
    assert!(error.contains("inbox as inbox_3:"), "{error}");
    let fixed = source
        .replace("as task:", "as task, inbox as inbox_3:")
        .replace("drop(inbox.recv())", "drop(inbox_3.recv())");
    support::check_source(&fixed).unwrap();
}

#[test]
fn receiving_joins_check_every_exit_and_failed_later_start() {
    for exit in ["return Ok(())", "break", "continue", "print(1)?"] {
        let error = rejected(&format!(
            "def main() -> Result[(), AllocError | IoError]:
    sender, receiver = channel[i64](1)?
    for i in range(1):
        with spawn(consume, receiver.share()).unwrap():
            {exit}
    Ok(())
"
        ));
        let keyword = if exit.ends_with('?') {
            "?"
        } else {
            exit.split(' ').next().unwrap()
        };
        assert!(
            error.contains(&format!("`{keyword}` leaves the block here")),
            "{error}"
        );
    }
    rejected(
        "def main() -> Result[(), AllocError | SpawnError[()]]:
    sender, receiver = channel[i64](1)?
    other = receiver.share()
    with spawn(consume, receiver).drop_job()?, spawn(consume, other).drop_job()?, sender as outbox:
        pass
    Ok(())
",
    );
    // Moving the sender later does not release it on an earlier failing path.
    rejected(
        "def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    sender, receiver = channel[i64](1)?
    with spawn(consume, receiver).drop_job()? as task:
        print(1)?
        drop(sender)
        task.join()
    Ok(())
",
    );
}

#[test]
fn receiving_joins_allow_guarded_early_exits() {
    for exit in ["return Ok(())", "break", "continue", "print(1)?"] {
        support::check_source(&format!(
            "{CONSUME}def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    for i in range(1):
        sender, receiver = channel[i64](1)?
        with spawn(consume, receiver).drop_job()?, sender as outbox:
            {exit}
    Ok(())
"
        ))
        .unwrap();
    }
}

#[test]
fn receiving_joins_find_aggregate_capture_shared_and_borrowed_holders() {
    let error = rejected(
        "class Hub:
    sender: Sender[i64]
def main() -> Result[(), AllocError]:
    sender, receiver = channel[i64](1)?
    spare = sender.share()
    hub = Hub(sender.share())
    handles = [sender.share()]?
    writer = def once [sender]() -> ():
        pass
    with spawn(consume, receiver).unwrap():
        pass
    Ok(())
",
    );
    for name in ["spare", "hub", "handles", "writer"] {
        assert!(error.contains(&format!("`{name}`")), "{error}");
    }
    for body in [
        "def run(receiver: Receiver[i64], outbox: &Sender[i64]) -> ():
    with spawn(consume, receiver).unwrap():
        pass
",
        "class Hub:
    sender: Sender[i64]
    def run(self, receiver: Receiver[i64]) -> ():
        with spawn(consume, receiver).unwrap():
            pass
",
        "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    job = def once [receiver, &sender]() -> ():
        with spawn(consume, receiver).unwrap():
            pass
    job()
",
    ] {
        let error = rejected(body);
        assert!(
            error.contains("its owner outlives this call, so take the sender by value"),
            "{error}"
        );
    }
    let error = rejected(
        "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    with spawn(consume, receiver).unwrap():
        if len(\"x\") == 1:
            drop(sender)
",
    );
    assert!(
        error.contains("`sender` holds `Sender[i64]` on some paths"),
        "{error}"
    );
    rejected(
        "def borrowed(receiver: &Receiver[i64]) -> ():
    drop(receiver.recv())
def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    with spawn(borrowed, &receiver).unwrap():
        pass
",
    );
}

#[test]
fn last_sender_cleanup_precedes_receiving_joins() {
    for body in [
        "with spawn(consume, receiver).unwrap(), sender as outbox:\n        pass",
        "with spawn(consume, receiver).unwrap() as task:\n        drop(sender)\n        task.join()",
        "with spawn(consume, receiver).unwrap() as task:\n        with sender as outbox:\n            pass\n        task.join()",
        "with spawn(consume, receiver).unwrap():\n        drop(sender)",
        "with spawn(consume, receiver).unwrap(), sender as outbox:\n        return",
    ] {
        let source = format!(
            "{CONSUME}def main() -> ():\n    sender, receiver = channel[i64](1).unwrap()\n    {body}\n"
        );
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("receiving-join");
        support::compile_source_to_executable(&source, &executable).unwrap();
        let output = support::output_within(
            &mut std::process::Command::new(&executable),
            b"",
            std::time::Duration::from_secs(5),
        );
        assert!(output.status.success(), "{body}: {output:?}");
    }
}

#[test]
fn sender_guard_order_and_message_type_are_conservative() {
    rejected(
        "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    with sender as outbox, spawn(consume, receiver).unwrap():
        pass
",
    );
    // Buffered messages are not counted: this worker could wait. A worker that
    // only receives with `recv_nowait` is accepted; see `test_nowait_workers`.
    rejected(
        "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    sender.send(1).unwrap()
    with spawn(consume, receiver).unwrap():
        pass
",
    );
    // An unrelated queue of the same type still counts as a holder.
    rejected(
        "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    spare, other = channel[i64](1).unwrap()
    with spawn(consume, receiver).unwrap(), sender as outbox:
        pass
",
    );
    support::check_source(&format!(
        "{CONSUME}def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    names, labels = channel[str](1).unwrap()
    with spawn(consume, receiver).unwrap(), sender as outbox:
        pass
"
    ))
    .unwrap();
}

const ENDS: &str = "class Ends:
    sender: Sender[i64]
    names: Sender[str]

class Outer:
    ends: Ends
    count: i64

";

fn field_moves(body: &str) -> Result<(), String> {
    support::check_source(&format!("{CONSUME}{ENDS}{body}")).map_err(|error| error.to_string())
}

/// Issue #794: a field moved out of a binding on every path no longer counts
/// as an endpoint the binding holds, as a `match` destructure does not.
#[test]
fn moved_fields_release_their_endpoints_at_joins() {
    let accepted = [
        "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    names, labels = channel[str](1).unwrap()
    ends = Ends(sender, names)
    kept = ends.names
    drop(ends.sender)
    with spawn(consume, receiver).unwrap():
        pass
    drop(kept)
",
        "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    names, labels = channel[str](1).unwrap()
    match Ends(sender, names):
        case Ends(sender, kept):
            drop(sender)
            with spawn(consume, receiver).unwrap():
                pass
            drop(kept)
",
        "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    names, labels = channel[str](1).unwrap()
    outer = Outer(Ends(sender, names), 0)
    drop(outer.ends.sender)
    with spawn(consume, receiver).unwrap():
        pass
",
        "def run(flag: bool) -> ():
    sender, receiver = channel[i64](1).unwrap()
    names, labels = channel[str](1).unwrap()
    ends = Ends(sender, names)
    if flag:
        drop(ends.sender)
    else:
        moved = ends.sender
        drop(moved)
    with spawn(consume, receiver).unwrap():
        pass
",
    ];
    for body in accepted {
        field_moves(body).unwrap_or_else(|error| panic!("{body}\n{error}"));
    }
    let blocked = "could block the worker's receive";
    for (body, note) in [
        // The moved field holds no `Sender[i64]`.
        (
            "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    names, labels = channel[str](1).unwrap()
    ends = Ends(sender, names)
    drop(ends.names)
    with spawn(consume, receiver).unwrap():
        pass
",
            "`ends` holds `Sender[i64]`;",
        ),
        (
            "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    names, labels = channel[str](1).unwrap()
    outer = Outer(Ends(sender, names), 0)
    drop(outer.ends.names)
    with spawn(consume, receiver).unwrap():
        pass
",
            "`outer` holds `Sender[i64]`;",
        ),
        // Assigning the field again restores a holder.
        (
            "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    spare, unused = channel[i64](1).unwrap()
    names, labels = channel[str](1).unwrap()
    mut ends = Ends(sender, names)
    drop(ends.sender)
    ends.sender = spare
    with spawn(consume, receiver).unwrap():
        pass
",
            "`ends` holds `Sender[i64]`;",
        ),
        (
            "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    spare, unused = channel[i64](1).unwrap()
    names, labels = channel[str](1).unwrap()
    mut ends = Ends(sender, names)
    drop(ends.sender)
    ends = Ends(spare, ends.names)
    with spawn(consume, receiver).unwrap():
        pass
",
            "`ends` holds `Sender[i64]`;",
        ),
        // A field moved on only one path still holds on the other.
        (
            "def run(flag: bool) -> ():
    sender, receiver = channel[i64](1).unwrap()
    names, labels = channel[str](1).unwrap()
    ends = Ends(sender, names)
    if flag:
        drop(ends.sender)
    with spawn(consume, receiver).unwrap():
        pass
",
            "`ends` holds `Sender[i64]` on some paths;",
        ),
        (
            "def run(flag: bool) -> ():
    sender, receiver = channel[i64](1).unwrap()
    names, labels = channel[str](1).unwrap()
    ends = Ends(sender, names)
    if flag:
        drop(ends.sender)
    else:
        drop(ends.names)
    with spawn(consume, receiver).unwrap():
        pass
",
            "`ends` holds `Sender[i64]` on some paths;",
        ),
    ] {
        let error = field_moves(body).expect_err(body);
        assert!(error.contains(blocked), "{body}\n{error}");
        assert!(error.contains(note), "{body}\n{error}");
    }
}

#[test]
fn moved_fields_of_generic_classes_release_only_themselves() {
    let pair = "class Pair[T]:
    a: T
    b: T

";
    let source = |drops: &str| {
        format!(
            "{CONSUME}{pair}def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    spare, unused = channel[i64](1).unwrap()
    p = Pair[Sender[i64]](sender, spare)
{drops}    with spawn(consume, receiver).unwrap():
        pass
"
        )
    };
    let error = support::check_source(&source("    drop(p.a)\n"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("`p` holds `Sender[i64]`;"), "{error}");
    support::check_source(&source("    drop(p.a)\n    drop(p.b)\n")).unwrap();
}

const RX_ENDS: &str = "def produce(sender: Sender[i64]) -> ():
    match sender.send(1):
        case Ok(_):
            pass
        case Err(_):
            pass

class RxEnds:
    rx: Receiver[i64]
    names: Sender[str]

";

/// Pool shutdowns and result waits share the join's holder check.
#[test]
fn moved_fields_release_their_endpoints_at_shutdowns_and_result_waits() {
    let shutdown = |moved: &str| {
        format!(
            "{RX_ENDS}def main() -> Result[(), AllocError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    names, labels = channel[str](1)?
    ends = RxEnds(receiver, names)
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(produce, sender)?
        drop(ends.{moved})
    Ok(())
"
        )
    };
    let result = |moved: &str| {
        format!(
            "{RX_ENDS}def main() -> Result[(), AllocError | FutureError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    names, labels = channel[str](1)?
    ends = RxEnds(receiver, names)
    drop(ends.{moved})
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(produce, sender)?
        future.result()?
    Ok(())
"
        )
    };
    support::check_source(&shutdown("rx")).unwrap();
    support::check_source(&result("rx")).unwrap();
    for (source, wait) in [
        (shutdown("names"), "shuts down this pool"),
        (result("names"), "this result() waits"),
    ] {
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(wait), "{error}");
        assert!(error.contains("`ends` holds `Receiver[i64]`;"), "{error}");
    }
}

/// A later iteration starts with what the body assigned, so a binding or field
/// moved before the loop and assigned in its body holds an endpoint at a join
/// earlier in the body.
#[test]
fn loop_bodies_see_endpoints_assigned_by_earlier_iterations() {
    let looped = |setup: &str, refill: &str| {
        format!(
            "{CONSUME}{ENDS}def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    names, labels = channel[str](1).unwrap()
{setup}    mut inbox = receiver
    for i in range(2):
        with spawn(consume, inbox).unwrap():
            pass
        s2, r2 = channel[i64](1).unwrap()
        inbox = r2
        {refill}
"
        )
    };
    let field = "    mut ends = Ends(sender, names)\n    drop(ends.sender)\n";
    let whole = "    mut held = sender\n    drop(held)\n";
    for (source, note) in [
        (
            looped(field, "ends.sender = s2"),
            "`ends` holds `Sender[i64]` on some paths;",
        ),
        (
            looped(whole, "held = s2"),
            "`held` holds `Sender[i64]` on some paths;",
        ),
        (
            looped(
                whole,
                "held = s2\n        if i == 0:\n            continue\n        drop(held)",
            ),
            "`held` holds `Sender[i64]` on some paths;",
        ),
        // Assigned on only some paths of the body.
        (
            looped(
                whole,
                "if i == 0:\n            held = s2\n        else:\n            drop(s2)",
            ),
            "`held` holds `Sender[i64]` on some paths;",
        ),
        (
            looped(
                field,
                "if i == 0:\n            ends.sender = s2\n        else:\n            drop(s2)",
            ),
            "`ends` holds `Sender[i64]` on some paths;",
        ),
        // Assigned in an inner loop that can end with `break`.
        (
            looped(
                whole,
                "for j in range(2):\n            if j == 1:\n                held = s2\n                break",
            ),
            "`held` holds `Sender[i64]` on some paths;",
        ),
    ] {
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(
            error.contains("could block the worker's receive"),
            "{source}\n{error}"
        );
        assert!(error.contains(note), "{source}\n{error}");
    }
    // Dropping the endpoint before the next iteration releases it.
    support::check_source(&looped(
        field,
        "ends.sender = s2\n        drop(ends.sender)",
    ))
    .unwrap();
    support::check_source(&looped(whole, "held = s2\n        drop(held)")).unwrap();
}

/// A pool assigned again in a loop shuts down the one an earlier iteration
/// submitted to, while that iteration's receiver is still held.
#[test]
fn loop_reassigned_pools_see_jobs_and_receivers_from_earlier_iterations() {
    let error = support::check_source(&format!(
        "{RX_ENDS}def main() -> Result[(), AllocError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    s0, r0 = channel[i64](1)?
    drop(s0)
    mut inbox = r0
    drop(inbox)
    mut pool = ThreadPoolExecutor(1, 1)?
    for i in range(2):
        pool = ThreadPoolExecutor(1, 1)?
        s, r = channel[i64](1)?
        future = pool.submit(produce, s)?
        inbox = r
        if i == 1:
            drop(inbox)
            break
    Ok(())
"
    ))
    .unwrap_err()
    .to_string();
    assert!(error.contains("shuts down this pool"), "{error}");
    assert!(
        error.contains("`inbox` holds `Receiver[i64]` on some paths;"),
        "{error}"
    );
}

/// A value moved on some paths into one branch and on every path into the
/// other is still owned on the remaining paths after the merge.
#[test]
fn merges_keep_values_moved_on_some_paths_live() {
    for body in [
        "    with spawn(consume, receiver).unwrap() as task:
        if a:
            drop(sender)
            task.join()
        else:
            if b:
                drop(sender)
                task.join()
",
        "    if a:
        drop(sender)
    else:
        if b:
            drop(sender)
    with spawn(consume, receiver).unwrap():
        pass
",
    ] {
        let error = support::check_source(&format!(
            "{CONSUME}def run(a: bool, b: bool) -> ():
    sender, receiver = channel[i64](1).unwrap()
{body}"
        ))
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("`sender` holds `Sender[i64]` on some paths;"),
            "{body}\n{error}"
        );
    }
}

/// A loop that makes no wait check still leaves what its body assigned, so a
/// join after it sees an endpoint moved before the loop and assigned inside.
#[test]
fn joins_after_loops_see_endpoints_assigned_in_the_body() {
    let field = "    mut ends = Ends(sender=sender, names=n)
    drop(ends.sender)
";
    let whole = "    mut held = sender
    drop(held)
";
    for (setup, holder, header, assign) in [
        (
            field,
            "ends",
            "    for i in range(2):\n",
            "ends.sender = s2",
        ),
        (whole, "held", "    for i in range(2):\n", "held = s2"),
        (
            field,
            "ends",
            "    mut i = 0\n    while True:\n        if i == 1:\n            break\n        i = 1\n",
            "ends.sender = s2",
        ),
    ] {
        let source = format!(
            "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    n, nr = channel[str](1).unwrap()
    drop(nr)
{setup}    mut rx = receiver
{header}        s2, r2 = channel[i64](1).unwrap()
        {assign}
        rx = r2
    with spawn(consume, rx).unwrap():
        pass
"
        );
        let error = field_moves(&source).unwrap_err();
        assert!(
            error.contains(&format!("`{holder}` holds `Sender[i64]` on some paths;")),
            "{source}\n{error}"
        );
    }
}
