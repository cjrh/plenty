//! The opt-in compiler phase profile (`--features profile`, issue #150).
//! `cargo test --all-features` builds the feature, so these tests run there.
#![cfg(feature = "profile")]

use std::path::Path;
use std::process::Command;

const PROGRAM: &str = "def main() -> ():\n    pass\n";

fn emit_object(directory: &Path, profile: bool) {
    let source = directory.join("main.plenty");
    std::fs::write(&source, PROGRAM).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_plenty"));
    command
        .arg("--emit-object")
        .arg(&source)
        .arg("-o")
        .arg(directory.join("main.o"))
        .env("PLENTY_CACHE", "off")
        .env("HOTPATH_OUTPUT_FORMAT", "json")
        .env("HOTPATH_OUTPUT_PATH", directory.join("report.json"))
        .env_remove("PLENTY_PROFILE");
    if profile {
        command.env("PLENTY_PROFILE", "1");
    }
    let output = command.output().unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn profile_reports_compiler_phases_when_requested() {
    let directory = tempfile::tempdir().unwrap();
    emit_object(directory.path(), true);
    let report = std::fs::read_to_string(directory.path().join("report.json")).unwrap();
    for phase in [
        "modules::parse",
        "frontend::lower_function",
        "ownership::check",
        "codegen::object_module",
        "output::compile",
    ] {
        assert!(report.contains(phase), "{phase} missing from {report}");
    }
}

#[test]
fn profile_records_nothing_unless_requested() {
    let directory = tempfile::tempdir().unwrap();
    emit_object(directory.path(), false);
    assert!(!directory.path().join("report.json").exists());
}
