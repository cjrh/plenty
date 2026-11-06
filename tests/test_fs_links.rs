//! Native symlink creation and resolution keep bytes, OS errors and worker use.
#![cfg(all(target_os = "linux", target_pointer_width = "64"))]
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Program {
    _directory: tempfile::TempDir,
    executable: PathBuf,
}

impl Program {
    fn new(source: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().join("main.plenty");
        let executable = directory.path().join("program");
        std::fs::write(&entry, source).unwrap();
        plenty::check_file(&entry, None).unwrap_or_else(|error| panic!("{error}\n{source}"));
        plenty::compile_file_to_executable(&entry, &executable, None)
            .unwrap_or_else(|error| panic!("{error}\n{source}"));
        Self {
            _directory: directory,
            executable,
        }
    }

    fn run(&self, root: &Path) -> String {
        let output = Command::new(&self.executable)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    }
}

#[test]
fn symlinks_preserve_native_targets_and_canonicalize_in_workers() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.path().join("actual")).unwrap();
    std::fs::create_dir(fixture.path().join("actual/nested")).unwrap();
    std::fs::write(fixture.path().join("actual/file"), "data").unwrap();
    std::fs::write(fixture.path().join(OsStr::from_bytes(b"bad\xff")), "raw").unwrap();
    let program = Program::new(
        r#"
import std.fs as fs
import std.path as path
import std.env

def work() -> Result[(), AllocError | IoError]:
    root = std.env.current_dir()?
    target = path.from_text("actual/nested")?
    link = path.from_text("alias")?
    fs.symlink(&target, &link)?
    fs.symlink(&link, &path.from_text("chain")?)?
    print(fs.read_link(&link)?.as_bytes() == target.as_bytes())?
    # Dot-dot is resolved after following alias, not lexically before it.
    resolved = fs.canonicalize(&path.from_text("chain/../file")?)?
    expected = root.join(&path.from_text("actual/file")?)?
    print(resolved.as_bytes() == expected.as_bytes(), resolved.is_absolute())?
    absolute = root.join(&target)?
    fs.symlink(&absolute, &path.from_text("absolute")?)?
    print(fs.canonicalize(&path.from_text("absolute/.")?)?.as_bytes() == absolute.as_bytes())?
    raw = path.from_bytes([98u8, 97u8, 100u8, 255u8]?)
    raw_link = path.from_bytes([108u8, 105u8, 110u8, 107u8, 254u8]?)
    fs.symlink(&raw, &raw_link)?
    print(fs.read_link(&raw_link)?.as_bytes() == raw.as_bytes())?
    raw_resolved = fs.canonicalize(&raw_link)?
    print(raw_resolved.as_bytes() == root.join(&raw)?.as_bytes())?
    print(raw_resolved.to_text().is_err())?
    Ok(())

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    with spawn(work).drop_job()? as task:
        task.join()?
    Ok(())
"#,
    );
    assert_eq!(
        program.run(fixture.path()),
        "True\nTrue True\nTrue\nTrue\nTrue\nTrue\n"
    );
    assert_eq!(
        std::fs::read_link(fixture.path().join("alias")).unwrap(),
        Path::new("actual/nested")
    );
}

#[test]
fn missing_components_loops_and_trailing_slashes_keep_native_errors() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("file"), "untouched").unwrap();
    let program = Program::new(
        r#"
import std.fs as fs
import std.path as path

def resolve(name: &str) -> Result[(), IoError]:
    match fs.canonicalize(&path.from_text(name)?):
        case Ok(_):
            print("unexpected success")?
        case Err(error):
            print(error)?
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    fs.symlink(&path.from_text("missing")?, &path.from_text("dangling")?)?
    print(fs.symlink_metadata(&path.from_text("dangling")?)?.is_symlink())?
    resolve("dangling")?
    resolve("missing/../file")?
    resolve("file/..")?
    resolve("file/")?
    fs.symlink(&path.from_text("loop-b")?, &path.from_text("loop-a")?)?
    fs.symlink(&path.from_text("loop-a")?, &path.from_text("loop-b")?)?
    resolve("loop-a")?
    print(str.repr(fs.symlink(&path.from_text("anything")?, &path.from_text("file")?))?)?
    print(str.repr(fs.symlink(&path.from_text("anything")?, &path.from_text("dangling")?))?)?
    resolve("")?
    Ok(())
"#,
    );
    assert_eq!(
        program.run(fixture.path()),
        concat!(
            "True\nIoError.System(2)\nIoError.System(2)\nIoError.System(20)\nIoError.System(20)\n",
            "IoError.System(40)\nResult[(), IoError].Err(IoError.System(17))\n",
            "Result[(), IoError].Err(IoError.System(17))\nIoError.System(2)\n"
        )
    );
    assert_eq!(
        std::fs::read_to_string(fixture.path().join("file")).unwrap(),
        "untouched"
    );
    assert_eq!(
        std::fs::read_link(fixture.path().join("dangling")).unwrap(),
        Path::new("missing")
    );
}

#[test]
fn oversized_or_nul_paths_fail_without_creating_a_link() {
    let fixture = tempfile::tempdir().unwrap();
    let program = Program::new(
        r#"
import std.fs as fs
import std.path as path

def report(result: Result[path.Path, IoError]) -> Result[(), IoError]:
    match result:
        case Ok(_):
            print("unexpected success")?
        case Err(error):
            print(error)?
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    nul = path.from_bytes([97u8, 0u8, 98u8]?)
    name = path.from_text("link")?
    report(fs.canonicalize(&nul))?
    print(str.repr(fs.symlink(&nul, &name))?)?
    print(str.repr(fs.symlink(&name, &nul))?)?
    long = path.from_text(&"a".repeat(4096)?)?
    report(fs.canonicalize(&long))?
    print(str.repr(fs.symlink(&long, &name))?)?
    print(str.repr(fs.symlink(&name, &long))?)?
    Ok(())
"#,
    );
    assert_eq!(
        program.run(fixture.path()),
        concat!(
            "IoError.InvalidInput\nResult[(), IoError].Err(IoError.InvalidInput)\n",
            "Result[(), IoError].Err(IoError.InvalidInput)\nIoError.System(36)\n",
            "Result[(), IoError].Err(IoError.System(36))\nResult[(), IoError].Err(IoError.System(36))\n"
        )
    );
    assert_eq!(
        std::fs::symlink_metadata(fixture.path().join("link"))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::NotFound
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn canonical_output_allocation_fails_recoverably_and_symlink_needs_no_heap() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("file"), "data").unwrap();
    let program = Program::new(
        r#"
import std.fs as fs
import std.path as path

def main() -> Result[(), AllocError | IoError]:
    file = path.from_text("file")?
    link = path.from_text("link")?
    print("__test_fail_allocations_after_0__").unwrap()
    created = fs.symlink(&file, &link)
    resolved = fs.canonicalize(&link)
    print("__test_restore_allocations__").unwrap()
    print(str.repr(created)?)?
    match resolved:
        case Ok(_):
            print("unexpected success")?
        case Err(error):
            print(error)?
    print(fs.canonicalize(&link)?.is_absolute())?
    Ok(())
"#,
    );
    let stdout = program.run(fixture.path());
    let visible: Vec<_> = stdout
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect();
    assert_eq!(
        visible,
        [
            "Result[(), IoError].Ok(())",
            "IoError.Data(DataError.Allocation(AllocError.OutOfMemory))",
            "True"
        ]
    );
}

#[test]
fn canonical_result_beyond_native_limit_is_rejected_without_truncation() {
    let fixture = tempfile::tempdir().unwrap();
    let mut short_path = fixture.path().to_path_buf();
    // The input uses short symlinks but its physical absolute name exceeds
    // PATH_MAX. Creating through each short alias avoids oversized syscalls.
    let long_name = "d".repeat(240);
    for _ in 0..18 {
        std::fs::create_dir(short_path.join(&long_name)).unwrap();
        std::os::unix::fs::symlink(&long_name, short_path.join("s")).unwrap();
        short_path.push("s");
    }
    let input = std::iter::repeat_n("s", 18).collect::<Vec<_>>().join("/");
    let source = format!(
        r#"
import std.fs as fs
import std.path as path

def main() -> Result[(), AllocError | IoError]:
    match fs.canonicalize(&path.from_text("{input}")?):
        case Ok(_):
            print("unexpected success")?
        case Err(error):
            print(error)?
    Ok(())
"#
    );
    assert_eq!(
        Program::new(&source).run(fixture.path()),
        "IoError.System(36)\n"
    );
}
