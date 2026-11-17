//! B13 executable experiments, not qualification of a general allocator API.
mod support;

fn execute(executable: &std::path::Path) -> std::process::Output {
    use std::process::Command;
    let mut command = if std::env::var_os("PLENTY_TEST_VALGRIND").is_some() {
        let mut command = Command::new("valgrind");
        command
            .args([
                "--quiet",
                "--error-exitcode=99",
                "--leak-check=full",
                "--errors-for-leak-kinds=definite,indirect,possible",
            ])
            .arg(executable);
        command
    } else {
        Command::new(executable)
    };
    support::assert_success(command.output().unwrap())
}

fn run_file_stdout(source: &str) -> String {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("block_provider.plenty"),
        include_str!("../examples/allocators/block_provider.plenty"),
    )
    .unwrap();
    let path = support::write_main(root.path(), source);
    let executable = root.path().join("program");
    let mut previous = None;
    for optimize in [false, true] {
        let options = plenty::CompileOptions {
            optimize,
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&path, &executable, None, &options)
            .unwrap();
        let output = String::from_utf8(execute(&executable).stdout)
            .unwrap()
            .trim_end()
            .to_owned();
        if let Some(expected) = &previous {
            assert_eq!(&output, expected);
        }
        previous = Some(output);
    }
    previous.unwrap()
}

#[test]
fn short_programs_use_system_arena_and_both() {
    for (source, expected) in [
        (
            include_str!("../examples/allocators/system.plenty"),
            "6 99 1\n0",
        ),
        (
            include_str!("../examples/allocators/arena.plenty"),
            "5 96 1\nTrue 8 7\n0 96\n0",
        ),
        (
            include_str!("../examples/allocators/mixed.plenty"),
            "10 20 20\n4 1\n30 40",
        ),
    ] {
        assert_eq!(run_file_stdout(source), expected);
    }
}

#[test]
fn generic_owned_values_move_and_drop_exactly_once() {
    assert_eq!(
        run_file_stdout(include_str!("../examples/allocators/owned_values.plenty")),
        "release\n6 8\n5 5\ndrop 5 5\ndrop 4 4\ndrop 3 3\ndrop 2 2\ndrop 1 1\ndrop 0 0\nrelease\nprovider dropped\n0"
    );
}

#[test]
fn generic_storage_relocates_nested_inline_payloads() {
    assert_eq!(
        run_file_stdout(
            r#"import std.experimental.alloc as memory

class Record:
    name: str
    rows: array[i64, 32]
    values: list[i64]

enum Entry:
    Empty
    Full(Record, str)

def round_trip[T](owner: &memory.Allocator, value: T) -> T:
    mut values = memory.owned_list[T, memory.Allocator](owner.share())
    values.append(value).unwrap()
    values.reserve(9).unwrap()
    result = values.pop().unwrap()
    drop(values)
    result

def main() -> Result[(), AllocError | IoError]:
    owner = memory.system()?
    # Numeric, unit, sum tags, and nested inline records use the same slot ABI.
    print(round_trip(&owner, True), round_trip(&owner, 255u8), round_trip(&owner, 1.5))?
    drop(round_trip(&owner, ()))
    mut values = memory.owned_list[Entry, memory.Allocator](owner.share())
    for id in range(6):
        label = ("item" + "!")?
        record = Record(label, array[i64, 32].filled(id), [id, 99]?)
        values.append(Entry.Full(record, ("tail" + "!")?)).unwrap()
    values.append(Entry.Empty).unwrap()
    print(values.len(), values.capacity())?
    match values.pop().unwrap():
        case Entry.Empty:
            print("empty")?
        case Entry.Full(_, _):
            print("wrong")?
    entry = values.pop().unwrap()
    drop(values)
    # Read only after the source buffer has been released.
    match entry:
        case Entry.Full(record, tail):
            print(record.name, record.rows[31], record.values[0], tail)?
        case Entry.Empty:
            print("wrong")?
    pair = round_trip(&owner, (("a" + "b")?, [8, 9]?))
    print(pair[0], pair[1][1], owner.live_blocks())?
    Ok(())
"#,
        ),
        "True 255 1.5\n7 8\nempty\nitem! 5 5 tail!\nab 9 0"
    );
}

#[test]
fn generic_failed_growth_returns_input_and_preserves_existing_owners() {
    assert_eq!(
        run_file_stdout(
            r#"import std.experimental.alloc as memory

class Item:
    id: i64
    def __del__(self: &mut Item) -> ():
        print("drop", self.id).unwrap()

pub class Once:
    owner: memory.Allocator
    used: bool
    pub def allocate(self: &mut Once, size: i64) -> Result[memory.ByteBlock, AllocError]:
        if self.used:
            return Err(AllocError.OutOfMemory)
        block = self.owner.block(size)?
        self.used = True
        Ok(block)
    pub def release(self: &mut Once, block: memory.ByteBlock) -> ():
        drop(block)

def partial(owner: &memory.Allocator) -> Result[(), memory.AppendFailure[Item]]:
    mut values = memory.owned_list[Item, Once](Once(owner.share(), False))
    for id in range(5):
        values.append(Item(id))?
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    system = memory.system()?
    mut values = memory.owned_list[Item, Once](Once(system.share(), False))
    for id in range(4):
        values.append(Item(id)).unwrap()
    match values.append(Item(4)):
        case Err(failure):
            print("rejected", failure.value.id, values.len(), values.capacity())?
            mut retry = memory.owned_list[Item, memory.Allocator](system.share())
            retry.append(failure.value).unwrap()
        case Ok(_):
            print("wrong")?
    last = values.pop().unwrap()
    print("last", last.id)?
    drop(last)
    drop(values)
    print("partial")?
    match partial(&system):
        case Err(failure):
            print("returned", failure.value.id)?
        case Ok(_):
            print("wrong")?
    print(system.live_blocks())?
    Ok(())
"#,
        ),
        "rejected 4 4 4\ndrop 4\nlast 3\ndrop 3\ndrop 2\ndrop 1\ndrop 0\npartial\ndrop 3\ndrop 2\ndrop 1\ndrop 0\nreturned 4\ndrop 4\n0"
    );
}

#[test]
fn provider_reclaims_only_emptied_storage_and_can_reuse_it() {
    assert_eq!(
        run_file_stdout(
            r#"import std.experimental.alloc as memory

pub class Pool:
    owner: memory.Allocator
    cached: Option[memory.ByteBlock]
    fresh: i64
    released: i64
    clean: bool
    pub def allocate(self: &mut Pool, size: i64) -> Result[memory.ByteBlock, AllocError]:
        if size > 512:
            return Err(AllocError.OutOfMemory)
        match replace(self.cached, Nothing):
            case Some(block):
                Ok(block)
            case Nothing:
                block = self.owner.block(512)?
                unchecked:
                    self.fresh += 1
                Ok(block)
    pub def release(self: &mut Pool, block: memory.ByteBlock) -> ():
        for index in range(block.len()):
            self.clean = self.clean and block.get(index) == 0u8
        unchecked:
            self.released += 1
        self.cached = Some(block)
    def __del__(self: &mut Pool) -> ():
        print(self.fresh, self.released, self.clean).unwrap()

def main() -> Result[(), AllocError | IoError]:
    system = memory.system()?
    mut values = memory.owned_list[i64, Pool](Pool(system.share(), Nothing, 0, 0, True))
    values.append(123).unwrap()
    values.reserve(8)?
    values.reserve(9)?
    print(values.pop().unwrap())?
    values.append(456).unwrap()
    drop(values)
    print(system.live_blocks())?
    Ok(())
"#,
        ),
        "123\n2 3 True\n0"
    );
}

#[test]
fn generic_storage_validates_provider_extent_and_alignment() {
    for body in [
        "self.owner.block(0)",
        "mut block = self.owner.block(size + 1)?\n            Ok(block.split_off(1))",
    ] {
        support::assert_file_traps(
            &format!(
                "import std.experimental.alloc as memory\npub class Bad:\n    owner: memory.Allocator\n    pub def allocate(self: &mut Bad, size: i64) -> Result[memory.ByteBlock, AllocError]:\n        unchecked:\n            {body}\n    pub def release(self: &mut Bad, block: memory.ByteBlock) -> ():\n        drop(block)\ndef main() -> ():\n    mut values = memory.owned_list[i64, Bad](Bad(memory.system().unwrap()))\n    values.append(42).unwrap()\n"
            ),
            "OwnedList.reserve: provider returned insufficient extent or alignment",
        );
    }
}

#[test]
fn typed_slot_intrinsics_are_private() {
    for call in [
        "__allocator_slot_size[i64]()",
        "__allocator_slot_take[i64](0, 0u64)",
        "__allocator_slot_put[i64](0, 0u64, 1)",
    ] {
        let error = support::check_error(&format!("def main() -> ():\n    drop({call})\n"));
        assert!(
            error.contains("reserved for the standard library"),
            "{error}"
        );
    }
}

#[test]
fn generic_storage_does_not_hide_restricted_ownership() {
    let error = support::check_error(
        "import std.experimental.alloc as memory\ndef main() -> ():\n    values = memory.owned_list[Sender[i64], memory.Allocator](memory.system().unwrap())\n    drop(values)\n",
    );
    assert!(error.contains("allocator slots cannot store"), "{error}");
    let error = support::check_error(
        "import std.experimental.alloc as memory\ndef main() -> ():\n    values = memory.owned_list[i64, memory.Allocator](memory.system().unwrap())\n    drop(copy(values))\n",
    );
    assert!(error.contains("cannot be copied"), "{error}");
    let error = support::check_error(
        r#"import std.experimental.alloc as memory

def worker(values: memory.OwnedList[i64, memory.Allocator]) -> i64:
    values.len()

def main() -> ():
    values = memory.owned_list[i64, memory.Allocator](memory.system().unwrap())
    with spawn(worker, values).drop_job().unwrap() as task:
        drop(task.join())
"#,
    );
    assert!(error.contains("Pointer has no thread contract"), "{error}");
}

#[test]
fn external_safe_block_provider() {
    assert_eq!(
        run_file_stdout(include_str!("../examples/allocators/owned_blocks.plenty")),
        "3 8 8 0\nTrue 0\n4\n7 0 99 42\n1 2 1 7 8"
    );
}

#[test]
fn byte_block_splits_are_disjoint_and_keep_storage_alive() {
    assert_eq!(
        run_file_stdout(
            r#"import std.experimental.alloc as memory

def main() -> Result[(), AllocError | IoError]:
    system = memory.system()?
    mut whole = system.block(16)?
    print(whole.len(), whole.alignment(), whole.get(15))?
    mut right = whole.split_off(8)
    mut middle = whole.split_off(4)
    mut small = middle.split_off(2)
    mut byte = small.split_off(1)
    print(whole.alignment(), middle.alignment(), small.alignment(), byte.alignment(), right.alignment())?
    whole.set(3, 1u8)
    middle.set(0, 2u8)
    small.set(0, 3u8)
    byte.set(0, 4u8)
    right.set(0, 5u8)
    print(whole.get(3), middle.get(0), small.get(0), byte.get(0), right.get(0))?
    empty = right.split_off(right.len())
    print(empty.len(), empty.alignment(), system.live_blocks())?
    drop(whole)
    drop(middle)
    drop(small)
    drop(byte)
    drop(right)
    # Even an empty range retains its backing until it too is dropped.
    print(system.live_blocks())?
    drop(empty)
    print(system.live_blocks())?
    mut zero = system.block(0)?
    drop(zero.split_off(0))
    print(zero.is_empty(), system.block(-1).is_err(), system.block(i64.MAX).is_err())?
    drop(zero)
    print(system.live_blocks())?
    mut clean = True
    for size in range(33):
        mut block = system.block(size)?
        clean = clean and block.len() == size and block.alignment() == 8
        for index in range(size):
            clean = clean and block.get(index) == 0u8
        mut all = block.split_off(0)
        if size > 0:
            all.set(size - 1, 255u8)
            clean = clean and all.get(size - 1) == 255u8
    print(clean, system.live_blocks())?
    parent = memory.arena(&system, 128)?
    mut nested = parent.block(9)?
    print(parent.used_bytes())?
    drop(parent)
    nested.set(8, 77u8)
    print(nested.get(8), system.live_blocks())?
    drop(nested)
    print(system.live_blocks())?
    Ok(())
"#,
        ),
        "16 8 0\n8 4 2 1 8\n1 2 3 4 5\n0 8 2\n2\n0\nTrue True True\n0\nTrue 0\n80\n77 2\n0"
    );
}

#[test]
fn external_block_provider_failures_preserve_budget_and_existing_values() {
    assert_eq!(
        run_file_stdout(
            r#"import std.experimental.alloc as memory
import block_provider

def main() -> Result[(), AllocError | IoError]:
    system = memory.system()?
    mut provider = block_provider.bump(&system, 17)?
    mut first = provider.allocate(3)?
    first.set(2, 9u8)
    print(provider.allocate(9).is_err(), provider.allocate(-1).is_err(), provider.allocate(i64.MAX).is_err())?
    print(provider.remaining_bytes(), first.get(2))?
    empty = provider.allocate(0)?
    print(empty.len(), provider.remaining_bytes())?
    second = provider.allocate(8)?
    print(second.alignment(), second.get(7), provider.remaining_bytes())?
    drop(first)
    drop(second)
    drop(empty)
    print(provider.allocate(1).is_err())?
    drop(provider)
    print(system.live_blocks())?
    # Backing allocation fails after the block's metadata fits in the parent.
    parent = memory.arena(&system, 64)?
    print(parent.block(1).is_err(), parent.live_blocks(), parent.used_bytes())?
    drop(parent)
    print(system.live_blocks())?
    Ok(())
"#,
        ),
        "True True True\n8 9\n0 8\n8 0 0\nTrue\n0\nTrue 0 64\n0"
    );
}

#[test]
fn byte_block_access_checks_ranges() {
    for (operation, message) in [
        ("drop(block.get(-1))", "ByteBlock.get: index"),
        ("drop(block.get(8))", "ByteBlock.get: index"),
        ("block.set(-1, 0u8)", "ByteBlock.set: index"),
        ("block.set(8, 0u8)", "ByteBlock.set: index"),
        ("drop(block.split_off(-1))", "ByteBlock.split_off: at"),
        ("drop(block.split_off(9))", "ByteBlock.split_off: at"),
    ] {
        support::assert_file_traps(
            &format!(
                "import std.experimental.alloc as memory\ndef main() -> ():\n    system = memory.system().unwrap()\n    mut block = system.block(8).unwrap()\n    {operation}\n"
            ),
            message,
        );
    }
    // Protocol conformance cannot justify trusting the provider's requested size.
    // Even a dishonest ordinary implementation cannot bypass the byte checks.
    support::assert_file_traps(
        r#"import std.experimental.alloc as memory

class Short:
    owner: memory.Allocator
    def allocate(self: &mut Short, size: i64) -> Result[memory.ByteBlock, AllocError]:
        self.owner.block(0)
    def release(self: &mut Short, block: memory.ByteBlock) -> ():
        drop(block)

def use[P: memory.BlockProvider](provider: &mut P) -> ():
    mut block = provider.allocate(8).unwrap()
    block.set(0, 1u8)

def main() -> ():
    mut provider = Short(memory.system().unwrap())
    use(&mut provider)
"#,
        "ByteBlock.set: index",
    );
}

#[test]
fn byte_blocks_remain_thread_confined() {
    let error = support::check_error(
        r#"import std.experimental.alloc as memory

def worker(block: memory.ByteBlock) -> i64:
    block.len()

def main() -> ():
    system = memory.system().unwrap()
    block = system.block(8).unwrap()
    with spawn(worker, block).drop_job().unwrap() as task:
        drop(task.join())
"#,
    );
    assert!(error.contains("Pointer has no thread contract"), "{error}");
}

#[test]
fn byte_block_cannot_be_forged_copied_or_have_raw_storage_extracted() {
    for operation in [
        "drop(block._owner)",
        "drop(memory.ByteBlock(system.share(), 0, 100))",
    ] {
        let error = support::check_error(&format!(
            "import std.experimental.alloc as memory\ndef main() -> ():\n    system = memory.system().unwrap()\n    block = system.block(8).unwrap()\n    {operation}\n"
        ));
        assert!(error.contains("private"), "{error}");
    }
    let error = support::check_error(
        "import std.experimental.alloc as memory\ndef main() -> ():\n    system = memory.system().unwrap()\n    block = system.block(8).unwrap()\n    drop(copy(block))\n",
    );
    assert!(error.contains("cannot be copied"), "{error}");
}

#[test]
fn independent_arenas_failed_collection_and_failed_growth() {
    let output = run_file_stdout(
        r#"import std.experimental.alloc as memory

def main() -> Result[(), AllocError | IoError]:
    system = memory.system()?
    first = memory.arena(&system, 32)?
    second = memory.arena(&system, 96)?
    # The fifth insertion fails; the partial result must release its block.
    print(first.list(a for a in range(5)).is_err())?
    print(first.live_blocks(), first.used_bytes(), second.used_bytes())?
    mut values = second.list(a for a in range(4))?
    print(values.reserve(100).is_err(), values.len(), values.capacity())?
    print(values.get(0).unwrap(), values.get(3).unwrap(), second.used_bytes())?
    values.append(4)?
    print(values.len(), second.used_bytes(), second.live_blocks())?
    # Exhausting one arena never falls back to the system or another arena.
    print(first.empty_list().copy_into(&system)?.is_empty())?
    print(system.live_blocks(), first.allocation_count(), second.allocation_count())?
    drop(values)
    drop(first)
    drop(second)
    print(system.live_blocks())?
    Ok(())
"#,
    );
    assert_eq!(
        output,
        "True\n0 32 0\nTrue 4 4\n0 3 32\n5 96 1\nTrue\n4 1 2\n0"
    );
}

#[test]
fn nested_arena_retains_its_parent_and_escaped_list_keeps_both_alive() {
    let output = run_file_stdout(
        r#"import std.experimental.alloc as memory

def escaped(system: &memory.Allocator) -> Result[memory.IntList, AllocError]:
    outer = memory.arena(system, 256)?
    inner = memory.arena(&outer, 96)?
    values = inner.list(a for a in range(4))?
    print(outer.live_blocks(), outer.used_bytes()).unwrap()
    Ok(values)

def main() -> Result[(), AllocError | IoError]:
    system = memory.system()?
    mut values = escaped(&system)?
    values.append(8)?
    print(values.get(4).unwrap(), system.live_blocks())?
    drop(values)
    print(system.live_blocks())?
    Ok(())
"#,
    );
    assert_eq!(output, "2 160\n8 2\n0");
}

#[test]
fn zero_capacity_invalid_sizes_and_failed_arena_construction() {
    let output = run_file_stdout(
        r#"import std.experimental.alloc as memory

def main() -> Result[(), AllocError | IoError]:
    system = memory.system()?
    zero = memory.arena(&system, 0)?
    mut values = zero.empty_list()
    print(values.append(1).is_err(), values.reserve(-1).is_err(), values.reserve(i64.MAX).is_err())?
    print(values.get(-1).is_none(), values.get(0).is_none(), zero.used_bytes())?
    small = memory.arena(&system, 64)?
    # Metadata fits but backing does not. The metadata block is released;
    # bump space is deliberately not reclaimed.
    print(memory.arena(&small, 8).is_err(), small.live_blocks(), small.used_bytes())?
    tiny = memory.arena(&system, 7)?
    print(tiny.empty_list().append(1).is_err())?
    drop(values)
    drop(zero)
    drop(small)
    drop(tiny)
    print(system.live_blocks())?
    Ok(())
"#,
    );
    assert_eq!(output, "True True True\nTrue True 0\nTrue 0 64\nTrue\n0");
}

#[test]
fn copying_owners_or_accessing_raw_storage_is_rejected() {
    for source in [
        "import std.experimental.alloc as memory\ndef main() -> ():\n    a = memory.system().unwrap()\n    drop(copy(a))\n",
        "import std.experimental.alloc as memory\ndef main() -> ():\n    a = memory.system().unwrap()\n    xs = a.empty_list()\n    drop(copy(xs))\n",
    ] {
        let error = support::check_error(source);
        assert!(error.contains("cannot be copied"), "{error}");
    }
    let error = support::check_error(
        "import std.experimental.alloc as memory\ndef main() -> ():\n    drop(memory.malloc(8u64))\n",
    );
    assert!(error.contains("private"), "{error}");
}

// GNU-compatible linker wrapping is used only by this Linux test fixture.
#[cfg(target_os = "linux")]
#[test]
fn real_malloc_failure_preserves_owners_and_releases_partial_results() {
    use std::process::Command;
    let root = tempfile::tempdir().unwrap();
    let c = root.path().join("failures.c");
    std::fs::write(&c, include_str!("fixtures/allocator_failures.c")).unwrap();
    let object = root.path().join("failures.o");
    support::assert_success(
        Command::new("cc")
            .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-c"])
            .arg(c)
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap(),
    );
    std::fs::write(
        root.path().join("fault.plentyi"),
        "pub extern def after(successes: i64) -> () = \"allocator_fail_after\"\n",
    )
    .unwrap();
    let source = root.path().join("main.plenty");
    std::fs::write(
        &source,
        r#"import std.experimental.alloc as memory
import fault

def main() -> Result[(), AllocError | IoError]:
    fault.after(0)
    unavailable = memory.system()
    fault.after(-1)
    print(unavailable.is_err())?
    system = memory.system()?
    for successful_allocations in range(2):
        fault.after(successful_allocations)
        result = memory.arena(&system, 96)
        fault.after(-1)
        print(result.is_err(), system.live_blocks())?
    for successful_allocations in range(2):
        fault.after(successful_allocations)
        result = system.block(9)
        fault.after(-1)
        print(result.is_err(), system.live_blocks())?
    fault.after(1)
    partial = system.list(a for a in range(5))
    fault.after(-1)
    print(partial.is_err(), system.live_blocks())?
    mut values = system.list(a for a in range(4))?
    fault.after(0)
    failed = values.append(9)
    fault.after(-1)
    print(failed.is_err(), values.len(), values.capacity(), values.get(3).unwrap())?
    # The retained source still works after the failed growth.
    values.append(9)?
    drop(values)
    print(system.live_blocks())?
    mut generic = memory.owned_list[str, memory.Allocator](system.share())
    generic.append(("kept" + "!")?).unwrap()
    for successful_allocations in range(2):
        fault.after(successful_allocations)
        growth_failed = generic.reserve(9)
        fault.after(-1)
        print(growth_failed.is_err(), generic.len(), generic.capacity(), system.live_blocks())?
    # Existing capacity needs no allocation in the typed move operations.
    text = ("second" + "!")?
    fault.after(0)
    generic.append(text).unwrap()
    moved = generic.pop().unwrap()
    fault.after(-1)
    print(moved, generic.pop().unwrap())?
    drop(generic)
    print(system.live_blocks())?
    Ok(())
"#,
    )
    .unwrap();
    for optimize in [false, true] {
        let executable = root
            .path()
            .join(if optimize { "optimized" } else { "fast" });
        let options = plenty::CompileOptions {
            optimize,
            link_args: vec![object.clone().into_os_string(), "-Wl,--wrap=malloc".into()],
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&source, &executable, None, &options)
            .unwrap();
        let output = execute(&executable);
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "True\nTrue 0\nTrue 0\nTrue 0\nTrue 0\nTrue 0\nTrue 4 4 3\n0\nTrue 1 4 2\nTrue 1 4 2\nsecond! kept!\n0\n"
        );
    }
}

#[test]
fn allocators_cannot_cross_worker_threads() {
    let error = support::check_error(
        r#"import std.experimental.alloc as memory

def worker(a: memory.Allocator) -> i64:
    a.live_blocks()

def main() -> ():
    a = memory.system().unwrap()
    with spawn(worker, a).drop_job().unwrap() as task:
        drop(task.join())
"#,
    );
    assert!(error.contains("Pointer has no thread contract"), "{error}");
}
