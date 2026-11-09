//! Experiment inputs and generated programs type-check with the current
//! compiler (issue #918).
//!
//! This is a syntax and contract check, not a rerun of recorded measurements
//! or foreign-library builds; each experiment's runner covers those.
use std::path::{Path, PathBuf};

mod support;

/// Stored inputs that must fail, with the diagnostic they demonstrate.
const REJECTED: &[(&str, &str)] = &[(
    "columnar-list/reject-row-reference.plenty",
    "references cannot be stored in class fields",
)];

/// Stored inputs checked only through a generated program.
const GENERATED_ONLY: &[&str] = &[
    // Its runner supplies the selected foreign API stubs; see `rust_crates`
    // in `tests/fixtures/experiments/write_generated.py`.
    "rust-crates/workload.plenty",
];

fn experiments() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("experiments")
}

/// Every program `write_generated.py` writes, relative to its output.
const GENERATED: &[&str] = &[
    "base/caller.plenty",
    "base/m0.plenty",
    "base/m1.plenty",
    "base/main.plenty",
    "body/caller.plenty",
    "body/m0.plenty",
    "body/m1.plenty",
    "body/main.plenty",
    "columns.plenty",
    "columns_False_False.plenty",
    "columns_True_False.plenty",
    "columns_True_True.plenty",
    "count_one.plenty",
    "count_random.plenty",
    "count_stream.plenty",
    "count_two.plenty",
    "count_whole.plenty",
    "emitter.plenty",
    "interface/caller.plenty",
    "interface/m0.plenty",
    "interface/m1.plenty",
    "interface/main.plenty",
    "removal_1.plenty",
    "removal_8.plenty",
    "rust_crates.plenty",
    "scalar_layout.plenty",
];

/// Files under `directory`.
fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, found);
        } else {
            found.push(path);
        }
    }
}

/// Git-tracked files under `experiments/`, so runner outputs and scratch
/// files in the checkout are not checked as inputs.
fn tracked_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("git")
        .args(["ls-files", "-z", "--", "experiments"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let mut files: Vec<PathBuf> = String::from_utf8(output.stdout)
        .unwrap()
        .split('\0')
        .filter(|name| !name.is_empty())
        .map(|name| root.join(name))
        .collect();
    files.sort();
    files
}

/// Directories with a `plenty.toml`. Their modules and tests resolve imports
/// through the manifest, not relative to each file (issue #910).
fn packages(files: &[PathBuf]) -> Vec<PathBuf> {
    files
        .iter()
        .filter(|path| path.file_name().is_some_and(|name| name == "plenty.toml"))
        .map(|manifest| manifest.parent().unwrap().to_path_buf())
        .collect()
}

fn is_plenty(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "plenty")
}

fn relative(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn stored_experiment_inputs_type_check() {
    let root = experiments();
    let files = tracked_files();
    let packages = packages(&files);
    let mut checked = 0;
    let mut seen = Vec::new();
    let mut failures = Vec::new();
    for path in files.iter().filter(|path| is_plenty(path)) {
        let name = relative(path, &root);
        seen.push(name.clone());
        if GENERATED_ONLY.contains(&name.as_str())
            || packages.iter().any(|package| path.starts_with(package))
        {
            continue;
        }
        checked += 1;
        let result = plenty::check_module_file(path, None);
        match REJECTED.iter().find(|(rejected, _)| *rejected == name) {
            Some((_, diagnostic)) => match result {
                Err(error) if error.to_string().contains(diagnostic) => {}
                Err(error) => {
                    failures.push(format!("{name}: expected `{diagnostic}`, got {error}"))
                }
                Ok(()) => failures.push(format!("{name}: expected `{diagnostic}`, but it checked")),
            },
            None => {
                if let Err(error) = result {
                    failures.push(format!("{name}: {error}"));
                }
            }
        }
    }
    assert!(checked > 0);
    // A renamed or deleted special case would otherwise drop its check.
    for special in REJECTED.iter().map(|(name, _)| name).chain(GENERATED_ONLY) {
        if !seen.iter().any(|name| name == special) {
            failures.push(format!(
                "{special}: listed as a special case but not tracked"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `check --tests` also checks each package's `tests/`, without building its
/// native library or running anything (issue #919).
#[test]
fn experiment_packages_type_check_with_their_tests() {
    let root = experiments();
    let packages = packages(&tracked_files());
    assert!(!packages.is_empty());
    for package in packages {
        let output = std::process::Command::new(support::plenty_bin())
            // `--locked` fails rather than rewriting the tracked `plenty.lock`.
            .args(["check", "--tests", "--offline", "--locked"])
            .current_dir(&package)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}:\n{}{}",
            relative(&package, &root),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn experiment_generators_produce_checked_programs() {
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/experiments/write_generated.py");
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let output = std::process::Command::new("python3")
        .arg("-B")
        .arg(&script)
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut files = Vec::new();
    walk(dir.path(), &mut files);
    files.retain(|path| is_plenty(path));
    files.sort();
    let mut names: Vec<String> = files
        .iter()
        .map(|path| relative(path, dir.path()))
        .collect();
    names.sort_unstable();
    let mut expected: Vec<&str> = GENERATED.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
    let failures: Vec<String> = files
        .iter()
        .filter_map(|path| {
            let error = plenty::check_module_file(path, None).err()?;
            Some(format!("{}: {error}", relative(path, dir.path())))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
