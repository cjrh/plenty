//! Every Plenty file under `examples/` type-checks with the current compiler.
//!
//! Other tests run selected examples and check their output; this one covers
//! the rest, so a language change that leaves an example stale fails here.
//! A file that defines a module-level `main` is checked as a program
//! (`--check`); any other file is a library module imported by a program in
//! the same directory (`--check-module`).
use std::path::{Path, PathBuf};

fn plenty_files(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            plenty_files(&path, found);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "plenty")
        {
            found.push(path);
        }
    }
}

fn defines_main(source: &str) -> bool {
    source.lines().any(|line| line.starts_with("def main("))
}

#[test]
fn every_example_type_checks() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut files = Vec::new();
    plenty_files(&examples, &mut files);
    files.sort();
    assert!(!files.is_empty());
    let failures: Vec<String> = files
        .iter()
        .filter_map(|path| {
            let source = std::fs::read_to_string(path).unwrap();
            let checked = if defines_main(&source) {
                plenty::check_file(path, None)
            } else {
                plenty::check_module_file(path, None)
            };
            let error = checked.err()?;
            Some(format!(
                "{}: {error}",
                path.strip_prefix(&examples).unwrap().display()
            ))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
