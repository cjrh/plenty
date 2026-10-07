//! Native artifact and driver contracts. Each test owns its environment.
use std::process::{Command, Output};

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[cfg(unix)]
fn script(path: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn configurable_driver_links_and_runs() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("main.plenty");
    std::fs::write(&source, "def main() -> i32:\n    23\n").unwrap();
    let output = success(
        Command::new(env!("CARGO_BIN_EXE_plenty"))
            .args(["--compile", source.to_str().unwrap(), "-o"])
            .arg(temp.path().join("app"))
            .args(["--linker", "cc", "--link-arg", "-lm"])
            .output()
            .unwrap(),
    );
    assert!(output.stdout.is_empty());
    assert_eq!(
        Command::new(temp.path().join("app"))
            .status()
            .unwrap()
            .code(),
        Some(23)
    );
}

#[cfg(unix)]
#[test]
fn driver_receives_literal_arguments_and_reports_failure() {
    let temp = tempfile::tempdir().unwrap();
    let driver = temp.path().join("custom driver");
    // This mock implements no link flags: it only reports argv and fails.
    script(&driver, "#!/bin/sh\nprintf '<%s>\\n' \"$@\" >&2\nexit 42\n");
    let options = plenty::CompileOptions {
        linker: driver.clone(),
        link_args: vec!["a b;$(false)".into(), "-lfixture".into()],
    };
    let error = plenty::compile_source_to_executable_with_options(
        "def main() -> ():\n    pass\n",
        &temp.path().join("out file"),
        &options,
    )
    .unwrap_err()
    .to_string();
    for part in [
        "custom driver",
        "42",
        "<a b;$(false)>",
        "<-lfixture>",
        "<-o>",
        "out file",
    ] {
        assert!(error.contains(part), "{error}");
    }
    let object = error.lines().find(|l| l.ends_with("program.o>")).unwrap();
    assert!(!std::path::Path::new(object.trim_matches(['<', '>'])).exists());
    let options = plenty::CompileOptions {
        linker: temp.path().join("missing driver"),
        ..options
    };
    let error = plenty::compile_source_to_executable_with_options(
        "def main() -> ():\n    pass\n",
        &temp.path().join("out"),
        &options,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("failed to invoke linker driver"), "{error}");
}

#[test]
fn cli_rejects_missing_or_unused_link_options() {
    for args in [
        vec!["--linker"],
        vec!["--link-arg"],
        vec!["--check", "missing.plenty", "--linker", "cc"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("reading"));
    }
}

#[test]
fn application_object_links_with_extracted_runtime() {
    let temp = tempfile::tempdir().unwrap();
    let object = temp.path().join("program.o");
    plenty::compile_source_to_object(
        "def main() -> i32:\n    print(42).unwrap()\n    7\n",
        &object,
    )
    .unwrap();
    let runtime = plenty::emit_runtime(&temp.path().join("runtime")).unwrap();
    let executable = temp.path().join("app");
    success(
        Command::new("cc")
            .arg(&object)
            .arg(runtime.archive)
            .args(runtime.link_args)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap(),
    );
    let output = Command::new(executable).output().unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout, b"42\n");
}

#[test]
fn cli_emits_objects_with_imports_without_any_toolchain_on_path() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("main.plenty");
    std::fs::write(
        &source,
        "import value\ndef main() -> i32:\n    value.answer()\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("value.plenty"),
        "pub def answer() -> i32:\n    42\n",
    )
    .unwrap();
    let object = temp.path().join("program.o");
    success(
        Command::new(env!("CARGO_BIN_EXE_plenty"))
            .env("PATH", "")
            .arg("--emit-object")
            .arg(&source)
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap(),
    );
    let runtime = temp.path().join("runtime");
    success(
        Command::new(env!("CARGO_BIN_EXE_plenty"))
            .env("PATH", "")
            .arg("--emit-runtime")
            .arg(&runtime)
            .output()
            .unwrap(),
    );
    let args = std::fs::read_to_string(runtime.join("link-args.txt")).unwrap();
    let executable = temp.path().join("app");
    success(
        Command::new("cc")
            .arg(&object)
            .arg(runtime.join("libplenty_runtime.a"))
            .args(args.lines())
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap(),
    );
    assert_eq!(Command::new(executable).status().unwrap().code(), Some(42));
    let error = plenty::compile_source_to_object("def helper() -> ():\n    pass\n", &object)
        .unwrap_err()
        .to_string();
    assert!(error.contains("main"), "{error}");
}
