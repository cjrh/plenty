//! Private creation and descriptor-based permission changes.
#![cfg(all(target_os = "linux", target_pointer_width = "64"))]
use std::os::unix::{fs::PermissionsExt, process::CommandExt};
use std::path::{Path, PathBuf};
use std::process::Command;

mod support;

struct Program {
    _directory: tempfile::TempDir,
    executable: PathBuf,
}
impl Program {
    fn new(declarations: &str, body: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let source = format!(
            "import std.fs as fs\nimport std.path as path\nimport std.io as io\nimport std.bytes\nfrom std.thread import spawn\ndef show[T, E](value: Result[T, E]) -> Result[(), IoError]:\n    match value:\n        case Ok(_):\n            print(\"Ok\")?\n        case Err(error):\n            print(error)?\n    Ok(())\n{declarations}\ndef main() -> Result[(), AllocError | IoError | io.StreamError | SpawnError[()]]:\n    argv = args_bytes()?\n    location = path.from_bytes(copy(argv[1])?)\n    parent = fs.root(&location)?\n{body}\n    Ok(())\n"
        );
        let entry = support::write_main(directory.path(), &source);
        let executable = directory.path().join("program");
        plenty::compile_file_to_executable(&entry, &executable, None).unwrap();
        Self {
            _directory: directory,
            executable,
        }
    }
    fn run(&self, path: &Path, mask: u32) -> String {
        let mut command = Command::new(&self.executable);
        command.arg(path);
        // SAFETY: the child sets its own mask before exec using only the
        // async-signal-safe syscall; the parallel test harness is unaffected.
        unsafe {
            command.pre_exec(move || {
                unsafe extern "C" {
                    fn umask(mode: u32) -> u32;
                }
                umask(mask);
                Ok(())
            });
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    }
}
fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn creation_modes_are_applied_at_creation_and_restricted_by_umask() {
    let program = Program::new(
        "",
        r#"
    parent.create_dir(&path.from_text("private-dir")?)?
    parent.create_dir(&path.from_text("shared-dir")?, unix_mode=0o770u32)?
    mut private = parent.create_file(&path.from_text("private")?)?
    mut shared = parent.create_file(&path.from_text("shared")?, unix_mode=0o666u32)?
    private.file.write("secret")?
    shared.file.write("shared")?
    drop(private.seek(io.SeekFrom.Start(0))?)
    mut buffer = std.bytes.with_capacity(6)?
    buffer.resize(6, 0u8)?
    if private.read(&mut buffer, 0, 6)? != 6:
        return Err(IoError.InvalidInput)
    if buffer.get(0) != 115u8 or buffer.get(5) != 116u8:
        return Err(IoError.InvalidInput)
    private.close()?
    shared.close()?
"#,
    );
    for mask in [0, 0o022, 0o077, 0o777] {
        let fixture = tempfile::tempdir().unwrap();
        program.run(fixture.path(), mask);
        for (name, requested) in [
            ("private-dir", 0o700),
            ("shared-dir", 0o770),
            ("private", 0o600),
            ("shared", 0o666),
        ] {
            assert_eq!(mode(&fixture.path().join(name)), requested & !mask);
        }
        // Restore test-owned directories so TempDir can traverse for cleanup.
        for name in ["private-dir", "shared-dir"] {
            std::fs::set_permissions(
                fixture.path().join(name),
                std::fs::Permissions::from_mode(0o700),
            )
            .unwrap();
        }
        // Restore test-owned files for content checks even under umask 0777.
        for name in ["private", "shared"] {
            std::fs::set_permissions(
                fixture.path().join(name),
                std::fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        }
        assert_eq!(
            std::fs::read(fixture.path().join("private")).unwrap(),
            b"secret"
        );
        assert_eq!(
            std::fs::read(fixture.path().join("shared")).unwrap(),
            b"shared"
        );
    }
}

#[test]
fn existing_names_and_symlinks_are_never_replaced_or_chmodded() {
    let fixture = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("existing"), b"keep").unwrap();
    std::fs::set_permissions(
        fixture.path().join("existing"),
        std::fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    std::fs::create_dir(fixture.path().join("directory")).unwrap();
    std::fs::write(outside.path().join("target"), b"outside").unwrap();
    std::os::unix::fs::symlink(outside.path().join("target"), fixture.path().join("link")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("missing"),
        fixture.path().join("dangling"),
    )
    .unwrap();
    let program = Program::new(
        "",
        r#"
    for name in ["existing", "directory", "link", "dangling"]?:
        child = path.from_text(name)?
        show(parent.create_file(&child))?
        show(parent.create_dir(&child))?
    for name in ["", ".", "..", "child/name", "nul\0name"]?:
        child = path.from_text(name)?
        show(parent.create_file(&child))?
        show(parent.create_dir(&child))?
"#,
    );
    let output = program.run(fixture.path(), 0);
    assert_eq!(output.matches("IoError.System(17)").count(), 8, "{output}");
    assert_eq!(
        output.matches("IoError.InvalidInput").count(),
        10,
        "{output}"
    );
    assert_eq!(
        std::fs::read(fixture.path().join("existing")).unwrap(),
        b"keep"
    );
    assert_eq!(mode(&fixture.path().join("existing")), 0o640);
    assert_eq!(
        std::fs::read(outside.path().join("target")).unwrap(),
        b"outside"
    );
    assert!(!outside.path().join("missing").exists());
}

#[test]
fn permission_updates_follow_owned_inodes_not_replacement_paths() {
    let program = Program::new(
        "",
        r#"
    child = path.from_text("child")?
    held = path.from_text("held")?
    parent.create_dir(&child)?
    directory = parent.open_dir(&child)?
    parent.move(&child, &held)?
    parent.create_dir(&child)?
    directory.set_unix_mode(0o750u32)?
    name = path.from_text("file")?
    moved = path.from_text("moved")?
    mut stream = directory.create_file(&name)?
    stream.file.write("original")?
    directory.move(&name, &moved)?
    mut replacement = directory.create_file(&name)?
    replacement.file.write("replacement")?
    fs.set_file_unix_mode(&stream, 0o400u32)?
    stream.close()?
    show(fs.set_file_unix_mode(&stream, 0o600u32))?
    replacement.close()?
"#,
    );
    let fixture = tempfile::tempdir().unwrap();
    let output = program.run(fixture.path(), 0o077);
    assert_eq!(output, "IoError.Closed\n");
    assert_eq!(mode(&fixture.path().join("held")), 0o750);
    assert_eq!(mode(&fixture.path().join("child")), 0o700);
    assert_eq!(mode(&fixture.path().join("held/moved")), 0o400);
    assert_eq!(mode(&fixture.path().join("held/file")), 0o600);
    assert_eq!(
        std::fs::read(fixture.path().join("held/moved")).unwrap(),
        b"original"
    );
}

#[test]
fn invalid_modes_fail_before_creation_or_permission_changes() {
    let program = Program::new(
        "",
        r#"
    name = path.from_text("invalid")?
    mut stream = parent.create_file(&path.from_text("valid")?)?
    for bits in [0o1000u32, 0o4000u32, 0o100600u32, u32.MAX]?:
        show(parent.create_file(&name, unix_mode=bits))?
        show(parent.create_dir(&name, unix_mode=bits))?
        show(parent.set_unix_mode(bits))?
        show(fs.set_file_unix_mode(&stream, bits))?
    fs.set_file_unix_mode(&stream, 0u32)?
    fs.set_file_unix_mode(&stream, 0o640u32)?
    stream.close()?
"#,
    );
    let fixture = tempfile::tempdir().unwrap();
    let original_mode = mode(fixture.path());
    let output = program.run(fixture.path(), 0);
    assert_eq!(
        output.matches("IoError.InvalidInput").count(),
        16,
        "{output}"
    );
    assert!(!fixture.path().join("invalid").exists());
    assert_eq!(mode(&fixture.path().join("valid")), 0o640);
    assert_eq!(mode(fixture.path()), original_mode);
}

#[test]
fn moved_root_can_create_and_change_permissions_on_a_worker() {
    let program = Program::new(
        r#"
def worker(parent: fs.Root) -> Result[(), AllocError | IoError | io.StreamError]:
    parent.create_dir(&path.from_text("worker")?)?
    directory = parent.open_dir(&path.from_text("worker")?)?
    directory.set_unix_mode(0o700u32)?
    mut stream = directory.create_file(&path.from_text("file")?)?
    fs.set_file_unix_mode(&stream, 0o600u32)?
    stream.file.write("worker")?
    stream.close()?
    Ok(())
"#,
        r#"
    with spawn(worker, parent).drop_job()? as task:
        task.join()?
"#,
    );
    let fixture = tempfile::tempdir().unwrap();
    program.run(fixture.path(), 0);
    assert_eq!(
        std::fs::read(fixture.path().join("worker/file")).unwrap(),
        b"worker"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn owner_allocation_failure_cannot_leave_created_file() {
    let program = Program::new(r#"
def attempt(parent: &fs.Root, name: &path.Path, marker: &str) -> Result[(), IoError]:
    print(marker).unwrap()
    result = parent.create_file(name)
    print("__test_restore_allocations__").unwrap()
    match result:
        case Ok(stream):
            drop(stream)
            parent.remove_file(name)?
        case Err(_):
            match parent.read_file(name):
                case Ok(_):
                    return Err(IoError.InvalidInput)
                case Err(_):
                    pass
    Ok(())
"#, &(0..12).map(|budget| format!("    attempt(&parent, &path.from_text(\"created\")?, \"__test_fail_allocations_after_{budget}__\")?\n")).collect::<String>());
    let fixture = tempfile::tempdir().unwrap();
    program.run(fixture.path(), 0);
    assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 0);
}
