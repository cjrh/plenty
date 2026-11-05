mod support;
use support::{check_source, run};

#[test]
fn named_functions_can_be_borrowed_directly_by_callback_apis() {
    let output = run(r#"
def double(value: i64) -> Result[i64, OverflowError[i64]]:
    Ok(value * 2)
def apply[F: Callable[[i64], Result[i64, OverflowError[i64]]]](f: &F, value: i64) -> Result[i64, OverflowError[i64]]:
    f(value)
def concrete(f: &Callable[[i64], Result[i64, OverflowError[i64]]], value: i64) -> Result[i64, OverflowError[i64]]:
    f(value)
def main() -> Result[(), IoError | OverflowError[i64]]:
    print(apply(&double, 4)?)?
    print(concrete(&(double), 5)?)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "8\n10\n");
}

#[test]
fn named_functions_can_be_borrowed_exclusively() {
    // A named function has no state, so `&mut F` accepts it (issue #619).
    check_source("def double(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 2)\ndef apply[F: Callable[[i64], Result[i64, OverflowError[i64]]]](f: &mut F) -> Result[i64, OverflowError[i64]]:\n    f(4)\ndrop(apply(&mut double))").unwrap();
}

#[test]
fn named_function_reference_bindings_survive_branches_and_repeated_borrows() {
    let output = run(r#"
def double(value: i64) -> Result[i64, OverflowError[i64]]:
    Ok(value * 2)
def choose(flag: bool) -> Result[i64, OverflowError[i64]]:
    if flag:
        first = &double
        print(first(3)?).unwrap()
    saved = &double
    again = &double
    copied = double
    Ok(saved(4)? + again(5)? + copied(6)?)
def main() -> Result[(), IoError | OverflowError[i64]]:
    print(choose(False)?)?
    print(choose(True)?)?
    mut callback = double
    writable = &mut callback
    print(writable(7)?)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "30\n6\n30\n14\n");
}

#[test]
fn specialized_functions_can_be_borrowed_and_local_bindings_shadow_declarations() {
    let output = run(r#"
def identity[T](value: T) -> T:
    value
def double(value: i64) -> Result[i64, OverflowError[i64]]:
    Ok(value * 2)
def apply[T, U, F: Callable[[T], U]](f: &F, value: T) -> U:
    f(value)
def main() -> Result[(), IoError]:
    byte = &identity[u8]
    wide = &identity[i64]
    print(apply(&identity[u8], 7u8))?
    print(byte(8))?
    print(wide(9))?
    double = identity[i64]
    print(apply(&double, 10))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n8\n9\n10\n");
}

#[test]
fn named_function_reference_bindings_do_not_escape_their_local_storage() {
    let error = check_source("def double(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 2)\ndef escape(origin: &Callable[[i64], Result[i64, OverflowError[i64]]]) -> &Callable[[i64], Result[i64, OverflowError[i64]]]:\n    &double").unwrap_err().to_string();
    assert!(error.contains("reference parameter"), "{error}");
}

#[test]
fn imported_functions_and_specializations_can_be_borrowed() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("callbacks.plenty"),
        "pub def double(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 2)\npub def identity[T](x: T) -> T:\n    x\n",
    )
    .unwrap();
    let source = directory.path().join("main.plenty");
    std::fs::write(
        &source,
        r#"
import callbacks
from callbacks import double as twice
def apply[T, U, F: Callable[[T], U]](f: &F, value: T) -> U:
    f(value)
def main() -> Result[(), IoError | OverflowError[i64]]:
    saved = &callbacks.double
    print(apply(&callbacks.double, 4)?)?
    print(apply(&twice, 5)?)?
    print(saved(6)?)?
    print(apply(&callbacks.identity[u8], 7u8))?
    Ok(())
"#,
    )
    .unwrap();
    let executable = directory.path().join("program");
    plenty::compile_file_to_executable(&source, &executable, Some(directory.path())).unwrap();
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "8\n10\n12\n7\n");
}

#[test]
fn repeated_function_borrows_share_one_local_slot() {
    let mut source = String::from(
        "def double(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 2)\ndef apply[F: Callable[[i64], Result[i64, OverflowError[i64]]]](f: &F) -> Result[i64, OverflowError[i64]]:\n    f(4)\ndef main() -> Result[(), OverflowError[i64]]:\n",
    );
    for _ in 0..300 {
        source.push_str("    apply(&double)?\n");
    }
    source.push_str("    Ok(())\n");
    check_source(&source).unwrap();
}

#[test]
fn generator_calls_can_borrow_named_functions_but_references_cannot_cross_yield() {
    let output = run(r#"
def double(x: i64) -> Result[i64, OverflowError[i64]]:
    Ok(x * 2)
def apply[F: Callable[[i64], Result[i64, OverflowError[i64]]]](callback: &F, value: i64) -> Result[i64, OverflowError[i64]]:
    callback(value)
def values() -> Generator[Result[i64, OverflowError[i64]]]:
    yield apply(&double, 3)
    yield apply(&double, 4)
    yield apply(&double, 5)
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    results = [value? for value in values()]?
    print(results)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "[6, 8, 10]\n");
    let error = check_source("def double(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 2)\ndef values() -> Generator[Result[i64, OverflowError[i64]]]:\n    callback = &double\n    yield callback(3)\n    yield callback(4)").unwrap_err().to_string();
    assert!(error.contains("cannot remain live across yield"), "{error}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn borrowing_named_functions_requires_no_allocation() {
    let output = run(r#"
def double(x: i64) -> Result[i64, OverflowError[i64]]:
    Ok(x * 2)
def apply[F: Callable[[i64], Result[i64, OverflowError[i64]]]](f: &F, value: i64) -> Result[i64, OverflowError[i64]]:
    f(value)
def main() -> Result[(), OverflowError[i64]]:
    print("__test_begin_no_allocations__").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    saved = &double
    mut total = 0
    for value in range(3):
        total = total + apply(&double, value)? + saved(value)?
    print("__test_restore_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
    print(total).unwrap()
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n12\n"
    );
}
