//! Descriptor-rooted mutation, anonymous staging and checked Plenty policies.
#![cfg(all(target_os = "linux", target_pointer_width = "64"))]
use std::os::unix::{ffi::OsStrExt, fs::PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod support;

struct Program {
    _directory: tempfile::TempDir,
    executable: PathBuf,
}
impl Program {
    fn new(declarations: &str, body: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().join("main.plenty");
        let executable = directory.path().join("program");
        let source = format!(
            "import std.fs as fs\nimport std.path as path\nimport std.io as io\nimport std.bytes\nfrom std.sync import CancellationToken\ndef show[T, E](value: Result[T, E]) -> Result[(), AllocError | IoError]:\n    match value:\n        case Ok(_):\n            print(\"Ok\")?\n        case Err(error):\n            print(error)?\n    Ok(())\n{declarations}\ndef main() -> Result[(), AllocError | IoError | fs.FsError]:\n    argv = args_bytes()?\n    location = path.from_bytes(copy(argv[1])?)\n    parent = fs.root(&location)?\n{body}\n    Ok(())\n"
        );
        std::fs::write(&entry, &source).unwrap();
        plenty::compile_file_to_executable(&entry, &executable, None)
            .unwrap_or_else(|error| panic!("{error}\n{source}"));
        Self {
            _directory: directory,
            executable,
        }
    }
    fn run(&self, path: &Path) -> Output {
        let output = Command::new(&self.executable).arg(path).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        output
    }
}

#[test]
fn anonymous_copy_is_lossless_bounded_and_never_overwrites() {
    let fixture = tempfile::tempdir().unwrap();
    let raw = std::ffi::OsStr::from_bytes(b"raw\xff");
    std::fs::write(fixture.path().join(raw), [0, 255, 13, 10, 128]).unwrap();
    std::fs::write(fixture.path().join("existing"), b"keep").unwrap();
    let program = Program::new(
        "",
        r#"
    source = path.from_bytes([114u8, 97u8, 119u8, 255u8]?)
    destination = path.from_text("copy")?
    existing = path.from_text("existing")?
    limit = path.from_text("limited")?
    cancel = CancellationToken()?
    print(fs.copy_file(&parent, &source, &destination, 5, &cancel)?)?
    show(fs.copy_file(&parent, &source, &existing, 5, &cancel))?
    show(fs.copy_file(&parent, &source, &limit, 4, &cancel))?
    renamed = path.from_text("renamed")?
    parent.move(&destination, &renamed)?
    show(parent.move(&renamed, &existing))?
"#,
    );
    let output = program.run(fixture.path());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("5\n"), "{text}");
    assert!(text.contains("Limit"), "{text}");
    assert_eq!(
        std::fs::read(fixture.path().join("renamed")).unwrap(),
        [0, 255, 13, 10, 128]
    );
    assert_eq!(
        std::fs::read(fixture.path().join("existing")).unwrap(),
        b"keep"
    );
    assert!(!fixture.path().join("limited").exists());
    assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 3);
}

#[test]
fn cancellation_after_progress_and_reader_error_discard_unnamed_inode() {
    let fixture = tempfile::tempdir().unwrap();
    let program = Program::new(
        r#"
pub class Source:
    token: CancellationToken
    calls: i64
    cancel: bool
    pub def read(self: &mut Source, target: &mut std.bytes.Bytes, start: i64, stop: i64) -> Result[i64, io.StreamError]:
        self.calls += 1
        if self.calls == 1:
            target.set(start, 42u8)
            return Ok(1)
        if self.cancel:
            self.token.cancel()
            Ok(0)
        else:
            Err(io.StreamError.UnexpectedEof)
"#,
        r#"
    cancel = CancellationToken()?
    mut source = Source(cancel.share(), 0, True)
    name = path.from_text("cancelled")?
    show(fs.copy_from(&mut source, &parent, &name, 100, &cancel))?
    fresh = CancellationToken()?
    mut broken = Source(fresh.share(), 0, False)
    show(fs.copy_from(&mut broken, &parent, &name, 100, &fresh))?
"#,
    );
    let output = program.run(fixture.path());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Cancelled"), "{text}");
    assert!(text.contains("UnexpectedEof"), "{text}");
    assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 0);
}

#[test]
fn temp_directories_are_exclusive_private_and_explicitly_cleaned() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.path().join("collision")).unwrap();
    std::fs::write(fixture.path().join("collision/keep"), b"keep").unwrap();
    let program = Program::new(
        "",
        r#"
    collision = path.from_text("collision")?
    show(fs.temporary_dir_named(&parent, &collision))?
    name = path.from_text("owned")?
    mut owned = fs.temporary_dir_named(&parent, &name)?
    nested = path.from_text("nested")?
    owned.root().create_dir(&nested)?
    show(owned.cleanup())?
    owned.root().remove_dir(&nested)?
    owned.cleanup()?
    owned.cleanup()?
    prefix = path.from_text("random-")?
    mut generated = fs.temporary_dir(&parent, &prefix, 4)?
    generated.keep()
    ephemeral = path.from_text("ephemeral")?
    drop(fs.temporary_dir_named(&parent, &ephemeral)?)
"#,
    );
    program.run(fixture.path());
    assert!(fixture.path().join("collision/keep").exists());
    assert!(!fixture.path().join("owned").exists());
    assert!(!fixture.path().join("ephemeral").exists());
    let random = std::fs::read_dir(fixture.path())
        .unwrap()
        .filter_map(Result::ok)
        .find(|e| e.file_name().as_bytes().starts_with(b"random-"))
        .unwrap();
    assert_eq!(random.file_name().as_bytes().len(), 39);
    assert_eq!(
        random.metadata().unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[test]
fn recursion_never_follows_symlinks_and_budgets_leave_partial_progress() {
    let fixture = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("keep"), b"safe").unwrap();
    std::fs::create_dir_all(fixture.path().join("tree/sub/deep")).unwrap();
    std::fs::write(fixture.path().join("tree/file"), b"file").unwrap();
    std::os::unix::fs::symlink(outside.path(), fixture.path().join("tree/outside")).unwrap();
    std::os::unix::fs::symlink("..", fixture.path().join("tree/sub/cycle")).unwrap();
    let program = Program::new(
        "",
        r#"
    name = path.from_text("tree")?
    cancel = CancellationToken()?
    show(fs.remove_tree(&parent, &name, 1, 1, &cancel))?
    fs.remove_tree(&parent, &name, 100, 10, &cancel)?
    forbidden = path.from_text("../escape")?
    show(parent.remove_dir(&forbidden))?
    dot = path.from_text(".")?
    show(parent.remove_dir(&dot))?
"#,
    );
    let text = String::from_utf8(program.run(fixture.path()).stdout).unwrap();
    assert!(text.contains("Limit"), "{text}");
    assert!(text.contains("InvalidInput"), "{text}");
    assert!(!fixture.path().join("tree").exists());
    assert_eq!(std::fs::read(outside.path().join("keep")).unwrap(), b"safe");
}

#[test]
fn glob_preserves_raw_names_and_enforces_all_budgets() {
    let fixture = tempfile::tempdir().unwrap();
    for name in [b"raw\xff".as_slice(), b".hidden", b"*ba", b"plain"] {
        std::fs::write(fixture.path().join(std::ffi::OsStr::from_bytes(name)), b"").unwrap();
    }
    let program = Program::new(
        "",
        r#"
    all = path.from_text("*")?
    print(len(fs.glob(&parent, &all, 10, 10, 1000)?))?
    raw = path.from_bytes([114u8, 97u8, 119u8, 63u8]?)
    found = fs.glob(&parent, &raw, 10, 10, 1000)?
    print(found[0].as_bytes())?
    star = path.from_text("*a")?
    print(len(fs.glob(&parent, &star, 10, 10, 1000)?))?
    show(fs.glob(&parent, &all, 0, 10, 1000))?
    show(fs.glob(&parent, &all, 10, 0, 1000))?
    show(fs.glob(&parent, &all, 10, 10, 0))?
"#,
    );
    let text = String::from_utf8(program.run(fixture.path()).stdout).unwrap();
    assert!(text.starts_with("4\n[114, 97, 119, 255]\n1\n"), "{text}");
    assert_eq!(text.matches("Limit").count(), 3, "{text}");
}

#[test]
fn observed_temp_directory_replacement_is_preserved() {
    let fixture = tempfile::tempdir().unwrap();
    let program = Program::new(
        "",
        r#"
    name = path.from_text("owned")?
    moved = path.from_text("moved")?
    mut temporary = fs.temporary_dir_named(&parent, &name)?
    parent.move(&name, &moved)?
    parent.create_dir(&name)?
    show(temporary.cleanup())?
    drop(temporary)
    parent.remove_dir(&moved)?
"#,
    );
    let text = String::from_utf8(program.run(fixture.path()).stdout).unwrap();
    assert!(text.contains("Replaced"), "{text}");
    assert!(fixture.path().join("owned").is_dir());
}

#[test]
fn permissions_special_files_and_final_symlinks_are_rejected() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.path().join("private")).unwrap();
    std::fs::write(fixture.path().join("denied"), b"secret").unwrap();
    std::fs::set_permissions(
        fixture.path().join("denied"),
        std::fs::Permissions::from_mode(0o0),
    )
    .unwrap();
    std::os::unix::fs::symlink("private", fixture.path().join("link")).unwrap();
    std::os::unix::fs::symlink("denied", fixture.path().join("filelink")).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(fixture.path().join("fifo"))
            .status()
            .unwrap()
            .success()
    );
    let program = Program::new(
        "",
        r#"
    link = path.from_text("link/")?
    full = location.join(&link)?
    show(fs.root(&full))?
    source = path.from_text("filelink")?
    show(parent.read_file(&source))?
    fifo = path.from_text("fifo")?
    show(parent.read_file(&fifo))?
    denied = path.from_text("denied")?
    show(parent.read_file(&denied))?
"#,
    );
    let text = String::from_utf8(program.run(fixture.path()).stdout).unwrap();
    assert_eq!(text.lines().count(), 4, "{text}");
    assert!(text.lines().take(3).all(|line| line != "Ok"), "{text}");
    // Root can bypass file modes; still exercise and check permission failure
    // when the fixture is run as its ordinary unprivileged owner.
    if std::fs::File::open(fixture.path().join("denied")).is_err() {
        assert!(text.lines().last().unwrap().contains("13"), "{text}");
    }
    std::fs::set_permissions(
        fixture.path().join("denied"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
}

#[test]
fn nofollow_file_open_resists_concurrent_symlink_replacement() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let fixture = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(outside.path(), b"outside").unwrap();
    std::fs::write(fixture.path().join("racing"), b"safe").unwrap();
    let program = Program::new(
        "",
        r#"
    name = path.from_text("racing")?
    for _ in range(200):
        match parent.read_file(&name):
            case Ok(stream):
                mut file = stream
                print(file.file.read()? == "safe")?
            case Err(_):
                print(True)?
"#,
    );
    let stopped = Arc::new(AtomicBool::new(false));
    let flag = stopped.clone();
    let path = fixture.path().to_owned();
    let external = outside.path().to_owned();
    let writer = std::thread::spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            std::os::unix::fs::symlink(&external, path.join("swap")).unwrap();
            std::fs::rename(path.join("swap"), path.join("racing")).unwrap();
            std::fs::write(path.join("swap"), b"safe").unwrap();
            std::fs::rename(path.join("swap"), path.join("racing")).unwrap();
        }
    });
    let output = Command::new(&program.executable)
        .arg(fixture.path())
        .output()
        .unwrap();
    stopped.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), 200);
    assert!(text.lines().all(|line| line == "True"), "{text}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn temporary_creation_failure_never_leaves_a_name_or_handle() {
    let fixture = tempfile::tempdir().unwrap();
    let declarations = r#"
def descriptors() -> Result[i64, AllocError | IoError]:
    location = path.from_text("/proc/self/fd")?
    mut directory = fs.read_dir(&location)?
    mut count = 0
    while True:
        match directory.next()?:
            case Some(_):
                count += 1
            case Nothing:
                return Ok(count)
    Ok(count)

def probe(parent: &fs.Root, name: &path.Path, marker: &str) -> Result[(), AllocError | IoError]:
    before = descriptors()?
    print(marker).unwrap()
    result = fs.temporary_dir_named(parent, name)
    print("__test_restore_allocations__").unwrap()
    match result:
        case Ok(owner):
            drop(owner)
        case Err(_):
            pass
    match parent.open_dir(name):
        case Ok(_):
            return Err(IoError.Closed)
        case Err(_):
            pass
    if descriptors()? != before:
        return Err(IoError.Closed)
    Ok(())
"#;
    let mut body = "    name = path.from_text(\"allocated-temporary-directory\")?\n".to_owned();
    for budget in 0..24 {
        body.push_str(&format!(
            "    probe(&parent, &name, \"__test_fail_allocations_after_{budget}__\")?\n"
        ));
    }
    Program::new(declarations, &body).run(fixture.path());
    assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 0);
}

#[test]
fn temporary_directory_open_failure_rolls_back_without_allocating() {
    use std::os::unix::process::CommandExt;
    let fixture = tempfile::tempdir().unwrap();
    let program = Program::new(
        "",
        r#"
    name = path.from_text("unopenable")?
    show(fs.temporary_dir_named(&parent, &name))?
"#,
    );
    let mut command = Command::new(&program.executable);
    command.arg(fixture.path());
    // SAFETY: umask is async-signal-safe and takes no Rust locks or allocation.
    unsafe {
        command.pre_exec(|| {
            unsafe extern "C" {
                fn umask(mode: u32) -> u32;
            }
            umask(0o777);
            Ok(())
        });
    }
    let output = command.output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(!fixture.path().join("unopenable").exists());
}

#[cfg(feature = "runtime-checks")]
#[test]
fn exhausted_allocator_drop_is_best_effort_and_explicit_retry_cleans_up() {
    let fixture = tempfile::tempdir().unwrap();
    let program = Program::new(
        "",
        r#"
    name = path.from_text("best-effort")?
    temporary = fs.temporary_dir_named(&parent, &name)?
    print("__test_fail_allocations_after_0__").unwrap()
    drop(temporary)
    print("__test_restore_allocations__").unwrap()
    drop(parent.open_dir(&name)?)
    parent.remove_dir(&name)?
"#,
    );
    program.run(fixture.path());
    assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 0);
}

#[test]
fn relocated_compiler_keeps_filesystem_helpers_embedded() {
    let installed = tempfile::tempdir().unwrap();
    let compiler = installed.path().join("plenty");
    support::install_executable(Path::new(env!("CARGO_BIN_EXE_plenty")), &compiler);
    let entry = installed.path().join("main.plenty");
    std::fs::write(&entry, "from std.fs import root, glob, FsError\nfrom std.path import from_text\ndef main() -> Result[(), AllocError | IoError | FsError]:\n    path = from_text(\".\")?\n    owner = root(&path)?\n    pattern = from_text(\"no-such-name\")?\n    print(len(glob(&owner, &pattern, 100, 10, 1000)?))?\n    Ok(())\n").unwrap();
    let executable = installed.path().join("program");
    let output = Command::new(&compiler)
        .current_dir(installed.path())
        .args(["--compile"])
        .arg(&entry)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let output = Command::new(executable)
        .current_dir(installed.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"0\n");
}

#[test]
fn filesystem_owners_keep_normal_move_and_worker_checks() {
    // A root is an owned handle, so it can move into a worker; see
    // design/facilities/03-concurrency/effects.md.
    for (body, expected) in [
        ("    moved = owner\n    drop(owner)\n", Some("moved")),
        (
            "    with spawn(worker, owner).drop_job()? as task:\n        task.join()\n",
            None,
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().join("main.plenty");
        let source = format!(
            "from std.fs import Root, root\nfrom std.path import from_text\nfrom std.thread import spawn\ndef worker(owner: Root) -> i64:\n    0\ndef main() -> Result[(), AllocError | IoError | SpawnError[()]]:\n    location = from_text(\".\")?\n    owner = root(&location)?\n{body}    Ok(())\n"
        );
        std::fs::write(&entry, source).unwrap();
        let checked = plenty::check_file(&entry, None);
        match expected {
            Some(expected) => {
                let error = checked.unwrap_err().to_string();
                assert!(error.contains(expected), "{error}");
            }
            None => checked.unwrap(),
        }
    }
}

#[test]
fn unsupported_anonymous_files_are_errors_without_named_fallback() {
    let program = Program::new("", "    show(fs.temporary_file(&parent))?\n");
    let text = String::from_utf8(program.run(Path::new("/proc")).stdout).unwrap();
    assert!(text.contains("IoError.System"), "{text}");
}

#[test]
fn copied_file_preserves_only_selected_permissions_and_observed_times() {
    use std::os::unix::fs::MetadataExt;
    use std::time::{Duration, UNIX_EPOCH};
    let fixture = tempfile::tempdir().unwrap();
    let source = fixture.path().join("source");
    std::fs::write(&source, b"binary\0\xff").unwrap();
    let input = std::fs::File::open(&source).unwrap();
    input
        .set_permissions(std::fs::Permissions::from_mode(0o6751))
        .unwrap();
    input
        .set_times(
            std::fs::FileTimes::new()
                .set_accessed(UNIX_EPOCH + Duration::new(1_500_000_001, 123_456_789))
                .set_modified(UNIX_EPOCH + Duration::new(1_500_000_002, 987_654_321)),
        )
        .unwrap();
    let expected = input.metadata().unwrap();
    let program = Program::new(
        "",
        r#"
    source = path.from_text("source")?
    destination = path.from_text("preserved")?
    ordinary = path.from_text("ordinary")?
    cancel = CancellationToken()?
    print(fs.copy_file(&parent, &source, &destination, 100, &cancel, preserve=fs.CopyMetadata.PermissionsAndTimes)?)?
    print(fs.copy_file(&parent, &source, &ordinary, 100, &cancel)?)?
"#,
    );
    program.run(fixture.path());
    let copied = std::fs::metadata(fixture.path().join("preserved")).unwrap();
    assert_eq!(copied.mode() & 0o7777, 0o751);
    assert_eq!(
        (copied.atime(), copied.atime_nsec()),
        (expected.atime(), expected.atime_nsec())
    );
    assert_eq!(
        (copied.mtime(), copied.mtime_nsec()),
        (expected.mtime(), expected.mtime_nsec())
    );
    assert_eq!(
        std::fs::metadata(fixture.path().join("ordinary"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::read(source).unwrap(),
        std::fs::read(fixture.path().join("preserved")).unwrap()
    );
}

#[test]
fn recursive_copy_streams_names_and_applies_directory_metadata_after_children() {
    use std::os::unix::fs::MetadataExt;
    let fixture = tempfile::tempdir().unwrap();
    let source = fixture.path().join("source");
    std::fs::create_dir_all(source.join("nested/empty")).unwrap();
    std::fs::write(source.join(".hidden"), b"hidden").unwrap();
    std::fs::write(
        source
            .join("nested")
            .join(std::ffi::OsStr::from_bytes(b"raw\xff")),
        [0, 255, 13, 10],
    )
    .unwrap();
    std::fs::set_permissions(
        source.join("nested"),
        std::fs::Permissions::from_mode(0o555),
    )
    .unwrap();
    let before = std::fs::metadata(source.join("nested")).unwrap();
    let program = Program::new(
        "",
        r#"
    source = path.from_text("source")?
    destination = path.from_text("copied")?
    cancel = CancellationToken()?
    limits = fs.copy_limits()
    match fs.copy_tree(&parent, &source, &parent, &destination, &limits, &cancel, preserve=fs.CopyMetadata.PermissionsAndTimes):
        case Ok(count):
            print(count)?
        case Err(error):
            print(error)?
"#,
    );
    assert_eq!(
        String::from_utf8(program.run(fixture.path()).stdout).unwrap(),
        "10\n"
    );
    let copied = fixture.path().join("copied");
    assert!(copied.join("nested/empty").is_dir());
    assert_eq!(std::fs::read(copied.join(".hidden")).unwrap(), b"hidden");
    assert_eq!(
        std::fs::read(
            copied
                .join("nested")
                .join(std::ffi::OsStr::from_bytes(b"raw\xff"))
        )
        .unwrap(),
        [0, 255, 13, 10]
    );
    let after = std::fs::metadata(copied.join("nested")).unwrap();
    assert_eq!(after.mode() & 0o7777, 0o555);
    assert_eq!(
        (after.mtime(), after.mtime_nsec()),
        (before.mtime(), before.mtime_nsec())
    );
}

#[test]
fn recursive_copy_rejects_links_special_files_overlap_and_existing_destination() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.path().join("source")).unwrap();
    std::os::unix::fs::symlink("../source", fixture.path().join("source/cycle")).unwrap();
    std::os::unix::fs::symlink("missing", fixture.path().join("dangling")).unwrap();
    let program = Program::new(
        "",
        r#"
    source = path.from_text("source")?
    destination = path.from_text("copy")?
    cancel = CancellationToken()?
    limits = fs.copy_limits()
    show(fs.copy_tree(&parent, &source, &parent, &destination, &limits, &cancel))?
    child = parent.open_dir(&source)?
    show(fs.copy_tree(&parent, &source, &child, &destination, &limits, &cancel))?
    show(fs.copy_tree(&parent, &source, &parent, &path.from_text("dangling")?, &limits, &cancel))?
    show(fs.copy_tree(&parent, &source, &parent, &source, &limits, &cancel))?
"#,
    );
    let output = String::from_utf8(program.run(fixture.path()).stdout).unwrap();
    assert!(output.contains("FsError.UnsupportedType"), "{output}");
    assert!(output.contains("FsError.Overlap"), "{output}");
    assert_eq!(output.matches("FsError.Io").count(), 2, "{output}");
    assert!(fixture.path().join("copy").is_dir());
    assert!(!fixture.path().join("source/copy").exists());
    assert_eq!(
        std::fs::read_dir(fixture.path().join("copy"))
            .unwrap()
            .count(),
        0
    );

    std::fs::remove_file(fixture.path().join("source/cycle")).unwrap();
    let fifo = fixture.path().join("source/fifo");
    let status = Command::new("mkfifo").arg(&fifo).status().unwrap();
    assert!(status.success());
    std::fs::remove_dir(fixture.path().join("copy")).unwrap();
    let output = String::from_utf8(program.run(fixture.path()).stdout).unwrap();
    assert!(output.contains("FsError.UnsupportedType"), "{output}");
}

#[test]
fn recursive_copy_budgets_cancellation_and_partial_cleanup_are_explicit() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(fixture.path().join("source/nested")).unwrap();
    std::fs::write(fixture.path().join("source/nested/file"), b"0123456789").unwrap();
    let program = Program::new(
        "",
        r#"
    source = path.from_text("source")?
    cancel = CancellationToken()?
    mut limits = fs.copy_limits()
    limits.max_depth = 1
    show(fs.copy_tree(&parent, &source, &parent, &path.from_text("depth")?, &limits, &cancel))?
    limits = fs.copy_limits()
    limits.max_entries = 1
    show(fs.copy_tree(&parent, &source, &parent, &path.from_text("entries")?, &limits, &cancel))?
    limits = fs.copy_limits()
    limits.max_bytes = 9
    show(fs.copy_tree(&parent, &source, &parent, &path.from_text("bytes")?, &limits, &cancel))?
    limits = fs.copy_limits()
    limits.max_open_handles = 11
    show(fs.copy_tree(&parent, &source, &parent, &path.from_text("handles")?, &limits, &cancel))?
    limits.max_depth = 65
    show(fs.copy_tree(&parent, &source, &parent, &path.from_text("invalid")?, &limits, &cancel))?
    limits = fs.copy_limits()
    cancel.cancel()
    show(fs.copy_tree(&parent, &source, &parent, &path.from_text("cancelled")?, &limits, &cancel))?
    fresh = CancellationToken()?
    fs.remove_tree(&parent, &path.from_text("bytes")?, 10, 4, &fresh)?
"#,
    );
    let output = String::from_utf8(program.run(fixture.path()).stdout).unwrap();
    assert_eq!(output.matches("FsError.Limit").count(), 4, "{output}");
    assert!(output.contains("FsError.InvalidLimits"), "{output}");
    assert!(output.contains("FsError.Cancelled"), "{output}");
    assert!(!fixture.path().join("invalid").exists());
    assert!(!fixture.path().join("cancelled").exists());
    assert!(!fixture.path().join("bytes").exists());
}

#[cfg(feature = "runtime-checks")]
#[test]
fn copy_failures_release_descriptors_and_leave_only_documented_outputs() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.path().join("source")).unwrap();
    std::fs::write(fixture.path().join("source/file"), b"complete").unwrap();
    std::fs::write(fixture.path().join("file"), b"complete").unwrap();
    let declarations = r#"
def descriptors() -> Result[i64, AllocError | IoError]:
    location = path.from_text("/proc/self/fd")?
    mut directory = fs.read_dir(&location)?
    mut count = 0
    while True:
        match directory.next()?:
            case Some(_):
                count += 1
            case Nothing:
                return Ok(count)
    Ok(count)

def probe(parent: &fs.Root, source: &path.Path, file: &path.Path, destination: &path.Path, cancel: &CancellationToken, limits: &fs.CopyLimits, marker: &str, tree: bool) -> Result[(), AllocError | IoError | fs.FsError]:
    before = descriptors()?
    print(marker).unwrap()
    if tree:
        result = fs.copy_tree(parent, source, parent, destination, limits, cancel, preserve=fs.CopyMetadata.PermissionsAndTimes)
        print("__test_restore_allocations__").unwrap()
        drop(result)
        match parent.open_dir(destination):
            case Ok(owner):
                drop(owner)
                fs.remove_tree(parent, destination, 10, 4, cancel)?
            case Err(_):
                pass
    else:
        result = fs.copy_file(parent, file, destination, 100, cancel, preserve=fs.CopyMetadata.PermissionsAndTimes)
        print("__test_restore_allocations__").unwrap()
        drop(result)
        match parent.read_file(destination):
            case Ok(stream):
                metadata = fs.file_metadata(&stream)?
                if metadata.size() != 8u64:
                    return Err(IoError.Closed)
                drop(stream)
                parent.remove_file(destination)?
            case Err(_):
                pass
    if descriptors()? != before:
        return Err(IoError.Closed)
    Ok(())
"#;
    let mut body = String::from(
        r#"
    source = path.from_text("source")?
    file = path.from_text("file")?
    destination = path.from_text("destination")?
    cancel = CancellationToken()?
    limits = fs.copy_limits()
"#,
    );
    for budget in 0..100 {
        for tree in ["False", "True"] {
            body.push_str(&format!("    probe(&parent, &source, &file, &destination, &cancel, &limits, \"__test_fail_allocations_after_{budget}__\", {tree})?\n"));
        }
    }
    Program::new(declarations, &body).run(fixture.path());
    assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 2);
}

#[test]
fn denied_metadata_syscall_never_publishes_staged_file() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("source"), b"contents").unwrap();
    let program = Program::new(
        "",
        r#"
    source = path.from_text("source")?
    plain = path.from_text("plain")?
    selected = path.from_text("selected")?
    cancel = CancellationToken()?
    print(fs.copy_file(&parent, &source, &plain, 100, &cancel)?)?
    show(fs.copy_file(&parent, &source, &selected, 100, &cancel, preserve=fs.CopyMetadata.PermissionsAndTimes))?
"#,
    );
    let native = tempfile::tempdir().unwrap();
    let source = native.path().join("deny.c");
    let library = native.path().join("deny.so");
    std::fs::write(&source,"#include <errno.h>\n#include <sys/types.h>\nint fchmod(int fd, mode_t mode) { (void)fd; (void)mode; errno = EPERM; return -1; }\n").unwrap();
    let built = Command::new("cc")
        .args(["-shared", "-fPIC"])
        .arg(&source)
        .arg("-o")
        .arg(&library)
        .output()
        .unwrap();
    assert!(built.status.success(), "{built:?}");
    let output = Command::new(&program.executable)
        .arg(fixture.path())
        .env("LD_PRELOAD", library)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("8\n"), "{text}");
    assert!(text.contains("IoError.System(1)"), "{text}");
    assert_eq!(
        std::fs::read(fixture.path().join("plain")).unwrap(),
        b"contents"
    );
    assert!(!fixture.path().join("selected").exists());
    assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 2);
}

#[test]
fn recursive_copy_never_follows_a_concurrently_replaced_file_link() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let fixture = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), b"outside-secret").unwrap();
    let source = fixture.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("racing"), b"safe").unwrap();
    let program = Program::new(
        "",
        r#"
    source = path.from_text("source")?
    cancel = CancellationToken()?
    limits = fs.copy_limits()
    for index in range(80):
        destination = path.from_text(f"copy-{index}"?)?
        show(fs.copy_tree(&parent, &source, &parent, &destination, &limits, &cancel))?
"#,
    );
    let stopped = Arc::new(AtomicBool::new(false));
    let flag = stopped.clone();
    let external = outside.path().join("secret");
    let staging = fixture.path().join("swap");
    let writer = std::thread::spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            std::os::unix::fs::symlink(&external, &staging).unwrap();
            std::fs::rename(&staging, source.join("racing")).unwrap();
            std::fs::write(&staging, b"safe").unwrap();
            std::fs::rename(&staging, source.join("racing")).unwrap();
        }
    });
    let output = Command::new(&program.executable)
        .arg(fixture.path())
        .output()
        .unwrap();
    stopped.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    assert!(output.status.success(), "{output:?}");
    for index in 0..80 {
        let destination = fixture.path().join(format!("copy-{index}"));
        assert!(destination.is_dir());
        for entry in std::fs::read_dir(destination).unwrap() {
            let entry = entry.unwrap();
            assert!(!entry.file_type().unwrap().is_symlink());
            assert_eq!(std::fs::read(entry.path()).unwrap(), b"safe");
        }
    }
    assert_eq!(
        std::fs::read(outside.path().join("secret")).unwrap(),
        b"outside-secret"
    );
}

#[test]
fn empty_tree_exact_budgets_and_distinct_roots_work_without_replacement() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.path().join("empty")).unwrap();
    std::fs::create_dir(fixture.path().join("other")).unwrap();
    let program = Program::new(
        "",
        r#"
    source = path.from_text("empty")?
    destination = path.from_text("copy")?
    other = parent.open_dir(&path.from_text("other")?)?
    cancel = CancellationToken()?
    limits = fs.CopyLimits(1, 1, 0, 11)
    show(fs.copy_tree(&parent, &source, &other, &destination, &limits, &cancel))?
    show(fs.copy_tree(&parent, &source, &other, &destination, &limits, &cancel))?
    bad_entries = fs.CopyLimits(0, 1, 0, 11)
    show(fs.copy_tree(&parent, &source, &other, &path.from_text("invalid-entries")?, &bad_entries, &cancel))?
    bad_bytes = fs.CopyLimits(1, 1, -1, 11)
    show(fs.copy_tree(&parent, &source, &other, &path.from_text("invalid-bytes")?, &bad_bytes, &cancel))?
    bad_handles = fs.CopyLimits(1, 1, 0, 10)
    show(fs.copy_tree(&parent, &source, &other, &path.from_text("invalid-handles")?, &bad_handles, &cancel))?
"#,
    );
    let text = String::from_utf8(program.run(fixture.path()).stdout).unwrap();
    assert!(text.starts_with("Ok\n"), "{text}");
    assert!(text.contains("IoError.System(17)"), "{text}");
    assert_eq!(text.matches("FsError.InvalidLimits").count(), 3, "{text}");
    assert_eq!(
        std::fs::read_dir(fixture.path().join("other"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(
        std::fs::read_dir(fixture.path().join("other/copy"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn bind_mount_alias_of_source_descendant_reports_partial_overlap_without_recursing() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(fixture.path().join("source/child")).unwrap();
    std::fs::create_dir(fixture.path().join("alias")).unwrap();
    let available = Command::new("unshare")
        .args([
            "-Urnm",
            "sh",
            "-ec",
            "mount --bind \"$1\" \"$2\"",
            "fs-copy-probe",
        ])
        .arg(fixture.path().join("source/child"))
        .arg(fixture.path().join("alias"))
        .output();
    if !matches!(&available, Ok(output) if output.status.success()) {
        eprintln!(
            "bind-mount overlap qualification requires permitted user/mount namespaces and bind mounts: {available:?}"
        );
        return;
    }
    let program = Program::new(
        "",
        r#"
    source = path.from_text("source")?
    alias = parent.open_dir(&path.from_text("alias")?)?
    destination = path.from_text("copy")?
    cancel = CancellationToken()?
    limits = fs.copy_limits()
    show(fs.copy_tree(&parent, &source, &alias, &destination, &limits, &cancel))?
"#,
    );
    // The bind exists only in the child mount namespace. Its directory aliases
    // source/child while dot-dot walks back through the independent alias name.
    let output = Command::new("unshare")
        .args([
            "-Urnm",
            "sh",
            "-ec",
            "mount --bind \"$1\" \"$2\"\nexec \"$3\" \"$4\"",
            "fs-copy-bind",
        ])
        .arg(fixture.path().join("source/child"))
        .arg(fixture.path().join("alias"))
        .arg(&program.executable)
        .arg(fixture.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("TreeCopyError.Entry"), "{text}");
    assert!(text.contains("FsError.Overlap"), "{text}");
    assert_eq!(
        std::fs::read_dir(fixture.path().join("source/child/copy"))
            .unwrap()
            .count(),
        0
    );
}

/// B95: budgets are `Positive[i64]`/`NonNegative[i64]`; a value above a fixed
/// ceiling is a caller bug, so it traps before touching the filesystem.
#[test]
fn budgets_above_the_ceiling_trap() {
    for (body, message) in [
        (
            "    prefix = path.from_text(\"random-\")?\n    drop(fs.temporary_dir(&parent, &prefix, Positive.try_from(1025).unwrap()))",
            "std.fs.temporary_dir: attempts is above 1024",
        ),
        (
            "    name = path.from_text(\"tree\")?\n    cancel = CancellationToken()?\n    drop(fs.remove_tree(&parent, &name, 10, Positive.try_from(65).unwrap(), &cancel))",
            "std.fs.remove_tree: max_depth is above 64",
        ),
        (
            "    name = path.from_text(\"owned\")?\n    mut owned = fs.temporary_dir_named(&parent, &name)?\n    cancel = CancellationToken()?\n    drop(owned.cleanup_tree(10, Positive.try_from(65).unwrap(), &cancel))",
            "TempDir.cleanup_tree: max_depth is above 64",
        ),
    ] {
        let fixture = tempfile::tempdir().unwrap();
        std::fs::create_dir(fixture.path().join("tree")).unwrap();
        let program = Program::new("", body);
        let output = Command::new(&program.executable)
            .arg(fixture.path())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{body}\n{stderr}");
        assert!(stderr.contains(message), "{body}\n{stderr}");
        assert!(fixture.path().join("tree").exists());
    }
}

#[test]
fn negative_byte_limit_literal_is_rejected() {
    let error = support::check_file_error(
        "import std.fs as fs\nimport std.path as path\nimport std.io as io\nimport std.bytes\nfrom std.sync import CancellationToken\ndef main() -> Result[(), AllocError | IoError | fs.FsError]:\n    parent = fs.root(&path.from_text(\".\")?)?\n    mut reader = io.cursor(std.bytes.empty()?)\n    cancel = CancellationToken()?\n    drop(fs.copy_from(&mut reader, &parent, &path.from_text(\"x\")?, -1, &cancel))\n    Ok(())\n",
    );
    assert!(error.contains("NonNegative"), "{error}");
}
