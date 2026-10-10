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
print(consume()).unwrap()
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
        print(self.n).unwrap()
def numbers(resource: Resource) -> Generator[i64]:
    print("resumed").unwrap()
    yield resource.n
drop(Some(numbers(Resource(7))))
"#,
        "7",
    );
}

#[test]
fn generator_constructor_defers_execution() {
    native(
        r#"
def numbers(n: i64) -> Generator[i64]:
    print("resumed").unwrap()
    yield n
def consume() -> Result[i64, AllocError]:
    mut source = numbers.new(12)
    print("created").unwrap()
    match next(source):
        case Some(n):
            return Ok(n)
        case Nothing:
            return Ok(0)
print(consume()).unwrap()
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
    print(list(numbers()).unwrap()).unwrap()
    drop(numbers.new())
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
fn inline_frame_creation_and_drop_release_captures_with_allocation_disabled() {
    native(
        r#"
def numbers(values: list[i64]) -> Generator[i64]:
    print("unexpected resume").unwrap()
    for n in values:
        yield n
values = [1, 2].unwrap()
print("__test_fail_allocations_after_0__").unwrap()
source = numbers(values)
drop(source)
print("__test_restore_allocations__").unwrap()
print("finished").unwrap()
drop(numbers.new([3].unwrap()))
"#,
        "finished",
    );
}
