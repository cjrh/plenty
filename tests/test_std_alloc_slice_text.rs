use super::test_std_alloc::assert_stdout;
use super::test_std_alloc_set_algebra::POLICY;
use super::test_std_alloc_slice_clist::check_provider_join;
use crate::test_support::check_error;

#[test]
fn scalar_bounds_select_from_a_view_or_a_whole_string() {
    assert_stdout(
        r#"import std.alloc as alloc

def reversed_part(out: &alloc.Allocator, text: &strview) -> Result[str, AllocError]:
    out.slice_text(text, alloc.Unset.Unset, step=-1)

def main() -> Result[(), AllocError | IoError | RangeError]:
    out = alloc.system()
    mut text = "xé🙂界ay"
    kept = reversed_part(&out, &text[1:-1])?
    print(out.slice_text(&text, i64.MIN, i64.MAX, 2)?, out.slice_text(text, stop=-1, step=-1)?.byte_len())?
    text = "changed"
    k = -2
    print(kept, len(kept), kept.byte_len(), out.slice_text(kept, step=NonZero.try_from(k)?)?)?
    Ok(())
"#,
        "x🙂a 0\na界🙂é 4 10 é界\n",
    );
}

#[test]
fn inline_threshold_refusal_and_worker_read() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def work(text: str) -> Result[(), AllocError | IoError]:
    # Ordinary brackets do not use the exhausted provider.
    print(text[::-1]?)?
    Ok(())

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    out = budget("text", 1, AllocError.CapacityOverflow)
    long = out.slice_text("🙂🙂")?
    print(out.slice_text(long, 2, 2)?.byte_len(), out.slice_text("🙂界")?)?
    match out.slice_text(long):
        case Err(error):
            print(error)?
        case Ok(unexpected):
            drop(unexpected)
    drop(out)
    with spawn(work, long).drop_job()? as task:
        task.join()?
    Ok(())
"#
        ),
        "0 🙂界\nrefused text\nAllocError.CapacityOverflow\n🙂🙂\ntext 0 1\n",
    );
}

#[test]
fn ordinary_boxes_and_string_only_records_release_selected_fields_in_order() {
    assert_stdout(
        &format!(
            r#"{POLICY}
class Names:
    first: str
    second: str

class Pair:
    left: Box[Names]
    right: Box[Names]

def field(label: str) -> Result[str, AllocError]:
    out = budget(label, 1)
    out.slice_text("immutable text")

def main() -> Result[(), AllocError]:
    pair = Box(Pair(Box(Names(field("one")?, field("two")?))?, Box(Names(field("three")?, field("four")?))?))?
    drop(pair)
    Ok(())
"#
        ),
        "one 0 1\ntwo 0 1\nthree 0 1\nfour 0 1\n",
    );
}

#[test]
fn text_slice_keeps_type_and_context_guards() {
    for (step, diagnostic) in [
        ("0", "a NonZero value cannot be zero"),
        ("len(data)", "NonZero[i64]"),
    ] {
        let error = check_error(&format!(
            "import std.alloc as alloc\ndef unused(out: &alloc.Allocator, data: &strview) -> ():\n    drop(out.slice_text(data, step={step}))\ndef main() -> ():\n    pass\n"
        ));
        assert!(error.contains(diagnostic), "{error}");
    }
    let error = check_error(
        "import std.alloc as alloc\nimport std.actor as actors\ndef unused(out: &alloc.Allocator, data: &strview) -> ():\n    drop(out.slice_text(data))\ndef main() -> ():\n    drop(actors.heaps_live())\n",
    );
    assert!(
        error.contains("retained allocator use cannot be combined"),
        "{error}"
    );
    let error = check_error("def main() -> ():\n    drop(__allocator_text_slice(0, 0, 0, 0, 1))\n");
    assert!(
        error.contains("reserved for the standard library"),
        "{error}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn selected_text_slice_requires_no_ordinary_scratch() {
    assert_stdout(
        r#"import std.alloc as alloc

pub class Preallocated:
    blocks: list[alloc.ByteBlock]
    pub def allocate(self: &mut Preallocated, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        self.blocks.pop().ok_or(AllocError.OutOfMemory)
    pub def release(self: &mut Preallocated, storage: alloc.ByteBlock) -> ():
        drop(storage)

def main() -> Result[(), AllocError | IoError]:
    root = alloc.system()
    layout = alloc.layout(1024, 64).unwrap()
    blocks = [root.block(&layout)?]?
    out = alloc.allocator(&root, Preallocated(blocks)).unwrap()
    text = "XL🙂R界A🙂Z"
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    result = out.slice_text(&text[1:-1], step=-2)
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    print(result?)?
    Ok(())
"#,
        "__test_fail_allocations_after_0__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_restore_allocations__\n🙂界🙂\n",
    );
}

#[test]
fn passing_a_last_use_copy_does_not_release_the_original_before_scope_exit() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def measure(value: str) -> i64:
    len(value)

def main() -> Result[(), AllocError | IoError]:
    out = budget("scope", 1)
    text = out.slice_text("abcdefgh")?
    drop(out)
    print(measure(text))?
    print("before scope exit")?
    Ok(())
"#
        ),
        "8\nbefore scope exit\nscope 0 1\n",
    );
}

#[test]
fn observed_text_temporaries_outlive_the_comparison() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def make(label: str) -> str:
    out = budget(label, 1)
    out.slice_text("same selected text").unwrap()

def main() -> ():
    print(make("left") == make("right")).unwrap()
    print("done").unwrap()
"#
        ),
        "True\nright 0 1\nleft 0 1\ndone\n",
    );
}

#[test]
fn unread_channel_message_and_untaken_job_result_release_selected_fields_in_order() {
    assert_stdout(
        &format!(
            r#"{POLICY}
class Message:
    first: str
    second: str

def make(label: str) -> str:
    out = budget(label, 1)
    out.slice_text("selected text").unwrap()

def make_message() -> Message:
    Message(make("first"), make("second"))

def main() -> ():
    tx, rx = channel[Message](1).unwrap()
    tx.send(make_message()).unwrap()
    drop(rx)
    print("closed").unwrap()
    with ThreadPoolExecutor(1, 1).unwrap() as pool:
        future = pool.submit(make_message).unwrap()
        pool.shutdown()
        drop(future)
    print("done").unwrap()
"#
        ),
        "first 0 1\nsecond 0 1\nclosed\nfirst 0 1\nsecond 0 1\ndone\n",
    );
}

// Compile only: neither the policy lock nor its FIFO may be waited on in a test.
const TEXT: &str = r#"out.slice_text("selected text").unwrap()"#;
const ORDINARY: &str = r#""ordinary text""#;
const JOIN: &str = "with spawn(work, ARG).drop_job().unwrap() as task:\n        with guard.lock():\n            task.join()";
const ENDPOINT: &str = "sender, receiver = channel[i64](1).unwrap()\n    with spawn(work, sender, ARG).drop_job().unwrap():\n        pass";
const LOCK: &str = "worker can lock a `Mutex[i64]`";
const WAITS: &str = "could block the worker's send";

#[test]
fn selected_text_worker_callbacks_include_construction_and_implicit_release() {
    let factory = r#"def build(out: &alloc.Allocator) -> ():
    drop(out.slice_text("selected text"))"#;
    for (caller, params, diagnostic) in
        [(JOIN, "", LOCK), (ENDPOINT, "sender: Sender[i64], ", WAITS)]
    {
        let worker =
            format!("{factory}\ndef work({params}out: &alloc.Allocator) -> ():\n    build(out)");
        check_provider_join(
            &worker,
            &caller.replace("ARG", "&out"),
            ORDINARY,
            true,
            Some(diagnostic),
        );
    }

    for (caller, params, diagnostic) in
        [(JOIN, "", LOCK), (ENDPOINT, "sender: Sender[i64], ", WAITS)]
    {
        let worker = format!(
            "class Message:\n    text: Option[Box[str]]\ndef work({params}message: Message) -> ():\n    pass"
        );
        let caller = caller.replace("ARG", "Message(Some(Box(picked).unwrap()))");
        for (owner, registered, expected) in [
            (TEXT, true, Some(diagnostic)),
            (TEXT, false, None),
            (ORDINARY, true, None),
        ] {
            check_provider_join(&worker, &caller, owner, registered, expected);
        }
    }
}

#[test]
fn selected_text_worker_reads_and_other_owner_kinds_do_not_call_policy() {
    let columns = "out.slice_clist(&rows).unwrap()";
    for (ty, arg, code, owner) in [
        ("&str", "&picked", "drop(len(value))", TEXT),
        ("&strview", "&picked", "drop(value.byte_len())", TEXT),
        ("str", ORDINARY, "drop(value)", columns),
        ("clist[Row]", "rows", "drop(value)", TEXT),
    ] {
        for (caller, params) in [(JOIN, ""), (ENDPOINT, "sender: Sender[i64], ")] {
            let worker = format!("def work({params}value: {ty}) -> ():\n    {code}");
            check_provider_join(&worker, &caller.replace("ARG", arg), owner, true, None);
        }
    }
}

#[test]
fn mixed_workers_do_not_attribute_local_cleanup_to_an_unrelated_borrow() {
    // Both producers are emitted. A local owner can wait on release, but
    // it did not bring a mutex from the caller across the task boundary.
    for (owner, other_producer, ty, body) in [
        (
            TEXT,
            "drop(out.slice_clist(&rows))",
            "&str",
            "drop(clist[Row]())\n    drop(len(value))",
        ),
        (
            "out.slice_clist(&rows).unwrap()",
            r#"drop(out.slice_text("selected text"))"#,
            "&clist[Row]",
            "text = \"ordinary\"\n    drop(text)\n    drop(value.len())",
        ),
    ] {
        let worker = format!("def work(value: {ty}) -> ():\n    {body}");
        let caller = format!("{other_producer}\n    {}", JOIN.replace("ARG", "&picked"));
        check_provider_join(&worker, &caller, owner, true, None);
    }
}

#[test]
fn selected_text_pool_result_under_provider_lock_is_rejected() {
    let caller = "with ThreadPoolExecutor(1, 1).unwrap() as pool:\n        future = pool.submit(work, picked).unwrap()\n        with guard.lock():\n            drop(future.result())";
    check_provider_join(
        "def work(value: str) -> ():\n    pass",
        caller,
        TEXT,
        true,
        Some("this result() waits for a job that can lock a `Mutex[i64]`"),
    );
}
