mod support;
use support::{check_source, run};

#[test]
fn recursive_captured_frames_are_rejected_without_panicking() {
    let source = "def recursive() -> Generator[i64]:\n    source = recursive()\n    f = def once [mut source]() -> i64:\n        next(source).unwrap()\n    yield f()\ndef main() -> ():\n    pass\n";
    let error = check_source(source).unwrap_err().to_string();
    assert!(error.contains("recursive inline generator"), "{error}");
}

#[test]
fn nested_one_shot_owners_relocate_and_transfer_their_captures() {
    let output = run(r#"
def main() -> Result[(), Failure]:
    values = [8, 9]?
    inner = def once [values]() -> list[i64]:
        values
    outer = def once [inner]() -> list[i64]:
        inner()
    moved = Some(outer)
    print(moved.unwrap()())?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "[8, 9]\n");
}

#[test]
fn one_shot_callbacks_can_own_and_resume_generator_frames() {
    let output = run(r#"
class Resource:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
def numbers(resource: Resource) -> Generator[i64]:
    yield resource.id
    yield resource.id + 1
def main() -> Result[(), Failure]:
    mut source = numbers(Resource(4))
    print(next(source).unwrap())?
    take = def once [mut source]() -> i64:
        next(source).unwrap()
    moved = Some(take)
    print(moved)?
    result = moved.unwrap()()
    print(result)?
    source = numbers(Resource(6))
    unused = def once [mut source]() -> i64:
        next(source).unwrap()
    drop(unused)
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("4\n"), "{stdout}");
    assert!(stdout.contains("<closure>"), "{stdout}");
    assert!(stdout.ends_with("4\n5\n6\n"), "{stdout}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn nested_and_suspended_one_shot_lifecycles_do_not_allocate() {
    let output = run(r#"
def numbers() -> Generator[i64]:
    yield 3
    yield 7
def deferred() -> Generator[i64]:
    source = numbers()
    inner = def once [mut source]() -> i64:
        next(source).unwrap()
    outer = def once [inner]() -> i64:
        inner()
    yield 1
    yield outer()
def main() -> ():
    print("__test_begin_no_allocations__").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    mut frame = deferred()
    first = next(frame).unwrap()
    mut moved = frame
    second = next(moved).unwrap()
    drop(moved)
    print("__test_restore_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
    print(first + second).unwrap()
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n4\n");
}
