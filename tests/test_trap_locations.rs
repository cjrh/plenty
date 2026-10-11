//! Runtime traps report the source position of the failing operation.
use rstest::rstest;
use std::process::Command;

/// Compile `files` with `main.plenty` as the entry and its directory as the
/// source root, and return what the program wrote to standard error.
fn trap(files: &[(&str, &str)]) -> String {
    let dir = tempfile::tempdir().unwrap();
    for (path, source) in files {
        std::fs::write(dir.path().join(path), source).unwrap();
    }
    let source = dir.path().join("main.plenty");
    let program = dir.path().join("program");
    plenty::compile_file_to_executable(&source, &program, Some(dir.path())).unwrap();
    let output = Command::new(&program).output().unwrap();
    assert_eq!(output.status.code(), Some(1), "{:?}", output.status);
    String::from_utf8(output.stderr).unwrap()
}

fn main_trap(body: &str) -> String {
    trap(&[(
        "main.plenty",
        &format!("def main() -> Result[(), Failure]:\n{body}    Ok(())\n"),
    )])
}

#[rstest]
#[case::add("    big: i8 = 127\n    print(big + 1i8)?\n", "3:11: integer overflow")]
#[case::subtract("    low: u8 = 0\n    print(low - 1u8)?\n", "3:11: integer overflow")]
#[case::multiply("    big: i8 = 100\n    print(2i8 * big)?\n", "3:11: integer overflow")]
#[case::negate(
    "    low: i8 = -127\n    lowest = low - 1i8\n    print(-lowest)?\n",
    "4:11: integer overflow"
)]
#[case::divide_overflow(
    "    low: i8 = -127\n    lowest = low - 1i8\n    print(lowest // -1i8)?\n",
    "4:11: integer overflow"
)]
#[case::augmented(
    "    mut total: i8 = 127\n    total += 1\n    print(total)?\n",
    "3:5: integer overflow"
)]
#[case::divide("    zero = 0\n    print(7 // zero)?\n", "3:11: division by zero")]
#[case::modulo("    zero = 0\n    print(7 % zero)?\n", "3:11: division by zero")]
#[case::nested_operand(
    "    zero = 0\n    print(1 + 2 * (7 // zero))?\n",
    "3:20: division by zero"
)]
#[case::shift_left(
    "    count = 64\n    print(1 << count)?\n",
    "3:11: shift count outside integer width"
)]
#[case::shift_right(
    "    count = -1\n    print(8 >> count)?\n",
    "3:11: shift count outside integer width"
)]
fn arithmetic_traps_name_the_expression(#[case] body: &str, #[case] expected: &str) {
    assert_eq!(main_trap(body), format!("error: main.plenty:{expected}\n"));
}

#[rstest]
#[case::scalar_list(
    "    xs = [1, 2, 3]?\n    print(xs[3])?\n",
    "3:11: index out of bounds: index 3, length 3"
)]
#[case::negative_index_as_written(
    "    xs = [1, 2, 3]?\n    print(xs[-4])?\n",
    "3:11: index out of bounds: index -4, length 3"
)]
#[case::text_list(
    "    names = [\"a\", \"b\"]?\n    print(names[2])?\n",
    "3:11: index out of bounds: index 2, length 2"
)]
#[case::assignment(
    "    mut xs = [1, 2, 3]?\n    xs[5] = 9\n",
    "3:5: index out of bounds: index 5, length 3"
)]
#[case::borrow(
    "    xs = [1, 2, 3]?\n    first = &xs[7]\n    print(*first)?\n",
    "3:14: index out of bounds: index 7, length 3"
)]
#[case::swap(
    "    mut xs = [1, 2, 3]?\n    xs.swap(0, 6)\n",
    "3:5: index out of bounds: index 6, length 3"
)]
#[case::text(
    "    word = \"héllo\"\n    print(word[5])?\n",
    "3:11: index out of bounds: index 5, length 5"
)]
#[case::range(
    "    span = range(0, 4)\n    print(span[4])?\n",
    "3:11: index out of bounds: index 4, length 4"
)]
fn bounds_traps_print_the_index_and_length(#[case] body: &str, #[case] expected: &str) {
    assert_eq!(main_trap(body), format!("error: main.plenty:{expected}\n"));
}

#[rstest]
#[case::dictionary_read(
    "    ages = {\"a\": 1}?\n    print(ages[\"b\"])?\n",
    "3:11: dictionary key not found"
)]
#[case::dictionary_assignment(
    "    mut ages = {\"a\": 1}?\n    ages[\"b\"] = 2\n",
    "3:5: dictionary key not found; use insert to add a new key"
)]
#[case::unwrap_nothing(
    "    absent: Option[i64] = Nothing\n    print(absent.unwrap())?\n",
    "3:11: unwrap on Nothing"
)]
#[case::unwrap_err(
    "    failed: Result[i64, str] = Err(\"no\")\n    print(failed.unwrap())?\n",
    "3:11: unwrap on Err"
)]
#[case::range_step(
    "    zero = 0\n    for i in range(0, 4, zero):\n        print(i)?\n",
    "3:14: range step cannot be zero"
)]
#[case::range_length(
    "    print(len(range(-9223372036854775808, 9223372036854775807)))?\n",
    "2:15: range length exceeds i64"
)]
#[case::split_separator(
    "    parts = \"a,b\".split(\"\")?\n    print(len(parts))?\n",
    "2:13: string split requires a nonempty separator"
)]
fn other_traps_name_the_operation(#[case] body: &str, #[case] expected: &str) {
    assert_eq!(main_trap(body), format!("error: main.plenty:{expected}\n"));
}

#[test]
fn output_before_a_trap_is_flushed() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(
        &source,
        "def main() -> Result[(), Failure]:\n    zero = 0\n    print(\"before\")?\n    print(1 // zero)?\n    Ok(())\n",
    )
    .unwrap();
    let program = dir.path().join("program");
    plenty::compile_file_to_executable(&source, &program, None).unwrap();
    let output = Command::new(&program).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"before\n");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: main.plenty:4:11: division by zero\n"
    );
}

/// Generated code is independent of the source directory (see "Reproducible
/// artifacts" in the linking reference), so a report cannot carry an absolute
/// path.
#[test]
fn a_trap_names_its_file_relative_to_the_source_root() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("app/tools")).unwrap();
    std::fs::write(
        dir.path().join("app/tools/numbers.plenty"),
        "pub def half(total: i64, parts: i64) -> i64:\n    total // parts\n",
    )
    .unwrap();
    let source = dir.path().join("app/main.plenty");
    std::fs::write(
        &source,
        "import app.tools.numbers\ndef main() -> Result[(), Failure]:\n    zero = 0\n    print(app.tools.numbers.half(8, zero))?\n    Ok(())\n",
    )
    .unwrap();
    let program = dir.path().join("program");
    plenty::compile_file_to_executable(&source, &program, Some(dir.path())).unwrap();
    let output = Command::new(&program).output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: app/tools/numbers.plenty:2:5: division by zero\n"
    );
}

#[test]
fn a_trap_in_an_imported_function_names_the_defining_file() {
    let error = trap(&[
        (
            "numbers.plenty",
            "pub def half(total: i64, parts: i64) -> i64:\n    total // parts\n",
        ),
        (
            "main.plenty",
            "import numbers\ndef main() -> Result[(), Failure]:\n    zero = 0\n    print(numbers.half(8, zero))?\n    Ok(())\n",
        ),
    ]);
    assert_eq!(error, "error: numbers.plenty:2:5: division by zero\n");
}

#[test]
fn a_trap_in_a_generic_instance_names_the_body_not_the_instantiation() {
    let error = trap(&[
        (
            "picking.plenty",
            "pub def pick[T](items: &list[T], index: i64) -> T:\n    items[index]\n",
        ),
        (
            "main.plenty",
            "from picking import pick\ndef main() -> Result[(), Failure]:\n    xs = [1, 2, 3]?\n    print(pick(&xs, 9))?\n    Ok(())\n",
        ),
    ]);
    assert_eq!(
        error,
        "error: picking.plenty:2:5: index out of bounds: index 9, length 3\n"
    );
}

#[test]
fn a_trap_in_a_generic_method_names_the_method_body() {
    let error = trap(&[
        (
            "cells.plenty",
            "pub class Cell[T]:\n    pub items: list[T]\n    pub def at(self, index: i64) -> T:\n        self.items[index]\n",
        ),
        (
            "main.plenty",
            "from cells import Cell\ndef main() -> Result[(), Failure]:\n    cell = Cell([1u8, 2u8]?)\n    print(cell.at(2))?\n    Ok(())\n",
        ),
    ]);
    assert_eq!(
        error,
        "error: cells.plenty:4:9: index out of bounds: index 2, length 2\n"
    );
}

#[test]
fn a_trap_in_a_generator_body_names_the_defining_file() {
    let error = trap(&[
        (
            "steps.plenty",
            "pub def shares(total: i64, parts: i64) -> Generator[i64]:\n    yield total\n    yield total // parts\n",
        ),
        (
            "main.plenty",
            "from steps import shares\ndef main() -> Result[(), Failure]:\n    zero = 0\n    for share in shares(8, zero):\n        print(share)?\n    Ok(())\n",
        ),
    ]);
    assert_eq!(error, "error: steps.plenty:3:11: division by zero\n");
}

#[test]
fn each_trap_site_keeps_its_own_position() {
    // Two operations on one line share a marker unless each places its own.
    let error = main_trap("    xs = [1, 2, 3]?\n    zero = 0\n    print(xs[0] // zero + xs[9])?\n");
    assert_eq!(error, "error: main.plenty:4:11: division by zero\n");
    let error = main_trap("    xs = [1, 2, 3]?\n    one = 1\n    print(xs[0] // one + xs[9])?\n");
    assert_eq!(
        error,
        "error: main.plenty:4:26: index out of bounds: index 9, length 3\n"
    );
}
