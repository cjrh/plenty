//! Ordinary-path creation deliberately has different lookup rules from Root.
#![cfg(all(target_os = "linux", target_pointer_width = "64"))]
use std::os::unix::{ffi::OsStrExt, fs::PermissionsExt, process::CommandExt};
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
            "import std.fs as fs\nimport std.path as path\nfrom std.thread import spawn\ndef show(value: Result[(), IoError]) -> Result[(), IoError]:\n    match value:\n        case Ok(_):\n            print(\"Ok\")?\n        case Err(error):\n            print(error)?\n    Ok(())\n{declarations}\ndef main() -> Result[(), AllocError | IoError | SpawnError[()]]:\n    argv = args_bytes()?\n    location = path.from_bytes(copy(argv[1])?)\n{body}\n    Ok(())\n"
        );
        let entry = support::write_main(directory.path(), &source);
        let executable = directory.path().join("program");
        plenty::compile_file_to_executable(&entry, &executable, None).unwrap();
        Self {
            _directory: directory,
            executable,
        }
    }
    fn run(&self, cwd: &Path, argument: &Path, mask: u32) -> String {
        let mut command = Command::new(&self.executable);
        command.current_dir(cwd).arg(argument);
        #[repr(C)]
        struct Limit {
            current: u64,
            maximum: u64,
        }
        // SAFETY: only async-signal-safe syscalls run before exec; no change
        // to the parallel test harness's process-global umask.
        unsafe {
            command.pre_exec(move || {
                unsafe extern "C" {
                    fn umask(mode: u32) -> u32;
                    fn setrlimit(resource: i32, limit: *const Limit) -> i32;
                }
                umask(mask);
                // All runs have enough descriptors for stdio, but not leaks.
                if setrlimit(
                    7,
                    &Limit {
                        current: 16,
                        maximum: 16,
                    },
                ) != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    }
}
fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn ordinary_creation_modes_paths_and_worker() {
    let program = Program::new(
        r#"
def worker(name: path.Path) -> Result[(), IoError]:
    fs.create_dirs(&name)
"#,
        r#"
    fs.create_dirs(&location, unix_mode=0o750u32)?
    fs.create_dirs(&location)?
    for text in [".", "/", "///", "./relative//a/", "relative/a/../b", "private/a"]?:
        fs.create_dirs(&path.from_text(text)?)?
    with spawn(worker, path.from_text("worker/a")?).drop_job()? as task:
        task.join()?
"#,
    );
    for mask in [0, 0o077] {
        let fixture = tempfile::tempdir().unwrap();
        let existing = fixture.path().join("existing");
        std::fs::create_dir(&existing).unwrap();
        std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o711)).unwrap();
        // Native-byte absolute path plus ordinary relative paths in the child.
        let native = Path::new(std::ffi::OsStr::from_bytes(b"nonutf8-\xff"));
        let target = existing.join(native).join("leaf");
        program.run(fixture.path(), &target, mask);
        assert_eq!(mode(&existing), 0o711);
        for path in [existing.join(native), target] {
            assert_eq!(mode(&path), 0o750 & !mask);
        }
        for path in [
            "private",
            "private/a",
            "relative/a",
            "relative/b",
            "worker/a",
        ] {
            assert_eq!(mode(&fixture.path().join(path)), 0o700 & !mask);
        }
    }
}

#[test]
fn ordinary_links_and_dot_segments_are_resolved_in_order() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(fixture.path().join("target/a")).unwrap();
    std::fs::set_permissions(
        fixture.path().join("target/a"),
        std::fs::Permissions::from_mode(0o751),
    )
    .unwrap();
    std::os::unix::fs::symlink("target/a", fixture.path().join("link")).unwrap();
    std::os::unix::fs::symlink("missing/target", fixture.path().join("dangling")).unwrap();
    std::os::unix::fs::symlink("loop", fixture.path().join("loop")).unwrap();
    std::fs::write(fixture.path().join("file"), b"keep").unwrap();
    let program = Program::new("", &r#"
    for text in ["link", "link/child", "link/../cache", "new/../cache"]?:
        fs.create_dirs(&path.from_text(text)?)?
    for text in ["dangling", "dangling/child", "loop", "loop/child", "file", "file/child", "partial/child/LONG"]?:
        show(fs.create_dirs(&path.from_text(text)?))?
"#.replace("LONG", &"x".repeat(300)));
    let output = program.run(fixture.path(), Path::new("."), 0);
    assert_eq!(
        output,
        "IoError.System(2)\nIoError.System(2)\nIoError.System(40)\nIoError.System(40)\nIoError.System(20)\nIoError.System(20)\nIoError.System(36)\n"
    );
    for path in [
        "target/a/child",
        "target/cache",
        "new",
        "cache",
        "partial/child",
    ] {
        assert!(fixture.path().join(path).is_dir(), "{path}");
    }
    assert_eq!(mode(&fixture.path().join("target/a")), 0o751);
    assert!(!fixture.path().join("missing").exists());
    assert!(fixture.path().join("dangling").is_symlink());
    assert!(fixture.path().join("loop").is_symlink());
    assert_eq!(std::fs::read(fixture.path().join("file")).unwrap(), b"keep");
}

#[test]
fn ordinary_invalid_arguments_precede_all_mutation() {
    let fixture = tempfile::tempdir().unwrap();
    let program = Program::new(
        "",
        r#"
    for text in ["", "new/a/\0bad", "new/b\0"]?:
        show(fs.create_dirs(&path.from_text(text)?))?
    for text in [".", "new/a"]?:
        show(fs.create_dirs(&path.from_text(text)?, unix_mode=0o1000u32))?
        show(fs.create_dirs(&path.from_text(text)?, unix_mode=u32.MAX))?
"#,
    );
    assert_eq!(
        program.run(fixture.path(), Path::new("."), 0),
        "IoError.InvalidInput\n".repeat(7)
    );
    assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 0);
}

#[test]
fn ordinary_creation_needs_no_directory_read_permission() {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    // Permission-denial assertions are meaningless for privileged execution.
    if unsafe { geteuid() } == 0 {
        return;
    }
    let fixture = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.path().join("searchable")).unwrap();
    std::fs::set_permissions(
        fixture.path().join("searchable"),
        std::fs::Permissions::from_mode(0o300),
    )
    .unwrap();
    let program = Program::new(
        "",
        r#"
    fs.create_dirs(&path.from_text("searchable/a/b")?, unix_mode=0o300u32)?
    fs.create_dirs(&path.from_text("unreadable")?, unix_mode=0o000u32)?
    show(fs.create_dirs(&path.from_text("blocked/child")?, unix_mode=0o000u32))?
"#,
    );
    let output = program.run(fixture.path(), Path::new("."), 0);
    assert_eq!(output, "IoError.System(13)\n");
    for path in [
        "searchable",
        "searchable/a",
        "searchable/a/b",
        "unreadable",
        "blocked",
    ] {
        let path = fixture.path().join(path);
        let expected = if path.ends_with("unreadable") || path.ends_with("blocked") {
            0
        } else {
            0o300
        };
        assert_eq!(mode(&path), expected);
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    assert!(!fixture.path().join("blocked/child").exists());
}

#[test]
fn ordinary_concurrent_creators_verify_existing_entries() {
    let program = Program::new("", "    show(fs.create_dirs(&location))?");
    for _ in 0..5 {
        let fixture = tempfile::tempdir().unwrap();
        let target = fixture.path().join("a/b/c/d/e/f");
        std::thread::scope(|scope| {
            let runs: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| program.run(fixture.path(), &target, 0)))
                .collect();
            for run in runs {
                assert_eq!(run.join().unwrap(), "Ok\n");
            }
        });
        std::fs::remove_dir(&target).unwrap();
        std::fs::write(&target, b"replacement").unwrap();
        assert_eq!(
            program.run(fixture.path(), &target, 0),
            "IoError.System(20)\n"
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn ordinary_allocation_failures_are_reported_without_leaks() {
    let program = Program::new(
        r#"
def attempt(name: &path.Path, marker: &str) -> Result[(), IoError]:
    print(marker).unwrap()
    result = fs.create_dirs(name)
    print("__test_restore_allocations__").unwrap()
    match result:
        case Ok(_):
            print("ok")?
        case Err(IoError.Data(DataError.Allocation(_))):
            print("oom")?
        case Err(error):
            return Err(error)
    Ok(())
"#,
        &(0..20)
            .map(|budget| {
                format!("    attempt(&location, \"__test_fail_allocations_after_{budget}__\")?\n")
            })
            .collect::<String>(),
    );
    let fixture = tempfile::tempdir().unwrap();
    let output = program.run(fixture.path(), Path::new("a/b/c"), 0);
    assert!(output.lines().any(|line| line == "oom"));
    assert!(output.lines().any(|line| line == "ok"));
    assert!(fixture.path().join("a/b/c").is_dir());
}

#[test]
fn ordinary_native_failures_do_not_leak_descriptors() {
    let program = Program::new(
        "",
        r#"
    for _ in range(100):
        for text in ["file/child", "loop/child", "dangling/child"]?:
            show(fs.create_dirs(&path.from_text(text)?))?
        fs.create_dirs(&path.from_text("good/a/b")?)?
"#,
    );
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("file"), b"keep").unwrap();
    std::os::unix::fs::symlink("loop", fixture.path().join("loop")).unwrap();
    std::os::unix::fs::symlink("missing", fixture.path().join("dangling")).unwrap();
    let output = program.run(fixture.path(), Path::new("."), 0);
    assert_eq!(
        output,
        "IoError.System(20)\nIoError.System(40)\nIoError.System(2)\n".repeat(100)
    );
}
