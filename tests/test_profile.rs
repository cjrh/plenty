//! The opt-in compiler phase profile (`--features profile`, issue #150).
//! `cargo test --all-features` builds the feature, so these tests run there.
#![cfg(feature = "profile")]

use std::path::Path;
use std::process::Command;

const PROGRAM: &str = "def main() -> ():\n    pass\n";

fn emit_object(directory: &Path, profile: bool, program: &str) {
    let source = directory.join("main.plenty");
    std::fs::write(&source, program).unwrap();
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
    emit_object(directory.path(), true, PROGRAM);
    let report = std::fs::read_to_string(directory.path().join("report.json")).unwrap();
    for phase in [
        "modules::parse",
        "frontend::lower_function",
        "ownership::check",
        "codegen::object_module",
        "output::compile",
        "output::cranelift_compile",
        "output::collect_code",
        "output::clear_context",
        "frontend::setup_function",
        "frontend::lower_function_body",
        "frontend::finish_function",
        "generics::expand_data",
        "tail_calls::validate",
        "ownership::prepare_futures",
        "ownership::initialize_state",
        "ownership::check_moves",
        "ownership::check_loans",
    ] {
        assert!(report.contains(phase), "{phase} missing from {report}");
    }
}

#[test]
fn profile_records_nothing_unless_requested() {
    let directory = tempfile::tempdir().unwrap();
    emit_object(directory.path(), false, PROGRAM);
    assert!(!directory.path().join("report.json").exists());
}

#[test]
fn ownership_subphases_are_measured_once_per_function_not_per_recursive_visit() {
    let directory = tempfile::tempdir().unwrap();
    emit_object(
        directory.path(),
        true,
        "\
def read(value: &i64) -> i64:
    *value

def main() -> i32:
    mut value = 1
    while value < 3:
        view = &value
        observed = read(view)
        value = observed + 1
    0i32
",
    );
    let report: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(directory.path().join("report.json")).unwrap(),
    )
    .unwrap();
    let rows = report["functions_timing"]["data"].as_array().unwrap();
    let calls = |phase: &str| {
        rows.iter()
            .find(|row| {
                row["name"]
                    .as_str()
                    .is_some_and(|name| name.ends_with(phase))
            })
            .unwrap_or_else(|| panic!("missing {phase}: {report}"))["calls"]
            .as_u64()
            .unwrap()
    };
    let checked = calls("ownership::check");
    assert!(checked > 0);
    for phase in [
        "ownership::prepare_futures",
        "ownership::initialize_state",
        "ownership::check_moves",
        "ownership::check_loans",
    ] {
        assert_eq!(calls(phase), checked, "{phase}");
    }
    let graphs = calls("ownership::loan_graph");
    assert!(graphs > 0 && graphs <= checked);
    assert_eq!(calls("ownership::loan_liveness"), graphs);
    assert_eq!(calls("ownership::validate_loans"), graphs);
    let compiled = calls("output::compile");
    for phase in [
        "output::cranelift_compile",
        "output::collect_code",
        "output::clear_context",
    ] {
        assert_eq!(calls(phase), compiled, "{phase}");
    }
}
