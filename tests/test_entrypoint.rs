//! Entrypoint contracts use complete programs, without the fragment test helper.
use std::process::Command;

fn run_both(source: &str, code: i32, stdout: &str) {
    plenty::check_source(source).unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("entry.plenty");
    let executable = workspace.path().join("entry");
    std::fs::write(&file, source).unwrap();
    plenty::compile_source_to_executable(source, &executable).unwrap();
    let checked = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg("--check")
        .arg(&file)
        .output()
        .unwrap();
    assert!(checked.status.success());
    assert!(checked.stdout.is_empty());
    assert!(checked.stderr.is_empty());
    for output in [
        Command::new(&executable).output().unwrap(),
        Command::new(env!("CARGO_BIN_EXE_plenty"))
            .arg(&file)
            .output()
            .unwrap(),
    ] {
        assert_eq!(output.status.code(), Some(code), "{source}");
        assert_eq!(output.stdout, stdout.as_bytes(), "{source}");
        assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    }
}

#[test]
fn main_is_called_once_and_other_declarations_have_no_startup_effects() {
    run_both(
        "def unused() -> ():\n    print('unused').unwrap()\ndef main() -> ():\n    print('started').unwrap()\n",
        0,
        "started\n",
    );
    run_both("def main() -> ():\n    pass\n", 0, "");
}

#[test]
fn main_returns_the_process_status_and_allows_forward_calls_and_aliases() {
    run_both(
        "type Status = i32\ndef main() -> Status:\n    status()\ndef status() -> i32:\n    23i32\n",
        23,
        "",
    );
    run_both(
        "type Done = ()\ndef main() -> Done:\n    if True:\n        return\n    print('unreached').unwrap()\n",
        0,
        "",
    );
}

#[test]
fn main_drops_resources_before_the_native_wrapper_returns() {
    let declarations = "class Resource:\n    name: str\n    def __del__(self) -> ():\n        print(self.name).unwrap()\ndef status() -> i32:\n    print('status').unwrap()\n    7i32\n";
    for tail in ["return status()", "status()"] {
        run_both(
            &format!("{declarations}def main() -> i32:\n    a = Resource('first').unwrap()\n    b = Resource('second').unwrap()\n    {tail}\n"),
            7,
            "status\nsecond\nfirst\n",
        );
    }
    run_both(
        &format!(
            "{declarations}def main() -> ():\n    a = Resource('unit').unwrap()\n    return\n"
        ),
        0,
        "unit\n",
    );
}

#[test]
fn invalid_entrypoints_are_rejected_before_execution_or_artifact_creation() {
    for (source, diagnostic) in [
        ("", "binary application requires"),
        ("# no program\n", "binary application requires"),
        (
            "def helper() -> ():\n    pass\n",
            "binary application requires",
        ),
        (
            "class App:\n    def main(self) -> ():\n        pass\n",
            "binary application requires",
        ),
        ("type main = i32\n", "binary application requires"),
        ("print('must not run').unwrap()\n", "module scope"),
        ("def main() -> ():\n    pass\nmain()\n", "module scope"),
        ("def main() -> ():\n    pass\nx = 1\n", "module scope"),
        (
            "def main(x: i64) -> ():\n    pass\n",
            "main must take no parameters",
        ),
        (
            "def main() -> i64:\n    0\n",
            "main must take no parameters",
        ),
        (
            "def main() -> Result[i64, str]:\n    Ok(0)\n",
            "main must take no parameters",
        ),
        (
            "def main() -> Generator[i64]:\n    yield 1\n",
            "cannot be a generator",
        ),
        ("def main() -> ():\n    yield 1\n", "cannot be a generator"),
        ("def main() -> i32:\n    0i64\n", "expected i32, got i64"),
        ("def main() -> ():\n    42\n", "expected (), got i64"),
        (
            "def main() -> ():\n    pass\ndef main() -> ():\n    pass\n",
            "already defined",
        ),
        (
            "def main() -> ():\n    x = 1\n    helper()\ndef helper() -> ():\n    print(x).unwrap()\n",
            "unknown binding",
        ),
    ] {
        let error = plenty::check_source(source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{source}\n{error}");
        let workspace = tempfile::tempdir().unwrap();
        let file = workspace.path().join("bad.plenty");
        let executable = workspace.path().join("bad");
        std::fs::write(&file, source).unwrap();
        let error = plenty::compile_source_to_executable(source, &executable)
            .unwrap_err()
            .to_string();
        assert!(error.contains(diagnostic), "{error}");
        for args in [vec![], vec!["--check"]] {
            let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
                .args(args)
                .arg(&file)
                .output()
                .unwrap();
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
        }
        let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
            .arg("--compile")
            .arg(&file)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
        assert!(!executable.exists());
    }
}
