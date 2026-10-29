//! Public embedded fs/path modules, native path identity and resource lifetime.
#![cfg(all(target_os = "linux", target_pointer_width = "64"))]
use std::os::unix::{ffi::OsStrExt, fs::PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
        plenty::check_file(&entry, None).unwrap_or_else(|e| panic!("{e}\n{source}"));
        plenty::compile_file_to_executable(&entry, &executable, None)
            .unwrap_or_else(|e| panic!("{e}\n{source}"));
        Self {
            _directory: directory,
            executable,
        }
    }
    fn run(&self, root: &Path) -> Output {
        let output = Command::new(&self.executable).arg(root).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        output
    }
}

const DESCRIPTORS: &str = r#"
def descriptors() -> Result[i64, AllocError | IoError]:
    root = path.from_text("/proc/self/fd")?
    mut directory = fs.read_dir(&root)?
    mut count = 0
    while True:
        match directory.next()?:
            case Some(_):
                count = count + 1
            case Nothing:
                break
    Ok(count)
"#;

#[test]
fn entries_are_detached_and_preserve_raw_names_and_special_types() {
    let fixture = tempfile::tempdir().unwrap();
    let names: &[&[u8]] = &[
        b"file",
        b".hidden",
        "é".as_bytes(),
        b"bad\xff",
        b"line\nCR\rtab\t space",
    ];
    for name in names {
        std::fs::write(
            fixture.path().join(std::ffi::OsStr::from_bytes(name)),
            b"abc",
        )
        .unwrap();
    }
    std::fs::create_dir(fixture.path().join("dir")).unwrap();
    std::os::unix::fs::symlink("file", fixture.path().join("link")).unwrap();
    std::os::unix::fs::symlink("missing", fixture.path().join("dangling")).unwrap();
    std::os::unix::fs::symlink("dir", fixture.path().join("dirlink")).unwrap();
    let socket = std::os::unix::net::UnixListener::bind(fixture.path().join("socket")).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(fixture.path().join("fifo"))
            .status()
            .unwrap()
            .success()
    );
    let source = format!(
        r#"
import std.fs as fs
import std.path as path
{DESCRIPTORS}
def collect(root: &path.Path) -> Result[list[fs.DirEntry], AllocError | IoError]:
    mut saved = list[fs.DirEntry].new()?
    mut directory = fs.read_dir(root)?
    while True:
        match directory.next()?:
            case Some(entry):
                saved.append(entry)?
            case Nothing:
                break
    directory.close()?
    directory.close()?
    match directory.next()?:
        case Some(_):
            return Err(IoError.Closed)
        case Nothing:
            pass
    Ok(saved)

def main() -> Result[(), AllocError | IoError | fs.WalkError]:
    argv = args_bytes()?
    root = path.from_bytes(copy(argv[1])?)
    before = descriptors()?
    entries = collect(&root)?
    print(descriptors()? == before)?
    for entry in &entries:
        name = entry.file_name()?
        write_stdout_bytes(name.as_bytes())?
        write_stdout_bytes(encode_utf8("\0")?)?
        print(entry.file_type()?)?
        info = entry.metadata()?
        print(info.file_type() == entry.file_type()?)?
    Ok(())
"#
    );
    let output = Program::new(&source).run(fixture.path());
    assert!(output.stdout.starts_with(b"True\n"));
    // Decode framing explicitly; embedded line terminators belong to the name.
    let mut data = &output.stdout[5..];
    let mut seen = std::collections::BTreeMap::new();
    while !data.is_empty() {
        let end = data.iter().position(|b| *b == 0).unwrap();
        let name = data[..end].to_vec();
        data = &data[end + 1..];
        let end = data.iter().position(|b| *b == b'\n').unwrap();
        let kind = String::from_utf8(data[..end].to_vec()).unwrap();
        data = &data[end + 1..];
        assert!(data.starts_with(b"True\n"));
        data = &data[5..];
        seen.insert(name, kind);
    }
    assert_eq!(seen.len(), names.len() + 6);
    for name in names {
        assert_eq!(seen[*name], "std.fs.FileType.File");
    }
    assert_eq!(seen[b"dir".as_slice()], "std.fs.FileType.Directory");
    for name in [b"link".as_slice(), b"dangling", b"dirlink"] {
        assert_eq!(seen[name], "std.fs.FileType.Symlink");
    }
    for name in [b"socket".as_slice(), b"fifo"] {
        assert_eq!(seen[name], "std.fs.FileType.Other");
    }
    drop(socket);
}

#[test]
fn metadata_follows_links_only_at_the_requested_boundary() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("file"), b"abc").unwrap();
    std::os::unix::fs::symlink("file", fixture.path().join("link")).unwrap();
    std::os::unix::fs::symlink("missing", fixture.path().join("dangling")).unwrap();
    let program = Program::new(
        r#"
import std.fs as fs
import std.path as path
def main() -> Result[(), AllocError | IoError | fs.WalkError]:
    argv = args_bytes()?
    root = path.from_bytes(copy(argv[1])?)
    link = root.join(&path.from_text("link")?)?
    info = fs.metadata(&link)?
    print(info.is_file())?
    print(info.size())?
    print(info.modified())?
    print(info.accessed())?
    print(info.permissions().unix_mode > 0u32)?
    print(fs.symlink_metadata(&link)?.is_symlink())?
    target = fs.read_link(&link)?
    print(target.to_text()?)?
    dangling = root.join(&path.from_text("dangling")?)?
    print(fs.symlink_metadata(&dangling)?.is_symlink())?
    match fs.metadata(&dangling):
        case Ok(_):
            return Err(IoError.Closed)
        case Err(error):
            print(error)?
    Ok(())
"#,
    );
    let output = String::from_utf8(program.run(fixture.path()).stdout).unwrap();
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(&lines[..2], ["True", "3"]);
    assert!(lines[2].contains("Some"));
    assert!(lines[3].contains("Some"));
    assert_eq!(
        &lines[4..],
        ["True", "True", "file", "True", "IoError.System(2)"]
    );
}

#[test]
fn lexical_paths_keep_identity_and_report_checked_text_conversion() {
    let program = Program::new(
        r#"
import std.path as path
def main() -> Result[(), AllocError | IoError]:
    base = path.from_text("a/link/..")?
    child = path.from_text("x")?
    joined = base.join(&child)?
    print(joined.to_text()?)?
    print(joined.components()?)?
    print(joined.is_absolute())?
    match joined.parent()?:
        case Some(parent):
            print(parent.to_text()?)?
        case Nothing:
            return Err(IoError.Closed)
    match joined.file_name()?:
        case Some(name):
            print(name.to_text()?)?
        case Nothing:
            return Err(IoError.Closed)
    absolute = base.join(&path.from_text("/root")?)?
    print(absolute.to_text()?)?
    raw = path.from_bytes([255u8, 10u8]?)
    print(raw.display()?)?
    match raw.to_text():
        case Ok(_):
            return Err(IoError.Closed)
        case Err(error):
            print(error)?
    Ok(())
"#,
    );
    let output = String::from_utf8(program.run(Path::new(".")).stdout).unwrap();
    assert_eq!(
        output,
        "a/link/../x\n[[97], [108, 105, 110, 107], [46, 46], [120]]\nFalse\na/link/..\nx\n/root\n\\xff\\x0a\nIoError.Data(DataError.InvalidUtf8)\n"
    );
}

fn walking_program(prune: bool, top_down: bool, inject_missing: bool) -> Program {
    Program::new(&format!(
        r#"
import std.fs as fs
import std.path as path
{DESCRIPTORS}
def main() -> Result[(), AllocError | IoError | fs.WalkError]:
    argv = args_bytes()?
    root = path.from_bytes(copy(argv[1])?)
    before = descriptors()?
    mut walker = fs.walk(&root, {top_down})?
    mut injected = False
    while True:
        match walker.next():
            case Ok(found):
                if not found:
                    break
                print(descriptors()? == before)?
                print(walker.current.path.to_text()?)?
                if {prune}:
                    mut index = len(walker.current.directories) - 1
                    while index >= 0:
                        if walker.current.directories[index].to_text()?.endswith("/pruned"):
                            drop(walker.current.directories.pop(index))
                        index = index - 1
                if {inject_missing} and not injected:
                    walker.current.directories.append(root.join(&path.from_text("missing")?)?)?
                    walker.current.directories.reverse()
                    injected = True
            case Err(error):
                match error:
                    case fs.WalkError.Io(at, operation, cause):
                        print(at.to_text()?)?
                        print(operation)?
                        print(cause)?
                    case fs.WalkError.Allocation(error):
                        print(error)?
                        return Err(IoError.Closed)
    walker.close()
    print(walker.next()?)?
    print(descriptors()? == before)?
    Ok(())
"#,
        top_down = if top_down { "True" } else { "False" },
        prune = if prune { "True" } else { "False" },
        inject_missing = if inject_missing { "True" } else { "False" }
    ))
}

#[test]
fn walking_orders_prunes_and_recovers_without_following_child_links() {
    let fixture = tempfile::tempdir().unwrap();
    for directory in ["a/deep", "b", "pruned/sub"] {
        std::fs::create_dir_all(fixture.path().join(directory)).unwrap();
    }
    std::os::unix::fs::symlink("..", fixture.path().join("a/ancestor")).unwrap();
    std::os::unix::fs::symlink("a", fixture.path().join("linked")).unwrap();
    std::fs::set_permissions(
        fixture.path().join("pruned"),
        std::fs::Permissions::from_mode(0o0),
    )
    .unwrap();
    let output =
        String::from_utf8(walking_program(true, true, true).run(fixture.path()).stdout).unwrap();
    std::fs::set_permissions(
        fixture.path().join("pruned"),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let root = fixture.path().display().to_string();
    // Check names relative to the fixture root: the root itself may contain
    // them, for example a TMPDIR under a directory named `linked-*`.
    for name in ["/pruned", "/linked", "/ancestor"] {
        assert!(
            !output.contains(&format!("{root}{name}")),
            "{name} in {output}"
        );
    }
    assert!(output.contains(&format!("{root}/missing\nread_dir\nIoError.System(2)\n")));
    let position = |name: &str| output.find(&format!("{root}{name}\n")).unwrap();
    assert!(position("") < position("/a"));
    assert!(position("/a") < position("/a/deep"));
    assert!(output.ends_with("False\nTrue\n"));
    let output = String::from_utf8(
        walking_program(false, false, false)
            .run(fixture.path())
            .stdout,
    )
    .unwrap();
    let position = |name: &str| output.find(&format!("{root}{name}\n")).unwrap();
    assert!(position("/a/deep") < position("/a"));
    assert!(position("/a") < position(""));
    assert!(position("/b") < position(""));
    assert!(output.ends_with("False\nTrue\n"));
}

#[test]
fn empty_missing_nondirectory_and_denied_roots_distinguish_eof_from_errors() {
    let fixture = tempfile::tempdir().unwrap();
    let empty = fixture.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    let file = fixture.path().join("file");
    std::fs::write(&file, b"x").unwrap();
    let program = Program::new(
        r#"
import std.fs as fs
import std.path as path
def main() -> Result[(), AllocError | IoError | fs.WalkError]:
    argv = args_bytes()?
    root = path.from_bytes(copy(argv[1])?)
    match fs.read_dir(&root):
        case Ok(owner):
            mut directory = owner
            match directory.next()?:
                case Some(_):
                    print("entry")?
                case Nothing:
                    print("EOF")?
            print(directory.closed())?
        case Err(error):
            print(error)?
    Ok(())
"#,
    );
    for root in [&empty, &empty.join(""), Path::new("empty")] {
        let output = if root.is_absolute() {
            program.run(root)
        } else {
            let output = Command::new(&program.executable)
                .arg(root)
                .current_dir(fixture.path())
                .output()
                .unwrap();
            assert!(output.status.success());
            output
        };
        assert_eq!(output.stdout, b"EOF\nTrue\n");
    }
    assert_eq!(program.run(&file).stdout, b"IoError.System(20)\n");
    assert_eq!(
        program.run(&fixture.path().join("missing")).stdout,
        b"IoError.System(2)\n"
    );
    std::fs::set_permissions(&empty, std::fs::Permissions::from_mode(0o0)).unwrap();
    let actually_denied = std::fs::read_dir(&empty).is_err();
    if actually_denied {
        assert_eq!(program.run(&empty).stdout, b"IoError.System(13)\n");
    }
    std::fs::set_permissions(&empty, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        actually_denied,
        "permission regression needs an unprivileged test process"
    );
}

#[test]
fn cleanup_on_break_return_and_propagated_error_leaves_no_directory_handles() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("file"), b"x").unwrap();
    let source = format!(
        r#"
import std.fs as fs
import std.path as path
{DESCRIPTORS}
def early(root: &path.Path) -> Result[(), AllocError | IoError]:
    directory = fs.read_dir(root)?
    return Ok(())
def failed(root: &path.Path) -> Result[(), AllocError | IoError]:
    directory = fs.read_dir(root)?
    missing = root.join(&path.from_text("missing")?)?
    drop(fs.metadata(&missing)?)
    Ok(())
def main() -> Result[(), AllocError | IoError | fs.WalkError]:
    argv = args_bytes()?
    root = path.from_bytes(copy(argv[1])?)
    before = descriptors()?
    for repetition in range(100):
        for result in fs.entries(fs.read_dir(&root)?):
            drop(result?)
            break
        early(&root)?
        match failed(&root):
            case Ok(_):
                return Err(IoError.Closed)
            case Err(_):
                pass
    print(descriptors()? == before)?
    Ok(())
"#
    );
    assert_eq!(Program::new(&source).run(fixture.path()).stdout, b"True\n");
}

#[test]
fn large_flat_and_deep_trees_keep_directory_handles_bounded() {
    let fixture = tempfile::tempdir().unwrap();
    let flat = fixture.path().join("flat");
    std::fs::create_dir(&flat).unwrap();
    for index in 0..2048 {
        std::fs::write(flat.join(format!("file-{index}")), b"x").unwrap();
    }
    let deep = fixture.path().join("deep");
    let mut child = deep.clone();
    std::fs::create_dir(&child).unwrap();
    for _ in 0..128 {
        child.push("d");
        std::fs::create_dir(&child).unwrap();
    }
    let source = format!(
        r#"
import std.fs as fs
import std.path as path
{DESCRIPTORS}
def main() -> Result[(), AllocError | IoError | fs.WalkError]:
    argv = args_bytes()?
    root = path.from_bytes(copy(argv[1])?)
    before = descriptors()?
    mut walker = fs.walk(&root, False)?
    mut rows = 0
    mut files = 0
    mut bounded = True
    while walker.next()?:
        rows = rows + 1
        files = files + len(walker.current.files)
        bounded = bounded and descriptors()? == before
    print(rows)?
    print(files)?
    print(bounded)?
    Ok(())
"#
    );
    let program = Program::new(&source);
    assert_eq!(program.run(&flat).stdout, b"1\n2048\nTrue\n");
    assert_eq!(program.run(&deep).stdout, b"129\n0\nTrue\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_during_advance_is_terminal_and_releases_the_handle() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("long-enough-name"), b"x").unwrap();
    let source = format!(
        r#"
import std.fs as fs
import std.path as path
{DESCRIPTORS}
def main() -> Result[(), AllocError | IoError | fs.WalkError]:
    argv = args_bytes()?
    root = path.from_bytes(copy(argv[1])?)
    before = descriptors()?
    mut directory = fs.read_dir(&root)?
    print("__test_fail_allocations_after_0__").unwrap()
    construction = fs.read_dir(&root)
    print("__test_restore_allocations__").unwrap()
    match construction:
        case Ok(_):
            return Err(IoError.Closed)
        case Err(error):
            print(error)?
    print(descriptors()? == before + 1)?
    print("__test_fail_allocations_after_0__").unwrap()
    result = directory.next()
    print("__test_restore_allocations__").unwrap()
    match result:
        case Ok(_):
            return Err(IoError.Closed)
        case Err(error):
            print(error)?
    print(directory.closed())?
    print(descriptors()? == before)?
    Ok(())
"#
    );
    let output = String::from_utf8(Program::new(&source).run(fixture.path()).stdout).unwrap();
    assert!(
        output
            .ends_with("IoError.Data(DataError.Allocation(AllocError.OutOfMemory))\nTrue\nTrue\n")
    );
}
