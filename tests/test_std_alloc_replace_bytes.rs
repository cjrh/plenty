use super::test_std_alloc::assert_stdout;
use super::test_std_alloc_set_algebra::POLICY;

#[test]
fn binary_semantics_and_selected_storage() {
    assert_stdout(
        &format!(
            r#"{POLICY}
import std.bytes as binary

def main() -> Result[(), AllocError | IoError]:
    mut src = binary.from_bytes(b"a\xff")?
    for blocks in range(2):
        out = budget("fail", blocks, AllocError.CapacityOverflow)
        match out.replace_bytes(&src, b"a", b"X"):
            case Err(error):
                print(error)?
            case Ok(unexpected):
                drop(unexpected)
    print(src)?
    out = alloc.system()
    print(out.replace_bytes(b"aaaaa", b"aa", b"Z")?, out.replace_bytes(b"ab", b"", b"-")?, out.replace_bytes(b"", b"", b"x")?)?
    copy = out.replace_bytes(&src, b"no", b"")?
    src.clear()
    print(copy)?
    policy = budget("empty", 1)
    mut empty = policy.replace_bytes(b"a", b"a", b"")?
    drop(policy)
    print(empty.reserve(64).is_err())?
    drop(empty)
    Ok(())
"#
        ),
        "refused fail\nAllocError.CapacityOverflow\nfail 0 0\nrefused fail\nAllocError.CapacityOverflow\nfail 0 1\nb\"a\\xff\"\nb\"ZZa\" b\"-a-b-\" b\"x\"\nb\"a\\xff\"\nrefused empty\nTrue\nempty 0 1\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn selected_result_uses_no_system_buffer() {
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
    parts = root.split_bytes(b"a|\xff", b"|")?
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    result = out.replace_bytes(&parts[1], b"", b"+")
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    print(result?)?
    Ok(())
"#,
        "__test_fail_allocations_after_0__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_restore_allocations__\nb\"+\\xff+\"\n",
    );
}
