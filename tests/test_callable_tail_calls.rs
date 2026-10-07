mod support;

#[test]
fn explicit_and_implicit_indirect_tail_recursion_use_bounded_stack() {
    let output = support::run(
        r#"
def count(n: i64, total: i64) -> i64:
    if n == 0:
        return total
    step = count
    if n % 2 == 0:
        return step(n - 1, total + 1)
    step(n - 1, total + 1)
def main() -> Result[(), Failure]:
    print(count(500000, 0))?
    Ok(())
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "500000\n");
}

#[test]
fn indirect_tail_positions_preserve_inline_results_and_cleanup_order() {
    let output = support::run(
        r#"
class Guard:
    def __del__(self) -> ():
        print("drop").unwrap()
def called() -> i64:
    print("call").unwrap()
    42
def guarded() -> i64:
    guard = Guard().unwrap()
    operation = called
    operation()
def interval() -> range[u8]:
    range[u8](3)
def relay() -> range[u8]:
    operation = interval
    operation()
def main() -> Result[(), Failure]:
    print(guarded())?
    for n in relay():
        print(n)?
    Ok(())
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "call\ndrop\n42\n0\n1\n2\n"
    );
}
