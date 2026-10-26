//! Optional linking works without faer; PLENTY_LINALG_TESTS exercises the engine.
mod support;
use std::path::{Path, PathBuf};
use std::process::Command;
use support::assert_success;

fn compile(root: &Path, source: &str, engine: PathBuf) -> Result<PathBuf, String> {
    let input = root.join("main.plenty");
    let output = root.join("program");
    std::fs::write(&input, source).unwrap();
    let options = plenty::CompileOptions {
        linalg_engine: Some(engine),
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&input, &output, None, &options)
        .map_err(|e| e.to_string())?;
    Ok(output)
}

#[test]
fn unused_and_pure_matrix_operations_need_no_engine() {
    for source in [
        "import std.linalg as la\ndef main() -> ():\n    pass\n",
        "import std.linalg as la\ndef unused(a: &la.Matrix) -> Result[la.Matrix, AllocError | la.LinalgError]:\n    a.matmul(a)\ndef main() -> ():\n    pass\n",
        "import std.linalg as la\ndef main() -> Result[(), AllocError | la.LinalgError]:\n    a = la.identity(2)?\n    drop(a.get(0, 0)?)\n    Ok(())\n",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let exe = compile(tmp.path(), source, tmp.path().join("missing.so")).unwrap();
        let dynamic = assert_success(
            Command::new("readelf")
                .arg("-d")
                .arg(&exe)
                .output()
                .unwrap(),
        );
        assert!(!String::from_utf8_lossy(&dynamic.stdout).contains("plenty_linalg"));
        assert_success(Command::new(exe).output().unwrap());
    }
}

#[test]
fn reached_kernel_reports_missing_engine() {
    let tmp = tempfile::tempdir().unwrap();
    let source = "import std.linalg as la\ndef main() -> Result[(), AllocError | la.LinalgError]:\n    a = la.identity(2)?\n    b = a.matmul(&a)?\n    Ok(())\n";
    for _ in 0..2 {
        let error = compile(tmp.path(), source, tmp.path().join("missing.so")).unwrap_err();
        assert!(error.contains("std.linalg requires"), "{error}");
        assert!(error.contains("PLENTY_LINALG_ENGINE"), "{error}");
    }
}

#[test]
fn real_engine_and_app_only_relocation() {
    if std::env::var_os("PLENTY_LINALG_TESTS").is_none() {
        return;
    }
    let engine = PathBuf::from(std::env::var_os("PLENTY_LINALG_ENGINE").expect("engine path"))
        .canonicalize()
        .unwrap();
    let build = tempfile::tempdir().unwrap();
    let isolated = build.path().join("engine/libplenty_linalg_v1.so");
    std::fs::create_dir(isolated.parent().unwrap()).unwrap();
    std::fs::copy(&engine, &isolated).unwrap();
    let exe = compile(
        build.path(),
        include_str!("fixtures/linalg.plenty"),
        isolated.clone(),
    )
    .unwrap();
    let dynamic = assert_success(
        Command::new("readelf")
            .arg("-d")
            .arg(&exe)
            .output()
            .unwrap(),
    );
    assert!(String::from_utf8_lossy(&dynamic.stdout).contains("libplenty_linalg_v1.so"));
    let bundle = tempfile::tempdir().unwrap();
    let app = bundle.path().join("analysis");
    std::fs::copy(exe, &app).unwrap();
    std::fs::copy(&isolated, bundle.path().join("libplenty_linalg_v1.so")).unwrap();
    std::fs::remove_file(isolated).unwrap();
    let result = assert_success(
        Command::new(&app)
            .env_remove("LD_LIBRARY_PATH")
            .env_remove("LD_PRELOAD")
            .current_dir("/")
            .output()
            .unwrap(),
    );
    assert_eq!(String::from_utf8(result.stdout).unwrap(), "linalg ok\n");
    std::fs::remove_file(bundle.path().join("libplenty_linalg_v1.so")).unwrap();
    assert!(
        !Command::new(app)
            .env_remove("LD_LIBRARY_PATH")
            .env_remove("LD_PRELOAD")
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn plenty_allocation_failure_preserves_inputs() {
    if std::env::var_os("PLENTY_LINALG_TESTS").is_none() {
        return;
    }
    let engine = PathBuf::from(std::env::var_os("PLENTY_LINALG_ENGINE").expect("engine path"))
        .canonicalize()
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let exe = compile(
        root.path(),
        include_str!("fixtures/linalg_allocation.plenty"),
        engine.clone(),
    )
    .unwrap();
    let result = assert_success(
        Command::new(exe)
            .env("LD_LIBRARY_PATH", engine.parent().unwrap())
            .output()
            .unwrap(),
    );
    let stdout = String::from_utf8(result.stdout).unwrap();
    let visible: Vec<_> = stdout
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect();
    assert_eq!(visible, ["linalg allocation ok"]);
}
