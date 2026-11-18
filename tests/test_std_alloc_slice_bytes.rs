use super::test_std_alloc::assert_stdout;
use super::test_std_alloc_set_algebra::POLICY;
use crate::test_support::check_error;

#[test]
fn byte_views_share_one_packed_abi_and_slice_bounds() {
    assert_stdout(
        r#"import std.alloc as alloc
import std.bytes as binary

def middle(out: &alloc.Allocator, data: &[u8]) -> Result[binary.Bytes, AllocError]:
    out.slice_bytes(&data[1:5])

def main() -> Result[(), AllocError | IoError | RangeError]:
    out = alloc.system()
    literal = b"X\x01\x80\x00\xffY"
    packed = binary.from_bytes(literal)?
    rows = [88u8, 1u8, 128u8, 0u8, 255u8, 89u8]?
    print(middle(&out, literal)?.to_list()?)?
    print(middle(&out, &packed)?.to_list()?)?
    print(middle(&out, &rows)?.to_list()?)?
    k = 2
    print(out.slice_bytes(&rows[1:5], i64.MIN, i64.MAX, NonZero.try_from(k)?)?.to_list()?)?
    print(out.slice_bytes(&packed[1:5], stop=-1, step=-1)?.len())?
    print(out.slice_bytes(&packed[1:5], alloc.Unset.Unset, step=-1)?.to_list()?)?
    print(out.slice_bytes(b"", step=-1)?.is_empty())?
    Ok(())
"#,
        "[1, 128, 0, 255]\n[1, 128, 0, 255]\n[1, 128, 0, 255]\n[1, 0]\n0\n[255, 0, 128, 1]\nTrue\n",
    );
}

#[test]
fn byte_slice_rolls_back_refusals_and_keeps_the_provider_not_the_source() {
    assert_stdout(
        &format!(
            r#"{POLICY}
import std.bytes as binary

def make() -> Result[binary.Bytes, AllocError | IoError]:
    mut src = binary.from_bytes(b"XabcdY")?
    for blocks in range(2):
        limited = budget("partial", blocks, AllocError.CapacityOverflow)
        match limited.slice_bytes(&src[1:5], step=-1):
            case Err(error):
                print(error)?
            case Ok(unexpected):
                drop(unexpected)
    print(src.to_list()?)?
    out = budget("kept", 3)
    result = out.slice_bytes(&src[1:5], step=-1)?
    src.clear()
    Ok(result)

def main() -> Result[(), AllocError | IoError]:
    mut result = make()?
    result.push(33u8)?
    print(result.reserve(100).is_err(), result.to_list()?)?
    # Neither ordinary owning form inherits the exhausted provider.
    print(result[1:4:2]?.to_list()?, result.slice(2, 4)?.to_list()?)?
    drop(result)
    Ok(())
"#
        ),
        "refused partial\nAllocError.CapacityOverflow\npartial 0 0\nrefused partial\nAllocError.CapacityOverflow\npartial 0 1\n[88, 97, 98, 99, 100, 89]\nrefused kept\nTrue [100, 99, 98, 97, 33]\n[99, 97] [98, 97]\nkept 0 3\n",
    );
}

#[test]
fn narrowed_source_loan_ends_with_the_call_not_the_result() {
    let source = r#"import std.alloc as alloc

def main() -> Result[(), AllocError | IoError]:
    out = alloc.system()
    mut rows = [1u8, 2u8, 3u8, 4u8]?
    view = &rows[1:3]
    result = out.slice_bytes(view)?
    rows[1] = 9u8
    print(result.to_list()?, rows)?
    Ok(())
"#;
    assert_stdout(source, "[2, 3] [1, 9, 3, 4]\n");
    let error = check_error(&source.replace("    result =", "    rows[1] = 8u8\n    result ="));
    assert!(
        error.contains("borrow") && error.contains("rows[1:3]"),
        "{error}"
    );
}

#[test]
fn selected_byte_slice_keeps_step_context_and_intrinsic_checks() {
    for (step, diagnostic) in [
        ("0", "a NonZero value cannot be zero"),
        ("len(data)", "NonZero[i64]"),
    ] {
        let error = check_error(&format!(
            "import std.alloc as alloc\ndef unused(out: &alloc.Allocator, data: &[u8]) -> ():\n    drop(out.slice_bytes(data, step={step}))\ndef main() -> ():\n    pass\n"
        ));
        assert!(error.contains(diagnostic), "{error}");
    }
    let error = check_error(
        "import std.alloc as alloc\nimport std.actor as actors\ndef unused(out: &alloc.Allocator, data: &[u8]) -> ():\n    drop(out.slice_bytes(data))\ndef main() -> ():\n    drop(actors.heaps_live())\n",
    );
    assert!(
        error.contains("retained allocator use cannot be combined"),
        "{error}"
    );
    let error =
        check_error("def main() -> ():\n    drop(__allocator_bytes_slice(0, 0, 0, 0, 1))\n");
    assert!(
        error.contains("reserved for the standard library"),
        "{error}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn selected_byte_slice_requires_no_ordinary_scratch() {
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
    blocks = [root.block(&layout)?, root.block(&layout)?]?
    out = alloc.allocator(&root, Preallocated(blocks)).unwrap()
    rows = [1u8, 2u8, 3u8, 4u8, 5u8]?
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    result = out.slice_bytes(&rows[1:4], step=-2)
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    print(result?.to_list()?)?
    Ok(())
"#,
        "__test_fail_allocations_after_0__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_restore_allocations__\n[4, 2]\n",
    );
}

#[test]
fn ordinary_box_drops_nested_selected_bytes_in_field_order() {
    assert_stdout(
        &format!(
            r#"{POLICY}
from std.bytes import Bytes

class Pair:
    first: Box[Bytes]
    second: Box[Bytes]

def field(name: str) -> Result[Box[Bytes], AllocError]:
    provider = budget(name, 2)
    Box(provider.slice_bytes(b"x")?)

def main() -> Result[(), AllocError]:
    outer = Box(Pair(field("first")?, field("second")?))?
    drop(outer)
    Ok(())
"#
        ),
        "first 0 2\nsecond 0 2\n",
    );
}
