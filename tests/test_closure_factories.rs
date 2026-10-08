mod support;
use support::{check_source, run};

#[test]
fn owned_closures_escape_into_caller_storage() {
    let output = run(r#"
def counter(start: i64) -> Closure[[], i64]:
    def [mut start]() -> i64:
        start = start + 1
        start
def apply(f: &mut Closure[[], i64]) -> i64:
    f()
def main() -> Result[(), Failure]:
    mut a = counter(10)
    mut b: Closure[[], i64] = counter(30)
    print(apply(&mut a))?
    print(b())?
    print(a())?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "11\n31\n12\n");
}

#[test]
fn closure_factories_reject_dangling_captures_and_layout_erasure() {
    let error = check_source("def bad() -> Closure[[], i64]:\n    value = 1\n    def [&value]() -> i64:\n        value\ndef main() -> ():\n    pass\n").unwrap_err().to_string();
    assert!(error.contains("cannot escape"), "{error}");
    let error = check_source("def bad(flag: bool) -> Closure[[], i64]:\n    n = 1\n    if flag:\n        return def [n]() -> i64:\n            n\n    return def [n]() -> i64:\n        n + 1\ndef main() -> ():\n    pass\n").unwrap_err().to_string();
    assert!(
        error.contains("expected") || error.contains("mismatch"),
        "{error}"
    );
}
