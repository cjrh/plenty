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
    let output = Command::new(target)
        .arg("-seed=1")
        .arg(format!("-runs={runs}"))
        .arg(format!("-artifact_prefix={}/", findings.display()))
        .arg(&corpus)
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&output.stderr);
    let found: Vec<_> = std::fs::read_dir(&findings)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    match found.as_slice() {
        [] => {
            assert!(output.status.success(), "{log}");
            None
        }
        [crash] => {
            assert_eq!(output.status.code(), Some(77), "{log}");
            Some(std::fs::read(crash).unwrap())
        }
        _ => panic!("several findings: {found:?}\n{log}"),
    }
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

def main() -> Result[(), Failure]:
    mut data = std.bytes.empty()?
    mut i = 0u64
    while i < fuzz_input.size():
        data.push(fuzz_input.byte(i))?
        i += 1u64
    match data.to_text():
        case Ok(text):
            if text == "Plenty fuzzing finds strings":
                return Err(Failure.Unspecified)
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
/// from the qualification corpus, finds nothing. A finding left by an earlier
/// run is not reported again (issue #296).
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
    assert!(log.contains("Done 20000 runs"), "{log}");
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
    assert!(Command::new("cc")
        .args(["-c", "-o"])
        .arg(&object)
        .arg(&stub)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("ar")
        .arg("rcs")
        .arg(&archive)
        .arg(&object)
        .status()
        .unwrap()
        .success());
    let target = build(
        dir.path(),
        r#"import fuzz_input

def main() -> Result[(), Failure]:
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
