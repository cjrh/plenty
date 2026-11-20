use super::test_std_alloc::assert_stdout;
use super::test_std_alloc_set_algebra::POLICY;

#[test]
fn binary_semantics_and_selected_storage() {
    assert_stdout(
        &format!(
            r#"{POLICY}
import std.bytes as binary

def main() -> Result[(), AllocError | IoError]:
    mut parts = alloc.system().split_bytes(b"a||\xff", b"|")?
    for blocks in range(2):
        out = budget("fail", blocks, AllocError.CapacityOverflow)
        match out.join_bytes(b"-", &parts):
            case Err(error):
                print(error)?
            case Ok(unexpected):
                drop(unexpected)
    print(parts)?
    out = alloc.system()
    print(out.join_bytes(b"--", &parts)?, out.join_bytes(b"", &parts)?)?
    parts.clear()
    print(out.join_bytes(b"|", &parts)?)?
    policy = budget("empty", 1)
    mut empty = policy.join_bytes(b"", &parts)?
    drop(policy)
    print(empty.reserve(64).is_err())?
    drop(empty)
    Ok(())
"#
        ),
        "refused fail\nAllocError.CapacityOverflow\nfail 0 0\nrefused fail\nAllocError.CapacityOverflow\nfail 0 1\n[b\"a\", b\"\", b\"\\xff\"]\nb\"a----\\xff\" b\"a\\xff\"\nb\"\"\nrefused empty\nTrue\nempty 0 1\n",
    );
}

#[test]
fn join_accepts_byte_views_but_not_text() {
    assert_stdout(
        r#"import std.alloc as alloc

def main() -> Result[(), AllocError | IoError]:
    out = alloc.system()
    print(out.join_bytes(b"-", &[b"a", b"b"]?)?)?
    rows = [[65u8]?, [66u8]?]?
    print(out.join_bytes(b"-", &rows)?)?
    shared = [Shared([65u8]?)?]?
    print(out.join_bytes(b"-", &shared)?)?
    Ok(())
"#,
        "b\"a-b\"\nb\"A-B\"\nb\"A\"\n",
    );
    let error = crate::test_support::check_file_error(
        r#"import std.alloc as alloc

def main() -> Result[(), AllocError]:
    parts = ["not bytes"]?
    drop(alloc.system().join_bytes(b"", &parts)?)
    Ok(())
"#,
    );
    assert!(error.contains("str") && error.contains("u8"), "{error}");
}

#[test]
fn single_part_reserves_one_payload_and_outlives_the_handle() {
    assert_stdout(
        r#"import std.alloc as alloc
import std.bytes as binary

pub class Probe:
    root: alloc.Allocator
    calls: i64
    released: i64
    pub def allocate(self: &mut Probe, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        if self.calls == 2:
            return Err(AllocError.OutOfMemory)
        unchecked:
            self.calls += 1
        self.root.block(request)
    pub def release(self: &mut Probe, block: alloc.ByteBlock) -> ():
        unchecked:
            self.released += 1
        drop(block)
    def __del__(self: &mut Probe) -> ():
        print(self.calls, self.released).unwrap()

def main() -> Result[(), AllocError | IoError]:
    root = alloc.system()
    out = alloc.allocator(&root, Probe(root.share(), 0, 0)).unwrap()
    mut parts = [binary.from_bytes(b"0123456789abc")?]?
    result = out.join_bytes(b"not written", &parts)?
    drop(out)
    mut original = parts.pop().unwrap()
    original.set(0, 88u8)
    # Provider layouts include metadata; capacity counts only payload bytes.
    print(result.capacity())?
    print(original, result)?
    drop(result)
    Ok(())
"#,
        "13\nb\"X123456789abc\" b\"0123456789abc\"\n2 2\n",
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
    result = out.join_bytes(b"+", &parts)
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    print(result?)?
    Ok(())
"#,
        "__test_fail_allocations_after_0__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_restore_allocations__\nb\"a+\\xff\"\n",
    );
}
