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
