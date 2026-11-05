//! Build modes for plain `unchecked:` suites: trapping by default, assumed
//! preconditions with `--unchecked assume`, and `unchecked trap:` (B14).
mod support;
use plenty::{CompileOptions, DumpOptions, DumpStage, UncheckedMode};
use std::path::Path;
use std::process::{Command, Output};
use support::plenty_bin;

fn options(unchecked: UncheckedMode) -> CompileOptions {
    CompileOptions {
        unchecked,
        cache: plenty::cache::CacheMode::Off,
        ..Default::default()
    }
}

fn run(dir: &Path, source: &str, options: &CompileOptions) -> Output {
    let path = dir.join("main.plenty");
    let executable = dir.join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable_with_options(&path, &executable, None, options)
        .unwrap_or_else(|error| panic!("{error}\n{source}"));
    Command::new(executable).output().unwrap()
}

fn assert_traps(output: &Output, message: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains(message), "{stderr}");
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// The `stage` text of the functions whose names contain `filter`.
fn dump(source: &str, mode: UncheckedMode, stage: DumpStage, filter: &str, std: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, source).unwrap();
    let dump = plenty::dump_file(
        &path,
        None,
        &options(mode),
        &DumpOptions {
            stages: vec![stage],
            standard_library: std,
            filter: Some(filter.into()),
        },
    )
    .unwrap();
    dump.files
        .into_iter()
        .find(|(name, _)| name != "index.txt")
        .unwrap()
        .1
}

const OVERFLOW: &str = r#"
def add(a: i64, b: i64) -> i64:
    unchecked:
        return a + b
def main() -> Result[(), IoError]:
    print(add(i64.MAX, 1))?
    Ok(())
"#;

/// `add` as in `OVERFLOW`, at a valid boundary. Assume mode never runs an
/// overflow: its result would be undefined.
const VALID: &str = r#"
def add(a: i64, b: i64) -> i64:
    unchecked:
        return a + b
def main() -> Result[(), IoError]:
    print(add(9223372036854775806, 1))?
    Ok(())
"#;

#[test]
fn plain_unchecked_traps_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let output = run(dir.path(), OVERFLOW, &options(UncheckedMode::Trap));
    assert_traps(&output, "integer overflow");
}

#[test]
fn unchecked_trap_traps_in_every_mode() {
    let source = r#"
def add(a: i64, b: i64) -> i64:
    unchecked trap:
        return a + b
def main() -> Result[(), IoError]:
    print(add(i64.MAX, 1))?
    Ok(())
"#;
    for mode in [UncheckedMode::Trap, UncheckedMode::Assume] {
        let dir = tempfile::tempdir().unwrap();
        assert_traps(&run(dir.path(), source, &options(mode)), "integer overflow");
    }
}

const KERNEL: &str = r#"
def kernel(a: i64, b: i64, s: i64, x: i64) -> i64:
    unchecked:
        return (a // b) + (a % b) + (a << s) + i64(i32(x)) + a * b - b + a.pow(2)
def main() -> Result[(), IoError]:
    print(kernel(7, -2, 3, -5))?
    unchecked:
        print(i64.MIN % -1, 1u64 << 63)?
    Ok(())
"#;

#[test]
fn assumed_preconditions_keep_valid_results() {
    // 7 // -2 = -4, 7 % -2 = -1, 7 << 3 = 56, -5, 7 * -2 = -14, + 2, 49.
    let expected = "83\n0 9223372036854775808\n";
    for mode in [UncheckedMode::Trap, UncheckedMode::Assume] {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(stdout(&run(dir.path(), KERNEL, &options(mode))), expected);
    }
}

#[test]
fn assumed_preconditions_omit_every_check() {
    let trapping = dump(
        KERNEL,
        UncheckedMode::Trap,
        DumpStage::ClifOpt,
        "kernel",
        false,
    );
    assert!(trapping.contains("plenty_trap_overflow"), "{trapping}");
    assert!(trapping.contains("plenty_trap_div_zero"), "{trapping}");
    let assumed = dump(
        KERNEL,
        UncheckedMode::Assume,
        DumpStage::ClifOpt,
        "kernel",
        false,
    );
    assert!(!assumed.contains("plenty_trap"), "{assumed}");
    assert!(!assumed.contains("trapnz"), "{assumed}");
}

#[test]
fn nested_suites_select_their_own_contracts() {
    let source = r#"
def nested(a: i64, b: i64) -> i64:
    unchecked trap:
        x = a * b
        y = unchecked:
            a + b
        return x + y
def recoverable(a: i64) -> Result[i64, OverflowError[i64]]:
    unchecked:
        return checked:
            a + 1
def main() -> Result[(), IoError]:
    print(nested(3, 4), recoverable(i64.MAX).is_err())?
    Ok(())
"#;
    let ops = dump(
        source,
        UncheckedMode::Assume,
        DumpStage::Ops,
        "nested",
        false,
    );
    assert_eq!(ops.matches("Assumed(").count(), 1, "{ops}");
    assert!(ops.contains("Assumed(Add)"), "{ops}");
    let dir = tempfile::tempdir().unwrap();
    let output = run(dir.path(), source, &options(UncheckedMode::Assume));
    assert_eq!(stdout(&output), "19 True\n");
}

#[test]
fn closures_keep_their_own_contracts() {
    let source = r#"
def apply(a: i64) -> i64:
    unchecked:
        f = def(x: i64) -> i64:
            return x + 1
        return f(a)
def main() -> ():
    pass
"#;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, source).unwrap();
    let error = plenty::compile_file_to_executable_with_options(
        &path,
        &dir.path().join("program"),
        None,
        &options(UncheckedMode::Assume),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("OverflowError[i64]"), "{error}");
}

#[test]
fn std_keeps_its_checks_in_assume_mode() {
    let source = r#"
import std.math as math
def main() -> Result[(), IoError]:
    print(math.isqrt(99))?
    Ok(())
"#;
    let ops = dump(source, UncheckedMode::Assume, DumpStage::Ops, "isqrt", true);
    assert!(ops.contains("FloorDiv"), "{ops}");
    assert!(!ops.contains("Assumed("), "{ops}");
}

#[test]
fn trap_and_assume_builds_use_separate_cache_entries() {
    let dir = tempfile::tempdir().unwrap();
    let cache = plenty::cache::CacheMode::Directory(dir.path().join("cache"));
    let mode = |unchecked| CompileOptions {
        unchecked,
        cache: cache.clone(),
        ..Default::default()
    };
    // The assumed `add` is cached first; the trapping build must not reuse it.
    let assumed = run(dir.path(), VALID, &mode(UncheckedMode::Assume));
    assert_eq!(stdout(&assumed), "9223372036854775807\n");
    let trapping = run(dir.path(), OVERFLOW, &mode(UncheckedMode::Trap));
    assert_traps(&trapping, "integer overflow");
}

#[test]
fn tail_suite_checks_only_the_enclosing_block_for_unreachable_code() {
    let source = r#"
def f(a: i64, b: i64) -> i64:
    unchecked:
        x = a * b
        return x + 1
def main() -> Result[(), IoError]:
    print(f(3, 4))?
    Ok(())
"#;
    let dir = tempfile::tempdir().unwrap();
    let output = run(dir.path(), source, &options(UncheckedMode::Trap));
    assert_eq!(stdout(&output), "13\n");
}

#[test]
fn trap_stays_a_name() {
    let source = r#"
def main() -> Result[(), IoError]:
    trap = 2
    unchecked: i64 = trap
    unchecked trap:
        print(unchecked + trap)?
    Ok(())
"#;
    let dir = tempfile::tempdir().unwrap();
    let output = run(dir.path(), source, &options(UncheckedMode::Trap));
    assert_eq!(stdout(&output), "4\n");
}

#[test]
fn command_line_selects_the_mode() {
    let dir = tempfile::tempdir().unwrap();
    let overflow = dir.path().join("overflow.plenty");
    let valid = dir.path().join("valid.plenty");
    std::fs::write(&overflow, OVERFLOW).unwrap();
    std::fs::write(&valid, VALID).unwrap();
    let plentyc = |args: &[&str], path: &Path| {
        Command::new(plenty_bin())
            .args(args)
            .arg(path)
            .env("PLENTY_CACHE", "off")
            .output()
            .unwrap()
    };
    assert_traps(
        &plentyc(&["--unchecked", "trap"], &overflow),
        "integer overflow",
    );
    let assumed = plentyc(&["--unchecked", "assume"], &valid);
    assert_eq!(stdout(&assumed), "9223372036854775807\n");
    let invalid = plentyc(&["--unchecked", "wrap"], &valid);
    let stderr = String::from_utf8_lossy(&invalid.stderr);
    assert!(stderr.contains("`trap` or `assume`"), "{stderr}");
    let check = plentyc(&["--check", "--unchecked", "assume"], &valid);
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(stderr.contains("requires a compilation mode"), "{stderr}");
}

#[test]
fn assumed_arithmetic_keeps_cached_list_rows() {
    // An assumed `k += 1` must not stop the loop from caching `ps`'s length
    // and rows: each pass checks only its index against the cached length.
    let source = r#"
@copy
class P:
    x: f64
    y: f64
def f(ps: &mut list[P], out: &mut list[f64]) -> i64:
    n = len(ps)
    mut k = 0
    for i in range(n):
        out[0] = ps[i].x - ps[i].y
        unchecked:
            k += 1
    k
def main() -> Result[(), AllocError | IoError]:
    mut ps = [P(1.0, 2.0), P(5.0, 3.0)]?
    mut out = [0.0]?
    print(f(&mut ps, &mut out), out[0])?
    Ok(())
"#;
    let mut counts = Vec::new();
    for mode in [UncheckedMode::Trap, UncheckedMode::Assume] {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(stdout(&run(dir.path(), source, &options(mode))), "2 2.0\n");
        let clif = dump(source, mode, DumpStage::ClifOpt, "f", false);
        counts.push(clif.matches("icmp.i64 ult").count() + clif.matches("icmp ult").count());
    }
    assert_eq!(counts[0], counts[1], "{counts:?}");
}
