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
fn stream_join_releases_each_piece_before_resuming() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def piece() -> str:
    owner = budget("piece", 1)
    owner.concat_text("🙂", "🙂").unwrap()

def words() -> Generator[str]:
    yield piece()
    print("resumed").unwrap()
    yield ""
    yield "界"

def main() -> Result[(), AllocError | IoError]:
    out = alloc.system()
    text = out.join_text_stream("·", words())?
    print(text, len(text), text.byte_len())?
    Ok(())
"#
        ),
        "piece 0 1\nresumed\n🙂🙂··界 5 15\n",
    );
}

#[test]
fn stream_join_unwinds_before_poll_during_append_and_at_finish() {
    assert_stdout(
        &format!(
            r#"{POLICY}
class Capture:
    def __del__(self) -> ():
        print("capture dropped").unwrap()

def words(capture: Capture) -> Generator[str]:
    print("poll").unwrap()
    yield "12345678"
    print("end").unwrap()

def main() -> Result[(), AllocError | IoError]:
    for count in range(4):
        out = budget("storage", count, AllocError.CapacityOverflow)
        match out.join_text_stream("-", words(Capture())):
            case Err(error):
                print(error)?
            case Ok(text):
                print(text)?
    Ok(())
"#
        ),
        "refused storage\ncapture dropped\nAllocError.CapacityOverflow\nstorage 0 0\npoll\nrefused storage\ncapture dropped\nAllocError.CapacityOverflow\nstorage 0 1\npoll\nend\ncapture dropped\nrefused storage\nAllocError.CapacityOverflow\nstorage 0 2\npoll\nend\ncapture dropped\n12345678\nstorage 0 3\n",
    );
}

#[test]
fn join_text_borrows_ranges_and_retains_only_result_storage() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def main() -> Result[(), AllocError | IoError]:
    out = budget("join", 1, AllocError.CapacityOverflow)
    pieces = ["skip", "🙂", "", "界", "skip"]?
    separator = "x·y"
    saved = out.join_text(&separator[1:2], &pieces[1:-1])?
    print(out.join_text("", &pieces[1:-1])?, out.join_text("unused", &pieces[2:2])?.byte_len(), out.join_text("/", &pieces[3:4])?)?
    match out.join_text("a", &pieces[1:-1]):
        case Err(error):
            print(error)?
        case Ok(unexpected):
            drop(unexpected)
    print(pieces[1], separator)?
    drop(pieces)
    drop(separator)
    drop(out)
    print(saved, len(saved), saved.byte_len())?
    drop(saved)
    Ok(())
"#
        ),
        "🙂界 0 界\nrefused join\nAllocError.CapacityOverflow\n🙂 x·y\n🙂··界 4 11\njoin 0 1\n",
    );
}

#[test]
fn affix_removal_copies_only_one_exact_match_into_selected_storage() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def main() -> Result[(), AllocError | IoError]:
    out = budget("affix", 1)
    source = ("x🙂" + "🙂界y")?
    view = &source[1:-1]
    print(out.removeprefix_text(view, &source[1:2])?, out.removesuffix_text("界🙂🙂", "🙂")?)?
    print(out.removeprefix_text("🙂", "🙂")?.byte_len(), out.removesuffix_text("界", "")?)?
    saved = out.removesuffix_text(view, "not present")?
    match out.removeprefix_text(view, ""):
        case Err(error):
            print(error)?
        case Ok(unexpected):
            drop(unexpected)
    print(source)?
    drop(source)
    drop(out)
    print(saved)?
    drop(saved)
    Ok(())
"#
        ),
        "🙂界 界🙂\n0 界\nrefused affix\nAllocError.OutOfMemory\nx🙂🙂界y\n🙂🙂界\naffix 0 1\n",
    );
}

#[test]
fn selected_split_text_releases_partial_list_and_fields() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def main() -> Result[(), AllocError | IoError]:
    source = ("x界abcdefgh" + "界🙂🙂界界y")?
    sep = "x界y"
    partial = budget("partial", 4, AllocError.CapacityOverflow)
    match partial.split_text(&source[1:-1], &sep[1:2]):
        case Err(error):
            print(error)?
        case Ok(unexpected):
            drop(unexpected)
    drop(partial)
    out = budget("output", 5)
    fields = out.split_text(&source[1:-1], &sep[1:2])?
    drop(source)
    drop(sep)
    drop(out)
    print(len(&fields), fields[0].byte_len(), fields[1], fields[2], fields[3].byte_len(), fields[4].byte_len())?
    drop(fields)
    root = alloc.system()
    print(len(&root.split_text("", ",")?), root.split_text("aaa", "aa")?[1])?
    Ok(())
"#
        ),
        "refused partial\nAllocError.CapacityOverflow\npartial 0 4\n5 0 abcdefgh 🙂🙂 0 0\noutput 0 5\n1 a\n",
    );
}

#[test]
fn selected_split_text_traps_before_allocating_for_computed_empty_separator() {
    let output = crate::test_support::run(&format!(
        r#"{POLICY}
def main() -> ():
    out = budget("unexpected", 0)
    sep = ""
    drop(out.split_text("abc", sep))
"#
    ));
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("string split requires a nonempty separator")
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("refused"));
}

#[test]
fn selected_split_text_rejects_literal_empty_separator() {
    for args in [r#""abc", ("")"#, r#"separator="", source="abc""#] {
        let e = check_error(&format!(
            "import std.alloc as alloc\ndef main() -> ():\n    drop(alloc.system().split_text({args}))\n"
        ));
        assert!(e.contains("split requires a nonempty separator"), "{e}");
    }
}

#[test]
fn case_text_sizes_mapped_unicode_and_copies_unchanged_text() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def main() -> Result[(), AllocError | IoError]:
    out = budget("case", 2)
    source = ("x" + "İABCDEy")?
    view = &source[1:-1]
    print(out.upper_text("straße")?, out.lower_text("")?.byte_len())?
    mapped = out.lower_text(view)?
    unchanged = out.upper_text("ABCDEFGH")?
    match out.lower_text(view):
        case Err(error):
            print(error)?
        case Ok(unexpected):
            drop(unexpected)
    print(mapped == view.lower()?, source)?
    drop(source)
    drop(out)
    print(mapped, len(mapped), mapped.byte_len(), unchanged)?
    drop(mapped)
    drop(unchanged)
    Ok(())
"#
        ),
        "STRASSE 0\nrefused case\nAllocError.OutOfMemory\nTrue xİABCDEy\ni\u{307}abcde 7 8 ABCDEFGH\ncase 0 2\n",
    );
}

#[test]
fn strip_text_selects_unicode_ends_and_copies_unchanged_text() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def main() -> Result[(), AllocError | IoError]:
    out = budget("trim", 1)
    label = "x　a b y"
    view = &label[1:-1]
    print(out.strip_text(view)?, out.lstrip_text(view)? == "a b ", out.rstrip_text(view)? == "　a b")?
    print(out.strip_text("　🙂界 ")?, out.strip_text("　 ")?.byte_len(), out.rstrip_text("")?.byte_len())?
    source = ("🙂" + "🙂")?
    saved = out.strip_text(source)?
    match out.lstrip_text(source):
        case Err(error):
            print(error)?
        case Ok(unexpected):
            drop(unexpected)
    print(source)?
    drop(source)
    drop(out)
    print(saved, len(saved), saved.byte_len())?
    drop(saved)
    Ok(())
"#
        ),
        "a b True True\n🙂界 0 0\nrefused trim\nAllocError.OutOfMemory\n🙂🙂\n🙂🙂 2 8\ntrim 0 1\n",
    );
}

#[test]
fn replace_text_matches_scalar_boundaries_and_copies_unmatched_text() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def main() -> Result[(), AllocError | IoError]:
    out = budget("replace", 1)
    print(out.replace_text("界", "", "·")?, out.replace_text("aaaaa", "aa", "X")?, out.replace_text("abc", "abc", "")?.byte_len())?
    source = ("x🙂" + "🙂y")?
    old = "q??q"
    new = "x界x"
    saved = out.replace_text(&source[1:3], &old[1:3], &new[1:2])?
    match out.replace_text(source, old, new):
        case Err(error):
            print(error)?
        case Ok(unexpected):
            drop(unexpected)
    print(source, old, new)?
    drop(source)
    drop(out)
    print(saved, len(saved), saved.byte_len())?
    drop(saved)
    Ok(())
"#
        ),
        "·界· XXa 0\nrefused replace\nAllocError.OutOfMemory\nx🙂🙂y q??q x界x\n🙂🙂 2 8\nreplace 0 1\n",
    );
}

#[test]
fn repeat_text_checks_size_before_requesting_selected_storage() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def main() -> Result[(), AllocError | IoError]:
    out = budget("repeat", 1)
    source = ("x🙂界" + "y")?
    print(out.repeat_text(&source[1:3], 1)?, out.repeat_text(source, i64.MIN)?.byte_len(), out.repeat_text(source, 0)?.byte_len(), out.repeat_text("", i64.MAX)?.byte_len())?
    match out.repeat_text("ab", i64.MAX):
        case Err(error):
            print(error)?
        case Ok(unexpected):
            drop(unexpected)
    saved = out.repeat_text(&source[1:2], 2)?
    match out.repeat_text(source, 1):
        case Err(error):
            print(error)?
        case Ok(unexpected):
            drop(unexpected)
    print(source)?
    drop(source)
    drop(out)
    print(saved, len(saved), saved.byte_len())?
    drop(saved)
    Ok(())
"#
        ),
        "🙂界 0 0 0\nAllocError.CapacityOverflow\nrefused repeat\nAllocError.OutOfMemory\nx🙂界y\n🙂🙂 2 8\nrepeat 0 1\n",
    );
}

#[test]
fn concat_text_writes_utf8_directly_to_selected_storage() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def main() -> Result[(), AllocError | IoError]:
    out = budget("concat", 1, AllocError.CapacityOverflow)
    left = ("prefix" + "🙂")?
    right = ("🙂" + "suffix")?
    saved = out.concat_text(&left[6:], &right[:1])?
    print(out.concat_text("", "")?.byte_len(), out.concat_text("🙂", "界")?)?
    match out.concat_text(left, right):
        case Err(error):
            print(error)?
        case Ok(unexpected):
            drop(unexpected)
    print(left, right)?
    drop(left)
    drop(right)
    drop(out)
    print(saved, len(saved), saved.byte_len())?
    drop(saved)
    Ok(())
"#
        ),
        "0 🙂界\nrefused concat\nAllocError.CapacityOverflow\nprefix🙂 🙂suffix\n🙂🙂 2 8\nconcat 0 1\n",
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
fn observed_tuple_and_record_owners_drop_after_print_in_reverse_order() {
    assert_stdout(
        &format!(
            r#"{POLICY}
class Record:
    item: (str, i64)

def pair(label: str) -> (str, i64):
    out = budget(label, 1)
    (out.slice_text("same selected text").unwrap(), 1)

def record(label: str) -> Record:
    Record(pair(label))

def main() -> ():
    print(pair("left") == pair("right")).unwrap()
    print(record("first") == record("second")).unwrap()
    print(record("projection").item[0]).unwrap()
    print("done").unwrap()
"#
        ),
        "True\nright 0 1\nleft 0 1\nTrue\nsecond 0 1\nfirst 0 1\nsame selected text\nprojection 0 1\ndone\n",
    );
}

#[test]
fn observed_borrowed_records_and_fields_leave_the_owner_intact() {
    assert_stdout(
        &format!(
            r#"{POLICY}
class Record:
    text: str

def inspect(records: &list[Record], index: i64) -> ():
    print(records[index] == records[0], records[index].text).unwrap()

def main() -> ():
    out = budget("owner", 1)
    records = [Record(out.slice_text("selected field").unwrap())].unwrap()
    drop(out)
    inspect(&records, 0)
    print(records[0].text).unwrap()
    drop(records)
    print("done").unwrap()
"#
        ),
        "True selected field\nselected field\nowner 0 1\ndone\n",
    );
}

#[test]
fn observed_slot_reuse_preserves_checked_failure_cleanup() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def make(label: str) -> str:
    out = budget(label, 1)
    out.slice_text("selected text").unwrap()

def exercise(skip: bool, n: i64) -> ():
    kept = make("kept")
    borrowed = &kept
    drop(make("warm") == make("up"))
    if skip:
        drop(make("then") == "selected text")
    else:
        failed = checked:
            make("left") == make("right") and n + 1 == 0
        print(failed.is_err()).unwrap()
    print(*borrowed).unwrap()

def main() -> ():
    exercise(False, i64.MAX)
"#
        ),
        "up 0 1\nwarm 0 1\nright 0 1\nleft 0 1\nTrue\nselected text\nkept 0 1\n",
    );
}

#[test]
fn observed_ordinary_temporaries_reuse_typed_slots_across_statements() {
    // More observations than a function has slots. Alternate types, include
    // a conditional store, and need two distinct string slots at once.
    let statements = "    drop(make() == \"x\")\n    drop(pair() == pair())\n    if flag and make() == make():\n        pass\n".repeat(100);
    assert_stdout(
        &format!(
            r#"def make() -> str:
    return "x"

def pair() -> (str, i64):
    (make(), 1)

def observe_all(flag: bool) -> ():
{statements}    print("done").unwrap()

def main() -> ():
    observe_all(False)
    observe_all(True)
"#
        ),
        "done\ndone\n",
    );
}

#[test]
fn observed_pair_slots_are_reused_across_sibling_blocks() {
    // With a bool parameter, 128 fresh pairs of slots exceed the frame.
    let comparison = "drop(pair() == pair())";
    let otherwise =
        format!("    if flag:\n        pass\n    else:\n        {comparison}\n").repeat(128);
    let mirrored =
        format!("    if flag:\n        {comparison}\n    else:\n        pass\n").repeat(128);
    let nested = format!(
        "    if flag:\n        drop(text() == text())\n    else:\n        if other:\n            pass\n        else:\n            {comparison}\n"
    )
    .repeat(128);
    assert_stdout(
        &format!(
            r#"def text() -> str:
    return "x"

def pair() -> (str, i64):
    ("x", 1)

def otherwise(flag: bool) -> ():
{otherwise}
def mirrored(flag: bool) -> ():
{mirrored}
def nested(flag: bool, other: bool) -> ():
{nested}
def main() -> ():
    otherwise(False)
    otherwise(True)
    mirrored(False)
    mirrored(True)
    nested(True, False)
    nested(False, True)
    nested(False, False)
    print("done").unwrap()
"#
        ),
        "done\n",
    );
}

#[test]
fn observed_pair_slots_are_reused_within_a_single_branch_chain() {
    // No intervening outer statement to recycle slots for the next arm.
    let mut elifs = String::new();
    let mut cases = String::new();
    for i in 0..128 {
        elifs.push_str(&format!(
            "    elif n == {i}:\n        drop(pair() == pair())\n"
        ));
        cases.push_str(&format!(
            "        case {i}:\n            drop(pair() == pair())\n"
        ));
    }
    assert_stdout(
        &format!(
            r#"def pair() -> (str, i64):
    ("x", 1)

def by_if(n: i64) -> ():
    if n < 0:
        pass
{elifs}    else:
        pass

def by_match(n: i64) -> ():
    match n:
{cases}        case _:
            pass

def main() -> ():
    by_if(127)
    by_match(127)
    print("done").unwrap()
"#
        ),
        "done\n",
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

// Only the outer allocator reaches the worker. Destroying its state can
// release the last selected string, calling the separate, lock-taking Policy.
const HOLDING_TEXT: &str = r#"pub class HoldingText:
    saved: Option[Box[str]]
    pub def allocate(self: &mut HoldingText, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        alloc.system().block(request)
    pub def release(self: &mut HoldingText, storage: alloc.ByteBlock) -> ():
        drop(storage)
"#;

fn holding_text(value: &str) -> String {
    format!("alloc.allocator(&root, HoldingText(Some(Box({value}).unwrap()))).unwrap()")
}

#[test]
fn final_allocator_release_at_scoped_join_includes_hidden_text_cleanup() {
    let owning =
        format!("{HOLDING_TEXT}\ndef work(owner: alloc.Allocator) -> ():\n    drop(owner)");
    let join = JOIN.replace("ARG", "picked");
    check_provider_join(&owning, &join, &holding_text(TEXT), true, Some(LOCK));

    let borrowed = format!(
        "{HOLDING_TEXT}\ndef work(owner: &alloc.Allocator) -> ():\n    drop(owner.share())"
    );
    check_provider_join(
        &borrowed,
        &JOIN.replace("ARG", "&picked"),
        &holding_text(TEXT),
        true,
        None,
    );

    // A live clist producer does not activate the ordinary str in HoldingText.
    let clist_only = format!("drop(out.slice_clist(&rows))\n    {join}");
    check_provider_join(&owning, &clist_only, &holding_text(ORDINARY), true, None);
}

#[test]
fn final_allocator_release_in_pool_job_accounts_for_hidden_text_at_result_and_shutdown() {
    let worker = format!("{HOLDING_TEXT}\ndef work(owner: alloc.Allocator) -> ():\n    pass");
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
            "with ThreadPoolExecutor(1, 1).unwrap() as pool:\n        future = pool.submit(work, picked).unwrap()\n        with guard.lock():\n            {wait}"
        );
        check_provider_join(
            &worker,
            &caller,
            &holding_text(TEXT),
            true,
            Some(diagnostic),
        );
    }
}
