//! A loop whose condition indexes a list or view (`while limits[k] < r:`) is
//! rotated and its index step has no overflow check (issue #932); a
//! unit-step `range` loop tests its variable against the stop and keeps no
//! countdown (issue #933); and a shared list's cached length and rows are
//! read once before the outermost loop the binding precedes.
use std::path::Path;
use std::process::Command;

mod support;

fn plenty(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(support::plenty_bin())
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap()
}

/// Standard output and error of `source`, which may exit with a failure.
fn run(source: &str, flags: &[&str]) -> (String, String) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let mut args = flags.to_vec();
    args.push("main.plenty");
    let output = plenty(dir.path(), &args);
    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

/// The `--dump` file `kind` of `source`, compiled with `--release`.
fn dump(source: &str, kind: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let output = plenty(
        dir.path(),
        &["--release", "--dump", kind, "main.plenty", "-o", "out"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read_to_string(dir.path().join(format!("out/{kind}.txt"))).unwrap()
}

/// The dump section of function `name`.
fn section<'a>(dump: &'a str, name: &str) -> &'a str {
    let start = dump
        .find(&format!("==== {name}"))
        .unwrap_or_else(|| panic!("no section {name} in:\n{dump}"));
    let rest = &dump[start + 5..];
    &rest[..rest.find("\n==== ").unwrap_or(rest.len())]
}

const SEARCH: &str = "\
def find_view(limits: &[i64], r: i64, start: i64) -> Result[i64, OverflowError[i64]]:
    mut k = start
    while limits[k] < r:
        k += 1
    Ok(k)


def find_list(limits: &list[i64], r: i64, start: i64) -> Result[i64, OverflowError[i64]]:
    mut k = start
    while limits[k] < r:
        k += 1
    Ok(k)


def find_trapping(limits: &list[i64], r: i64) -> i64:
    unchecked:
        mut k = 0
        while limits[k] < r:
            k += 1
        k


def find_by_two(limits: &list[i64], r: i64) -> Result[i64, OverflowError[i64]]:
    mut k = 0
    while limits[k] < r:
        k += 2
    Ok(k)


def select_all(limits: &list[i64], targets: &[i64]) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for i in range(len(targets)):
        mut k = 0
        while limits[k] < targets[i]:
            k += 1
        total += k
        mut j = 0
        while limits[j] < targets[i] - 5:
            j += 1
        total += j
    Ok(total)
";

/// Results match in both build modes, for nonnegative and negative start
/// indices, and a search that runs off the end traps at the condition's
/// index with the list's length.
#[test]
fn indexed_conditions_keep_results_and_traps() {
    let main = "
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    limits = [10, 20, 30, 40]?
    targets = [5, 15, 25, 35, 40]?
    empty: list[i64] = []?
    print(find_view(&limits, 25, 0)?, find_list(&limits, 25, 0)?, find_trapping(&limits, 40))?
    print(find_view(&limits, 15, -4)?, find_list(&limits, 15, -4)?, find_list(&limits, 35, -1)?)?
    print(find_by_two(&limits, 25)?, select_all(&limits, &targets)?)?
    print(select_all(&limits, &empty)?)?
    print(find_list(&limits, 41, 0)?)?
    Ok(())
";
    let source = format!("{SEARCH}\n{main}");
    for flags in [&["--release"][..], &[]] {
        let (out, err) = run(&source, flags);
        assert_eq!(out, "2 2 3\n-3 -3 -1\n2 15\n0\n", "{flags:?}");
        assert_eq!(
            err, "error: main.plenty:10:11: index out of bounds: index 4, length 4\n",
            "{flags:?}"
        );
    }
    let view = "
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    limits = [10, 20]?
    print(find_view(&limits, 21, -2)?)?
    Ok(())
";
    let (out, err) = run(&format!("{SEARCH}\n{view}"), &["--release"]);
    assert_eq!(out, "");
    assert_eq!(
        err,
        "error: main.plenty:3:11: index out of bounds: index 2, length 2\n"
    );
    let empty = "
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    limits: list[i64] = []?
    print(find_trapping(&limits, 1))?
    Ok(())
";
    let (out, err) = run(&format!("{SEARCH}\n{empty}"), &["--release"]);
    assert_eq!(out, "");
    assert_eq!(
        err,
        "error: main.plenty:18:15: index out of bounds: index 0, length 0\n"
    );
}

/// The step after a passed index check has no overflow check; a step of 2
/// can exceed the length and keeps its check. The list's header is read
/// before the outer loop, once for both inner loops.
#[test]
fn indexed_conditions_drop_the_step_check_and_hoist_the_header() {
    let source = format!("{SEARCH}\n\ndef main() -> ():\n    ()\n");
    let clif = dump(&source, "clif-opt");
    for name in ["find_view", "find_list", "find_trapping"] {
        assert!(
            !section(&clif, name).contains("sadd_overflow"),
            "{name}:\n{}",
            section(&clif, name)
        );
    }
    assert!(section(&clif, "find_by_two").contains("sadd_overflow"));
    let ops = dump(&source, "ops");
    let select = section(&ops, "select_all");
    let outer = select.find("\n  Loop").unwrap();
    for header in ["ListLenRef", "ListRowsRef"] {
        assert_eq!(select.matches(header).count(), 1, "{select}");
        assert!(select.find(header).unwrap() < outer, "{select}");
    }
}

const RANGES: &str = "\
def ranges(lo: i64, hi: i64) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for i in range(lo, hi):
        if i % 3 == 0:
            continue
        if i == 11:
            break
        total += i
    Ok(total)


def passes(lo: i64, hi: i64) -> (i64, i64):
    mut count = 0
    mut last = 0
    for i in range(lo, hi):
        count = count.wrapping_add(1)
        last = i
    (count, last)


def nested(n: i64) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for i in range(n):
        for j in range(i, n):
            total += i * j
    Ok(total)


def stepped(n: i64) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for i in range(0, n, 3):
        total += i
    for i in range(n, 0, -1):
        total += i
    Ok(total)


def total(values: &[i64], n: i64) -> i64:
    unchecked:
        mut t = 0
        for i in range(n):
            t = t + values[i]
        t
";

/// Empty, reversed, single-pass and extreme ranges, with `break`,
/// `continue` and nesting, in both build modes.
#[test]
fn unit_step_ranges_keep_their_passes() {
    let main = "
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    values = [1, 2, 3]?
    print(ranges(0, 20)?, ranges(-5, 5)?, ranges(5, 2)?, ranges(7, 8)?, ranges(4, 4)?)?
    print(passes(i64.MAX - 2, i64.MAX), passes(i64.MIN, i64.MIN + 2))?
    print(nested(5)?, nested(0)?, stepped(10)?, total(&values, 3), total(&values, 0))?
    print(total(&values, 4))?
    Ok(())
";
    let source = format!("{RANGES}\n{main}");
    for flags in [&["--release"][..], &[]] {
        let (out, err) = run(&source, flags);
        let mut lines = out.lines();
        assert_eq!(lines.next(), Some("37 -5 0 7 0"), "{flags:?}");
        assert_eq!(
            lines.next(),
            Some("(2, 9223372036854775806) (2, -9223372036854775807)"),
            "{flags:?}"
        );
        assert_eq!(lines.next(), Some("65 0 73 6 0"), "{flags:?}");
        assert_eq!(
            err, "error: main.plenty:42:21: index out of bounds: index 3, length 3\n",
            "{flags:?}"
        );
    }
}

/// A unit-step range loop compares its variable with the stop and has no
/// countdown subtraction; another step keeps the countdown.
#[test]
fn unit_step_ranges_have_no_countdown() {
    let source = format!("{RANGES}\n\ndef main() -> ():\n    ()\n");
    let clif = dump(&source, "clif-opt");
    let total = section(&clif, "total");
    assert!(!total.contains("isub"), "{total}");
    assert!(!total.contains("icmp sgt"), "{total}");
    assert!(section(&clif, "stepped").contains("isub"));
}
