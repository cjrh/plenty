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
            "True\nTrue 0\nTrue 0\nTrue 0\nTrue 4 4 3\n0\n"
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
