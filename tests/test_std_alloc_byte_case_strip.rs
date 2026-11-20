use super::test_std_alloc::assert_stdout;
use super::test_std_alloc_set_algebra::POLICY;

#[test]
fn ascii_case_matches_ordinary_on_every_byte() {
    assert_stdout(
        r#"import std.alloc as alloc
import std.bytes as binary

def main() -> Result[(), AllocError | IoError]:
    out = alloc.system()
    mut source = binary.empty()?
    for n in range(256):
        source.push(u8(n))?
    lower = out.lower_bytes(&source)?
    upper = out.upper_bytes(&source)?
    print(lower.to_list()? == binary.lower(&source)?.to_list()?, upper.to_list()? == binary.upper(&source)?.to_list()?)?
    source.clear()
    print(lower.get(65), upper.get(97), lower.get(255), upper.get(128))?
    Ok(())
"#,
        "True True\n97 65 255 128\n",
    );
}

#[test]
fn strip_limits_the_copy_to_surviving_view_bytes() {
    assert_stdout(
        r#"import std.alloc as alloc

def main() -> Result[(), AllocError | IoError]:
    out = alloc.system()
    ws = b"\r\n\t \x0c\x0b\x00 \xff\x0b\r"
    print(out.strip_bytes(ws)?.to_list()?)?
    print(out.lstrip_bytes(ws)?.to_list()?, out.rstrip_bytes(ws)?.to_list()?)?
    mut source = [1u8, 255u8, 0u8, 65u8, 255u8, 66u8, 0u8, 1u8]?
    cut = &source[1:3]
    data = &source[1:-1]
    l = out.lstrip_bytes(data, cut)?
    r = out.rstrip_bytes(data, cut)?
    both = out.strip_bytes(data, set=cut)?
    unchanged = out.strip_bytes(data, b"")?
    drop(source)
    print(l.to_list()?, r.to_list()?, both.to_list()?, unchanged.to_list()?)?
    Ok(())
"#,
        "[0, 32, 255]\n[0, 32, 255, 11, 13] [13, 10, 9, 32, 12, 11, 0, 32, 255]\n[65, 255, 66, 0] [255, 0, 65, 255, 66] [65, 255, 66] [255, 0, 65, 255, 66, 0]\n",
    );
}

#[test]
fn refused_owner_rolls_back_and_empty_results_retain_the_provider() {
    assert_stdout(
        &format!(
            r#"{POLICY}
from std.bytes import Bytes

def make_empty() -> Result[Bytes, AllocError]:
    out = budget("empty", 2)
    out.strip_bytes(b"\xff\xff", b"\xff")

def main() -> Result[(), AllocError | IoError]:
    for n in range(2):
        limited = budget("case", n, AllocError.CapacityOverflow)
        match limited.upper_bytes(b"\xff"):
            case Err(error):
                print(error)?
            case Ok(unexpected):
                drop(unexpected)
    none = budget("no owner", 0)
    print(none.lower_bytes(b"").is_err(), none.lstrip_bytes(b" \n").is_err())?
    mut empty = make_empty()?
    empty.push(255u8)?
    print(empty.reserve(1000).is_err(), empty.to_list()?)?
    drop(empty)
    Ok(())
"#
        ),
        "refused case\nAllocError.CapacityOverflow\ncase 0 0\nrefused case\nAllocError.CapacityOverflow\ncase 0 1\nrefused no owner\nrefused no owner\nTrue True\nrefused empty\nTrue [255]\nempty 0 2\nno owner 0 0\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn transforms_use_only_the_selected_packed_storage() {
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
    mut blocks: list[alloc.ByteBlock] = []?
    for _ in range(10):
        blocks.append(root.block(&layout)?)?
    out = alloc.allocator(&root, Preallocated(blocks)).unwrap()
    rows = [0u8, 65u8, 98u8, 255u8]?
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    lower = out.lower_bytes(&rows)
    upper = out.upper_bytes(&rows)
    both = out.strip_bytes(&rows, b"\x00\xff")
    left = out.lstrip_bytes(&rows, b"\x00")
    right = out.rstrip_bytes(&rows, b"\xff")
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    print(lower?.get(1), upper?.get(2), both?.len(), left?.get(0), right?.len())?
    Ok(())
"#,
        "__test_fail_allocations_after_0__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_restore_allocations__\n97 66 2 65 3\n",
    );
}
