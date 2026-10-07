//! Generator construction and ownership through standard sums.
mod support;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        text.lines()
            .filter(|s| !s.starts_with("__test_"))
            .collect::<Vec<_>>()
            .join("\n"),
        expected
    );
}

#[test]
fn results_transfer_generators_through_propagation() {
    native(
        r#"
def numbers() -> Generator[i64]:
    yield 4
    yield 8
def build() -> Result[Generator[i64], AllocError]:
    Ok(numbers())
def consume() -> Result[i64, AllocError]:
    mut source = build()?
    mut total = 0
    for n in source:
        total = total + n
    Ok(total)
print(consume())
"#,
        "Result[i64, AllocError].Ok(12)",
    );
}

#[test]
fn dropping_wrapped_generator_releases_capture_without_resuming() {
    native(
        r#"
class Resource:
    n: i64
    def __del__(self: &mut Resource) -> ():
        print(self.n)
def numbers(resource: Resource) -> Generator[i64]:
    print("resumed")
    yield resource.n
drop(Some(numbers(Resource(7))))
"#,
        "7",
    );
}

#[test]
fn checked_generator_constructor_defers_execution_and_propagates() {
    native(
        r#"
def numbers(n: i64) -> Generator[i64]:
    print("resumed")
    yield n
def consume() -> Result[i64, AllocError]:
    mut source = numbers.try_new(12)?
    print("created")
    match next(source):
        case Some(n):
            return Ok(n)
        case Nothing:
            return Ok(0)
print(consume())
"#,
        "created\nresumed\nResult[i64, AllocError].Ok(12)",
    );
}

#[test]
fn generator_entry_points_share_native_resume_code() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("generator");
    let source = temp.path().join("program.plenty");
    std::fs::write(
        &source,
        r#"
def numbers() -> Generator[i64]:
    yield 5
def main() -> ():
    print(list(numbers()))
    drop(numbers.try_new())
"#,
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg("--compile")
        .arg(source)
        .arg("-o")
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = std::fs::read(path).unwrap();
    let ordinary = b"__plenty_resume_numbers\0";
    let duplicate = b"__plenty_resume___plenty_try_generator_";
    assert_eq!(
        binary
            .windows(ordinary.len())
            .filter(|b| *b == ordinary)
            .count(),
        1
    );
    assert!(!binary.windows(duplicate.len()).any(|b| b == duplicate));
}

#[cfg(feature = "runtime-checks")]
#[test]
fn checked_frame_failure_releases_moved_captures() {
    native(
        r#"
def numbers(values: list[i64]) -> Generator[i64]:
    print("unexpected resume")
    for n in values:
        yield n
values = [1, 2]
print("__test_fail_allocations_after_0__")
result = numbers.try_new(values)
print("__test_restore_allocations__")
match result:
    case Ok(source):
        print("unexpected success")
    case Err(error):
        print(error)
drop(numbers.try_new([3]))
"#,
        "AllocError.OutOfMemory",
    );
}
