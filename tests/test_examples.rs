//! Every Plenty file under `examples/` type-checks with the current compiler.
//!
//! Other tests run selected examples and check their output; this one covers
//! the rest, so a language change that leaves an example stale fails here.
//! A file that defines a module-level `main` is checked as a program
//! (`--check`); any other file is a library module imported by a program in
//! the same directory (`--check-module`).
use std::path::{Path, PathBuf};

mod support;

fn plenty_files(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            plenty_files(&path, found);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "plenty" || extension == "p")
        {
            found.push(path);
        }
    }
}

fn defines_main(source: &str) -> bool {
    source.lines().any(|line| line.starts_with("def main("))
}

#[test]
fn classes_arithmetic_preserves_the_point_on_failed_shift() {
    let helpers = include_str!("../examples/classes.plenty")
        .split("def main(")
        .next()
        .unwrap();
    let source = format!(
        "{helpers}{}",
        r#"
def main() -> Result[(), AllocError | IoError]:
    mut point = Point(1, i64.MAX)
    print(str.repr(point.shift(1))?)?
    print(point)?
    mut low = Point(i64.MIN, 2)
    print(str.repr(low.shift(-1))?)?
    print(low)?
    print(str.repr(point.squared_length())?)?
    mut edge = Point(0, i64.MAX)
    print(str.repr(edge.shift(0))?)?
    print(edge)?
    Ok(())
"#
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "Result[(), OverflowError[i64]].Err(OverflowError[i64](wrapped=-9223372036854775808))\nPoint(x=1, y=9223372036854775807)\nResult[(), OverflowError[i64]].Err(OverflowError[i64](wrapped=9223372036854775807))\nPoint(x=-9223372036854775808, y=2)\nResult[i64, OverflowError[i64]].Err(OverflowError[i64](wrapped=1))\nResult[(), OverflowError[i64]].Ok(())\nPoint(x=0, y=9223372036854775807)"
    );
}

#[test]
fn cpu_codegen_audit_kernels_keep_scalar_boundary_results() {
    let helpers = include_str!("../examples/cpu_codegen_audit/kernels.plenty")
        .split("def main(")
        .next()
        .unwrap();
    let source = format!(
        "{helpers}{}",
        r#"
def main() -> Result[(), AllocError | IoError]:
    print(div10(u64.MAX), rem10(u64.MAX))?
    print(escape(0.0, 0.0, -1), escape(0.0, 0.0, 0), escape(3.0, 0.0, i64.MAX))?
    values = [u64.MAX, 1u64, 9u64]?
    print(sum_words(&values), sum_indexed(&values))?
    print(mix(u64.MAX, 0u64))?
    Ok(())
"#
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "1844674407370955161 5\n0 0 1\n9 9\n18446744073709551615"
    );
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

/// `examples/collection_bench.py` generates its programs, so the file walk
/// above never sees them (issue #756). `--check-only` type-checks each one
/// and prints one line per workload; compare that with the generator.
#[test]
fn collection_benchmark_workloads_type_check() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let script = root.join("examples/collection_bench.py");
    let generated = std::process::Command::new("python3")
        .args(["-B", "-c"])
        .arg(
            "import runpy, sys; \
             print(len(list(runpy.run_path(sys.argv[1])['workloads']())))",
        )
        .arg(&script)
        .output()
        .unwrap();
    assert!(generated.status.success(), "{generated:?}");
    let expected: usize = String::from_utf8(generated.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(expected > 0);

    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new("python3")
        .arg("-B")
        .arg(&script)
        .arg("--compiler")
        .arg(support::plenty_bin())
        .arg("--output")
        .arg(dir.path())
        .arg("--check-only")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let checked = stdout
        .lines()
        .filter(|line| line.starts_with("checked "))
        .count();
    assert_eq!(checked, expected, "{stdout}");
}
