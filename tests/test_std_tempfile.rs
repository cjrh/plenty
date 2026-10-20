//! Native lifecycle tests for temporary resources.
use std::process::Command;

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failures_leave_no_names_and_keep_failure_stays_armed() {
    let mut body = String::new();
    for constructor in ["file", "named_file", "directory"] {
        for budget in 0..64 {
            body.push_str(&format!("print(\"probe {constructor} {budget}\")?\nprint(\"__test_fail_allocations_after_{budget}__\").unwrap()\nresult_{constructor}_{budget} = std.tempfile.{constructor}()\nprint(\"__test_restore_allocations__\").unwrap()\ndrop(result_{constructor}_{budget})\n"));
        }
    }
    for constructor in ["named_file", "directory"] {
        body.push_str(&format!("mut owner_{constructor} = std.tempfile.{constructor}()?\nprint(\"__test_fail_allocations_after_0__\").unwrap()\nkept_{constructor} = owner_{constructor}.keep()\nprint(\"__test_restore_allocations__\").unwrap()\nstd.test.is_err(&kept_{constructor})?\ndrop(owner_{constructor})\n"));
    }
    for budget in 0..8 {
        body.push_str(&format!("print(\"__test_fail_allocations_after_{budget}__\").unwrap()\ncwd_{budget} = std.env.current_dir()\nprint(\"__test_restore_allocations__\").unwrap()\ndrop(cwd_{budget})\n"));
    }
    run(&body, &[]);
}

#[test]
fn default_directory_precedence_relative_and_native_bytes() {
    use std::os::unix::ffi::OsStringExt;
    let build = tempfile::tempdir().unwrap();
    let source = build.path().join("main.plenty");
    let executable = build.path().join("program");
    std::fs::write(&source, "import std.tempfile\nimport std.path\nimport std.env\nimport std.test\ndef main() -> Result[(), IoError | std.test.TestError]:\n    actual = std.tempfile.default_dir()?\n    expected = std.env.get_bytes(&encode_utf8(\"EXPECTED\")?)?\n    match expected:\n        case Some(bytes):\n            std.test.equal(actual.as_bytes(), &bytes)?\n        case Nothing:\n            std.test.check(False, \"missing expectation\")?\n    Ok(())\n").unwrap();
    plenty::compile_file_to_executable(&source, &executable, None).unwrap();
    let raw = std::ffi::OsString::from_vec(b"/tmp/native-\xff".to_vec());
    let cases = [
        ("/one".into(), "/two", "/three", "/one".into()),
        ("".into(), "/two", "/three", "/two".into()),
        ("".into(), "", "/three", "/three".into()),
        ("".into(), "", "", "/tmp".into()),
        (
            "relative".into(),
            "",
            "",
            build.path().join("relative").into_os_string(),
        ),
        (raw.clone(), "", "", raw),
    ];
    for (first, second, third, expected) in cases {
        let output = Command::new(&executable)
            .current_dir(build.path())
            .env("TMPDIR", first)
            .env("TEMP", second)
            .env("TMP", third)
            .env("EXPECTED", expected)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    // Exercise growth beyond the native current-directory buffer's first size.
    let mut deep = build.path().to_path_buf();
    for _ in 0..24 {
        deep.push("long-component");
    }
    std::fs::create_dir_all(&deep).unwrap();
    let output = Command::new(&executable)
        .current_dir(&deep)
        .env("TMPDIR", "relative")
        .env("EXPECTED", deep.join("relative"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn propagated_error_cleans_all_contexts() {
    let build = tempfile::tempdir().unwrap();
    let resources = tempfile::tempdir().unwrap();
    let source = build.path().join("main.plenty");
    let executable = build.path().join("program");
    std::fs::write(
        &source,
        r#"
import std.tempfile
import std.path
import std.fs
def fail() -> Result[(), IoError]:
    Err(IoError.Closed)
def main() -> Result[(), IoError | std.fs.FsError]:
    with std.tempfile.named_file()? as named:
        with std.tempfile.directory()? as directory:
            path = directory.path().join(&std.path.from_text("child")?)?
            write_text(&path.to_text()?, "work")?
            fail()?
    Ok(())
"#,
    )
    .unwrap();
    plenty::compile_file_to_executable(&source, &executable, None).unwrap();
    let output = Command::new(executable)
        .env("TMPDIR", resources.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(std::fs::read_dir(resources.path()).unwrap().count(), 0);
}

fn run(body: &str, remaining: &[&str]) {
    run_with_setup(body, remaining, |_| {});
}

fn run_with_setup(body: &str, remaining: &[&str], setup: impl FnOnce(&std::path::Path)) {
    let build = tempfile::tempdir().unwrap();
    let resources = tempfile::tempdir().unwrap();
    setup(resources.path());
    let source = build.path().join("main.plenty");
    let executable = build.path().join("program");
    let indented = body
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    std::fs::write(&source, format!("import std.tempfile\nimport std.path\nimport std.fs\nimport std.io\nimport std.bytes\nimport std.test\nimport std.env\ndef main() -> Result[(), AllocError | IoError | std.fs.FsError | std.io.StreamError | std.io.Partial | std.bytes.BytesError | std.test.TestError]:\n{indented}    Ok(())\n")).unwrap();
    plenty::compile_file_to_executable(&source, &executable, None).unwrap();
    let output = Command::new(executable)
        .env("TMPDIR", resources.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut actual = std::fs::read_dir(resources.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    actual.sort();
    assert_eq!(actual, remaining);
}

#[test]
fn replaced_file_is_preserved() {
    run(
        r#"
mut named = std.tempfile.named_file()?
path = named.path().copy()?
parent = std.fs.root(&std.tempfile.default_dir()?)?
match path.file_name()?:
    case Some(name):
        parent.move(&name, &std.path.from_text("original")?)?
        write_text(&path.to_text()?, "replacement")?
        match named.close():
            case Err(std.fs.FsError.Replaced):
                pass
            case _:
                std.test.check(False, "expected replacement error")?
        drop(named)
        std.test.equal(&read_text(&path.to_text()?)?, &"replacement")?
        parent.move(&name, &std.path.from_text("replacement")?)?
    case Nothing:
        std.test.check(False, "missing filename")?
"#,
        &["original", "replacement"],
    );
}

#[test]
fn file_cleanup_does_not_follow_symlink_replacements() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("sentinel"), "untouched").unwrap();
    run_with_setup(
        r#"
mut scratch = std.tempfile.named_file()?
root = std.fs.root(&std.tempfile.default_dir()?)?
match scratch.path().file_name()?:
    case Some(name):
        root.move(&name, &std.path.from_text("original")?)?
        root.move(&std.path.from_text("link")?, &name)?
        std.test.is_err(&scratch.close())?
        drop(scratch)
        root.move(&name, &std.path.from_text("link")?)?
    case Nothing:
        std.test.check(False, "missing filename")?
"#,
        &["link", "original"],
        |parent| std::os::unix::fs::symlink(outside.path(), parent.join("link")).unwrap(),
    );
    assert_eq!(
        std::fs::read_to_string(outside.path().join("sentinel")).unwrap(),
        "untouched"
    );
}

#[test]
fn rejects_invalid_options_without_leaving_resources() {
    run(
        r#"
std.test.is_err(&std.tempfile.named_file(prefix="bad/name"))?
std.test.is_err(&std.tempfile.named_file(suffix="bad/name"))?
std.test.is_err(&std.tempfile.directory(prefix="bad/name"))?
std.test.is_err(&std.tempfile.directory(max_depth=0))?
std.test.is_err(&std.tempfile.directory(max_depth=65))?
std.test.is_err(&std.tempfile.directory(max_entries=0))?
missing = std.tempfile.default_dir()?.join(&std.path.from_text("missing")?)?
std.test.is_err(&std.tempfile.file(parent=&missing))?
std.test.is_err(&std.tempfile.named_file(parent=&missing))?
std.test.is_err(&std.tempfile.directory(parent=&missing))?
"#,
        &[],
    );
}

#[test]
fn directory_keep_and_cleanup_limits() {
    run(
        r#"
mut scratch = std.tempfile.directory(max_depth=1)?
scratch.root().create_dir(&std.path.from_text("child")?)?
match scratch.cleanup():
    case Err(std.fs.FsError.Limit):
        pass
    case _:
        std.test.check(False, "expected depth limit")?
path = scratch.keep()?
parent = std.fs.root(&std.tempfile.default_dir()?)?
match path.file_name()?:
    case Some(name):
        parent.move(&name, &std.path.from_text("kept")?)?
    case Nothing:
        std.test.check(False, "missing filename")?
"#,
        &["kept"],
    );
}

#[test]
fn context_managers_read_write_and_recursively_clean() {
    run(
        r#"
with std.tempfile.file()? as stream:
    std.io.write_all(stream, &b"hello", 0, 5)?
    stream.seek(std.io.SeekFrom.Start(0))?
    mut output = std.bytes.with_capacity(5)?
    output.resize(5, 0u8)?
    std.test.equal(&stream.read(&mut output, 0, 5)?, &5)?
    std.test.equal(&output.to_text()?, &"hello")?
with std.tempfile.named_file(prefix="report-", suffix=".txt")? as named:
    std.io.write_all(named, &b"named", 0, 5)?
    std.test.equal(&read_text(&named.path().to_text()?)?, &"named")?
with std.tempfile.directory()? as scratch:
    scratch.root().create_dir(&std.path.from_text("nested")?)?
    child = scratch.path().join(&std.path.from_text("nested/note")?)?
    write_text(&child.to_text()?, "hello")?
"#,
        &[],
    );
}

#[test]
fn explicit_cleanup_is_idempotent_and_keep_preserves_resources() {
    run(
        r#"
mut named = std.tempfile.named_file()?
named.close()?
named.close()?
mut scratch = std.tempfile.directory()?
scratch.cleanup()?
scratch.cleanup()?
mut kept = std.tempfile.named_file()?
std.io.write_all(&mut kept, &b"kept", 0, 4)?
path = kept.keep()?
kept.close()?
std.test.equal(&read_text(&path.to_text()?)?, &"kept")?
parent = std.fs.root(&std.tempfile.default_dir()?)?
match path.file_name()?:
    case Some(name):
        parent.move(&name, &std.path.from_text("kept")?)?
    case Nothing:
        std.test.check(False, "missing filename")?
"#,
        &["kept"],
    );
}

#[test]
fn replaced_directory_is_not_traversed() {
    run(
        r#"
mut scratch = std.tempfile.directory()?
path = scratch.path().copy()?
write_text(&path.join(&std.path.from_text("original")?)?.to_text()?, "original")?
parent = std.fs.root(&std.tempfile.default_dir()?)?
match path.file_name()?:
    case Some(name):
        parent.move(&name, &std.path.from_text("moved")?)?
        parent.create_dir(&name)?
        write_text(&path.join(&std.path.from_text("replacement")?)?.to_text()?, "replacement")?
        match scratch.cleanup():
            case Err(std.fs.FsError.Replaced):
                pass
            case _:
                std.test.check(False, "expected replacement error")?
        std.test.equal(&read_text(&path.join(&std.path.from_text("replacement")?)?.to_text()?)?, &"replacement")?
        parent.move(&name, &std.path.from_text("replacement")?)?
    case Nothing:
        std.test.check(False, "missing filename")?
"#,
        &["moved", "replacement"],
    );
}
