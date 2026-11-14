//! Per-program shared resources and human-readable multi-version provenance.
mod support;
use std::path::Path;
use std::process::{Command, Output};
use support::write_file as write;

fn command(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap()
}

fn succeeds(directory: &Path, args: &[&str]) -> String {
    let output = command(directory, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn fails(directory: &Path, args: &[&str], message: &str) -> String {
    let output = command(directory, args);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!output.status.success(), "{args:?} unexpectedly succeeded");
    assert!(
        stderr.contains(message),
        "expected {message:?}, got {stderr}"
    );
    stderr
}

fn package(directory: &Path, name: &str, version: &str, resources: &str, dependencies: &str) {
    write(
        &directory.join("plenty.toml"),
        &format!(
            "manifest-version = 1\n[package]\nname = \"{name}\"\nversion = \"{version}\"\nshared-resources = [{resources}]\n[dependencies]\n{dependencies}"
        ),
    );
    write(
        &directory.join("src/lib.plenty"),
        "pub def value() -> i64:\n    1\n",
    );
}

#[test]
fn shared_resource_conflict_reports_both_sources_and_paths() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    package(
        &root.join("one"),
        "first_wrapper",
        "1.0.0",
        "\"org.example.runtime\"",
        "",
    );
    package(
        &root.join("two"),
        "second_wrapper",
        "2.0.0",
        "\"org.example.runtime\"",
        "",
    );
    package(
        &root.join("app"),
        "app",
        "1.0.0",
        "",
        "left = { path = \"../one\" }\nright = { path = \"../two\" }\n",
    );
    let error = fails(
        &root.join("app"),
        &["check"],
        "shared-resource conflict for `org.example.runtime`",
    );
    for expected in [
        "first_wrapper 1.0.0",
        "second_wrapper 2.0.0",
        "app/left",
        "app/right",
        "identity:",
        "source:",
    ] {
        assert!(error.contains(expected), "missing {expected:?} in {error}");
    }
}

#[test]
fn shared_resource_same_instance_diamond_and_aliases_are_valid() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    package(
        &root.join("runtime"),
        "runtime",
        "1.0.0",
        "\"org.example.runtime\"",
        "",
    );
    package(
        &root.join("a"),
        "a",
        "1.0.0",
        "",
        "runtime = { path = \"../runtime\" }\n",
    );
    package(
        &root.join("b"),
        "b",
        "1.0.0",
        "",
        "different_alias = { path = \"../runtime\" }\n",
    );
    package(
        &root.join("app"),
        "app",
        "1.0.0",
        "",
        "a = { path = \"../a\" }\nb = { path = \"../b\" }\n",
    );
    succeeds(&root.join("app"), &["check"]);
    let why = succeeds(
        &root.join("app"),
        &["graph", "--why", "runtime", "--locked"],
    );
    assert_eq!(why.matches("\n  identity:").count(), 1, "{why}");
    assert!(why.contains("via: app/a/runtime"), "{why}");
    assert!(why.contains("via: app/b/different_alias"), "{why}");
}

#[test]
fn independent_workspace_closures_may_claim_different_instances() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        &root.join("plenty.toml"),
        "manifest-version = 1\n[workspace]\nmembers = [\"a\", \"b\"]\n",
    );
    package(
        &root.join("old"),
        "runtime",
        "1.0.0",
        "\"org.example.runtime\"",
        "",
    );
    package(
        &root.join("new"),
        "runtime",
        "2.0.0",
        "\"org.example.runtime\"",
        "",
    );
    package(
        &root.join("a"),
        "a",
        "1.0.0",
        "",
        "runtime = { path = \"../old\" }\n",
    );
    package(
        &root.join("b"),
        "b",
        "1.0.0",
        "",
        "runtime = { path = \"../new\" }\n",
    );
    succeeds(root, &["check"]);
    let duplicates = succeeds(root, &["graph", "--duplicates", "--locked"]);
    assert!(duplicates.contains("runtime 1.0.0"), "{duplicates}");
    assert!(duplicates.contains("runtime 2.0.0"), "{duplicates}");
    assert!(duplicates.contains("via: a/runtime"), "{duplicates}");
    assert!(duplicates.contains("via: b/runtime"), "{duplicates}");
}

#[test]
fn duplicate_inspection_distinguishes_equal_versions_and_why_can_select_alias_path() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    package(&root.join("old"), "c", "1.0.0", "", "");
    package(&root.join("new"), "c", "1.0.0", "", "");
    package(
        &root.join("a"),
        "a",
        "1.0.0",
        "",
        "c = { path = \"../old\" }\n",
    );
    package(
        &root.join("b"),
        "b",
        "1.0.0",
        "",
        "c = { path = \"../new\" }\n",
    );
    package(
        &root.join("app"),
        "app",
        "1.0.0",
        "",
        "a = { path = \"../a\" }\nb = { path = \"../b\" }\n",
    );
    let app = root.join("app");
    let duplicates = succeeds(&app, &["graph", "--duplicates"]);
    assert_eq!(duplicates.matches("c 1.0.0\n").count(), 2, "{duplicates}");
    assert!(duplicates.contains("via: app/a/c"), "{duplicates}");
    assert!(duplicates.contains("via: app/b/c"), "{duplicates}");
    assert!(duplicates.contains("old"), "{duplicates}");
    assert!(duplicates.contains("new"), "{duplicates}");
    let why = succeeds(&app, &["graph", "--why", "a/c", "--locked"]);
    assert_eq!(why.matches("\n  identity:").count(), 1, "{why}");
    assert!(why.contains("via: app/a/c"), "{why}");
    assert!(!why.contains("via: app/b/c"), "{why}");
    let identity = why
        .lines()
        .find_map(|line| line.strip_prefix("  identity: "))
        .unwrap();
    assert_eq!(
        succeeds(&app, &["graph", "--why", identity, "--locked"]),
        why
    );
    fails(
        &app,
        &["graph", "--why", "absent"],
        "no package matches `absent`",
    );
    fails(&app, &["graph", "--why"], "--why requires");
    fails(
        &app,
        &["graph", "--why", "c", "--duplicates"],
        "choose either",
    );
    let machine = succeeds(&app, &["graph", "--locked"]);
    assert!(machine.contains("graph-version = "), "{machine}");
    assert!(!machine.contains("via:"), "{machine}");
}

#[test]
fn shared_resource_keys_are_explicit_validated_metadata() {
    let temp = tempfile::tempdir().unwrap();
    for (resources, expected) in [
        ("\"\"", "a shared-resource key must contain"),
        ("\"not a stable key\"", "a shared-resource key must contain"),
        (
            "\"org.example.runtime\", \"org.example.runtime\"",
            "duplicate shared-resource key",
        ),
        ("42", "must be an array of resource keys"),
    ] {
        package(temp.path(), "metadata", "1.0.0", resources, "");
        fails(temp.path(), &["check"], expected);
    }
    package(
        temp.path(),
        "metadata",
        "1.0.0",
        "\"org.example.runtime\", \"native/engine_v1\"",
        "",
    );
    succeeds(temp.path(), &["check"]);
    assert_eq!(
        succeeds(temp.path(), &["graph", "--duplicates"]),
        "No duplicate package names in the selected dependency graph.\n"
    );
}
