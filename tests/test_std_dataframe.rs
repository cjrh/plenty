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
fn nested_numeric_and_binary_columns_with_real_engine() {
    if std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none() {
        eprintln!("nested scalar execution requires PLENTY_DATAFRAME_TESTS=1");
        return;
    }
    let engine = engine_under_test();
    let temp = tempfile::tempdir().unwrap();
    let binary = compile(
        temp.path(),
        include_str!("fixtures/dataframe_nested_scalars.plenty"),
        Some(engine.clone()),
    )
    .unwrap();
    let output = assert_success(
        Command::new(binary)
            .current_dir(temp.path())
            .env("LD_LIBRARY_PATH", engine.parent().unwrap())
            .env("POLARS_MAX_THREADS", "2")
            .output()
            .unwrap(),
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "dataframe nested scalars ok\n"
    );
}

#[test]
fn nested_columns_with_real_engine() {
    if std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none() {
        eprintln!("nested execution requires PLENTY_DATAFRAME_TESTS=1");
        return;
    }
    let engine = engine_under_test();
    let temp = tempfile::tempdir().unwrap();
    let binary = compile(
        temp.path(),
        include_str!("fixtures/dataframe_nested.plenty"),
        Some(engine.clone()),
    )
    .unwrap();
    let output = assert_success(
        Command::new(binary)
            .current_dir(temp.path())
            .env("LD_LIBRARY_PATH", engine.parent().unwrap())
            .env("POLARS_MAX_THREADS", "2")
            .output()
            .unwrap(),
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "dataframe nested ok\n"
    );
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

#[test]
fn typed_nullable_columns_and_schema_with_real_engine() {
    if std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none() {
        eprintln!("typed dataframe execution requires PLENTY_DATAFRAME_TESTS=1");
        return;
    }
    let engine = engine_under_test();
    let temp = tempfile::tempdir().unwrap();
    let binary = compile(
        temp.path(),
        include_str!("fixtures/dataframe_types.plenty"),
        Some(engine.clone()),
    )
    .unwrap();
    let output = assert_success(
        Command::new(binary)
            .current_dir(temp.path())
            .env("LD_LIBRARY_PATH", engine.parent().unwrap())
            .env("POLARS_MAX_THREADS", "2")
            .output()
            .unwrap(),
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "dataframe types ok\n"
    );
}

#[test]
fn all_numeric_widths_with_real_engine() {
    if std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none() {
        eprintln!("numeric dataframe execution requires PLENTY_DATAFRAME_TESTS=1");
        return;
    }
    let engine = engine_under_test();
    let temp = tempfile::tempdir().unwrap();
    let binary = compile(
        temp.path(),
        include_str!("fixtures/dataframe_numeric.plenty"),
        Some(engine.clone()),
    )
    .unwrap();
    let output = assert_success(
        Command::new(binary)
            .current_dir(temp.path())
            .env("LD_LIBRARY_PATH", engine.parent().unwrap())
            .env("POLARS_MAX_THREADS", "2")
            .output()
            .unwrap(),
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "dataframe numeric types ok\n"
    );
}
#[test]
fn binary_columns_with_real_engine() {
    if std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none() {
        eprintln!("binary dataframe execution requires PLENTY_DATAFRAME_TESTS=1");
        return;
    }
    let engine = engine_under_test();
    let temp = tempfile::tempdir().unwrap();
    let binary = compile(
        temp.path(),
        include_str!("fixtures/dataframe_binary.plenty"),
        Some(engine.clone()),
    )
    .unwrap();
    let output = assert_success(
        Command::new(binary)
            .current_dir(temp.path())
            .env("LD_LIBRARY_PATH", engine.parent().unwrap())
            .env("POLARS_MAX_THREADS", "2")
            .output()
            .unwrap(),
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "dataframe binary ok\n"
    );
}

#[test]
fn expanded_expressions_with_real_engine() {
    if std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none() {
        eprintln!("expression execution requires PLENTY_DATAFRAME_TESTS=1");
        return;
    }
    let engine = engine_under_test();
    let temp = tempfile::tempdir().unwrap();
    let binary = compile(
        temp.path(),
        include_str!("fixtures/dataframe_expressions.plenty"),
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
        "dataframe expressions ok\n"
    );
}

#[test]
fn csv_and_streaming_io_with_real_engine() {
    if std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none() {
        eprintln!("CSV/streaming execution requires PLENTY_DATAFRAME_TESTS=1");
        return;
    }
    let engine = engine_under_test();
    let temp = tempfile::tempdir().unwrap();
    for (name, bytes) in [
        (
            "custom.csv",
            b"id;active;note\n001;true;\"\"\n002;NA;NA\n003;false;\"\xe9\x9b\xaa;quoted\"\n"
                .as_slice(),
        ),
        ("inference.csv", b"v\n1\ntwo\n".as_slice()),
        ("tokens.csv", b"value\n\"NA\"\n\"\"\nNA\nx\n".as_slice()),
        ("bad_utf8.csv", b"s\n\xff\n".as_slice()),
        ("empty_typed.csv", b"id,name\n".as_slice()),
    ] {
        std::fs::write(temp.path().join(name), bytes).unwrap();
    }
    let binary = compile(
        temp.path(),
        include_str!("fixtures/dataframe_io.plenty"),
        Some(engine.clone()),
    )
    .unwrap();
    let output = assert_success(
        Command::new(binary)
            .current_dir(temp.path())
            .env("LD_LIBRARY_PATH", engine.parent().unwrap())
            .env("POLARS_MAX_THREADS", "2")
            .output()
            .unwrap(),
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "dataframe io ok\n"
    );
}

#[test]
fn full_semi_anti_joins_with_real_engine() {
    if std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none() {
        eprintln!("join execution requires PLENTY_DATAFRAME_TESTS=1");
        return;
    }
    let engine = engine_under_test();
    let temp = tempfile::tempdir().unwrap();
    let binary = compile(
        temp.path(),
        include_str!("fixtures/dataframe_joins.plenty"),
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
        "dataframe joins ok\n"
    );
}

#[test]
fn checked_integer_asof_joins_with_real_engine() {
    if std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none() {
        eprintln!("as-of execution requires PLENTY_DATAFRAME_TESTS=1");
        return;
    }
    let engine = engine_under_test();
    let temp = tempfile::tempdir().unwrap();
    let binary = compile(
        temp.path(),
        include_str!("fixtures/dataframe_asof.plenty"),
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
        "dataframe asof ok\n"
    );
}

#[test]
fn dates_and_datetimes_with_real_engine() {
    if std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none() {
        eprintln!("datetime execution requires PLENTY_DATAFRAME_TESTS=1");
        return;
    }
    let engine = engine_under_test();
    let temp = tempfile::tempdir().unwrap();
    let binary = compile(
        temp.path(),
        include_str!("fixtures/dataframe_datetime.plenty"),
        Some(engine.clone()),
    )
    .unwrap();
    let output = assert_success(
        Command::new(binary)
            .current_dir(temp.path())
            .env("LD_LIBRARY_PATH", engine.parent().unwrap())
            .env("POLARS_MAX_THREADS", "2")
            .output()
            .unwrap(),
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "dataframe datetime ok\n"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn binary_literal_has_no_plenty_side_allocation_with_real_engine() {
    if std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none() {
        eprintln!("binary literal allocation test requires PLENTY_DATAFRAME_TESTS=1");
        return;
    }
    let engine = engine_under_test();
    let temp = tempfile::tempdir().unwrap();
    let binary = compile(
        temp.path(),
        r#"
import std.dataframe as df
import std.bytes

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    buffer = std.bytes.from_bytes(b"xabcy")?
    values = [97u8, 98u8, 99u8]?
    print("__test_begin_no_allocations__")?
    a = df.lit_binary(b"abc")?
    b = df.lit_binary(&buffer[1:4])?
    c = df.lit_binary(&values)?
    d = df.lit_binary(b"")?
    print("__test_end_no_allocations__")?
    Ok(())
"#,
        Some(engine.clone()),
    )
    .unwrap();
    assert_success(
        Command::new(binary)
            .env("LD_LIBRARY_PATH", engine.parent().unwrap())
            .env("POLARS_MAX_THREADS", "2")
            .output()
            .unwrap(),
    );
}

#[test]
fn extent_add_checks_representation_limits_without_engine_or_allocation() {
    let library = include_str!("../stdlib/std/dataframe.plentyi");
    let helper = library[library.find("def extent_add(").unwrap()..]
        .split("\n\n")
        .next()
        .unwrap();
    let source = format!(
        r#"
enum DataFrameError:
    Allocation(AllocError)

{helper}

def capacity_error(value: Result[i64, DataFrameError]) -> bool:
    match value:
        case Err(DataFrameError.Allocation(AllocError.CapacityOverflow)):
            True
        case _:
            False

def main() -> Result[(), IoError]:
    print("__test_begin_no_allocations__")?
    empty = extent_add(0, 1).unwrap() == 1
    last = extent_add(i64.MAX, 0).unwrap() == i64.MAX
    boundary = extent_add(9223372036854775806, 1).unwrap() == i64.MAX
    overflow = capacity_error(extent_add(i64.MAX, 1))
    packed = capacity_error(extent_add(9223372036854775806, 2))
    print("__test_end_no_allocations__")?
    print(empty, last, boundary, overflow, packed)?
    Ok(())
"#
    );
    for optimize in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("main.plenty");
        let binary = temp.path().join("program");
        std::fs::write(&input, &source).unwrap();
        let options = plenty::CompileOptions {
            optimize,
            test_hooks: true,
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&input, &binary, None, &options).unwrap();
        let output = assert_success(Command::new(binary).output().unwrap());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("True True True True True\n"), "{stdout}");
    }
}

#[test]
fn dataframe_fixtures_typecheck_without_optional_engine() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut paths = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "plenty")
                && path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("dataframe")
        })
        .collect::<Vec<_>>();
    paths.sort();
    assert!(!paths.is_empty());
    for path in paths {
        plenty::check_file(&path, None)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    }
}

/// Every engine type is `@thread_shareable` and every call `@thread_safe`
/// (issue #833), so worker certification needs no engine.
#[test]
fn dataframe_values_are_certified_for_workers_without_engine() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("main.plenty");
    std::fs::write(&input, include_str!("fixtures/dataframe_workers.plenty")).unwrap();
    plenty::check_file(&input, None).unwrap();
}

#[test]
fn dataframe_values_move_share_and_drop_on_workers_with_real_engine() {
    if std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none() {
        eprintln!("worker execution requires PLENTY_DATAFRAME_TESTS=1");
        return;
    }
    let engine = engine_under_test();
    let temp = tempfile::tempdir().unwrap();
    let binary = compile(
        temp.path(),
        include_str!("fixtures/dataframe_workers.plenty"),
        Some(engine.clone()),
    )
    .unwrap();
    let output = assert_success(
        Command::new(binary)
            .current_dir(temp.path())
            .env("LD_LIBRARY_PATH", engine.parent().unwrap())
            .env("POLARS_MAX_THREADS", "2")
            .output()
            .unwrap(),
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "4\n168.0\n3\n[\"north\", \"south\", \"north\", \"south\"]\n85.0\n4\n84.0\ndataframe workers ok\n"
    );
}
