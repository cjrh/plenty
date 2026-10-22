//! Coverage-guided fuzzing builds, `plenty --fuzz`: block counters and PC
//! tables in the libFuzzer layout, the driver that runs `main` once per
//! input, and short libFuzzer runs over std.http.server. See
//! book/src/design/toolchain/01-building/fuzzing.md.
//!
//! The libFuzzer runs need clang's compiler-rt. Without it they print why and
//! pass, unless `PLENTY_FUZZ_TESTS=1` requires them. The stub test needs only
//! `cc` and `ar`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const INPUT: &str = "pub extern def size() -> u64 = \"fuzz_input_size\"\n\
                     pub extern def byte(at: u64) -> u8 = \"fuzz_input_byte\"\n";

/// clang's libFuzzer, or `None` with a note when it is not installed.
fn libfuzzer() -> Option<PathBuf> {
    match plenty::find_libfuzzer() {
        Ok(path) => Some(path),
        Err(error) if std::env::var_os("PLENTY_FUZZ_TESTS").is_none() => {
            eprintln!("skipping libFuzzer run: {error}");
            None
        }
        Err(error) => panic!("PLENTY_FUZZ_TESTS is set: {error}"),
    }
}

fn plenty(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args(args)
        .output()
        .unwrap()
}

/// Exec a small interpreter, then spawn the target from its small address
/// space. On Linux, exec preserves peak RSS: directly launching libFuzzer
/// from the integration harness can charge it for other tests' memory.
/// Python is already required by the HTTP fuzz test. Do not replace the
/// subprocess call with exec, which would preserve the inherited peak again.
fn isolated_target(target: &Path) -> Command {
    let mut command = Command::new("python3");
    command.args([
        "-B",
        "-c",
        "import subprocess, sys; code = subprocess.call(sys.argv[1:]); \
         sys.exit(code if code >= 0 else 128 - code)",
    ]);
    command.arg(target);
    command
}

/// `source` as `main.plenty` beside the input module, built with `--fuzz`
/// and `libfuzzer` into `dir/target`.
fn build(dir: &Path, source: &str, libfuzzer: &Path) -> PathBuf {
    std::fs::write(dir.join("fuzz_input.plentyi"), INPUT).unwrap();
    std::fs::write(dir.join("main.plenty"), source).unwrap();
    let target = dir.join("target");
    let built = plenty(&[
        "--fuzz",
        dir.join("main.plenty").to_str().unwrap(),
        "-o",
        target.to_str().unwrap(),
        "--libfuzzer",
        libfuzzer.to_str().unwrap(),
    ]);
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    target
}

/// Runs `target` with a fixed seed until it finds a crash or has run
/// `runs` inputs, and returns the saved crash input, if any.
fn fuzz(dir: &Path, target: &Path, runs: u32) -> Option<Vec<u8>> {
    let findings = dir.join("findings");
    std::fs::create_dir_all(&findings).unwrap();
    let corpus = dir.join("corpus");
    std::fs::create_dir_all(&corpus).unwrap();
    let output = isolated_target(target)
        .arg("-seed=1")
        .arg("-rss_limit_mb=2048")
        .arg(format!("-runs={runs}"))
        .arg(format!("-artifact_prefix={}/", findings.display()))
        .arg(&corpus)
        .output()
        .unwrap();
    let found: Vec<_> = std::fs::read_dir(&findings)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    crash_input(&output, &found).unwrap_or_else(|error| panic!("{error}"))
}

/// Resource limits and unexpected exits must fail the test, even if they
/// saved an input. Only a crash artifact with the semantic exit code counts.
fn crash_input(output: &Output, found: &[PathBuf]) -> Result<Option<Vec<u8>>, String> {
    let log = String::from_utf8_lossy(&output.stderr);
    let resource_limit = matches!(output.status.code(), Some(70 | 71))
        || found.iter().any(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.starts_with("oom-") || name.starts_with("timeout-")
        });
    if resource_limit {
        return Err(format!(
            "libFuzzer resource-limit failure: {}; artifacts: {found:?}\n{log}",
            output.status
        ));
    }
    match found {
        [] if output.status.success() => Ok(None),
        [crash]
            if output.status.code() == Some(77)
                && crash
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("crash-") =>
        {
            Ok(Some(std::fs::read(crash).unwrap()))
        }
        _ => Err(format!(
            "unexpected libFuzzer result: {}; artifacts: {found:?}\n{log}",
            output.status
        )),
    }
}

/// Reproduce the high peak in a separate process, keeping the concurrent
/// integration harness small. Exec leaves the peak behind after freeing.
#[cfg(target_os = "linux")]
fn after_high_peak(command: &Command) -> Output {
    Command::new("python3")
        .args([
            "-B",
            "-c",
            "import os, sys; data = bytearray(96 * 1024 * 1024); del data; \
             os.execvp(sys.argv[1], sys.argv[1:])",
        ])
        .arg(command.get_program())
        .args(command.get_args())
        .output()
        .unwrap()
}

#[test]
#[cfg(target_os = "linux")]
fn launcher_resets_peak_rss_and_forwards_arguments_output_and_status() {
    let probe = "import resource, sys; \
                 print(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss); \
                 print(sys.argv[1], file=sys.stderr); sys.exit(77)";
    let mut direct = Command::new("python3");
    direct.args(["-B", "-c", probe, "one argument with spaces"]);
    let inherited = after_high_peak(&direct);
    let peak = |output: &Output| {
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u64>()
            .unwrap()
    };
    assert_eq!(inherited.status.code(), Some(77), "{inherited:?}");
    assert!(peak(&inherited) >= 96 * 1024, "{inherited:?}");

    let mut isolated = isolated_target(Path::new("python3"));
    isolated.args(direct.get_args());
    let fresh = after_high_peak(&isolated);
    assert_eq!(fresh.status.code(), Some(77), "{fresh:?}");
    assert!(peak(&fresh) < 64 * 1024, "{fresh:?}");
    assert_eq!(fresh.stderr, b"one argument with spaces\n");
}

#[test]
#[cfg(target_os = "linux")]
fn libfuzzer_starts_below_launcher_peak_and_still_reports_oom() {
    let Some(libfuzzer) = libfuzzer() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let target = build(dir.path(), "def main() -> ():\n    pass\n", &libfuzzer);
    let mut command = isolated_target(&target);
    command.args(["-runs=2", "-rss_limit_mb=64", "-print_final_stats=1"]);
    let output = after_high_peak(&command);
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{log}");
    let peak = log
        .lines()
        .find_map(|line| line.strip_prefix("stat::peak_rss_mb:").map(str::trim))
        .unwrap_or_else(|| panic!("no peak RSS statistic\n{log}"))
        .parse::<u64>()
        .unwrap();
    assert!(peak < 64, "{log}");

    // The target's own footprint exceeds 1 MiB. Run long enough for the
    // libFuzzer RSS monitor to fire; do not disable limits or retry failures.
    let findings = dir.path().join("findings");
    std::fs::create_dir(&findings).unwrap();
    let output = isolated_target(&target)
        .args(["-rss_limit_mb=1", "-runs=1000000000", "-max_total_time=5"])
        .arg(format!("-artifact_prefix={}/", findings.display()))
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(71), "{log}");
    let found: Vec<_> = std::fs::read_dir(&findings)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(
        found.iter().any(|path| path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("oom-")),
        "{found:?}\n{log}"
    );
    let error = crash_input(&output, &found).unwrap_err();
    assert!(error.contains("resource-limit failure"), "{error}");
    assert!(error.contains("oom-"), "{error}");
    assert!(error.contains("out-of-memory"), "{error}");
}

/// Six nested byte tests guard a trap. Random inputs would need about 2^48
/// tries; counting blocks makes each matched byte a new input to build on.
#[test]
fn block_coverage_reaches_a_nested_trap() {
    let Some(libfuzzer) = libfuzzer() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let target = build(
        dir.path(),
        r#"import fuzz_input

def at(i: u64) -> u8:
    fuzz_input.byte(i)

def main() -> ():
    if fuzz_input.size() >= 6u64 and at(0u64) == 80u8:
        if at(1u64) == 108u8:
            if at(2u64) == 101u8:
                if at(3u64) == 110u8:
                    if at(4u64) == 116u8:
                        if at(5u64) == 121u8:
                            empty: Option[i64] = Nothing
                            value = empty.unwrap()
"#,
        &libfuzzer,
    );
    let crash = fuzz(dir.path(), &target, 5_000_000).expect("no crash found");
    assert!(crash.starts_with(b"Plenty"), "{crash:?}");
}

/// String equality reaches `memcmp`, whose interceptor tells libFuzzer the
/// string compared against, and `main` returning `Err` is a finding.
#[test]
fn compared_strings_reach_an_error() {
    let Some(libfuzzer) = libfuzzer() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let target = build(
        dir.path(),
        r#"import fuzz_input
import std.bytes

def main() -> Result[(), AllocError | std.bytes.BytesError]:
    mut data = std.bytes.empty()?
    mut i = 0u64
    while i < fuzz_input.size():
        data.push(fuzz_input.byte(i))?
        i += 1u64
    match data.to_text():
        case Ok(text):
            if text == "Plenty fuzzing finds strings":
                return Err(AllocError.OutOfMemory)
        case Err(error):
            pass
    Ok(())
"#,
        &libfuzzer,
    );
    let crash = fuzz(dir.path(), &target, 2_000_000).expect("no crash found");
    assert_eq!(crash, b"Plenty fuzzing finds strings");
}

/// Exhausting the stack is a finding too: the driver moves libFuzzer's
/// handler to an alternate stack, so it saves the input.
#[test]
fn stack_exhaustion_is_reported() {
    let Some(libfuzzer) = libfuzzer() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let target = build(
        dir.path(),
        r#"import fuzz_input

def down(n: i64) -> i64:
    if n == 0:
        return 0
    1 + down(n - 1)

def main() -> ():
    if fuzz_input.size() > 0u64 and fuzz_input.byte(0u64) == 33u8:
        depth = down(100000000)
"#,
        &libfuzzer,
    );
    let crash = fuzz(dir.path(), &target, 100_000).expect("no crash found");
    assert_eq!(crash.first(), Some(&b'!'));
}

/// A short run of the std.http.server target in fuzz/http_server, seeded
/// from the qualification corpus, completes and finds nothing. A finding left
/// by an earlier run is not reported again (issue #296).
#[test]
fn http_server_target_survives_a_short_run() {
    if libfuzzer().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("findings")).unwrap();
    std::fs::write(dir.path().join("findings/crash-earlier"), b"old").unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("python3")
        .arg("-B")
        .arg(root.join("fuzz/http_server/run.py"))
        .args(["--plenty", env!("CARGO_BIN_EXE_plenty")])
        .arg("--work")
        .arg(dir.path())
        .args(["--runs", "20000", "--seed", "1", "--debug-build"])
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}\n{log}");
    // At least, not exactly: every `-reload` interval libFuzzer reruns output
    // corpus files missing from its in-memory corpus, such as a reduced input
    // that a later reduction replaced, without checking `-runs` (issue #392).
    let runs: u32 = log
        .split("\nDone ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("no run count\n{log}"));
    assert!(runs >= 20000, "{log}");
    assert!(!stdout.contains("finding:"), "{stdout}");
    assert!(stdout.contains("1 earlier findings"), "{stdout}");
}

/// libFuzzer's entry points, recording what the driver hands them: it runs
/// the target on two inputs and reports the counters and PC entries.
const STUB: &str = r#"#include <stdint.h>
#include <stdio.h>
#include <string.h>

static uint8_t *counters, *counters_end;
static const uintptr_t *pcs, *pcs_end;

void __sanitizer_cov_8bit_counters_init(uint8_t *start, uint8_t *stop) {
    counters = start;
    counters_end = stop;
}

void __sanitizer_cov_pcs_init(const uintptr_t *start, const uintptr_t *stop) {
    pcs = start;
    pcs_end = stop;
}

static size_t hit(void) {
    size_t count = 0;
    for (uint8_t *at = counters; at < counters_end; at++) count += *at != 0;
    return count;
}

int LLVMFuzzerRunDriver(int *argc, char ***argv, int (*one)(const uint8_t *, size_t)) {
    size_t functions = 0;
    for (const uintptr_t *at = pcs; at < pcs_end; at += 2) functions += at[1] & 1;
    printf("counters=%zu pcs=%zu functions=%zu\n", (size_t)(counters_end - counters),
           (size_t)(pcs_end - pcs) / 2, functions);
    fflush(stdout);
    one((const uint8_t *)"no", 2);
    size_t first = hit();
    one((const uint8_t *)"yes", 3);
    printf("first=%zu second=%zu\n", first, hit());
    fflush(stdout);
    return 0;
}
"#;

/// Without libFuzzer: every function's counters and PC entries reach the
/// driver, in matching numbers, and a branch taken only by the second input
/// adds counters.
#[test]
fn counters_and_pc_tables_reach_the_driver() {
    let dir = tempfile::tempdir().unwrap();
    let stub = dir.path().join("stub.c");
    std::fs::write(&stub, STUB).unwrap();
    let object = dir.path().join("stub.o");
    let archive = dir.path().join("libstub.a");
    assert!(
        Command::new("cc")
            .args(["-c", "-o"])
            .arg(&object)
            .arg(&stub)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("ar")
            .arg("rcs")
            .arg(&archive)
            .arg(&object)
            .status()
            .unwrap()
            .success()
    );
    let target = build(
        dir.path(),
        r#"import fuzz_input

def main() -> Result[(), IoError]:
    if fuzz_input.size() == 3u64:
        print("three bytes")?
    print(fuzz_input.byte(0u64))?
    Ok(())
"#,
        &archive,
    );
    let output = Command::new(&target).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    let mut lines = text.lines();
    let totals: Vec<usize> = numbers(lines.next().unwrap());
    let (counters, pcs, functions) = (totals[0], totals[1], totals[2]);
    assert_eq!(counters, pcs, "{text}");
    assert!(functions > 1 && counters > functions, "{text}");
    assert_eq!(lines.next(), Some("110"), "{text}");
    assert_eq!(lines.next(), Some("three bytes"), "{text}");
    assert_eq!(lines.next(), Some("121"), "{text}");
    let hits = numbers(lines.next().unwrap());
    assert!(hits[0] > 0 && hits[1] > hits[0], "{text}");
}

/// The `name=N` values of `line`, in order.
fn numbers(line: &str) -> Vec<usize> {
    line.split(' ')
        .map(|pair| pair.split_once('=').unwrap().1.parse().unwrap())
        .collect()
}

#[test]
fn libfuzzer_flag_requires_fuzz() {
    let output = plenty(&["--compile", "x.plenty", "-o", "x", "--libfuzzer", "lib.a"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--libfuzzer requires --fuzz"));
}
