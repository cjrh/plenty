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
            "def main() -> Result[(), ChannelError | IoError]:
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
        "def main() -> Result[(), ChannelError | ThreadError]:
    sender, receiver = channel[i64](1)?
    other = receiver.share()
    with spawn(consume, receiver)?, spawn(consume, other)?, sender as outbox:
        pass
    Ok(())
",
    );
    // Moving the sender later does not release it on an earlier failing path.
    rejected(
        "def main() -> Result[(), ChannelError | IoError | ThreadError]:
    sender, receiver = channel[i64](1)?
    with spawn(consume, receiver)? as task:
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
            "{CONSUME}def main() -> Result[(), ChannelError | IoError | ThreadError]:
    for i in range(1):
        sender, receiver = channel[i64](1)?
        with spawn(consume, receiver)?, sender as outbox:
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
def main() -> Result[(), AllocError | ChannelError]:
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
    // Availability and nonblocking calls are not inferred from worker bodies.
    rejected(
        "def nowait(receiver: Receiver[i64]) -> ():
    drop(receiver.recv_nowait())
def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    sender.send(1).unwrap()
    with spawn(nowait, receiver).unwrap():
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
