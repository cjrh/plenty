//! Executables use the runtime's Rust panic handler instead of std's, so they
//! do not link std's panic hook or backtrace symbolizer (issue #503).
#![cfg(target_os = "linux")]

mod support;

use object::{Object, ObjectSymbol, SymbolKind};
use std::os::unix::process::ExitStatusExt;
use std::process::Command;

const SIGABRT: i32 = 6;

#[test]
fn hello_links_no_backtrace_symbolizer() {
    // Unstripped, so the symbol check below sees every linked function.
    let (_dir, executable) = support::build_unstripped(
        "def main() -> Result[(), AllocError | IoError]:\n    print(\"hello\")?\n    Ok(())\n",
    );
    let bytes = std::fs::read(&executable).unwrap();
    // GNU ld can retain unused "panicked at" literals in a merged string
    // section even after discarding the hook. Check its backtrace control text
    // and linked functions instead of treating every panic literal as code.
    let text = "RUST_BACKTRACE";
    assert!(
        !bytes.windows(text.len()).any(|w| w == text.as_bytes()),
        "found {text:?}"
    );
    // Generic functions folded with symbolizer instances may keep their names,
    // so check for crates whose code has no such aliases.
    let file = object::File::parse(&*bytes).unwrap();
    let linked: Vec<_> = file
        .symbols()
        .filter(|symbol| symbol.kind() == SymbolKind::Text)
        .filter_map(|symbol| symbol.name().ok())
        .filter(|name| {
            [
                "rustc_demangle",
                "miniz_oxide",
                "default_hook",
                "panic_with_hook",
            ]
            .iter()
            .any(|part| name.contains(part))
        })
        .collect();
    assert!(linked.is_empty(), "{linked:?}");
    let panic_entry = |part: &str| {
        file.symbols()
            .find(|symbol| {
                symbol.kind() == SymbolKind::Text
                    && symbol.name().is_ok_and(|name| name.contains(part))
            })
            .unwrap_or_else(|| panic!("missing panic entry {part}"))
            .address()
    };
    assert_eq!(
        panic_entry("rust_begin_unwind"),
        panic_entry("plenty_rust_panic"),
        "Rust panics must enter the runtime handler"
    );
    let output = Command::new(&executable).output().unwrap();
    assert!(output.status.success(), "{:?}", output.status);
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
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
