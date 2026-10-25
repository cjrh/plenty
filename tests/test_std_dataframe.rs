//! The ordinary suite checks optional linking without installing Polars.
//! PLENTY_DATAFRAME_TESTS=1 additionally requires and exercises the real engine.
mod support;
use std::path::{Path, PathBuf};
use std::process::Command;
use support::assert_success;

fn compile(root: &Path, source: &str, engine: Option<PathBuf>) -> Result<PathBuf, String> {
    let input = root.join("main.plenty");
    let output = root.join("program");
    std::fs::write(&input, source).unwrap();
    let options = plenty::CompileOptions {
        dataframe_engine: engine,
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&input, &output, None, &options)
        .map_err(|error| error.to_string())?;
    Ok(output)
}

fn dynamic(path: &Path) -> String {
    String::from_utf8(
        assert_success(
            Command::new("readelf")
                .arg("-d")
                .arg(path)
                .output()
                .unwrap(),
        )
        .stdout,
    )
    .unwrap()
}

fn engine_under_test() -> PathBuf {
    PathBuf::from(std::env::var_os("PLENTY_DATAFRAME_ENGINE").expect("engine under test"))
        .canonicalize()
        .expect("selected dataframe engine must exist")
}

#[test]
fn unused_dataframe_does_not_need_or_link_engine() {
    for source in [
        "def main() -> ():\n    pass\n",
        "import std.dataframe\ndef main() -> ():\n    pass\n",
        "import std.dataframe as df\ndef unused() -> Result[df.Expr, df.DataFrameError]:\n    df.col(\"x\")\ndef main() -> ():\n    pass\n",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let binary = compile(temp.path(), source, Some(temp.path().join("missing.so"))).unwrap();
        assert!(!dynamic(&binary).contains("plenty_dataframe"));
        assert_success(Command::new(binary).output().unwrap());
    }
}

#[test]
fn dependency_use_reports_missing_engine_even_with_cached_object() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("analytics.plenty"), "import std.dataframe as df\npub def expression() -> Result[df.Expr, df.DataFrameError]:\n    df.col(\"x\")\n").unwrap();
    let source = "import analytics\nimport std.dataframe as df\ndef main() -> Result[(), df.DataFrameError]:\n    x = analytics.expression()?\n    Ok(())\n";
    for _ in 0..2 {
        let error = compile(temp.path(), source, Some(temp.path().join("missing.so"))).unwrap_err();
        assert!(
            error.contains("std.dataframe requires the optional Polars engine"),
            "{error}"
        );
        assert!(error.contains("PLENTY_DATAFRAME_ENGINE"), "{error}");
    }
}

#[test]
fn dataframe_end_to_end_with_real_engine() {
    if std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none() {
        eprintln!(
            "real dataframe engine tests require PLENTY_DATAFRAME_TESTS=1; covered by the dataframe CI job"
        );
        return;
    }
    let engine = engine_under_test();
    let temp = tempfile::tempdir().unwrap();
    let binary = compile(
        temp.path(),
        include_str!("fixtures/dataframe.plenty"),
        Some(engine.clone()),
    )
    .unwrap();
    let dependencies = dynamic(&binary);
    assert!(
        dependencies.contains("libplenty_dataframe_v1.so"),
        "{dependencies}"
    );
    assert!(
        !dependencies.contains("/libplenty_dataframe_v1.so"),
        "SONAME must make deployment relocatable"
    );
    let result = assert_success(
        Command::new(binary)
            .current_dir(temp.path())
            // Cargo prepends target directories to LD_LIBRARY_PATH. Do not
            // accidentally load an older same-SONAME engine from that cache.
            .env("LD_LIBRARY_PATH", engine.parent().unwrap())
            .env("POLARS_MAX_THREADS", "2")
            .output()
            .unwrap(),
    );
    assert_eq!(String::from_utf8(result.stdout).unwrap(), "dataframe ok\n");
}

#[test]
fn joins_sorts_and_slices_with_real_engine() {
    if std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none() {
        eprintln!("query operation execution requires PLENTY_DATAFRAME_TESTS=1");
        return;
    }
    let engine = engine_under_test();
    let temp = tempfile::tempdir().unwrap();
    let binary = compile(
        temp.path(),
        include_str!("fixtures/dataframe_queries.plenty"),
        Some(engine.clone()),
    )
    .unwrap();
    let output = assert_success(
        Command::new(binary)
            .env("LD_LIBRARY_PATH", engine.parent().unwrap())
            .env("POLARS_MAX_THREADS", "2")
            .output()
            .unwrap(),
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "dataframe queries ok\n"
    );
}
