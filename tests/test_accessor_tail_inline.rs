//! A small wrapper's terminal accessor may disappear when the tail callee
//! itself is inlined and the wrapper has no owned cleanup (issue #958).
mod support;
use std::process::{Command, Output};

fn command(source: &str, fast: bool, dump: bool) -> (tempfile::TempDir, Output) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let mut command = Command::new(support::plenty_bin());
    command.current_dir(dir.path()).env("PLENTY_CACHE", "off");
    if fast {
        command.arg("--fast-compile");
    }
    if dump {
        command.args(["--dump", "clif-opt"]);
    }
    command.arg("main.plenty");
    if dump {
        command.args(["-o", "dump"]);
    }
    let output = command.output().unwrap();
    (dir, output)
}

fn section(source: &str, function: &str) -> String {
    let (dir, output) = command(source, false, true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dump = std::fs::read_to_string(dir.path().join("dump/clif-opt.txt")).unwrap();
    let (_, rest) = dump.split_once(&format!("==== {function} ")).unwrap();
    rest.split("\n==== ").next().unwrap().to_string()
}

fn equivalent(source: &str, expected: &str) {
    for fast in [true, false] {
        let (_, output) = command(source, fast, false);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }
}

#[test]
fn terminal_bounded_and_user_accessors_disappear_from_the_loop() {
    let source = include_str!("../experiments/kernels/accessor-tail/kernels.plenty");
    equivalent(source, "[52439, 102040, 89085, 111062] 15\n");
    for function in ["fill", "sum_accessor"] {
        let code = section(source, function);
        assert!(!code.contains("call "), "{code}");
        assert!(!code.contains("return_call"), "{code}");
    }
}

#[test]
fn nested_terminal_calls_preserve_fallible_owned_results() {
    let source = r#"
def allocate(n: i64) -> Result[str, AllocError]:
    if n < 0:
        return Err(AllocError.CapacityOverflow)
    text = "item".concat("-value")?
    Ok(text)
def forward(n: i64) -> Result[str, AllocError]:
    allocate(n)
def forward_again(n: i64) -> Result[str, AllocError]:
    forward(n)
def main() -> Result[(), AllocError | IoError]:
    print(forward_again(1)?, forward_again(-1).is_err())?
    Ok(())
"#;
    equivalent(source, "item-value True\n");
    let main = section(source, "main");
    assert!(!main.contains("__plenty_fn_forward"), "{main}");
    assert!(!main.contains("__plenty_fn_allocate"), "{main}");
}

#[test]
fn cleanup_recursive_and_indirect_tail_boundaries_remain_calls() {
    let source = r#"
class Guard:
    def __del__(self) -> ():
        print("drop").unwrap()
def target() -> i64:
    print("callee").unwrap()
    7
def guarded() -> i64:
    guard = Guard()
    target()
def count(n: i64) -> i64:
    if n == 0:
        return 0
    unchecked:
        count(n - 1)
def recursive(n: i64) -> i64:
    count(n)
def indirect(operation: Callable[[], i64]) -> i64:
    operation()
def main() -> Result[(), IoError]:
    print(guarded(), recursive(100000), indirect(target))?
    Ok(())
"#;
    equivalent(source, "drop\ncallee\ncallee\n7 0 7\n");
    let main = section(source, "main");
    for retained in ["guarded", "recursive", "indirect"] {
        assert!(main.contains(&format!("__plenty_fn_{retained}")), "{main}");
    }
}

#[test]
fn substituted_terminal_traps_keep_their_source_position() {
    let source = "def divide(n: i64) -> i64:\n    unchecked:\n        12 // n\ndef forward(n: i64) -> i64:\n    divide(n)\ndef main() -> ():\n    print(forward(0)).unwrap()\n";
    let mut previous = None;
    for fast in [true, false] {
        let (_, output) = command(source, fast, false);
        assert_eq!(output.status.code(), Some(1));
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.contains("main.plenty:3:9: division by zero"),
            "{error}"
        );
        if let Some(previous) = &previous {
            assert_eq!(&error, previous);
        }
        previous = Some(error);
    }
}
