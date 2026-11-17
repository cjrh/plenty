//! First language bindings to retained native allocator storage.
mod support;

fn assert_stdout(source: &str, expected: &str) {
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
