//! Script CLI acceptance: original source paths, isolated resolution and native argv.
mod support;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use support::write_file as write;

struct Fixture(tempfile::TempDir);
impl Fixture {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }
    fn root(&self) -> &Path {
        self.0.path()
    }
    fn write(&self, path: &str, text: &str) {
        write(&self.root().join(path), text);
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_plenty"));
        c.current_dir(self.root())
            .env("PLENTY_CACHE_DIR", self.root().join("cache"))
            .env_remove("PLENTY_CACHE")
            .env_remove("COMPILER_PATH")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0");
        c
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8(o.stdout).unwrap()
    }
    fn fail(&self, args: &[&str], message: &str) {
        let o = self.run(args);
        assert!(!o.status.success(), "{args:?} unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&o.stderr).contains(message),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }
    fn library(&self) {
        self.write(
            "util/plenty.toml",
            "manifest-version = 1\n[package]\nname = 'util'\nversion = '0.1.0'\n",
        );
        self.write("util/src/lib.plenty", "pub def answer() -> i64:\n    42\n");
    }
    fn script(&self, dep: &str) {
        self.write("scripts/app.plenty", &format!("#!/usr/bin/env -S plenty script\n---plenty\n[dependencies]\nutil = {{ {dep} }}\n---\nfrom util import answer\ndef main() -> Result[(), IoError]:\n    print(answer())?\n    Ok(())\n"));
    }
}
const ARGS: &str = "def main() -> Result[(), IoError]:\n    print(args()?)?\n    Ok(())\n";
const HELLO: &str = "def main() -> Result[(), IoError]:\n    print(42)?\n    Ok(())\n";

#[test]
fn arguments_are_forwarded_after_file_and_runner_options_stay_before_it() {
    let f = Fixture::new();
    f.write("hello.plenty", ARGS);
    assert_eq!(
        f.ok(&[
            "script",
            "hello.plenty",
            "123",
            "",
            "a b",
            "--help",
            "--release",
            "--",
            "--linker",
            "invalid"
        ]),
        "[\"hello.plenty\", \"123\", \"\", \"a b\", \"--help\", \"--release\", \"--\", \"--linker\", \"invalid\"]\n"
    );
    f.write("-hello.plenty", HELLO);
    assert_eq!(f.ok(&["script", "--", "-hello.plenty"]), "42\n");
    assert!(f.ok(&["script", "--help"]).contains("OPTIONS"));
    f.fail(&["hello.plenty", "123"], "unrecognised arguments");
}

#[test]
fn check_compile_and_option_errors() {
    let f = Fixture::new();
    f.write("hello.plenty", HELLO);
    assert_eq!(
        f.ok(&["script", "--check", "--offline", "--locked", "hello.plenty"]),
        ""
    );
    f.ok(&[
        "script",
        "--check",
        "--offline",
        "--locked",
        "--lockfile",
        "empty.lock",
        "hello.plenty",
    ]);
    assert!(!f.root().join("empty.lock").exists());
    f.fail(
        &["script", "--lockfile", "cache/durable.lock", "hello.plenty"],
        "outside the Plenty cache",
    );
    assert_eq!(
        f.ok(&[
            "script",
            "--compile",
            "--release",
            "-o",
            "hello",
            "hello.plenty"
        ]),
        ""
    );
    assert_eq!(
        String::from_utf8(
            Command::new(f.root().join("hello"))
                .output()
                .unwrap()
                .stdout
        )
        .unwrap(),
        "42\n"
    );
    for args in [
        vec!["--check", "hello.plenty", "x"],
        vec!["--compile", "-o", "hello", "hello.plenty", "x"],
    ] {
        let mut all = vec!["script"];
        all.extend(args);
        f.fail(&all, "reject program arguments");
    }
    f.fail(
        &["script", "--check", "--compile", "hello.plenty"],
        "mutually exclusive",
    );
    f.fail(&["script", "--compile", "hello.plenty"], "requires -o");
    f.fail(
        &["script", "-o", "hello", "hello.plenty"],
        "requires --compile",
    );
    f.fail(&["script", "--lockfile"], "requires a path");
    f.fail(
        &["script", "--wat", "hello.plenty"],
        "unknown script option",
    );
    f.fail(
        &["script", "--compile", "-o", "hello.plenty", "hello.plenty"],
        "overwrite the script",
    );
    f.fail(
        &["script", "--lockfile", "hello.plenty", "hello.plenty"],
        "must not name the script",
    );
}

#[test]
fn manifests_are_strict_and_diagnostics_keep_original_crlf_lines() {
    let f = Fixture::new();
    for (body, message) in [
        (
            "---plenty\n[workspace]\nmembers=[]\n---\n",
            "unknown key `workspace`",
        ),
        ("---plenty\n[lib]\n---\n", "unknown key `lib`"),
        ("---plenty\n[[bin]]\n---\n", "unknown key `bin`"),
        ("---plenty\n[package]\nwat=1\n---\n", "unknown key `wat`"),
        (
            "---plenty\nmanifest-version=99\n---\n",
            "unsupported manifest-version",
        ),
        ("---plenty\n[package]\nname='std'\n---\n", "reserved"),
        (
            "---plenty\n[package]\nplenty-version='999.0.0'\n---\n",
            "requires Plenty",
        ),
        ("---plenty\n", "unterminated"),
        ("---plenty\n---plenty\n---\n", "duplicate or misplaced"),
        ("---plenty\n---\n---plenty\n---\n", "duplicate or misplaced"),
        ("---\n", "closer without"),
        (" ---plenty\n---\n", "unindented"),
    ] {
        f.write("bad.plenty", &format!("{body}{HELLO}"));
        f.fail(&["script", "--check", "bad.plenty"], message);
    }
    f.write("bad.plenty", &format!("{HELLO}---plenty\n---\n"));
    f.fail(&["script", "bad.plenty"], "misplaced");
    f.write("bad.plenty", "#!/usr/bin/env -S plenty script\r\n\r\n---plenty\r\n[package]\r\nname = 3\r\n---\r\ndef main() -> ():\r\n    pass\r\n");
    f.fail(&["script", "--check", "bad.plenty"], "bad.plenty:5:");
    f.write("bad.plenty", "#!/usr/bin/env -S plenty script\r\n---plenty\r\n---\r\ndef main() -> ():\r\n    missing()\r\n");
    f.fail(&["script", "--check", "bad.plenty"], "bad.plenty:5:");
}

#[test]
fn metadata_defaults_do_not_discover_neighbor_targets_or_manifests() {
    let f = Fixture::new();
    f.write("plenty.toml", "this is not a manifest");
    f.write("src/lib.plenty", "not valid Plenty");
    for name in ["42 HELLO.plenty", "run.plenty"] {
        f.write(name, &format!("\n---plenty\n[package]\ndescription='test'\nauthors=['one']\nlicense='MIT'\n---\n{HELLO}"));
        assert_eq!(f.ok(&["script", name]), "42\n");
    }
    f.fail(&["--check", "run.plenty"], "plenty script");
    f.write(
        "sibling.plenty",
        "---plenty\n---\npub def value() -> i64:\n    42\n",
    );
    f.write(
        "root.plenty",
        "from sibling import value\ndef main() -> i64:\n    value()\n",
    );
    f.fail(
        &["script", "--check", "root.plenty"],
        "imported modules cannot carry",
    );
}

#[test]
fn path_dependencies_locks_and_sibling_modules_are_isolated() {
    let f = Fixture::new();
    f.library();
    f.script("path = '../util', version = '^0.1.0'");
    f.write("plenty.toml", "not a manifest");
    f.fail(&["script", "--locked", "scripts/app.plenty"], "--locked");
    assert_eq!(
        f.ok(&["script", "--lockfile", "durable.lock", "scripts/app.plenty"]),
        "42\n"
    );
    let lock = std::fs::read_to_string(f.root().join("durable.lock")).unwrap();
    assert!(lock.contains("path+../util"), "{lock}");
    assert!(lock.contains("source = \"script+app.plenty\""), "{lock}");
    assert_eq!(
        f.ok(&[
            "script",
            "--locked",
            "--offline",
            "--lockfile",
            "durable.lock",
            "scripts/app.plenty"
        ]),
        "42\n"
    );
    assert_eq!(f.ok(&["script", "scripts/app.plenty"]), "42\n");
    assert_eq!(
        f.ok(&["script", "--locked", "--offline", "scripts/app.plenty"]),
        "42\n"
    );
    f.write("util/src/lib.plenty", "pub def answer() -> i64:\n    77\n");
    assert_eq!(f.ok(&["script", "scripts/app.plenty"]), "77\n");
    f.write(
        "scripts/helper.plenty",
        "pub def answer() -> i64:\n    12\n",
    );
    f.write(
        "scripts/local.plenty",
        "from helper import answer\ndef main() -> i32:\n    i32(answer())\n",
    );
    assert_eq!(
        f.run(&["script", "scripts/local.plenty"]).status.code(),
        Some(12)
    );
    assert!(!f.root().join("scripts/plenty.lock").exists());
    assert!(!f.root().join("scripts/.pkg").exists());
    assert!(!f.root().join("scripts/target").exists());
}

#[test]
fn a_script_inside_a_dependency_directory_owns_only_itself() {
    // A script owns no package directory, so one stored in a dependency's
    // directory leaves the dependency's includes from there alone, and its
    // own includes stay below its directory.
    let f = Fixture::new();
    f.write(
        "util/plenty.toml",
        "manifest-version = 1\n[package]\nname = 'util'\nversion = '0.1.0'\n",
    );
    f.write(
        "util/src/lib.plenty",
        "const BANNER: str = include_str(\"../data/banner.txt\")\npub def answer() -> str:\n    BANNER\n",
    );
    f.write("util/data/banner.txt", "banner");
    let script = |include: &str| {
        format!(
            "#!/usr/bin/env -S plenty script\n---plenty\n[dependencies]\nutil = {{ path = '..' }}\n---\nfrom util import answer\nconst OWN: str = include_str(\"{include}\")\ndef main() -> Result[(), IoError]:\n    print(answer())?\n    print(OWN)?\n    Ok(())\n"
        )
    };
    f.write("util/data/app.plenty", &script("banner.txt"));
    assert_eq!(
        f.ok(&["script", "util/data/app.plenty"]),
        "banner\nbanner\n"
    );
    f.write("util/data/app.plenty", &script("../src/lib.plenty"));
    let root = f.root().join("util/data").canonicalize().unwrap();
    f.fail(
        &["script", "util/data/app.plenty"],
        &format!(
            "included path `../src/lib.plenty` is outside the source root {}",
            root.display()
        ),
    );
}

fn git(directory: &Path, args: &[&str]) {
    let o = Command::new("git")
        .args(args)
        .current_dir(directory)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Script Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Script Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn git_dependencies_use_cached_pins_and_durable_locks_survive_cleanup() {
    let f = Fixture::new();
    f.library();
    let repo = f.root().join("util");
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "first"]);
    git(&repo, &["tag", "v1"]);
    f.script(&format!("git = '{}', tag = 'v1'", repo.display()));
    f.fail(&["script", "--offline", "scripts/app.plenty"], "--offline");
    f.fail(&["script", "--locked", "scripts/app.plenty"], "--locked");
    assert!(
        !f.root()
            .join("cache/scripts/state")
            .read_dir()
            .unwrap()
            .any(|e| e.unwrap().path().join(".pkg").exists())
    );
    assert_eq!(
        f.ok(&["script", "--lockfile", "durable.lock", "scripts/app.plenty"]),
        "42\n"
    );
    f.write("util/src/lib.plenty", "pub def answer() -> i64:\n    99\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "second"]);
    git(&repo, &["tag", "-f", "v1"]);
    assert_eq!(
        f.ok(&[
            "script",
            "--locked",
            "--offline",
            "--lockfile",
            "durable.lock",
            "scripts/app.plenty"
        ]),
        "42\n"
    );
    f.ok(&["cache", "clean"]);
    assert!(f.root().join("durable.lock").exists());
    f.fail(
        &[
            "script",
            "--locked",
            "--offline",
            "--lockfile",
            "durable.lock",
            "scripts/app.plenty",
        ],
        "--offline",
    );
    assert_eq!(
        f.ok(&[
            "script",
            "--locked",
            "--lockfile",
            "durable.lock",
            "scripts/app.plenty"
        ]),
        "42\n"
    );
}

#[test]
fn warm_runs_skip_compilation_and_linking_and_failed_edits_never_run_old_code() {
    let f = Fixture::new();
    f.write("app.plenty", HELLO);
    let first = f.run(&["script", "--verbose", "--cache-stats", "app.plenty"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let second = f.run(&["script", "--verbose", "--cache-stats", "app.plenty"]);
    assert!(second.status.success());
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(stderr.contains("program hit"), "{stderr}");
    if String::from_utf8_lossy(&first.stderr).contains("executable cache: miss") {
        assert!(stderr.contains("executable cache: hit"), "{stderr}");
        assert!(!stderr.contains("link:"), "{stderr}");
    }
    assert!(!stderr.contains("Compiling"), "{stderr}");
    assert!(f.run(&["script", "app.plenty"]).stderr.is_empty());
    f.write("app.plenty", "def main() -> ():\n    missing()\n");
    let failed = f.run(&["script", "app.plenty"]);
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    f.write("app.plenty", &HELLO.replace("42", "99"));
    assert_eq!(f.ok(&["script", "app.plenty"]), "99\n");
    f.write("app.plenty", "def main() -> i32:\n    37\n");
    assert_eq!(f.run(&["script", "app.plenty"]).status.code(), Some(37));
}

#[cfg(unix)]
#[test]
fn executable_shebang_native_bytes_symlinks_and_read_only_sources() {
    use std::os::unix::{
        ffi::OsStrExt,
        fs::{PermissionsExt, symlink},
    };
    let f = Fixture::new();
    // Keep the compiler filename `plenty` for env -S without relying on install state.
    let bin = f.root().join("bin");
    std::fs::create_dir(&bin).unwrap();
    symlink(env!("CARGO_BIN_EXE_plenty"), bin.join("plenty")).unwrap();
    f.write(
        "scripts/app.plenty",
        &format!("#!/usr/bin/env -S plenty script\n{ARGS}"),
    );
    let path = f.root().join("scripts/app.plenty");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o555)).unwrap();
    std::fs::set_permissions(
        path.parent().unwrap(),
        std::fs::Permissions::from_mode(0o555),
    )
    .unwrap();
    symlink(&path, f.root().join("launcher")).unwrap();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let o = Command::new(f.root().join("launcher"))
        .arg("123")
        .current_dir(f.root())
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env("PLENTY_CACHE_DIR", f.root().join("cache"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("123"));
    std::fs::set_permissions(
        path.parent().unwrap(),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    f.write("bytes.plenty", "def main() -> Result[(), IoError]:\n    values = args_bytes()?\n    print(values[1])?\n    Ok(())\n");
    let o = f
        .command()
        .args([
            OsStr::new("script"),
            OsStr::new("bytes.plenty"),
            OsStr::from_bytes(b"\xff a"),
        ])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(o.stdout, b"[255, 32, 97]\n");
    let native_name = OsStr::from_bytes(b"native-\xff.plenty");
    write(&f.root().join(native_name), HELLO);
    let o = f
        .command()
        .args([OsStr::new("script"), native_name])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(o.stdout, b"42\n");

    f.write(
        "scripts/helper.plenty",
        "pub def answer() -> i32:\n    23\n",
    );
    f.write(
        "scripts/physical.plenty",
        "from helper import answer\ndef main() -> i32:\n    answer()\n",
    );
    symlink(
        f.root().join("scripts/physical.plenty"),
        f.root().join("physical-launcher"),
    )
    .unwrap();
    assert_eq!(
        f.run(&["script", "physical-launcher"]).status.code(),
        Some(23)
    );

    let mode = std::fs::metadata(f.root().join("cache/scripts"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0, "script dependency state must be private");
}

#[test]
fn concurrent_runs_and_equal_basenames_do_not_share_outputs() {
    let f = Fixture::new();
    f.write("one/app.plenty", HELLO);
    f.write("two/app.plenty", &HELLO.replace("42", "17"));
    let mut children = vec![];
    for _ in 0..4 {
        children.push(
            f.command()
                .args(["script", "one/app.plenty"])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    for child in children {
        let o = child.wait_with_output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(o.stdout, b"42\n");
    }
    assert_eq!(f.ok(&["script", "two/app.plenty"]), "17\n");
    let state: Vec<PathBuf> = std::fs::read_dir(f.root().join("cache/scripts/state"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(state.len(), 2);
}

#[test]
fn locked_manifest_changes_fail_before_fetching() {
    let f = Fixture::new();
    f.library();
    let repo = f.root().join("util");
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "first"]);
    git(&repo, &["tag", "v1"]);
    f.script(&format!("git = '{}', tag = 'v1'", repo.display()));
    f.ok(&["script", "--lockfile", "durable.lock", "scripts/app.plenty"]);
    f.ok(&["cache", "clean"]);
    let text = std::fs::read_to_string(f.root().join("scripts/app.plenty")).unwrap();
    f.write(
        "scripts/app.plenty",
        &text.replace("---plenty\n", "---plenty\n[package]\nversion='0.2.0'\n"),
    );
    std::fs::rename(&repo, f.root().join("unavailable")).unwrap();
    f.fail(
        &[
            "script",
            "--locked",
            "--lockfile",
            "durable.lock",
            "scripts/app.plenty",
        ],
        "--locked",
    );
    assert!(
        !std::fs::read_dir(f.root().join("cache/scripts/state"))
            .unwrap()
            .any(|e| e.unwrap().path().join(".pkg").exists())
    );
}

#[test]
fn editor_checks_unsaved_headers_and_keeps_body_locations() {
    let f = Fixture::new();
    f.write("app.plenty", HELLO);
    let path = f.root().join("app.plenty").canonicalize().unwrap();
    let text = format!("---plenty\n[package]\ndescription='unsaved'\n---\n{HELLO}");
    let analysis = plenty::ide::analyze(&path, None, [(path.clone(), text)].into_iter().collect());
    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    let text = "---plenty\n---\ndef main() -> ():\n    missing()\n";
    let analysis = plenty::ide::analyze(
        &path,
        None,
        [(path.clone(), text.into())].into_iter().collect(),
    );
    assert_eq!(analysis.diagnostics[0].line, 4);
}

#[test]
fn inherited_directory_and_retained_output_survive_failed_rebuilds() {
    let f = Fixture::new();
    f.write("relative.txt", "caller directory");
    f.write("scripts/app.plenty", "def main() -> Result[(), IoError]:\n    print(read_text(\"relative.txt\")?)?\n    Ok(())\n");
    assert_eq!(
        f.ok(&["script", "scripts/app.plenty"]),
        "caller directory\n"
    );
    f.ok(&["script", "--compile", "-o", "app", "scripts/app.plenty"]);
    f.write("scripts/app.plenty", "def main() -> ():\n    missing()\n");
    f.fail(
        &["script", "--compile", "-o", "app", "scripts/app.plenty"],
        "missing",
    );
    let o = Command::new(f.root().join("app"))
        .current_dir(f.root())
        .output()
        .unwrap();
    assert!(o.status.success());
    assert_eq!(o.stdout, b"caller directory\n");
}
