//! First language bindings to retained native allocator storage.
mod support;

pub(super) fn assert_stdout(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("program");
    for optimize in [false, true] {
        let options = plenty::CompileOptions {
            optimize,
            ..Default::default()
        };
        plenty::compile_source_to_executable_with_options(source, &executable, &options).unwrap();
        let mut command = if std::env::var_os("PLENTY_TEST_VALGRIND").is_some() {
            let mut command = std::process::Command::new("valgrind");
            command
                .args([
                    "--quiet",
                    "--error-exitcode=99",
                    "--leak-check=full",
                    "--errors-for-leak-kinds=definite,indirect,possible",
                ])
                .arg(&executable);
            command
        } else {
            std::process::Command::new(&executable)
        };
        let output = support::assert_success(command.output().unwrap());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }
}

#[test]
fn explicit_system_lists_grow_and_outlive_handles() {
    assert_stdout(
        r#"import std.alloc as alloc

def make() -> Result[list[i64], AllocError]:
    a = alloc.system()
    b = a.share()
    mut values = b.empty_list[i64]()?
    for i in range(30):
        values.append(i)?
    values.reserve(100)?
    Ok(values)

def main() -> Result[(), AllocError | IoError]:
    mut values = make()?
    values.append(99)?
    print(len(values), values[0], values[29], values[30])?
    a = alloc.system()
    print(a.list_with_capacity[i64](-1).is_err())?
    print(a.list_with_capacity[i64](i64.MAX).is_err())?
    other = a.list_with_capacity[f64](5)?
    print(len(other))?
    Ok(())
"#,
        "31 0 29 99\nTrue\nTrue\n0\n",
    );
}

#[test]
fn handles_and_builtin_lists_can_move_to_workers() {
    assert_stdout(
        r#"import std.alloc as alloc

def work(owner: alloc.Allocator, values: list[i64]) -> i64:
    mut extra = owner.empty_list[i64]().unwrap()
    extra.append(7).unwrap()
    len(values)

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    owner = alloc.system()
    mut values = owner.empty_list[i64]()?
    values.append(3)?
    with spawn(work, owner.share(), values).drop_job()? as task:
        print(task.join())?
    Ok(())
"#,
        "1\n",
    );
}

#[test]
fn retained_system_use_rejects_actor_heap_observation() {
    let error = support::check_error(
        r#"import std.alloc as alloc
import std.actor as actors

def main() -> ():
    owner = alloc.system()
    drop(actors.heaps_live())
"#,
    );
    assert!(
        error.contains("retained allocator use cannot be combined"),
        "{error}"
    );
}

#[test]
fn nested_inline_values_relocate_and_destroy_once() {
    assert_stdout(
        r#"import std.alloc as alloc

class Row:
    id: i64
    payload: array[i64, 8]
    nested: list[i64]
    def __del__(self: &mut Row) -> ():
        print(self.id, self.payload[7], self.nested[0]).unwrap()

def main() -> Result[(), AllocError | IoError]:
    owner = alloc.system()
    mut rows = owner.empty_list[Row]()?
    for id in range(3):
        mut nested = owner.empty_list[i64]()?
        nested.append(id)?
        rows.append(Row(id, array[i64, 8].filled(id), nested))?
    rows.reserve(64)?
    print(len(rows))?
    drop(rows)
    Ok(())
"#,
        "3\n0 0 0\n1 1 1\n2 2 2\n",
    );
}

#[test]
fn raw_allocator_intrinsics_are_not_public() {
    let error = support::check_error("def main() -> ():\n    drop(__allocator_system())\n");
    assert!(
        error.contains("reserved for the standard library"),
        "{error}"
    );
}

#[test]
fn layouts_validate_without_allocating_and_cannot_be_forged() {
    assert_stdout(
        r#"import std.alloc as alloc

def main() -> Result[(), IoError]:
    a = alloc.layout(0).unwrap()
    b = a
    print(a.size(), b.alignment())?
    for alignment in [1, 2, 8, 64, 4096].unwrap():
        request = alloc.layout(17, alignment).unwrap()
        print(request.size(), request.alignment())?
    print(str.repr(alloc.layout(-1)).unwrap())?
    print(str.repr(alloc.layout(1, 0)).unwrap())?
    print(str.repr(alloc.layout(1, -8)).unwrap())?
    print(str.repr(alloc.layout(1, 3)).unwrap())?
    print(alloc.layout(i64.MAX).is_err(), alloc.layout(i64.MAX, 1).is_ok())?
    print(alloc.layout(0, 4611686018427387904).is_ok())?
    print(alloc.layout(9223372036854775800).is_ok(), alloc.layout(9223372036854775801).is_err())?
    print(alloc.layout(1, i64.MAX).is_err(), alloc.layout(1, i64.MIN).is_err())?
    Ok(())
"#,
        "0 8\n17 1\n17 2\n17 8\n17 64\n17 4096\nResult[std.alloc.Layout, std.alloc.LayoutError].Err(std.alloc.LayoutError.Allocation(AllocError.CapacityOverflow))\nResult[std.alloc.Layout, std.alloc.LayoutError].Err(std.alloc.LayoutError.InvalidAlignment)\nResult[std.alloc.Layout, std.alloc.LayoutError].Err(std.alloc.LayoutError.InvalidAlignment)\nResult[std.alloc.Layout, std.alloc.LayoutError].Err(std.alloc.LayoutError.InvalidAlignment)\nTrue True\nTrue\nTrue True\nTrue True\n",
    );
    for body in [
        "drop(alloc.Layout(-1, 3))",
        "a = alloc.layout(1).unwrap()\n    drop(a._size)",
        "mut a = alloc.layout(1).unwrap()\n    a._alignment = 3",
        "drop(alloc.ByteBlock(array[u64, 3].filled(0u64)))",
    ] {
        let error = support::check_error(&format!(
            "import std.alloc as alloc\ndef main() -> ():\n    {body}\n"
        ));
        assert!(error.contains("private"), "{body}: {error}");
    }
}

#[test]
fn byte_blocks_split_move_return_and_keep_disjoint_storage() {
    assert_stdout(
        r#"import std.alloc as alloc

def make() -> alloc.ByteBlock:
    owner = alloc.system()
    mut block = owner.block(&alloc.layout(9, 4096).unwrap()).unwrap()
    print(block.len(), block.alignment() >= 4096, block.get(8)).unwrap()
    block.set(0, 11u8)
    block

def move_block(block: alloc.ByteBlock) -> alloc.ByteBlock:
    block

def main() -> Result[(), IoError]:
    mut left = make()
    mut right = left.split_off(3)
    left.set(2, 22u8)
    right.set(0, 33u8)
    print(left.len(), right.len(), left.get(0), left.get(2), right.get(0), right.alignment())?
    empty_end = right.split_off(right.len())
    mut all = right.split_off(0)
    print(empty_end.is_empty(), right.is_empty(), all.len())?
    drop(left)
    drop(right)
    for i in range(6):
        mut one = all.split_off(0)
        one.set(i, u8(i))
        all = move_block(one)
    print(all.get(0), all.get(5))?
    drop(all)
    print(empty_end.is_empty())?
    owner = alloc.system()
    empty = owner.block(&alloc.layout(0, 64).unwrap()).unwrap()
    print(empty.len(), empty.is_empty(), empty.alignment() >= 64)?
    Ok(())
"#,
        "9 True 0\n3 6 11 22 33 1\nTrue True 6\n0 5\nTrue\n0 True True\n",
    );
}

#[test]
fn byte_blocks_move_to_workers_and_allow_shared_reads() {
    assert_stdout(
        r#"import std.alloc as alloc

def read(block: &alloc.ByteBlock) -> u8:
    block.get(0)

def work(input: alloc.ByteBlock) -> alloc.ByteBlock:
    mut block = input
    block.set(0, 42u8)
    block

def main() -> Result[(), IoError | SpawnError[()]]:
    owner = alloc.system()
    mut first = owner.block(&alloc.layout(2).unwrap()).unwrap()
    second = first.split_off(1)
    drop(owner)
    with spawn(work, second).drop_job()? as task:
        first.set(0, 17u8)
        last = task.join()
        with spawn(read, &last).drop_job()? as reader:
            print(first.get(0), read(&last), reader.join())?
    Ok(())
"#,
        "17 42 42\n",
    );
}

#[test]
fn byte_blocks_reject_duplication_private_storage_and_conflicting_borrows() {
    for (body, expected) in [
        (
            "other = block\n    drop(block.len())\n    drop(other)",
            "moved",
        ),
        ("drop(copy(block).unwrap())", "cannot be copied"),
        ("drop(block._storage)", "private"),
        (
            "match block:\n        case alloc.ByteBlock(storage):\n            drop(storage)",
            "private",
        ),
        (
            "view = &block\n    block.set(0, 1u8)\n    drop(view.len())",
            "borrow",
        ),
        ("drop(block.share())", "share"),
    ] {
        let error = support::check_error(&format!(
            "import std.alloc as alloc\ndef main() -> ():\n    owner = alloc.system()\n    mut block = owner.block(&alloc.layout(4).unwrap()).unwrap()\n    {body}\n"
        ));
        assert!(error.contains(expected), "{body}: {error}");
    }
    for intrinsic in [
        "__allocator_block",
        "__allocator_block_release",
        "__allocator_block_len",
        "__allocator_block_alignment",
        "__allocator_block_get",
        "__allocator_block_set",
        "__allocator_block_split",
    ] {
        let error = support::check_error(&format!("def main() -> ():\n    drop({intrinsic}())\n"));
        assert!(
            error.contains("reserved for the standard library"),
            "{error}"
        );
    }
}

#[test]
fn byte_block_positions_trap_with_operation_and_argument() {
    for (call, message) in [
        ("block.get(-1)", "ByteBlock.get: index"),
        ("block.get(4)", "ByteBlock.get: index"),
        ("block.set(-1, 0u8)", "ByteBlock.set: index"),
        ("block.set(4, 0u8)", "ByteBlock.set: index"),
        ("block.split_off(-1)", "ByteBlock.split_off: at"),
        ("block.split_off(5)", "ByteBlock.split_off: at"),
    ] {
        let source = format!(
            "import std.alloc as alloc\ndef main() -> ():\n    owner = alloc.system()\n    mut block = owner.block(&alloc.layout(4).unwrap()).unwrap()\n    drop({call})\n"
        );
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("program");
        for optimize in [false, true] {
            let options = plenty::CompileOptions {
                optimize,
                ..Default::default()
            };
            plenty::compile_source_to_executable_with_options(&source, &executable, &options)
                .unwrap();
            let output = std::process::Command::new(&executable).output().unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(1), "{call}: {stderr}");
            assert!(stderr.contains(message), "{call}: {stderr}");
        }
    }
}

#[test]
fn byte_block_operations_and_layout_import_preserve_actor_exclusion() {
    for call in [
        "block.len()",
        "block.is_empty()",
        "block.alignment()",
        "block.get(0)",
        "block.set(0, 1u8)",
        "block.split_off(0)",
    ] {
        let error = support::check_error(&format!(
            "import std.alloc as alloc\nimport std.actor as actors\ndef inspect(block: &mut alloc.ByteBlock) -> ():\n    drop({call})\n    drop(actors.heaps_live())\ndef main() -> ():\n    pass\n"
        ));
        assert!(
            error.contains("retained allocator use cannot be combined"),
            "{call}: {error}"
        );
    }
    // The deliberately whole-lowered-program gate also sees unused handle
    // bodies when only the pure Layout factory is called (same module).
    let error = support::check_error(
        r#"import std.alloc as alloc
import std.actor as actors

def main() -> ():
    request = alloc.layout(16).unwrap()
    drop(actors.heaps_live())
    drop(request.size())
"#,
    );
    assert!(
        error.contains("retained allocator use cannot be combined"),
        "{error}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn layout_and_split_need_no_allocation_and_block_propagates_failure() {
    assert_stdout(
        r#"import std.alloc as alloc

def main() -> Result[(), IoError]:
    owner = alloc.system()
    mut first = owner.block(&alloc.layout(16).unwrap()).unwrap()
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    request = alloc.layout(8, 64).unwrap()
    invalid = alloc.layout(-1)
    second = first.split_off(3)
    empty = first.split_off(0)
    drop(second.get(0))
    print("__test_end_no_allocations__")?
    failed = owner.block(&request)
    print("__test_restore_allocations__")?
    drop(invalid)
    print(first.len(), second.len(), empty.len(), failed.is_err())?
    Ok(())
"#,
        "__test_fail_allocations_after_0__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_restore_allocations__\n0 13 3 True\n",
    );
}

#[test]
fn block_results_survive_interleaved_calls_and_container_relocation() {
    assert_stdout(
        r#"import std.alloc as alloc

def make(owner: &alloc.Allocator, value: u8) -> Result[alloc.ByteBlock, AllocError]:
    mut block = owner.block(&alloc.layout(8).unwrap())?
    block.set(0, value)
    Ok(block)

def main() -> Result[(), AllocError | IoError]:
    owner = alloc.system()
    mut blocks = owner.empty_list[alloc.ByteBlock]()?
    for i in range(20):
        first = make(&owner, u8(i))
        rejected = owner.block(&alloc.layout(i64.MAX, 1).unwrap())
        second = make(&owner, u8(i + 1))
        blocks.append(first?)?
        blocks.append(second?)?
        if rejected.is_ok():
            return Err(AllocError.CapacityOverflow)
    drop(owner)
    blocks.reserve(100)?
    mut total = 0
    for block in &blocks:
        unchecked:
            total += i64(block.get(0))
    print(len(blocks), total)?
    Ok(())
"#,
        "40 400\n",
    );
}
