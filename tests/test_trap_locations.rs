//! Runtime traps report the source position of the failing operation.
//! Arithmetic traps opt out at their primitive operation. Std provenance uses
//! Bytes.get (forwarded reference, scalar native tail result) or unpack_i64
//! (ordinary calls); integer abs now returns its public Result instead.
//! Worker-owned buffers reach std through an ordinary call to read_tail; only
//! read_tail -> Bytes.get claims a native tail transfer.
use rstest::rstest;
use std::process::Command;

/// Compile `files` with `main.plenty` as the entry and its directory as the
/// source root, and return what the program wrote to standard error.
fn trap(files: &[(&str, &str)]) -> String {
    trap_with_tail_calls(files, &[], None)
}

/// Tail provenance needs an actual transfer, not a call followed by cleanup.
fn trap_with_tail_calls(
    files: &[(&str, &str)],
    tail_functions: &[&str],
    expected_stdout: Option<&str>,
) -> String {
    let dir = tempfile::tempdir().unwrap();
    for (path, source) in files {
        std::fs::create_dir_all(dir.path().join(path).parent().unwrap()).unwrap();
        std::fs::write(dir.path().join(path), source).unwrap();
    }
    let source = dir.path().join("main.plenty");
    let program = dir.path().join("program");
    let mut error = None;
    for optimize in [false, true] {
        let options = plenty::CompileOptions {
            optimize,
            ..Default::default()
        };
        for name in tail_functions {
            let dump = plenty::dump_file(
                &source,
                Some(dir.path()),
                &options,
                &plenty::DumpOptions {
                    stages: vec![plenty::DumpStage::Ops, plenty::DumpStage::ClifOpt],
                    filter: Some((*name).into()),
                    ..Default::default()
                },
            )
            .unwrap();
            for (stage, text) in dump.files.iter().filter(|(stage, _)| stage != "index.txt") {
                let body = text.split(&format!("==== {name} ")).nth(1).unwrap();
                let body = body.split("\n==== ").next().unwrap();
                if stage == "ops.txt" {
                    assert!(
                        body.contains("ForwardsReferences"),
                        "{name}, {optimize}: {body}"
                    );
                    assert!(
                        body.contains("TailCall(\"__plenty_class_std.bytes.Bytes.get\")"),
                        "{name}, {optimize}: {body}"
                    );
                } else if !optimize {
                    // Optimized builds inline Bytes.get into the caller
                    // (issue #722), so no transfer remains; the stderr
                    // comparison below still covers their provenance.
                    assert!(body.contains("return_call"), "{name}, {optimize}: {body}");
                    assert!(
                        !body.contains("plenty_caller_leave"),
                        "{name}, {optimize}: {body}"
                    );
                }
            }
        }
        plenty::compile_file_to_executable_with_options(
            &source,
            &program,
            Some(dir.path()),
            &options,
        )
        .unwrap();
        let output = Command::new(&program).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{optimize}: {output:?}");
        if let Some(expected) = expected_stdout {
            assert_eq!(output.stdout, expected.as_bytes(), "{optimize}: {output:?}");
        }
        let stderr = String::from_utf8(output.stderr).unwrap();
        if let Some(previous) = &error {
            assert_eq!(previous, &stderr, "optimized trap provenance changed");
        }
        error = Some(stderr);
    }
    error.unwrap()
}

fn main_trap(body: &str) -> String {
    trap(&[(
        "main.plenty",
        &format!(
            "def main() -> Result[(), AllocError | IoError | OverflowError[i64] | ZeroDivisionError]:\n{body}    Ok(())\n"
        ),
    )])
}

#[rstest]
#[case::add(
    "    big: i8 = 127\n    unchecked:\n        print(big + 1i8)?\n",
    "4:15: integer overflow"
)]
#[case::subtract(
    "    low: u8 = 0\n    unchecked:\n        print(low - 1u8)?\n",
    "4:15: integer overflow"
)]
#[case::multiply(
    "    big: i8 = 100\n    unchecked:\n        print(2i8 * big)?\n",
    "4:15: integer overflow"
)]
#[case::negate(
    "    low: i8 = -127\n    lowest = unchecked:\n        low - 1i8\n    unchecked:\n        print(-lowest)?\n",
    "6:15: integer overflow"
)]
#[case::divide_overflow(
    "    low: i8 = -127\n    lowest = unchecked:\n        low - 1i8\n    unchecked:\n        print(lowest // -1i8)?\n",
    "6:15: integer overflow"
)]
#[case::augmented(
    "    mut total: i8 = 127\n    unchecked:\n        total += 1\n    print(total)?\n",
    "4:9: integer overflow"
)]
#[case::divide(
    "    zero = 0\n    unchecked:\n        print(7 // zero)?\n",
    "4:15: division by zero"
)]
#[case::modulo(
    "    zero = 0\n    unchecked:\n        print(7 % zero)?\n",
    "4:15: division by zero"
)]
#[case::nested_operand(
    "    zero = 0\n    unchecked:\n        print(1 + 2 * (7 // zero))?\n",
    "4:24: division by zero"
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
        "class Holder:\n    xs: list[i64]\ndef main() -> Result[(), AllocError | IoError | OverflowError[i64]]:\n    mut h = Holder([1, 2]?)\n{body}    Ok(())\n"
    );
    let error = trap(&[("main.plenty", &source)]);
    assert_eq!(error, format!("error: main.plenty:{expected}\n"));
}

#[rstest]
#[case::dictionary_read(
    "    ages = {\"a\": 1}?\n    print(ages[\"b\"])?\n",
    "3:11: dictionary key not found"
)]
#[case::unwrap_nothing(
    "    absent: Option[i64] = Nothing\n    print(absent.unwrap())?\n",
    "3:11: unwrap on Nothing"
)]
#[case::unwrap_err(
    "    failed: Result[i64, str] = Err(\"no\")\n    print(failed.unwrap())?\n",
    "3:11: unwrap on Err"
)]
#[case::range_length(
    "    print(len(range(-9223372036854775808, 9223372036854775807)))?\n",
    "2:15: range length exceeds i64"
)]
#[case::range_loop_length(
    "    for i in range(-9223372036854775808, 9223372036854775807):\n        print(i)?\n",
    "2:14: range length exceeds i64"
)]
#[case::split_separator(
    "    separator = \"\"\n    parts = \"a,b\".split(separator)?\n    print(len(parts))?\n",
    "3:13: string split requires a nonempty separator"
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
        "def main() -> Result[(), IoError]:\n    zero = 0\n    print(\"before\")?\n    unchecked:\n        print(1 // zero)?\n    Ok(())\n",
    )
    .unwrap();
    let program = dir.path().join("program");
    plenty::compile_file_to_executable(&source, &program, None).unwrap();
    let output = Command::new(&program).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"before\n");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: main.plenty:5:15: division by zero\n"
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
        "pub def half(total: i64, parts: i64) -> i64:\n    unchecked:\n        total // parts\n",
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
        "error: app/tools/numbers.plenty:3:9: division by zero\n"
    );
}

#[test]
fn a_trap_in_an_imported_function_names_the_defining_file() {
    let error = trap(&[
        (
            "numbers.plenty",
            "pub def half(total: i64, parts: i64) -> i64:\n    unchecked:\n        total // parts\n",
        ),
        (
            "main.plenty",
            "import numbers\ndef main() -> Result[(), IoError]:\n    zero = 0\n    print(numbers.half(8, zero))?\n    Ok(())\n",
        ),
    ]);
    assert_eq!(error, "error: numbers.plenty:3:9: division by zero\n");
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
            "pub def shares(total: i64, parts: i64) -> Generator[i64]:\n    yield total\n    share = unchecked:\n        total // parts\n    yield share\n",
        ),
        (
            "main.plenty",
            "from steps import shares\ndef main() -> Result[(), IoError]:\n    zero = 0\n    for share in shares(8, zero):\n        print(share)?\n    Ok(())\n",
        ),
    ]);
    assert_eq!(error, "error: steps.plenty:4:9: division by zero\n");
}

#[test]
fn each_trap_site_keeps_its_own_position() {
    // Two operations on one line share a marker unless each places its own.
    let error = main_trap(
        "    xs = [1, 2, 3]?\n    zero = 0\n    unchecked:\n        print(xs[0] // zero + xs[9])?\n",
    );
    assert_eq!(error, "error: main.plenty:5:15: division by zero\n");
    let error = main_trap(
        "    xs = [1, 2, 3]?\n    one = 1\n    unchecked:\n        print(xs[0] // one + xs[9])?\n",
    );
    assert_eq!(
        error,
        "error: main.plenty:5:30: index out of bounds: index 9, length 3\n"
    );
}

#[test]
fn an_invalid_graph_handle_names_the_actual_call_not_its_first_instantiation() {
    let error = trap(&[(
        "main.plenty",
        "from std.graph import DiGraph\ndef main() -> Result[(), AllocError | IoError]:\n    mut full = DiGraph[i64, i64]()?\n    node = full.add_node(1)?\n    print(*full.node(node))?\n    empty = DiGraph[i64, i64]()?\n    print(*empty.node(node))?\n    Ok(())\n",
    )]);
    assert!(
        error.starts_with("error: std/graph.plenty:118:20: index out of bounds"),
        "{error}"
    );
    assert!(
        error.ends_with("note: called from main.plenty:7:12\n"),
        "{error}"
    );
}

#[test]
fn standard_traps_keep_the_nearest_imported_user_caller_through_tail_calls() {
    let error = trap_with_tail_calls(
        &[
            (
                "numbers.plenty",
                "from std.bytes import Bytes\npub def recurse(source: &Bytes, n: i64, x: i64) -> u8:\n    if n > 0:\n        previous = unchecked:\n            n - 1\n        return recurse(source, previous, x)\n    source.get(x)\n",
            ),
            (
                "main.plenty",
                "from numbers import recurse\nfrom std.bytes import from_bytes\ndef main() -> Result[(), AllocError | IoError]:\n    source = from_bytes(b\"*\")?\n    print(recurse(source, 100000, 1))?\n    Ok(())\n",
            ),
        ],
        &["numbers.recurse"],
        None,
    );
    assert_eq!(
        error,
        "error: std/bytes.plenty:48:13: Bytes.get: index is outside 0..len()\nnote: called from numbers.plenty:7:5\n"
    );
}

#[test]
fn indirect_standard_calls_keep_the_dynamic_call_site() {
    let error = trap(&[(
        "main.plenty",
        "from std.encoding import unpack_i64, Endian\ndef main() -> Result[(), IoError]:\n    f = unpack_i64\n    print(f(b\"12345678\", 1, Endian.Little))?\n    Ok(())\n",
    )]);
    assert!(
        error.starts_with(
            "error: std/encoding.plenty:800:13: unpack: offset outside source buffer\n"
        ),
        "{error}"
    );
    assert!(
        error.ends_with("note: called from main.plenty:4:11\n"),
        "{error}"
    );
}

#[test]
fn a_user_callback_trap_keeps_its_own_position_without_a_standard_note() {
    let error = trap(&[(
        "main.plenty",
        "from std.error import map_error\ndef divide(b: i64) -> i64:\n    unchecked:\n        1 // b\ndef main() -> Result[(), IoError]:\n    print(map_error(Result[i64, i64].Err(0), divide).is_err())?\n    Ok(())\n",
    )]);
    assert_eq!(error, "error: main.plenty:4:9: division by zero\n");
}

#[test]
fn a_callback_can_enter_std_again_from_a_standard_call() {
    let error = trap_with_tail_calls(
        &[(
            "main.plenty",
            "from std.error import map_error\nfrom std.bytes import Bytes, from_bytes\ndef read_tail(source: &Bytes, x: i64) -> u8:\n    source.get(x)\ndef read(x: i64) -> Result[u8, AllocError]:\n    source = from_bytes(b\"*\")?\n    Ok(read_tail(source, x))\ndef main() -> Result[(), IoError]:\n    print(map_error(Result[i64, i64].Err(1), read).is_err())?\n    Ok(())\n",
        )],
        &["read_tail"],
        None,
    );
    assert_eq!(
        error,
        "error: std/bytes.plenty:48:13: Bytes.get: index is outside 0..len()\nnote: called from main.plenty:4:5\n"
    );
}

#[test]
fn a_user_module_named_like_std_does_not_gain_standard_provenance() {
    let error = trap(&[
        (
            "app/std/collections.plenty",
            "pub def fail() -> i64:\n    unchecked:\n        1 // 0\n",
        ),
        (
            "main.plenty",
            "from app.std.collections import fail\nfrom std.math import abs\ndef main() -> Result[(), AllocError | IoError | OverflowError[i64]]:\n    print(abs(1)?)?\n    print(fail())?\n    Ok(())\n",
        ),
    ]);
    assert_eq!(
        error,
        "error: app/std/collections.plenty:3:9: division by zero\n"
    );
}

#[test]
fn worker_calls_report_the_worker_source_without_inheriting_the_parent() {
    let error = trap_with_tail_calls(
        &[(
            "main.plenty",
            "from std.bytes import Bytes, from_bytes\nfrom std.thread import spawn\ndef read_tail(source: &Bytes, x: i64) -> u8:\n    source.get(x)\ndef read(x: i64) -> Result[u8, AllocError]:\n    source = from_bytes(b\"*\")?\n    Ok(read_tail(source, x))\ndef main() -> Result[(), AllocError | SpawnError[()] | IoError]:\n    source = from_bytes(b\"*\")?\n    print(source.get(0))?\n    with spawn(read, 1).drop_job()? as task:\n        print(task.join()?)?\n    Ok(())\n",
        )],
        &["read_tail"],
        Some("42\n"),
    );
    assert_eq!(
        error,
        "error: std/bytes.plenty:48:13: Bytes.get: index is outside 0..len()\nnote: called from main.plenty:4:5\n"
    );
}

#[test]
fn directly_scheduled_std_jobs_do_not_inherit_an_earlier_jobs_tail_caller() {
    let error = trap_with_tail_calls(
        &[(
            "main.plenty",
            "from std.bytes import Bytes, from_bytes\nfrom std.fmt import string_formatter\nfrom std.concurrent import ThreadPoolExecutor, FutureError, PoolError, SubmitError\ndef read_tail(source: &Bytes, x: i64) -> u8:\n    source.get(x)\ndef read(x: i64) -> Result[u8, AllocError]:\n    source = from_bytes(b\"*\")?\n    Ok(read_tail(source, x))\ndef main() -> Result[(), AllocError | FutureError | IoError | PoolError | SubmitError[()]]:\n    with ThreadPoolExecutor(1, 1)? as pool:\n        first = pool.submit(read, 0).drop_job()?\n        print((first.result()?)?)?\n        depth: NonNegative[i64] = 65\n        second = pool.submit(string_formatter, 0, depth).drop_job()?\n        drop(second.result()?)\n    Ok(())\n",
        )],
        &["read_tail"],
        Some("42\n"),
    );
    // The guard can move as the formatter implementation changes. Keep the
    // assertion exact so an inherited caller note still fails this regression.
    let guard = include_str!("../stdlib/std/fmt.plenty")
        .lines()
        .position(|line| {
            line.contains("__trap(\"std.fmt.string_formatter: max_depth is above 64\")")
        })
        .expect("string_formatter depth guard")
        + 1;
    assert_eq!(
        error,
        format!(
            "error: std/fmt.plenty:{guard}:9: std.fmt.string_formatter: max_depth is above 64\n"
        )
    );
}

#[rstest]
#[case::debug(false)]
#[case::release(true)]
fn scheduled_integer_abs_returns_its_declared_overflow(#[case] optimize: bool) {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(&source, "from std.math import abs\nfrom std.concurrent import ThreadPoolExecutor, FutureError, PoolError, SubmitError\ndef main() -> Result[(), AllocError | FutureError | IoError | PoolError | SubmitError[()]]:\n    with ThreadPoolExecutor(1, 1)? as pool:\n        job = pool.submit(abs, i64.MIN).drop_job()?\n        match job.result()?:\n            case Ok(value):\n                print(\"unexpected success\", value)?\n            case Err(OverflowError(wrapped)):\n                print(wrapped)?\n    Ok(())\n").unwrap();
    let executable = dir.path().join("program");
    let options = plenty::CompileOptions {
        optimize,
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&source, &executable, None, &options).unwrap();
    let output = Command::new(executable).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"-9223372036854775808\n");
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[rstest]
#[case::direct(
    "    print(unpack_i64(b\"12345678\", 1 + 0, Endian.Little))?\n",
    "4:11",
    "error: std/encoding.plenty:800:13: unpack: offset outside source buffer\n"
)]
#[case::boxed(
    "    f = BoxedCallable(unpack_i64)?\n    print(f(b\"12345678\", 1 + 0, Endian.Little))?\n",
    "5:11",
    "error: std/encoding.plenty:800:13: unpack: offset outside source buffer\n"
)]
#[case::closure(
    "    source = from_bytes(b\"*\")?\n    k = 0\n    f = def [k](source: &Bytes, x: i64) -> u8:\n        offset = unchecked:\n            x + k\n        source.get(offset)\n    print(f(source, 1))?\n",
    "9:9",
    "error: std/bytes.plenty:48:13: Bytes.get: index is outside 0..len()\n"
)]
fn caller_positions_name_the_call_after_argument_evaluation(
    #[case] body: &str,
    #[case] site: &str,
    #[case] operation: &str,
) {
    let error = trap(&[(
        "main.plenty",
        &format!(
            "from std.encoding import unpack_i64, Endian\nfrom std.bytes import Bytes, from_bytes\ndef main() -> Result[(), AllocError | IoError | OverflowError[i64]]:\n{body}    Ok(())\n"
        ),
    )]);
    assert!(error.starts_with(operation), "{error}");
    assert!(
        error.ends_with(&format!("note: called from main.plenty:{site}\n")),
        "{error}"
    );
}

/// Calls into the caller-context runtime in each function of a release clif
/// dump that includes `std`, keyed by the dump's function header.
fn caller_context_calls(source: &str) -> Vec<(String, usize)> {
    dumped_functions(source)
        .into_iter()
        .map(|(header, body)| {
            let count = body.matches("plenty_caller_enter").count();
            (header, count)
        })
        .collect()
}

/// Header and clif of each function in a release dump that includes `std`.
fn dumped_functions(source: &str) -> Vec<(String, String)> {
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
            (header, function.to_string())
        })
        .collect()
}

/// Issue #366: standard-library code keeps the user caller it was entered
/// with, so formatting and collection-heavy std functions make no caller
/// context calls around direct calls; user calls into std still name their
/// call site. A call through a callable still saves the context (see
/// `standard_indirect_calls_still_restore_the_caller_context`), as an inlined
/// `map_error` in std formatting does (issue #770).
#[test]
fn standard_library_code_makes_no_caller_context_calls() {
    let source = "from std.graph import DiGraph\nfrom std.fmt import FormatError\ndef main() -> Result[(), AllocError | IoError | FormatError]:\n    xs = [3, 4, 5]?\n    print(f\"{xs} {1.5}\"?)?\n    empty = DiGraph[i64, i64]()?\n    print(*empty.node(0))?\n    Ok(())\n";
    let calls = caller_context_calls(source);
    let standard: Vec<_> = dumped_functions(source)
        .into_iter()
        .filter(|(header, _)| header.contains("source=std/"))
        .map(|(header, body)| {
            let enters = body.matches("plenty_caller_enter").count();
            (header, enters, body.matches("call_indirect").count())
        })
        .collect();
    assert!(standard.len() > 20, "{calls:?}");
    assert!(
        standard
            .iter()
            .all(|(_, enters, indirect)| enters <= indirect),
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
        error.starts_with("error: std/graph.plenty:118:20: index out of bounds"),
        "{error}"
    );
    assert!(
        error.ends_with("note: called from main.plenty:7:12\n"),
        "{error}"
    );
}

/// Issue #750: releasing strings, plain records and collections of them runs
/// no compiled code, so user code releases them without caller bookkeeping;
/// a release that can run a `__del__` keeps it.
#[test]
fn releases_that_run_no_compiled_code_make_no_caller_context_calls() {
    let calls = caller_context_calls(
        "import std.bytes\nclass Plain:\n    name: str\n    counts: dict[str, list[i64]]\nclass Tree:\n    kids: list[Tree]\nclass Guard:\n    n: i64\n    def __del__(self) -> ():\n        pass\ndef plain(s: str, xs: list[str], p: Plain, o: Option[Box[Plain]], t: Tree) -> i64:\n    unchecked: # Small lengths.\n        s.byte_len() + len(xs) + len(p.counts) + len(t.kids)\ndef guarded(g: Guard, gs: list[Guard]) -> i64:\n    unchecked: # Small values.\n        g.n + len(gs)\ndef main() -> Result[(), AllocError | IoError]:\n    print(std.bytes.with_capacity(1)?.len())?\n    p = Plain(\"p\", {}?)\n    print(plain(\"s\", [\"x\"]?, p, Nothing, Tree(list[Tree].with_capacity(0)?)))?\n    print(guarded(Guard(1), [Guard(2)]?))?\n    Ok(())\n",
    );
    let count = |name: &str| {
        calls
            .iter()
            .find(|(header, _)| header.starts_with(&format!("{name} ")))
            .unwrap_or_else(|| panic!("{name}: {calls:?}"))
            .1
    };
    assert_eq!(count("plain"), 0, "{calls:?}");
    assert!(count("guarded") > 0, "{calls:?}");
}

/// Issue #750: a std accessor that only reads a field, such as a domain
/// type's `.get()`, cannot trap or run code, so user calls to it need no
/// caller site; a std call that can trap keeps one.
#[test]
fn field_reading_standard_accessors_make_no_caller_context_calls() {
    let calls = caller_context_calls(
        "import std.bytes\ndef nn(xs: &list[i64], s: NonNegative[i64], b: Bounded[i64, 0, 7]) -> i64:\n    unchecked: # Small values.\n        xs[s.get()] + xs[b.get()]\ndef sized(n: i64) -> Result[i64, AllocError]:\n    Ok(std.bytes.with_capacity(n)?.len())\ndef main() -> Result[(), AllocError | IoError]:\n    xs = [1, 2, 3]?\n    print(nn(&xs, 1, 2))?\n    print(sized(1)?)?\n    Ok(())\n",
    );
    let count = |name: &str| {
        calls
            .iter()
            .find(|(header, _)| header.starts_with(&format!("{name} ")))
            .unwrap_or_else(|| panic!("{name}: {calls:?}"))
            .1
    };
    assert_eq!(count("nn"), 0, "{calls:?}");
    assert!(count("sized") > 0, "{calls:?}");
}

/// A std function calling back through a callable keeps saving and restoring
/// the context: the callback may tail-call std and leave its own site behind.
#[test]
fn standard_indirect_calls_still_restore_the_caller_context() {
    let calls = caller_context_calls(
        "from std.error import map_error\nfrom std.bytes import Bytes, from_bytes\ndef read_tail(source: &Bytes, x: i64) -> u8:\n    source.get(x)\ndef read(x: i64) -> Result[u8, AllocError]:\n    source = from_bytes(b\"*\")?\n    Ok(read_tail(source, x))\ndef main() -> Result[(), IoError]:\n    print(map_error(Result[i64, i64].Err(1), read).is_err())?\n    Ok(())\n",
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
        "def worker(big: i8) -> i8:\n    unchecked:\n        big + 1i8\ndef main() -> Result[(), AllocError | IoError | SpawnError[()]]:\n    with spawn(worker, 127i8).drop_job()? as task:\n        print(task.join())?\n    Ok(())\n",
    )]);
    assert_eq!(error, "error: main.plenty:3:9: integer overflow\n");
}

/// A function has 247 trap codes for its checks; later checks call their
/// helper. Starting at `start`, the increment on line `line` overflows.
#[rstest]
#[case::last_code(-119, 250)]
#[case::past_the_codes(-120, 251)]
fn checks_past_a_functions_trap_codes_still_name_their_site(
    #[case] start: i8,
    #[case] line: usize,
) {
    let increments = "        t += 1i8\n".repeat(255);
    let source = format!(
        "def climb(x: i8) -> i8:\n    mut t = x\n    unchecked:\n{increments}    t\ndef main() -> Result[(), AllocError | IoError]:\n    print(climb({start}i8))?\n    Ok(())\n"
    );
    let error = trap(&[("main.plenty", &source)]);
    assert_eq!(
        error,
        format!("error: main.plenty:{line}:9: integer overflow\n")
    );
}

#[rstest]
#[case::debug(false)]
#[case::release(true)]
fn generic_helper_sites_survive_cold_warm_and_disabled_function_caches(#[case] optimize: bool) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("picking.plenty"),
        "pub def pick[T](items: &list[T], index: i64) -> T:\n    items[index]\n",
    )
    .unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(&source, "from picking import pick\ndef main() -> Result[(), AllocError | IoError]:\n    xs = [1, 2, 3]?\n    print(pick(&xs, 9))?\n    Ok(())\n").unwrap();
    let cache = plenty::cache::CacheMode::Directory(dir.path().join("cache"));
    let executable = dir.path().join("program");
    for mode in [cache.clone(), cache, plenty::cache::CacheMode::Off] {
        let options = plenty::CompileOptions {
            optimize,
            cache: mode.clone(),
            program_cache: false,
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(
            &source,
            &executable,
            Some(dir.path()),
            &options,
        )
        .unwrap();
        let output = Command::new(&executable).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{optimize}, {mode:?}");
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "error: picking.plenty:2:5: index out of bounds: index 9, length 3\n",
            "{optimize}, {mode:?}"
        );
    }
}

/// Issue #842: user code that inlines `Bytes.get`/`set` names its call site
/// in the std trap records instead of setting the caller context around each
/// call. Each site keeps its own note, including two calls of one method in
/// one function and a call in an imported module. The loop pushes, so it keeps
/// the method calls rather than caching the buffer (issue #843). `trap` also
/// runs an unoptimized build, which inlines nothing and so checks only the
/// unchanged caller-scope path.
#[rstest]
#[case::first_get([9, 1, 0, 0], "48:13: Bytes.get", "numbers.plenty:5:17")]
#[case::second_get([0, 9, 0, 0], "48:13: Bytes.get", "numbers.plenty:6:25")]
#[case::set([0, 1, 9, 0], "53:13: Bytes.set", "numbers.plenty:7:9")]
#[case::main_get([0, 1, 0, 9], "48:13: Bytes.get", "main.plenty:6:11")]
fn inlined_standard_traps_name_their_call_site_without_a_caller_scope(
    #[case] indexes: [i64; 4],
    #[case] site: &str,
    #[case] caller: &str,
) {
    let [a, b, c, last] = indexes;
    let numbers = "from std.bytes import Bytes\npub def mix(data: &mut Bytes, a: i64, b: i64, c: i64) -> Result[u8, AllocError]:\n    mut total = 0u8\n    for i in range(2):\n        total = data.get(a) ^ total\n        total = total ^ data.get(b)\n        data.set(c, total)\n        data.push(total)?\n    Ok(total)\n";
    let main = format!(
        "from numbers import mix\nfrom std.bytes import from_bytes\ndef main() -> Result[(), AllocError | IoError]:\n    mut data = from_bytes(b\"ab\")?\n    total = mix(&mut data, {a}, {b}, {c})?\n    print(data.get({last}) ^ total)?\n    Ok(())\n"
    );
    let error = trap(&[("numbers.plenty", numbers), ("main.plenty", &main)]);
    assert_eq!(
        error,
        format!(
            "error: std/bytes.plenty:{site}: index is outside 0..len()\nnote: called from {caller}\n"
        )
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("numbers.plenty"), numbers).unwrap();
    std::fs::write(dir.path().join("main.plenty"), &main).unwrap();
    let dump = plenty::dump_file(
        &dir.path().join("main.plenty"),
        Some(dir.path()),
        &plenty::CompileOptions {
            optimize: true,
            ..Default::default()
        },
        &plenty::DumpOptions {
            stages: vec![plenty::DumpStage::Asm],
            filter: Some("mix".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let (_, asm) = dump
        .files
        .iter()
        .find(|(stage, _)| stage == "asm.txt")
        .unwrap();
    assert!(asm.contains("plenty_bytes_push"), "{asm}");
    assert!(!asm.contains("plenty_caller_enter"), "{asm}");
}
