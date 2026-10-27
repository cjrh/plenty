//! Runtime traps report the source position of the failing operation.
use rstest::rstest;
use std::process::Command;

/// Compile `files` with `main.plenty` as the entry and its directory as the
/// source root, and return what the program wrote to standard error.
fn trap(files: &[(&str, &str)]) -> String {
    let dir = tempfile::tempdir().unwrap();
    for (path, source) in files {
        std::fs::create_dir_all(dir.path().join(path).parent().unwrap()).unwrap();
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
        &format!("def main() -> Result[(), AllocError | IoError]:\n{body}    Ok(())\n"),
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
#[case::cast(
    "    wide = 300\n    print(u8(wide))?\n",
    "3:11: integer cast out of range"
)]
#[case::float_cast(
    "    wide = 1e30\n    total = 1 + i64(wide)\n    print(total)?\n",
    "3:17: integer cast out of range"
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

// Scalar rows reached through fields use narrow helpers (issue #122); the
// reported site and message match the generic element path.
#[rstest]
#[case::field_read(
    "    print(h.xs[3])?\n",
    "5:11: index out of bounds: index 3, length 2"
)]
#[case::field_assignment("    h.xs[-3] = 1\n", "5:5: index out of bounds: index -3, length 2")]
#[case::field_augmented("    h.xs[2] += 1\n", "5:5: index out of bounds: index 2, length 2")]
#[case::nested_assignment(
    "    mut grid = [[1]?]?\n    grid[0][1] = 2\n",
    "6:5: index out of bounds: index 1, length 1"
)]
fn field_element_traps_keep_their_sites(#[case] body: &str, #[case] expected: &str) {
    let source = format!(
        "class Holder:\n    xs: list[i64]\ndef main() -> Result[(), AllocError | IoError]:\n    mut h = Holder([1, 2]?)\n{body}    Ok(())\n"
    );
    let error = trap(&[("main.plenty", &source)]);
    assert_eq!(error, format!("error: main.plenty:{expected}\n"));
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
#[case::range_loop_length(
    "    for i in range(-9223372036854775808, 9223372036854775807):\n        print(i)?\n",
    "2:14: range length exceeds i64"
)]
#[case::reversed_range_step(
    "    zero = 0\n    for i in reversed(range(3, 9, zero)):\n        print(i)?\n",
    "3:23: range step cannot be zero"
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
        "def main() -> Result[(), IoError]:\n    zero = 0\n    print(\"before\")?\n    print(1 // zero)?\n    Ok(())\n",
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
        "import app.tools.numbers\ndef main() -> Result[(), IoError]:\n    zero = 0\n    print(app.tools.numbers.half(8, zero))?\n    Ok(())\n",
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
            "import numbers\ndef main() -> Result[(), IoError]:\n    zero = 0\n    print(numbers.half(8, zero))?\n    Ok(())\n",
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
            "from picking import pick\ndef main() -> Result[(), AllocError | IoError]:\n    xs = [1, 2, 3]?\n    print(pick(&xs, 9))?\n    Ok(())\n",
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
            "from cells import Cell\ndef main() -> Result[(), AllocError | IoError]:\n    cell = Cell([1u8, 2u8]?)\n    print(cell.at(2))?\n    Ok(())\n",
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
            "from steps import shares\ndef main() -> Result[(), IoError]:\n    zero = 0\n    for share in shares(8, zero):\n        print(share)?\n    Ok(())\n",
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

#[test]
fn an_invalid_graph_handle_names_the_actual_call_not_its_first_instantiation() {
    let error = trap(&[(
        "main.plenty",
        "from std.graph import DiGraph\ndef main() -> Result[(), AllocError | IoError]:\n    mut full = DiGraph[i64, i64]()?\n    node = full.add_node(1)?\n    print(*full.node(node))?\n    empty = DiGraph[i64, i64]()?\n    print(*empty.node(node))?\n    Ok(())\n",
    )]);
    assert!(
        error.starts_with("error: std/graph.plenty:117:20: index out of bounds"),
        "{error}"
    );
    assert!(
        error.ends_with("note: called from main.plenty:7:12\n"),
        "{error}"
    );
}

#[test]
fn standard_traps_keep_the_nearest_imported_user_caller_through_tail_calls() {
    let error = trap(&[
        (
            "numbers.plenty",
            "from std.math import abs\npub def recurse(n: i64, x: i64) -> i64:\n    if n > 0:\n        return recurse(n - 1, x)\n    abs(x)\n",
        ),
        (
            "main.plenty",
            "from numbers import recurse\ndef main() -> Result[(), IoError]:\n    print(recurse(100000, -9223372036854775808))?\n    Ok(())\n",
        ),
    ]);
    assert!(error.contains("integer overflow"), "{error}");
    assert!(
        error.ends_with("note: called from numbers.plenty:5:5\n"),
        "{error}"
    );
}

#[test]
fn indirect_standard_calls_keep_the_dynamic_call_site() {
    let error = trap(&[(
        "main.plenty",
        "from std.math import abs\ndef main() -> Result[(), IoError]:\n    f = abs[i64]\n    print(f(-9223372036854775808))?\n    Ok(())\n",
    )]);
    assert!(error.contains("integer overflow"), "{error}");
    assert!(
        error.ends_with("note: called from main.plenty:4:11\n"),
        "{error}"
    );
}

#[test]
fn a_user_callback_trap_keeps_its_own_position_without_a_standard_note() {
    let error = trap(&[(
        "main.plenty",
        "from std.error import map_error\ndef divide(b: i64) -> i64:\n    1 // b\ndef main() -> Result[(), IoError]:\n    print(map_error(Result[i64, i64].Err(0), divide).is_err())?\n    Ok(())\n",
    )]);
    assert_eq!(error, "error: main.plenty:3:5: division by zero\n");
}

#[test]
fn a_callback_can_enter_std_again_from_a_standard_call() {
    let error = trap(&[(
        "main.plenty",
        "from std.error import map_error\nfrom std.math import abs\ndef positive(x: i64) -> i64:\n    abs(x)\ndef main() -> Result[(), IoError]:\n    print(map_error(Result[i64, i64].Err(-9223372036854775808), positive).is_err())?\n    Ok(())\n",
    )]);
    assert!(error.contains("integer overflow"), "{error}");
    assert!(
        error.ends_with("note: called from main.plenty:4:5\n"),
        "{error}"
    );
}

#[test]
fn a_user_module_named_like_std_does_not_gain_standard_provenance() {
    let error = trap(&[
        (
            "app/std/collections.plenty",
            "pub def fail() -> i64:\n    1 // 0\n",
        ),
        (
            "main.plenty",
            "from app.std.collections import fail\nfrom std.math import abs\ndef main() -> Result[(), AllocError | IoError]:\n    print(abs(1))?\n    print(fail())?\n    Ok(())\n",
        ),
    ]);
    assert_eq!(
        error,
        "error: app/std/collections.plenty:2:5: division by zero\n"
    );
}

#[test]
fn worker_calls_report_the_worker_source_without_inheriting_the_parent() {
    let error = trap(&[(
        "main.plenty",
        "from std.math import abs\nfrom std.thread import spawn\ndef positive(x: i64) -> i64:\n    abs(x)\ndef main() -> Result[(), AllocError | ThreadError | IoError]:\n    print(abs(1))?\n    with spawn(positive, -9223372036854775808)? as task:\n        print(task.join())?\n    Ok(())\n",
    )]);
    assert!(
        error.ends_with("note: called from main.plenty:4:5\n"),
        "{error}"
    );
}

#[test]
fn directly_scheduled_std_jobs_do_not_inherit_an_earlier_jobs_tail_caller() {
    let error = trap(&[(
        "main.plenty",
        "from std.math import abs\nfrom std.concurrent import ThreadPoolExecutor, FutureError, PoolError, SubmitError\ndef positive(x: i64) -> i64:\n    abs(x)\ndef main() -> Result[(), AllocError | FutureError | IoError | PoolError | SubmitError[()]]:\n    with ThreadPoolExecutor(1, 1)? as pool:\n        first = pool.submit(positive, 1).drop_job()?\n        print(first.result()?)?\n        second = pool.submit(abs, -9223372036854775808).drop_job()?\n        print(second.result()?)?\n    Ok(())\n",
    )]);
    assert!(error.contains("integer overflow"), "{error}");
    assert!(!error.contains("note:"), "{error}");
}

#[rstest]
#[case::direct("    print(abs(-9223372036854775808 + 0))?\n", "3:11")]
#[case::boxed(
    "    f = BoxedCallable(abs[i64])?\n    print(f(-9223372036854775808 + 0))?\n",
    "4:11"
)]
#[case::closure(
    "    k = 0\n    f = def [k](x: i64) -> i64:\n        abs(x + k)\n    print(f(-9223372036854775808))?\n",
    "5:9"
)]
fn caller_positions_name_the_call_after_argument_evaluation(
    #[case] body: &str,
    #[case] site: &str,
) {
    let error = trap(&[(
        "main.plenty",
        &format!(
            "from std.math import abs\ndef main() -> Result[(), AllocError | IoError]:\n{body}    Ok(())\n"
        ),
    )]);
    assert!(error.contains("integer overflow"), "{error}");
    assert!(
        error.ends_with(&format!("note: called from main.plenty:{site}\n")),
        "{error}"
    );
}

/// Calls into the caller-context runtime in each function of a release clif
/// dump that includes `std`, keyed by the dump's function header.
fn caller_context_calls(source: &str) -> Vec<(String, usize)> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .current_dir(dir.path())
        .env("PLENTY_CACHE", "off")
        .args([
            "--release",
            "--dump",
            "clif",
            "--dump-std",
            "main.plenty",
            "-o",
            "-",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dump = String::from_utf8(output.stdout).unwrap();
    dump.split("\n==== ")
        .skip(1)
        .map(|function| {
            let header = function.lines().next().unwrap().to_string();
            (header, function.matches("plenty_caller_enter").count())
        })
        .collect()
}

/// Issue #366: standard-library code keeps the user caller it was entered
/// with, so formatting and collection-heavy std functions make no caller
/// context calls; user calls into std still name their call site.
#[test]
fn standard_library_code_makes_no_caller_context_calls() {
    let source = "from std.graph import DiGraph\nfrom std.fmt import FormatError\ndef main() -> Result[(), AllocError | IoError | FormatError]:\n    xs = [3, 4, 5]?\n    print(f\"{xs} {1.5}\"?)?\n    empty = DiGraph[i64, i64]()?\n    print(*empty.node(0))?\n    Ok(())\n";
    let calls = caller_context_calls(source);
    let standard: Vec<_> = calls
        .iter()
        .filter(|(header, _)| header.contains("source=std/"))
        .collect();
    assert!(standard.len() > 20, "{calls:?}");
    assert!(
        standard.iter().all(|(_, count)| *count == 0),
        "{standard:?}"
    );
    assert!(
        calls
            .iter()
            .any(|(header, count)| header.starts_with("main ") && *count > 0),
        "{calls:?}"
    );
    let error = trap(&[("main.plenty", source)]);
    assert!(
        error.starts_with("error: std/graph.plenty:117:20: index out of bounds"),
        "{error}"
    );
    assert!(
        error.ends_with("note: called from main.plenty:7:12\n"),
        "{error}"
    );
}

/// A std function calling back through a callable keeps saving and restoring
/// the context: the callback may tail-call std and leave its own site behind.
#[test]
fn standard_indirect_calls_still_restore_the_caller_context() {
    let calls = caller_context_calls(
        "from std.error import map_error\nfrom std.math import abs\ndef positive(a: i64) -> i64:\n    abs(a + 1)\ndef main() -> Result[(), IoError]:\n    print(map_error(Result[i64, i64].Err(1), positive).is_err())?\n    Ok(())\n",
    );
    let map_error = calls
        .iter()
        .find(|(header, _)| header.contains("source=std/error.plenty"))
        .unwrap_or_else(|| panic!("{calls:?}"));
    assert_eq!(map_error.1, 1, "{calls:?}");
}

/// An executable's check ends in a `ud2` that the runtime's `SIGILL` handler
/// maps back to its site (issue #425); a worker thread's failure takes the
/// same path on its own stack.
#[test]
fn a_check_failing_in_a_worker_thread_names_its_site() {
    let error = trap(&[(
        "main.plenty",
        "def worker(big: i8) -> i8:\n    big + 1i8\ndef main() -> Result[(), AllocError | IoError | ThreadError]:\n    with spawn(worker, 127i8)? as task:\n        print(task.join())?\n    Ok(())\n",
    )]);
    assert_eq!(error, "error: main.plenty:2:5: integer overflow\n");
}

/// A function has 247 trap codes for its checks; later checks call their
/// helper. Starting at `start`, the increment on line `line` overflows.
#[rstest]
#[case::last_code(-119, 249)]
#[case::past_the_codes(-120, 250)]
fn checks_past_a_functions_trap_codes_still_name_their_site(
    #[case] start: i8,
    #[case] line: usize,
) {
    let increments = "    t += 1i8\n".repeat(255);
    let source = format!(
        "def climb(x: i8) -> i8:\n    mut t = x\n{increments}    t\ndef main() -> Result[(), AllocError | IoError]:\n    print(climb({start}i8))?\n    Ok(())\n"
    );
    let error = trap(&[("main.plenty", &source)]);
    assert_eq!(
        error,
        format!("error: main.plenty:{line}:5: integer overflow\n")
    );
}
