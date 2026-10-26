//! Executables use the runtime's Rust panic handler instead of std's, so they
//! do not link std's panic hook or backtrace symbolizer (issue #503).
#![cfg(target_os = "linux")]

use object::{Object, ObjectSymbol, SymbolKind};
use std::os::unix::process::ExitStatusExt;
use std::process::Command;

const SIGABRT: i32 = 6;

#[test]
fn hello_links_no_backtrace_symbolizer() {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("hello");
    plenty::compile_source_to_executable(
        "def main() -> Result[(), AllocError | IoError]:\n    print(\"hello\")?\n    Ok(())\n",
        &executable,
    )
    .unwrap();
    let bytes = std::fs::read(&executable).unwrap();
    // std's default panic hook text, present even in a stripped executable.
    for text in ["RUST_BACKTRACE", "panicked at"] {
        assert!(
            !bytes.windows(text.len()).any(|w| w == text.as_bytes()),
            "found {text:?}"
        );
    }
    // Generic functions folded with symbolizer instances may keep their names,
    // so check for crates whose code has no such aliases.
    let file = object::File::parse(&*bytes).unwrap();
    let linked: Vec<_> = file
        .symbols()
        .filter(|symbol| symbol.kind() == SymbolKind::Text)
        .filter_map(|symbol| symbol.name().ok())
        .filter(|name| {
            ["rustc_demangle", "miniz_oxide", "default_hook"]
                .iter()
                .any(|part| name.contains(part))
        })
        .collect();
    assert!(linked.is_empty(), "{linked:?}");
    let output = Command::new(&executable).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hello\n");
}

/// No Plenty program should reach a Rust panic, so the probe calls the
/// runtime's float-math helper with an invalid operation through an alias.
#[test]
fn runtime_panic_reports_and_aborts() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path();
    std::fs::write(
        root.join("native.plentyi"),
        "pub extern def probe(kind: u64, x: f64, y: f64) -> f64 = \"panic_probe\"\n",
    )
    .unwrap();
    let source = root.join("main.plenty");
    std::fs::write(
        &source,
        "from native import probe\n\ndef main() -> Result[(), AllocError | IoError]:\n    print(probe(99, 1.0, 2.0))?\n    Ok(())\n",
    )
    .unwrap();
    let executable = root.join("program");
    let options = plenty::CompileOptions {
        link_args: vec!["-Wl,--defsym=panic_probe=plenty_f64_math".into()],
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&source, &executable, None, &options).unwrap();
    let output = Command::new("sh")
        .args(["-c", "ulimit -c 0; exec \"$1\"", "panic-test"])
        .arg(&executable)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("error: internal runtime panic at plenty-runtime/src/math.rs:")
            && stderr
                .ends_with(": internal error: entered unreachable code: invalid float operation\n"),
        "{stderr}"
    );
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert_eq!(output.status.signal(), Some(SIGABRT), "{:?}", output.status);
}
