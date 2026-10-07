//! Package a target-specific native runtime once when building the compiler.
//! Invoke rustc directly on the dependency-free runtime crate: invoking Cargo
//! recursively from a build script would contend for Cargo's package locks.
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=plenty-runtime/src");
    println!("cargo:rerun-if-changed=plenty-runtime/Cargo.toml");
    println!("cargo:rerun-if-env-changed=PLENTY_RUNTIME_RUSTFLAGS");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    println!(
        "cargo:rustc-env=PLENTY_RUNTIME_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
    build_runtime(&out, true);
    build_runtime(&out, false);
}

fn build_runtime(out: &std::path::Path, application: bool) {
    let archive = out.join(if application {
        "libplenty_runtime.a"
    } else {
        "libplenty_library_runtime.a"
    });
    let mut rustc = Command::new(std::env::var_os("RUSTC").unwrap());
    rustc.args([
        "plenty-runtime/src/lib.rs",
        "--crate-name",
        "plenty_runtime",
        "--crate-type",
        "staticlib",
        "--edition=2021",
        "--target",
        &std::env::var("TARGET").unwrap(),
        "-Copt-level=2",
        "-Cpanic=abort",
        "-Clto=thin",
        "-Ccodegen-units=1",
        "--check-cfg",
        "cfg(plenty_runtime_embedded)",
        "--check-cfg",
        "cfg(feature, values(\"allocation-checks\"))",
        "--print",
        "native-static-libs",
        "-o",
    ]);
    rustc.arg(&archive);
    if application {
        rustc.args(["--cfg", "plenty_runtime_embedded"]);
    }
    if std::env::var_os("CARGO_FEATURE_RUNTIME_CHECKS").is_some() {
        rustc.args(["--cfg", "feature=\"allocation-checks\""]);
    }
    // Separate, explicit flags allow runtime sanitizer builds without silently
    // leaking compiler-only flags (coverage/link arguments/metadata) into an ABI library.
    if let Ok(flags) = std::env::var("PLENTY_RUNTIME_RUSTFLAGS") {
        rustc.args(flags.split_whitespace());
    }
    let output = rustc
        .output()
        .expect("invoke rustc to build plenty-runtime");
    let diagnostics = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "building plenty-runtime failed:\n{diagnostics}"
    );
    let libs = diagnostics
        .lines()
        .find_map(|line| line.strip_prefix("note: native-static-libs: "))
        .expect("rustc must report the runtime's native link dependencies");
    std::fs::write(
        out.join(if application {
            "runtime-link-args.txt"
        } else {
            "library-runtime-link-args.txt"
        }),
        libs,
    )
    .unwrap();
}
