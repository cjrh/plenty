//! `plenty.toml` projects: the `plenty` project commands, path dependencies,
//! `plenty.lock`, profiles, and the `plentyc` compiler binary.
use std::path::Path;
use std::process::{Command, Output};

fn plenty(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap()
}

fn succeeds(directory: &Path, args: &[&str]) -> String {
    let output = plenty(directory, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn fails(directory: &Path, args: &[&str], message: &str) {
    let output = plenty(directory, args);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{args:?} succeeded");
    assert!(
        stderr.contains(message),
        "{args:?}: expected {message:?}, got {stderr}"
    );
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A package directory with a manifest, `dependencies` lines, and sources.
fn package(root: &Path, name: &str, dependencies: &str, files: &[(&str, &str)]) {
    write(
        &root.join(name).join("plenty.toml"),
        &format!(
            "manifest-version = 1\n[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[dependencies]\n{dependencies}"
        ),
    );
    for (path, text) in files {
        write(&root.join(name).join(path), text);
    }
}

#[test]
fn new_application_runs_and_library_checks() {
    let dir = tempfile::tempdir().unwrap();
    succeeds(dir.path(), &["new", "hello"]);
    let app = dir.path().join("hello");
    assert_eq!(
        std::fs::read_to_string(app.join(".gitignore")).unwrap(),
        "/target\n/.pkg\n"
    );
    assert_eq!(succeeds(&app, &["run"]), "Hello from hello!\n");
    // Commands find the manifest from a subdirectory.
    assert_eq!(succeeds(&app.join("src"), &["run"]), "Hello from hello!\n");
    assert!(app.join("target/debug/hello").is_file());
    fails(&app, &["init"], "already exists");
    fails(dir.path(), &["new", "hello"], "already exists");

    succeeds(dir.path(), &["new", "shapes", "--lib"]);
    let lib = dir.path().join("shapes");
    succeeds(&lib, &["check"]);
    succeeds(&lib, &["build"]);
    fails(&lib, &["run"], "has no executable target");

    let named = dir.path().join("Named-Dir");
    std::fs::create_dir(&named).unwrap();
    fails(&named, &["init"], "--name NAME");
    succeeds(&named, &["init", "--name", "named"]);
    succeeds(&named, &["check"]);
    fails(dir.path(), &["check"], "could not find plenty.toml");
}

#[test]
fn run_forwards_arguments_and_exit_status() {
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "args",
        "",
        &[(
            "src/main.plenty",
            "def main() -> Result[i32, Failure]:\n    print(args()?)?\n    Ok(5)\n",
        )],
    );
    let output = plenty(&dir.path().join("args"), &["run", "--", "a", "--release"]);
    assert_eq!(output.status.code(), Some(5));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.ends_with("args\", \"a\", \"--release\"]\n"),
        "{stdout}"
    );
    fails(&dir.path().join("args"), &["run", "extra"], "after --");
}

#[test]
fn path_dependencies_resolve_by_alias_with_one_package_per_name() {
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "base",
        "",
        &[(
            "src/lib.plenty",
            "from helper import scale\n\npub class Point:\n    pub x: i64\n\npub def make(x: i64) -> Point:\n    Point(scale(x))\n",
        ), ("src/helper.plenty", "pub def scale(x: i64) -> i64:\n    x * 10\n")],
    );
    package(
        dir.path(),
        "left",
        "base = { path = \"../base\" }\n",
        &[(
            "src/lib.plenty",
            "import base\n\npub class Point:\n    pub y: i64\n\npub def via(x: i64) -> i64:\n    base.make(x).x\n",
        )],
    );
    // A different alias for the same package is still one package.
    package(
        dir.path(),
        "right",
        "[dependencies.core]\npath = \"../base\"\n",
        &[(
            "src/lib.plenty",
            "from core import make\n\npub def twice(x: i64) -> i64:\n    make(x).x * 2\n",
        )],
    );
    package(
        dir.path(),
        "app",
        "left = { path = \"../left\" }\nright = { path = \"../right\" }\n",
        &[
            (
                "src/main.plenty",
                "import left\nimport right\nfrom helper import scale\n\nclass Point:\n    z: i64\n\ndef main() -> Result[(), Failure]:\n    print(left.via(1) + right.twice(1) + left.Point(1).y + Point(2).z + scale(1))?\n    Ok(())\n",
            ),
            // The root package's own `helper` is distinct from base's.
            ("src/helper.plenty", "pub def scale(x: i64) -> i64:\n    x + 100\n"),
        ],
    );
    let app = dir.path().join("app");
    assert_eq!(succeeds(&app, &["run"]), "134\n");
    let lock = std::fs::read_to_string(app.join("plenty.lock")).unwrap();
    assert_eq!(lock.matches("[[package]]").count(), 4, "{lock}");
    assert!(lock.contains("source = \"path+../base\""), "{lock}");
    assert!(
        lock.contains("{ alias = \"core\", package = \"base\" }"),
        "{lock}"
    );

    // Distinct packages' types stay distinct.
    write(
        &app.join("src/main.plenty"),
        "import left\n\nclass Point:\n    z: i64\n\ndef take(p: Point) -> ():\n    pass\n\ndef main() -> ():\n    take(left.Point(1))\n",
    );
    fails(&app, &["check"], "expected Point, got left::lib.Point");

    // Transitive dependencies are not visible without a direct declaration.
    write(
        &app.join("src/main.plenty"),
        "import base\n\ndef main() -> ():\n    pass\n",
    );
    fails(
        &app,
        &["check"],
        "`base` is neither a local module nor a dependency declared by package `app`",
    );
}

#[test]
fn graph_errors_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let main = ("src/main.plenty", "def main() -> ():\n    pass\n");
    let lib = ("src/lib.plenty", "pub def f() -> i64:\n    1\n");
    package(dir.path(), "util", "", &[lib]);
    package(dir.path(), "other", "", &[lib]);
    write(
        &dir.path().join("other/plenty.toml"),
        "manifest-version = 1\n[package]\nname = \"util\"\nversion = \"0.2.0\"\n",
    );
    package(dir.path(), "binonly", "", &[main]);
    package(dir.path(), "a", "b = { path = \"../b\" }\n", &[lib]);
    package(dir.path(), "b", "a = { path = \"../a\" }\n", &[lib]);
    for (dependencies, files, message) in [
        (
            "util = { path = \"../util\" }\nalt = { path = \"../other\" }\n",
            vec![main],
            "two different packages are named `util`",
        ),
        (
            "a = { path = \"../a\" }\n",
            vec![main],
            "dependency cycle: app -> a -> b -> a",
        ),
        (
            "me = { path = \".\" }\n",
            vec![main],
            "dependency cycle: app -> app",
        ),
        (
            "binonly = { path = \"../binonly\" }\n",
            vec![main],
            "has no library target",
        ),
        (
            "gone = { path = \"../gone\" }\n",
            vec![main],
            "has no plenty.toml",
        ),
        (
            "util = { git = \"https://example.invalid/util\", tag = \"v1\" }\n",
            vec![main],
            "git dependencies are not supported yet",
        ),
        (
            "util = { path = \"../util\" }\n",
            vec![main, ("src/util.plenty", "")],
            "dependency `util` collides with the local module",
        ),
        (
            "std = { path = \"../util\" }\n",
            vec![main],
            "`std` is reserved",
        ),
    ] {
        let _ = std::fs::remove_dir_all(dir.path().join("app"));
        package(dir.path(), "app", dependencies, &files);
        fails(&dir.path().join("app"), &["check"], message);
    }
    write(
        &dir.path().join("app/plenty.toml"),
        "manifest-version = 1\n[package]\nname = \"app\"\nversion = \"0.1.0\"\nplenty-version = \"999.0.0\"\n",
    );
    fails(
        &dir.path().join("app"),
        &["check"],
        "requires Plenty 999.0.0 or newer",
    );
}

#[test]
fn lockfile_is_deterministic_and_locked_rejects_changes() {
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "util",
        "",
        &[("src/lib.plenty", "pub def f() -> i64:\n    1\n")],
    );
    package(
        dir.path(),
        "app",
        "util = { path = \"../util\" }\n",
        &[("src/main.plenty", "def main() -> ():\n    pass\n")],
    );
    let app = dir.path().join("app");
    fails(&app, &["build", "--locked"], "plenty.lock is missing");
    succeeds(&app, &["check"]);
    let lock = std::fs::read(app.join("plenty.lock")).unwrap();
    succeeds(&app, &["build", "--locked"]);
    assert_eq!(std::fs::read(app.join("plenty.lock")).unwrap(), lock);

    write(
        &app.join("plenty.toml"),
        "manifest-version = 1\n[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    );
    fails(&app, &["check", "--locked"], "does not match");
    succeeds(&app, &["check"]);
    assert_ne!(std::fs::read(app.join("plenty.lock")).unwrap(), lock);

    // Relocating the checkout keeps the lockfile valid.
    let moved = tempfile::tempdir().unwrap();
    for name in ["app", "util"] {
        std::fs::rename(dir.path().join(name), moved.path().join(name)).unwrap();
    }
    succeeds(&moved.path().join("app"), &["check", "--locked"]);
}

#[test]
fn profiles_tests_and_clean() {
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "calc",
        "",
        &[
            ("src/lib.plenty", "pub def add(a: i64, b: i64) -> i64:\n    a + b\n"),
            (
                "src/main.plenty",
                "from calc import add\n\ndef main() -> Result[(), Failure]:\n    print(add(2, 3))?\n    Ok(())\n",
            ),
            ("src/calc.plenty", "pub def add(a: i64, b: i64) -> i64:\n    a + b\n"),
            (
                "tests/adds.plenty",
                "import calc\n\ndef main() -> i32:\n    if calc.add(2, 2) == 4:\n        0\n    else:\n        1\n",
            ),
            ("tests/broken.plenty", "def main() -> i32:\n    3\n"),
            ("tests/support/not_a_test.plenty", "pub def x() -> i64:\n    1\n"),
        ],
    );
    let calc = dir.path().join("calc");
    succeeds(&calc, &["build", "--release"]);
    assert!(calc.join("target/release/calc").is_file());
    assert!(!calc.join("target/debug").exists());
    assert_eq!(succeeds(&calc, &["run", "--release"]), "5\n");

    let output = plenty(&calc, &["test"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success());
    assert_eq!(
        stdout,
        "running 2 tests\ntest adds ... ok\ntest broken ... FAILED (exit status 3)\n\ntest result: FAILED. 1 passed; 1 failed\n"
    );
    std::fs::remove_file(calc.join("tests/broken.plenty")).unwrap();
    assert!(
        succeeds(&calc, &["test", "--release"]).ends_with("test result: ok. 1 passed; 0 failed\n")
    );

    succeeds(&calc, &["clean"]);
    assert!(!calc.join("target").exists());
}

#[test]
fn manifest_targets_and_errors() {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("tools");
    write(
        &app.join("plenty.toml"),
        "manifest-version = 1\n[package]\nname = \"tools\"\nversion = \"1.0.0\"\n\n[[bin]]\nname = \"one\"\npath = \"src/one.plenty\"\n\n[[bin]]\nname = \"two\"\npath = \"src/two.plenty\"\n",
    );
    for name in ["one", "two"] {
        write(
            &app.join(format!("src/{name}.plenty")),
            &format!("def main() -> Result[(), Failure]:\n    print(\"{name}\")?\n    Ok(())\n"),
        );
    }
    fails(&app, &["run"], "choose one with --bin NAME: one, two");
    assert_eq!(succeeds(&app, &["run", "--bin", "two"]), "two\n");
    fails(
        &app,
        &["run", "--bin", "three"],
        "no executable target `three`",
    );

    write(&app.join("plenty.toml"), "manifest-version = 1\n[package]\nname = \"tools\"\nversion = \"1.0.0\"\nedition = \"2024\"\n");
    fails(
        &app,
        &["check"],
        "plenty.toml:5: unknown key `edition` in [package]",
    );
    write(&app.join("plenty.toml"), "[package]\nname = \"tools\"\n");
    fails(&app, &["check"], "missing `manifest-version = 1`");
}

#[test]
fn plentyc_is_the_single_file_compiler_and_names_that_match_commands_need_a_path() {
    let dir = tempfile::tempdir().unwrap();
    let source = "def main() -> Result[(), Failure]:\n    print(\"file\")?\n    Ok(())\n";
    write(&dir.path().join("run"), source);
    let output = Command::new(env!("CARGO_BIN_EXE_plentyc"))
        .args(["--release", "run"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"file\n");
    assert_eq!(succeeds(dir.path(), &["./run"]), "file\n");
    let help = Command::new(env!("CARGO_BIN_EXE_plentyc"))
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&help.stdout).starts_with("Usage: plentyc FILE"));
    fails(
        dir.path(),
        &["--release", "--check", "run"],
        "--release requires FILE or --compile",
    );
}
