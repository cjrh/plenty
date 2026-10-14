//! Descriptor-rooted mutation, anonymous staging and checked Plenty policies.
#![cfg(all(target_os = "linux", target_pointer_width = "64"))]
use std::os::unix::{ffi::OsStrExt, fs::PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
            "import std.fs as fs\nimport std.path as path\nimport std.io as io\nimport std.bytes as bytes\nfrom std.sync import CancellationToken\ndef show[T, E](value: Result[T, E]) -> Result[(), Failure]:\n    match value:\n        case Ok(_):\n            print(\"Ok\")?\n        case Err(error):\n            print(error)?\n    Ok(())\n{declarations}\ndef main() -> Result[(), Failure]:\n    argv = args_bytes()?\n    location = path.from_bytes(copy(argv[1])?)\n    parent = fs.root(&location)?\n{body}\n    Ok(())\n"
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
    pub def read(self: &mut Source, target: &mut bytes.Bytes, start: i64, stop: i64) -> Result[i64, io.StreamError]:
        self.calls += 1
        if self.calls == 1:
            drop(target.set(start, 42u8))
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
    assert!(Command::new("mkfifo")
        .arg(fixture.path().join("fifo"))
        .status()
        .unwrap()
        .success());
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
        atomic::{AtomicBool, Ordering},
        Arc,
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
def descriptors() -> Result[i64, Failure]:
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

def probe(parent: &fs.Root, name: &path.Path, marker: &str) -> Result[(), Failure]:
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
            return Err(Failure.Unspecified)
        case Err(_):
            pass
    if descriptors()? != before:
        return Err(Failure.Unspecified)
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
    std::fs::copy(env!("CARGO_BIN_EXE_plenty"), &compiler).unwrap();
    let entry = installed.path().join("main.plenty");
    std::fs::write(&entry, "from std.fs import root, glob\nfrom std.path import from_text\ndef main() -> Result[(), Failure]:\n    path = from_text(\".\")?\n    owner = root(&path)?\n    pattern = from_text(\"no-such-name\")?\n    print(len(glob(&owner, &pattern, 100, 10, 1000)?))?\n    Ok(())\n").unwrap();
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
    for (body, expected) in [
        ("    moved = owner\n    drop(owner)\n", "moved"),
        (
            "    with spawn(worker, owner)? as task:\n        task.join()\n",
            "cross-thread ownership",
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().join("main.plenty");
        let source = format!("from std.fs import Root, root\nfrom std.path import from_text\nfrom std.thread import spawn\ndef worker(owner: Root) -> i64:\n    0\ndef main() -> Result[(), Failure]:\n    location = from_text(\".\")?\n    owner = root(&location)?\n{body}    Ok(())\n");
        std::fs::write(&entry, source).unwrap();
        let error = plenty::check_file(&entry, None).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn unsupported_anonymous_files_are_errors_without_named_fallback() {
    let program = Program::new("", "    show(fs.temporary_file(&parent))?\n");
    let text = String::from_utf8(program.run(Path::new("/proc")).stdout).unwrap();
    assert!(text.contains("IoError.System"), "{text}");
}
