use super::test_std_alloc::assert_stdout;
use super::test_std_alloc_set_algebra::POLICY;

#[test]
fn split_binary_views_preserves_fields_and_ignores_non_ascii_space() {
    assert_stdout(
        r#"import std.alloc as alloc
import std.bytes as binary

def main() -> Result[(), AllocError | IoError]:
    out = alloc.system()
    # Both the source and separator are narrowed, and neither is UTF-8 text.
    mut frame = binary.from_bytes(b"x\xff\x00A\xff\x00\xff\x00y")?
    mut sep = [9u8, 255u8, 0u8, 8u8]?
    fields = out.split_bytes(&frame[1:-1], &sep[1:3])?
    frame.clear()
    sep[1] = 0u8
    print(fields)?
    print(out.split_bytes(b"aaaaa", b"aa")?, out.split_bytes(b"\x00", b"missing")?, out.split_bytes(b"", b"x")?)?
    rows = [9u8, 128u8, 0u8, 10u8, 13u8, 160u8, 11u8, 192u8, 12u8, 133u8, 32u8]?
    print(out.split_whitespace_bytes(&rows)?, out.split_whitespace_bytes(b"")?)?
    Ok(())
"#,
        "[b\"\", b\"A\", b\"\", b\"\"]\n[b\"\", b\"\", b\"a\"] [b\"\\x00\"] [b\"\"]\n[b\"\\x80\\x00\", b\"\\xa0\", b\"\\xc0\", b\"\\x85\"] []\n",
    );
}

#[test]
fn empty_separator_traps_before_any_provider_request() {
    let result = crate::test_support::run(&format!(
        r#"{POLICY}
def main() -> ():
    out = budget("unexpected", 0)
    sep = b"xy"
    drop(out.split_bytes(b"", &sep[1:1]))
"#
    ));
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("bytes.split: separator is empty"));
    assert!(!String::from_utf8_lossy(&result.stdout).contains("refused"));
}

#[test]
fn refusal_unwinds_the_partial_list_and_leaves_both_inputs_usable() {
    // Fail every request up through the last field's owner. Includes refusals
    // during list growth and after earlier nonempty fields are in the list.
    for call in [
        "out.split_bytes(&src, &sep)",
        "out.split_whitespace_bytes(&src)",
    ] {
        let mut expected = String::new();
        for blocks in 0..9 {
            expected.push_str(&format!(
                "refused failed\nAllocError.CapacityOverflow\nfailed 0 {blocks}\n"
            ));
        }
        expected.push_str("b\"a b c\" b\" \"\nb\"A b c\" b\"+\"\n");
        assert_stdout(
            &format!(
                r#"{POLICY}
import std.bytes as binary

def main() -> Result[(), AllocError | IoError]:
    mut src = binary.from_bytes(b"a b c")?
    mut sep = binary.from_bytes(b" ")?
    for blocks in range(9):
        out = budget("failed", blocks, AllocError.CapacityOverflow)
        match {call}:
            case Err(error):
                print(error)?
            case Ok(unexpected):
                drop(unexpected)
    print(src, sep)?
    src.set(0, 65u8)
    sep.set(0, 43u8)
    print(src, sep)?
    Ok(())
"#
            ),
            &expected,
        );
    }
}

#[test]
fn lists_and_fields_including_empty_results_retain_growth_policy() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def main() -> Result[(), AllocError | IoError]:
    # Two list headers, list backing, one empty owner and one nonempty buffer.
    out = budget("fields", 7)
    mut fields = out.split_bytes(b"|word", b"|")?
    drop(out)
    mut word = fields.pop().unwrap()
    mut empty = fields.pop().unwrap()
    empty.push(33u8)?
    print(empty.reserve(100).is_err(), word.reserve(100).is_err(), fields.reserve(100).is_err())?
    print(empty, word)?
    drop(fields)
    drop(word)
    drop(empty)

    policy = budget("words", 3)
    mut words = policy.split_whitespace_bytes(b"\x0b \t\r\n\x0c")?
    drop(policy)
    print(len(&words))?
    words.reserve(8)?
    print(words.reserve(100).is_err())?
    drop(words)
    Ok(())
"#
        ),
        "refused fields\nrefused fields\nrefused fields\nTrue True True\nb\"!\" b\"word\"\nfields 0 7\n0\nrefused words\nTrue\nwords 0 3\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn selected_splits_do_not_use_ordinary_scratch() {
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
    mut blocks = list[alloc.ByteBlock]()?
    for i in range(20):
        blocks.append(root.block(&layout)?)?
    out = alloc.allocator(&root, Preallocated(blocks)).unwrap()
    src = [42u8, 255u8, 32u8, 32u8, 128u8, 42u8]?
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    fields = out.split_bytes(&src[1:5], b" ")
    words = out.split_whitespace_bytes(&src[1:5])
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    print(fields?, words?)?
    Ok(())
"#,
        "__test_fail_allocations_after_0__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_restore_allocations__\n[b\"\\xff\", b\"\", b\"\\x80\"] [b\"\\xff\", b\"\\x80\"]\n",
    );
}
