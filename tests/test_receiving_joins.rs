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
fn receiving_joins_check_every_exit_and_failed_later_start() {
    for exit in ["return Ok(())", "break", "continue", "print(1)?"] {
        let error = rejected(&format!(
            "def main() -> Result[(), Failure]:
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
        "def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    other = receiver.share()
    with spawn(consume, receiver)?, spawn(consume, other)?, sender as outbox:
        pass
    Ok(())
",
    );
    // Moving the sender later does not release it on an earlier failing path.
    rejected(
        "def main() -> Result[(), Failure]:
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
            "{CONSUME}def main() -> Result[(), Failure]:
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
def main() -> Result[(), Failure]:
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
        let source = format!("{CONSUME}def main() -> ():\n    sender, receiver = channel[i64](1).unwrap()\n    {body}\n");
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("receiving-join");
        support::compile_source_to_executable(&source, &executable).unwrap();
        let output = std::process::Command::new("timeout")
            .arg("5")
            .arg(&executable)
            .output()
            .unwrap();
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
