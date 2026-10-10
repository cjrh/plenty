//! Running out of native stack prints `error: stack overflow` and aborts, on
//! the main thread and on every thread the runtime starts (issue 2). Other
//! faults keep their default action.
#![cfg(target_os = "linux")]

mod support;

use rstest::rstest;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Output};

const SIGABRT: i32 = 6;
const SIGSEGV: i32 = 11;
const REPORT: &str = "error: stack overflow\n";

const DEPTH: &str = r#"
def depth(n: i64) -> i64:
    if n == 0:
        return 0
    1 + depth(n - 1)
"#;

/// Each `depth` frame holds twenty 800-byte values, so it spans many pages.
fn wide_depth() -> String {
    let fields: String = (0..100).map(|i| format!("    f{i}: i64\n")).collect();
    let arguments = vec!["n"; 100].join(", ");
    let locals: String = (0..20)
        .map(|i| format!("    a{i} = Wide({arguments})\n"))
        .collect();
    let sum = (0..20)
        .map(|i| format!("a{i}.f{i}"))
        .collect::<Vec<_>>()
        .join(" + ");
    format!(
        "class Wide:\n{fields}\ndef depth(n: i64) -> i64:\n    if n == 0:\n        return 0\n{locals}    below = depth(n - 1)\n    below + {sum}\n"
    )
}

fn on_main(depth: i64) -> String {
    format!("def main() -> Result[(), Failure]:\n    print(depth({depth}))?\n    Ok(())\n")
}

fn on_scoped_worker(depth: i64) -> String {
    format!(
        "def main() -> Result[(), Failure]:\n    with spawn(depth, {depth})? as worker:\n        print(worker.join())?\n    Ok(())\n"
    )
}

fn on_pool_worker(depth: i64) -> String {
    format!(
        "def main() -> Result[(), Failure]:\n    with ThreadPoolExecutor(2, 4)? as pool:\n        print(pool.submit(depth, {depth})?.result()?)?\n    Ok(())\n"
    )
}

/// Run without a core file, optionally with a stack limit in KiB. The limit
/// also sizes the stacks of threads the program starts.
fn execute(executable: &Path, stack_kib: Option<u32>) -> Output {
    let limit = stack_kib.map_or(String::new(), |kib| format!("ulimit -s {kib}; "));
    Command::new("sh")
        .args([
            "-c",
            &format!("ulimit -c 0; {limit}exec \"$1\""),
            "stack-overflow-test",
        ])
        .arg(executable)
        .output()
        .unwrap()
}

fn run(source: &str, stack_kib: Option<u32>) -> Output {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("program");
    support::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    execute(&executable, stack_kib)
}

fn assert_reported(output: &Output) {
    assert_eq!(String::from_utf8_lossy(&output.stderr), REPORT);
    assert_eq!(output.status.signal(), Some(SIGABRT), "{:?}", output.status);
}

#[test]
fn recursion_over_a_long_chain_reports_on_the_main_thread() {
    let output = run(
        r#"
class Node:
    value: i64
    next: Option[Box[Node]]

def length(node: &Node) -> i64:
    match &node.next:
        case Some(next):
            1 + length(next)
        case Nothing:
            1

def main() -> Result[(), Failure]:
    mut head = Node(1, Nothing)
    for value in range(2, 1000001):
        head = Node(value, Some(Box(head)?))
    print("built")?
    print(length(&head))?
    Ok(())
"#,
        None,
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "built\n");
    assert_reported(&output);
}

#[rstest]
fn every_thread_reports_exhaustion(
    #[values(on_main, on_scoped_worker, on_pool_worker)] thread: fn(i64) -> String,
    #[values(None, Some(256))] stack_kib: Option<u32>,
) {
    let output = run(&format!("{DEPTH}{}", thread(100_000_000)), stack_kib);
    assert_eq!(output.stdout, b"");
    assert_reported(&output);
}

/// A frame wider than a page must still fault inside the stack: it probes each
/// page, so it cannot step over a worker's one-page guard.
#[rstest]
fn frames_wider_than_a_page_report_exhaustion(
    #[values(on_main, on_scoped_worker, on_pool_worker)] thread: fn(i64) -> String,
    #[values(None, Some(256))] stack_kib: Option<u32>,
) {
    let output = run(&format!("{}{}", wide_depth(), thread(1_000_000)), stack_kib);
    assert_reported(&output);
}

#[rstest]
fn recursion_that_fits_is_unaffected(
    #[values(on_main, on_scoped_worker, on_pool_worker)] thread: fn(i64) -> String,
    #[values((None, 20_000), (Some(256), 1_000))] limit: (Option<u32>, i64),
) {
    let (stack_kib, depth) = limit;
    let output = run(&format!("{DEPTH}{}", thread(depth)), stack_kib);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{depth}\n")
    );
}

/// A fault that is not stack exhaustion, and a `SIGSEGV` sent to the process,
/// both end the program as they would without the handler.
#[rstest]
#[case::null_read("print(native.measure(native.Text.null()))?")]
#[case::sent_signal("print(native.send(11))?")]
fn other_segmentation_faults_keep_the_default_action(#[case] fault: &str) {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(
        workspace.path().join("native.plentyi"),
        "pub opaque Text\npub extern def measure(text: Text) -> u64 = \"strlen\"\npub extern def send(signal: i32) -> i32 = \"raise\"\n",
    )
    .unwrap();
    let path = workspace.path().join("main.plenty");
    std::fs::write(
        &path,
        format!(
            "import native\ndef main() -> Result[(), Failure]:\n    print(\"ready\")?\n    {fault}\n    Ok(())\n"
        ),
    )
    .unwrap();
    let executable = workspace.path().join("program");
    plenty::compile_file_to_executable_with_options(
        &path,
        &executable,
        None,
        &plenty::CompileOptions::default(),
    )
    .unwrap();
    let output = execute(&executable, None);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ready\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    assert_eq!(output.status.signal(), Some(SIGSEGV), "{:?}", output.status);
}
