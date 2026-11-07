//! `plenty.toml` projects: the `plenty` project commands, path dependencies,
//! `plenty.lock`, profiles, and the `plentyc` compiler binary.

mod support;
use std::path::Path;
use std::process::{Command, Output};
use support::write_file as write;

/// Git settings that make fixture repositories independent of the user's
/// configuration (signing, tag editors, default branch names).
fn isolate_git(command: &mut Command) -> &mut Command {
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
}

fn plenty(directory: &Path, args: &[&str]) -> Output {
    isolate_git(&mut Command::new(env!("CARGO_BIN_EXE_plenty")))
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap()
}

/// Standard error of a successful command, for its status lines.
fn status_lines(directory: &Path, args: &[&str]) -> String {
    let output = plenty(directory, args);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{args:?}: {stderr}");
    stderr
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
    assert!(app.join("target/optimized/hello").is_file());
    fails(&app, &["init"], "already exists");
    fails(dir.path(), &["new", "hello"], "already exists");

    succeeds(dir.path(), &["new", "shapes", "--lib"]);
    let lib = dir.path().join("shapes");
    succeeds(&lib, &["check"]);
    succeeds(&lib, &["build"]);
    fails(&lib, &["run"], "has no executable target");
    assert_eq!(
        std::fs::read_to_string(lib.join("src/lib.plenty")).unwrap(),
        "pub def add(left: i64, right: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(left + right)\n"
    );
    package(
        dir.path(),
        "consumer",
        "shapes = { path = \"../shapes\" }\n",
        &[(
            "src/main.plenty",
            "import shapes\ndef main() -> Result[(), IoError | OverflowError[i64]]:\n    print(shapes.add(2, 3)?)?\n    match shapes.add(i64.MAX, 1):\n        case Ok(value):\n            print(\"unexpected\", value)?\n        case Err(error):\n            print(error.wrapped)?\n    Ok(())\n",
        )],
    );
    assert_eq!(
        succeeds(&dir.path().join("consumer"), &["run"]),
        "5\n-9223372036854775808\n"
    );

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
            "def main() -> Result[i32, AllocError | IoError]:\n    print(args()?)?\n    Ok(5)\n",
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
        &[
            (
                "src/lib.plenty",
                "from helper import scale\n\npub class Point:\n    pub x: i64\n\npub def make(x: i64) -> Result[Point, OverflowError[i64]]:\n    Ok(Point(scale(x)?))\n",
            ),
            (
                "src/helper.plenty",
                "pub def scale(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 10)\n",
            ),
        ],
    );
    package(
        dir.path(),
        "left",
        "base = { path = \"../base\" }\n",
        &[(
            "src/lib.plenty",
            "import base\n\npub class Point:\n    pub y: i64\n\npub def via(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(base.make(x)?.x)\n",
        )],
    );
    // A different alias for the same package is still one package.
    package(
        dir.path(),
        "right",
        "[dependencies.core]\npath = \"../base\"\n",
        &[(
            "src/lib.plenty",
            "from core import make\n\npub def twice(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(make(x)?.x * 2)\n",
        )],
    );
    package(
        dir.path(),
        "app",
        "left = { path = \"../left\" }\nright = { path = \"../right\" }\n",
        &[
            (
                "src/main.plenty",
                "import left\nimport right\nfrom helper import scale\n\nclass Point:\n    z: i64\n\ndef main() -> Result[(), IoError | OverflowError[i64]]:\n    print(left.via(1)? + right.twice(1)? + left.Point(1).y + Point(2).z + scale(1)?)?\n    Ok(())\n",
            ),
            // The root package's own `helper` is distinct from base's.
            (
                "src/helper.plenty",
                "pub def scale(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x + 100)\n",
            ),
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

/// A dependency's library and its other modules bind under their own paths
/// together, unless the library has a member that the module path would also
/// name (issue #568).
#[test]
fn a_dependency_library_and_its_modules_import_together() {
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "tz",
        "",
        &[
            ("src/lib.plenty", "pub def base() -> i64:\n    100\n"),
            (
                "src/zones.plenty",
                "pub enum Zone:\n    Utc\n    Fixed(i64)\n\npub const HOURS: i64 = 24\n\npub def offset(zone: Zone) -> i64:\n    match zone:\n        case Zone.Utc:\n            0\n        case Zone.Fixed(n):\n            n\n",
            ),
            ("src/rules/dst.plenty", "pub def shift() -> i64:\n    1\n"),
            (
                "tests/zones.plenty",
                "import tz\nimport tz.zones\n\ndef main() -> Result[i32, OverflowError[i64]]:\n    if tz.base() + tz.zones.offset(tz.zones.Zone.Fixed(2)) == 102:\n        Ok(0)\n    else:\n        Ok(1)\n",
            ),
        ],
    );
    let tz = dir.path().join("tz");
    assert!(succeeds(&tz, &["test"]).ends_with("test result: ok. 1 passed; 0 failed\n"));

    package(
        dir.path(),
        "app",
        "tz = { path = \"../tz\" }\n",
        &[(
            "src/main.plenty",
            "import tz.zones\nimport tz\nimport tz.rules.dst\n\ndef local(zone: tz.zones.Zone) -> Result[i64, OverflowError[i64]]:\n    Ok(tz.zones.offset(zone) + tz.rules.dst.shift())\n\ndef main() -> Result[(), IoError | OverflowError[i64]]:\n    print(tz.base() + local(tz.zones.Zone.Fixed(3))? + tz.zones.HOURS)?\n    Ok(())\n",
        )],
    );
    let app = dir.path().join("app");
    assert_eq!(succeeds(&app, &["run"]), "128\n");

    // `from` imports a declaration of the library, not a module beside it.
    write(
        &app.join("src/main.plenty"),
        "from tz import zones\n\ndef main() -> ():\n    pass\n",
    );
    fails(
        &app,
        &["check"],
        "`from` imports declarations, and `tz.zones` is a module; use `import tz.zones` or `import tz.zones as zones`",
    );

    // A library member named like the module path is a real clash, in
    // either import order and for deeper paths.
    write(
        &tz.join("src/lib.plenty"),
        "pub def base() -> i64:\n    100\n\npub def zones() -> i64:\n    1\n\ndef rules() -> i64:\n    2\n",
    );
    for (main, message) in [
        (
            "import tz\nimport tz.zones\n",
            "`import tz.zones` conflicts with `import tz`: module `tz` declares `zones`, so `tz.zones` would name both; bind the nested module under another name, such as `import tz.zones as zones`",
        ),
        (
            "import tz.zones\nimport tz\n",
            "`import tz` conflicts with `import tz.zones`: module `tz` declares `zones`",
        ),
        (
            "import tz\nimport tz.rules.dst\n",
            "module `tz` declares `rules`, so `tz.rules` would name both; bind the nested module under another name, such as `import tz.rules.dst as dst`",
        ),
    ] {
        write(
            &app.join("src/main.plenty"),
            &format!("{main}\ndef main() -> ():\n    pass\n"),
        );
        fails(&app, &["check"], message);
    }
    write(
        &app.join("src/main.plenty"),
        "import tz\nimport tz.zones as zones\n\ndef main() -> Result[(), IoError | OverflowError[i64]]:\n    print(tz.zones() + zones.HOURS)?\n    Ok(())\n",
    );
    assert_eq!(succeeds(&app, &["run"]), "25\n");
}

/// A module path names no declaration: `from` cannot import a module, and a
/// module is not a value (issue #587). A missing name that is not a module
/// keeps the visibility diagnostic.
#[test]
fn a_dependency_module_is_not_a_declaration() {
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "tz",
        "",
        &[
            (
                "src/lib.plenty",
                "pub def base() -> i64:\n    100\n\ndef hidden() -> i64:\n    1\n",
            ),
            (
                "src/zones.plenty",
                "pub def offset() -> i64:\n    1\n\npub const HOURS: i64 = 24\n",
            ),
            ("src/hidden.plenty", "pub def f() -> i64:\n    1\n"),
            ("src/rules/dst.plenty", "pub def shift() -> i64:\n    1\n"),
        ],
    );
    package(dir.path(), "app", "tz = { path = \"../tz\" }\n", &[]);
    let app = dir.path().join("app");
    let private = |path: &str| format!("`{path}` is private or is not a declared export");
    for (main, message) in [
        (
            "import tz.zones\nfrom tz import zones as z\n",
            "`from` imports declarations, and `tz.zones` is a module; use `import tz.zones` or `import tz.zones as z`".to_string(),
        ),
        (
            "from tz.rules import dst\n",
            "`from` imports declarations, and `tz.rules.dst` is a module; use `import tz.rules.dst` or `import tz.rules.dst as dst`".to_string(),
        ),
        ("from tz import missing\n", private("tz.missing")),
        // A private declaration of the library is reported as private, even
        // beside a module of the same name.
        ("from tz import hidden\n", private("tz.hidden")),
        ("from tz.zones import missing\n", private("tz.zones.missing")),
        (
            "import tz\nimport tz.zones\n\ndef local() -> ():\n    x = tz.zones\n",
            "`tz.zones` is a module, not a declaration; name one of its declarations, such as `tz.zones.HOURS`".to_string(),
        ),
        (
            "import tz\nimport tz.zones\n\ndef local(x: tz.zones) -> ():\n    pass\n",
            "`tz.zones` is a module, not a declaration".to_string(),
        ),
        (
            "import tz\nimport tz.rules.dst\n\ndef local() -> ():\n    x = tz.rules\n",
            "`tz.rules` is a module namespace, not a declaration; name a declaration of an imported module, such as `tz.rules.dst.shift`".to_string(),
        ),
        (
            "import tz\n\ndef local() -> ():\n    x = tz.missing\n",
            private("tz.missing"),
        ),
        // Without `import tz`, the path is not below a bound module.
        (
            "import tz.zones\n\ndef local() -> ():\n    x = tz.zones\n",
            "`tz.zones` is a module, not a declaration".to_string(),
        ),
        (
            "import tz.rules.dst\n\ndef local() -> ():\n    x = tz.rules\n",
            "`tz.rules` is a module namespace, not a declaration; name a declaration of an imported module, such as `tz.rules.dst.shift`".to_string(),
        ),
        (
            "import tz\n\ndef local() -> ():\n    x = tz\n",
            "`tz` is a module, not a declaration; name one of its declarations, such as `tz.base`".to_string(),
        ),
    ] {
        write(
            &app.join("src/main.plenty"),
            &format!("{main}\ndef main() -> ():\n    pass\n"),
        );
        fails(&app, &["check"], &message);
    }
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
            "util = { path = \"../util\", version = \"^0.2.0\" }\n",
            vec![main],
            "package `app` requires `util` ^0.2.0, but",
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
            (
                "src/lib.plenty",
                "pub def add(a: i64, b: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(a + b)\n",
            ),
            (
                "src/main.plenty",
                "from calc import add\n\ndef main() -> Result[(), IoError | OverflowError[i64]]:\n    print(add(2, 3)?)?\n    Ok(())\n",
            ),
            (
                "src/calc.plenty",
                "pub def add(a: i64, b: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(a + b)\n",
            ),
            (
                "tests/adds.plenty",
                "import calc\n\ndef main() -> Result[i32, OverflowError[i64]]:\n    if calc.add(2, 2)? == 4:\n        Ok(0)\n    else:\n        Ok(1)\n",
            ),
            ("tests/broken.plenty", "def main() -> i32:\n    3\n"),
            (
                "tests/support/not_a_test.plenty",
                "pub def x() -> i64:\n    1\n",
            ),
        ],
    );
    let calc = dir.path().join("calc");
    succeeds(&calc, &["build", "--release"]);
    assert!(calc.join("target/optimized/calc").is_file());
    assert!(!calc.join("target/fast-compile").exists());
    succeeds(&calc, &["build", "--fast-compile"]);
    assert!(calc.join("target/fast-compile/calc").is_file());
    assert_eq!(succeeds(&calc, &["run", "--fast-compile"]), "5\n");
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
        succeeds(&calc, &["test", "--fast-compile"])
            .ends_with("test result: ok. 1 passed; 0 failed\n")
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
            &format!("def main() -> Result[(), IoError]:\n    print(\"{name}\")?\n    Ok(())\n"),
        );
    }
    fails(&app, &["run"], "choose one with --bin NAME: one, two");
    assert_eq!(succeeds(&app, &["run", "--bin", "two"]), "two\n");
    fails(
        &app,
        &["run", "--bin", "three"],
        "no executable target `three`",
    );

    write(
        &app.join("plenty.toml"),
        "manifest-version = 1\n[package]\nname = \"tools\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
    );
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
    let source = "def main() -> Result[(), IoError]:\n    print(\"file\")?\n    Ok(())\n";
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
        "--release requires a compilation mode",
    );
}

/// Run `git` in `directory` and return its trimmed standard output.
fn git(directory: &Path, args: &[&str]) -> String {
    let output = isolate_git(&mut Command::new("git"))
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// Write `files` into the repository at `repo` (creating it if needed),
/// commit them, and return the commit.
fn commit(repo: &Path, files: &[(&str, &str)]) -> String {
    if !repo.join(".git").exists() {
        std::fs::create_dir_all(repo).unwrap();
        git(repo, &["init", "--quiet"]);
    }
    for (path, text) in files {
        write(&repo.join(path), text);
    }
    git(repo, &["add", "--all"]);
    git(repo, &["commit", "--quiet", "--message", "change"]);
    git(repo, &["rev-parse", "HEAD"])
}

fn url(path: &Path) -> String {
    format!("file://{}", path.display())
}

fn library_manifest(name: &str, version: &str, dependencies: &str) -> String {
    format!(
        "manifest-version = 1\n[package]\nname = \"{name}\"\nversion = \"{version}\"\n\n[dependencies]\n{dependencies}"
    )
}

fn twice_application(root: &Path, dependency: &str) -> std::path::PathBuf {
    package(
        root,
        "app",
        dependency,
        &[(
            "src/main.plenty",
            "import util\n\ndef main() -> Result[(), IoError | OverflowError[i64]]:\n    print(util.twice(21)?)?\n    Ok(())\n",
        )],
    );
    root.join("app")
}

#[test]
fn git_dependencies_pin_commits_in_the_lockfile() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("util-repo");
    let first = commit(
        &repo,
        &[
            ("plenty.toml", &library_manifest("util", "1.0.0", "")),
            (
                "src/lib.plenty",
                "pub def twice(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 2)\n",
            ),
        ],
    );
    git(&repo, &["tag", "v1"]);
    let app = twice_application(
        dir.path(),
        &format!("util = {{ git = \"{}\", tag = \"v1\" }}\n", url(&repo)),
    );
    fails(&app, &["check", "--locked"], "plenty.lock is missing");
    fails(
        &app,
        &["check", "--offline"],
        "is not in the package cache, and --offline forbids fetching it",
    );
    let stderr = status_lines(&app, &["build"]);
    assert_eq!(stderr.matches("Fetching").count(), 1, "{stderr}");
    assert_eq!(succeeds(&app, &["run"]), "42\n");
    let lock = std::fs::read_to_string(app.join("plenty.lock")).unwrap();
    assert!(
        lock.contains(&format!(
            "source = \"git+{}\"\ncommit = \"{first}\"\nselectors = [\"tag=v1\"]\n",
            url(&repo)
        )),
        "{lock}"
    );

    // Moving the tag upstream does not move the locked build.
    let second = commit(
        &repo,
        &[(
            "src/lib.plenty",
            "pub def twice(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 3)\n",
        )],
    );
    git(&repo, &["tag", "--force", "v1"]);
    assert_eq!(succeeds(&app, &["run", "--locked"]), "42\n");
    assert_eq!(succeeds(&app, &["run", "--offline"]), "42\n");

    // A deleted cache is refetched at the same commit.
    std::fs::remove_dir_all(app.join(".pkg")).unwrap();
    fails(&app, &["run", "--offline"], "--offline forbids fetching it");
    assert_eq!(succeeds(&app, &["run", "--locked"]), "42\n");
    assert_eq!(
        std::fs::read_to_string(app.join("plenty.lock")).unwrap(),
        lock
    );

    // `plenty update` follows the tag.
    let stderr = status_lines(&app, &["update"]);
    assert!(
        stderr.contains(&format!(
            "Updating util {} -> {}",
            &first[..12],
            &second[..12]
        )),
        "{stderr}"
    );
    assert_eq!(succeeds(&app, &["run"]), "63\n");
    fails(&app, &["update", "nothing"], "no package named `nothing`");

    // An exact revision, abbreviated.
    write(
        &app.join("plenty.toml"),
        &library_manifest(
            "app",
            "0.1.0",
            &format!(
                "util = {{ git = \"{}\", rev = \"{}\" }}\n",
                url(&repo),
                &first[..10]
            ),
        ),
    );
    fails(
        &app,
        &["check", "--locked"],
        "pins no commit for dependency `util`",
    );
    assert_eq!(succeeds(&app, &["run"]), "42\n");
    let lock = std::fs::read_to_string(app.join("plenty.lock")).unwrap();
    assert!(
        lock.contains(&format!(
            "commit = \"{first}\"\nselectors = [\"rev={}\"]",
            &first[..10]
        )),
        "{lock}"
    );

    for (selector, message) in [
        ("tag = \"v9\"", "has no tag `v9`"),
        ("rev = \"0000000\"", "has no commit 0000000"),
    ] {
        write(
            &app.join("plenty.toml"),
            &library_manifest(
                "app",
                "0.1.0",
                &format!("util = {{ git = \"{}\", {selector} }}\n", url(&repo)),
            ),
        );
        fails(&app, &["check"], message);
    }
}

#[test]
fn a_locked_commit_that_upstream_deleted_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("util-repo");
    commit(
        &repo,
        &[
            ("plenty.toml", &library_manifest("util", "1.0.0", "")),
            (
                "src/lib.plenty",
                "pub def twice(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 2)\n",
            ),
        ],
    );
    git(&repo, &["checkout", "--quiet", "-b", "side"]);
    let side = commit(
        &repo,
        &[(
            "src/lib.plenty",
            "pub def twice(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x + x)\n",
        )],
    );
    git(&repo, &["checkout", "--quiet", "main"]);
    let app = twice_application(
        dir.path(),
        &format!("util = {{ git = \"{}\", rev = \"{side}\" }}\n", url(&repo)),
    );
    assert_eq!(succeeds(&app, &["run"]), "42\n");

    git(&repo, &["branch", "--quiet", "-D", "side"]);
    git(&repo, &["reflog", "expire", "--expire=now", "--all"]);
    git(&repo, &["gc", "--quiet", "--prune=now"]);
    std::fs::remove_dir_all(app.join(".pkg")).unwrap();
    fails(
        &app,
        &["check"],
        &format!("the locked commit {side} is no longer available from"),
    );
}

/// A lockfile commit must be a full commit ID: diagnostics abbreviate it,
/// and Git would otherwise accept a short prefix (issue #171).
#[test]
fn a_locked_commit_that_is_not_a_full_commit_id_is_malformed() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("util-repo");
    let head = commit(
        &repo,
        &[
            ("plenty.toml", &library_manifest("util", "1.0.0", "")),
            (
                "src/lib.plenty",
                "pub def twice(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 2)\n",
            ),
        ],
    );
    let app = twice_application(
        dir.path(),
        &format!("util = {{ git = \"{}\", rev = \"{head}\" }}\n", url(&repo)),
    );
    assert_eq!(succeeds(&app, &["run"]), "42\n");
    let lock = std::fs::read_to_string(app.join("plenty.lock")).unwrap();
    for bad in [&head[..7], "ééééééééééé", &head.to_uppercase()] {
        write(&app.join("plenty.lock"), &lock.replace(&head, bad));
        fails(&app, &["check"], "plenty.lock is malformed");
    }
}

#[test]
fn git_subdirectories_share_one_commit_per_package() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("mono");
    let geo = (
        "geo/src/lib.plenty",
        "pub def scale(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 10)\n",
    );
    commit(
        &repo,
        &[
            ("geo/plenty.toml", &library_manifest("geo", "1.4.0", "")),
            geo,
            (
                "shapes/plenty.toml",
                &library_manifest("shapes", "1.0.0", "geo = { path = \"../geo\" }\n"),
            ),
            (
                "shapes/src/lib.plenty",
                "import geo\n\npub def area(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(geo.scale(x)? * x)\n",
            ),
        ],
    );
    git(&repo, &["tag", "v1"]);
    let mono = url(&repo);
    let dependencies = |geo_tag: &str, extra: &str| {
        format!(
            "geo = {{ git = \"{mono}\", subdir = \"geo\", tag = \"{geo_tag}\"{extra} }}\nshapes = {{ git = \"{mono}\", subdir = \"shapes\", tag = \"v1\" }}\n"
        )
    };
    package(
        dir.path(),
        "app",
        &dependencies("v1", ", version = \"1.2.0\""),
        &[(
            "src/main.plenty",
            "import geo\nimport shapes\n\ndef main() -> Result[(), IoError | OverflowError[i64]]:\n    print(shapes.area(2)? + geo.scale(1)?)?\n    Ok(())\n",
        )],
    );
    let app = dir.path().join("app");
    let stderr = status_lines(&app, &["build"]);
    assert_eq!(stderr.matches("Fetching").count(), 1, "{stderr}");
    assert_eq!(succeeds(&app, &["run"]), "50\n");
    let lock = std::fs::read_to_string(app.join("plenty.lock")).unwrap();
    assert_eq!(lock.matches("[[package]]").count(), 3, "{lock}");
    assert!(lock.contains("subdir = \"geo\""), "{lock}");

    // A second tag at a different commit cannot join the same graph.
    commit(
        &repo,
        &[(
            "geo/src/lib.plenty",
            "pub def scale(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 100)\n",
        )],
    );
    git(&repo, &["tag", "v2"]);
    write(
        &app.join("plenty.toml"),
        &library_manifest("app", "0.1.0", &dependencies("v2", "")),
    );
    fails(
        &app,
        &["check"],
        "conflicting requirements for package `geo`",
    );
    fails(&app, &["check"], "app -> shapes requires");

    write(
        &app.join("plenty.toml"),
        &library_manifest(
            "app",
            "0.1.0",
            &dependencies("v1", ", version = \"^2.0.0\""),
        ),
    );
    fails(&app, &["check"], "requires `geo` ^2.0.0, but");

    // Path dependencies inside a repository cannot leave it.
    commit(
        &repo,
        &[(
            "shapes/plenty.toml",
            &library_manifest("shapes", "1.0.0", "geo = { path = \"../../outside\" }\n"),
        )],
    );
    git(&repo, &["tag", "v3"]);
    package(dir.path(), "outside", "", &[geo]);
    write(
        &app.join("plenty.toml"),
        &library_manifest(
            "app",
            "0.1.0",
            &format!("shapes = {{ git = \"{mono}\", subdir = \"shapes\", tag = \"v3\" }}\n"),
        ),
    );
    fails(&app, &["check"], "leaves its repository");
}

// A cycle through Git subdirectories is rejected like a local path cycle.
#[test]
fn git_subdirectory_cycles_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("mono");
    let mono = url(&repo);
    let lib = ("src/lib.plenty", "pub def one() -> i64:\n    1\n");
    commit(
        &repo,
        &[
            (
                "a/plenty.toml",
                &library_manifest("a", "1.0.0", "b = { path = \"../b\" }\n"),
            ),
            ("a/src/lib.plenty", lib.1),
            (
                "b/plenty.toml",
                &library_manifest(
                    "b",
                    "1.0.0",
                    &format!("a = {{ git = \"{mono}\", subdir = \"a\", tag = \"v1\" }}\n"),
                ),
            ),
            ("b/src/lib.plenty", lib.1),
        ],
    );
    git(&repo, &["tag", "v1"]);
    package(
        dir.path(),
        "app",
        &format!("a = {{ git = \"{mono}\", subdir = \"a\", tag = \"v1\" }}\n"),
        &[lib],
    );
    let app = dir.path().join("app");
    fails(&app, &["check"], "dependency cycle: app -> a -> b -> a");
    assert!(!app.join("plenty.lock").exists());
}

#[test]
fn add_edits_the_manifest_and_lockfile() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("util-repo");
    commit(
        &repo,
        &[
            ("plenty.toml", &library_manifest("util", "1.0.0", "")),
            (
                "src/lib.plenty",
                "pub def twice(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 2)\n",
            ),
        ],
    );
    git(&repo, &["tag", "v1"]);
    package(
        dir.path(),
        "local",
        "",
        &[("src/lib.plenty", "pub def one() -> i64:\n    1\n")],
    );
    let app = twice_application(dir.path(), "");
    let manifest = std::fs::read_to_string(app.join("plenty.toml")).unwrap();

    // A failed resolution leaves the manifest unchanged.
    fails(
        &app,
        &[
            "add",
            "util",
            "--git",
            &url(&dir.path().join("missing")),
            "--tag",
            "v1",
        ],
        "fetching",
    );
    fails(
        &app,
        &[
            "add",
            "util",
            "--git",
            &url(&repo),
            "--tag",
            "v1",
            "--version",
            "2.0.0",
        ],
        "requires `util` ^2.0.0, but",
    );
    assert_eq!(
        std::fs::read_to_string(app.join("plenty.toml")).unwrap(),
        manifest
    );
    fails(&app, &["add", "--git", &url(&repo)], "usage: plenty add");

    let stderr = status_lines(&app, &["add", "--git", &url(&repo), "--tag", "v1"]);
    assert!(stderr.contains("Added dependency `util`"), "{stderr}");
    assert_eq!(stderr.matches("Fetching").count(), 1, "{stderr}");
    assert_eq!(succeeds(&app, &["run", "--locked"]), "42\n");
    fails(
        &app,
        &["add", "util", "--path", "../local"],
        "already has a dependency named `util`",
    );

    // `--path` is relative to the current directory, not the manifest.
    let stderr = status_lines(&app.join("src"), &["add", "--path", "../../local"]);
    assert!(stderr.contains("Added dependency `local`"), "{stderr}");
    let manifest = std::fs::read_to_string(app.join("plenty.toml")).unwrap();
    assert!(
        manifest.ends_with(&format!(
            "[dependencies]\nutil = {{ git = \"{}\", tag = \"v1\" }}\nlocal = {{ path = \"../local\" }}\n",
            url(&repo)
        )),
        "{manifest}"
    );
    succeeds(&app, &["check", "--locked"]);
}

#[test]
fn builds_reuse_up_to_date_outputs() {
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "util",
        "",
        &[(
            "src/lib.plenty",
            "pub def twice(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 2)\n",
        )],
    );
    let app = twice_application(dir.path(), "util = { path = \"../util\" }\n");
    write(
        &app.join("tests/math.plenty"),
        "import std.test\nimport util\n\ndef test_twice() -> Result[(), std.test.TestError | OverflowError[i64]]:\n    std.test.equal(&util.twice(2)?, &5)?\n    Ok(())\n",
    );
    assert!(status_lines(&app, &["build"]).contains("Compiling app v0.1.0 (optimized)"));
    let stderr = status_lines(&app, &["build"]);
    assert!(stderr.contains("Fresh app v0.1.0 (optimized)"), "{stderr}");
    assert!(!stderr.contains("Compiling"), "{stderr}");
    assert!(status_lines(&app, &["build", "--fast-compile"]).contains("Compiling"));

    // A dependency's source is an input.
    write(
        &dir.path().join("util/src/lib.plenty"),
        "pub def twice(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x + x + 1)\n",
    );
    assert!(status_lines(&app, &["build"]).contains("Compiling"));
    assert_eq!(succeeds(&app, &["run"]), "43\n");

    // A deleted output is rebuilt, and a failed build is not recorded.
    std::fs::remove_file(app.join("target/optimized/app")).unwrap();
    assert!(status_lines(&app, &["build"]).contains("Compiling"));
    write(
        &app.join("src/extra.plenty"),
        "pub def broken() -> i64:\n    nope\n",
    );
    write(
        &app.join("src/main.plenty"),
        "import extra\n\ndef main() -> ():\n    pass\n",
    );
    fails(&app, &["build"], "nope");
    fails(&app, &["build"], "nope");
    std::fs::remove_file(app.join("src/extra.plenty")).unwrap();
    write(
        &app.join("src/main.plenty"),
        "def main() -> ():\n    pass\n",
    );

    let first = plenty(&app, &["test"]);
    assert!(first.status.success());
    let second = plenty(&app, &["test"]);
    assert!(String::from_utf8_lossy(&second.stderr).contains("Fresh"));
    assert_eq!(first.stdout, second.stdout);
    assert!(app.join("target/optimized/tests/app/math").is_file());
}

#[test]
fn workspaces_share_a_lockfile_and_target_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("plenty.toml"),
        "manifest-version = 1\n\n[workspace]\nmembers = [\"util\", \"app\"]\n",
    );
    package(
        root,
        "util",
        "",
        &[
            (
                "src/lib.plenty",
                "pub def twice(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 2)\n",
            ),
            (
                "tests/unit.plenty",
                "import std.test\nimport util\n\ndef test_twice() -> Result[(), std.test.TestError | OverflowError[i64]]:\n    std.test.equal(&util.twice(1)?, &2)?\n    Ok(())\n",
            ),
        ],
    );
    twice_application(root, "util = { path = \"../util\" }\n");
    write(
        &root.join("app/tests/smoke.plenty"),
        "def main() -> i32:\n    0\n",
    );

    assert_eq!(succeeds(root, &["run"]), "42\n");
    assert!(root.join("plenty.lock").is_file());
    assert!(root.join("target/optimized/app").is_file());
    let stdout = succeeds(root, &["test"]);
    assert!(
        stdout.starts_with(
            "running 2 tests\ntest util/unit::test_twice ... ok\ntest app/smoke ... ok\n"
        ),
        "{stdout}"
    );
    let stdout = succeeds(&root.join("app/src"), &["test"]);
    assert!(
        stdout.starts_with("running 1 test\ntest smoke ... ok\n"),
        "{stdout}"
    );
    assert!(!root.join("app/plenty.lock").exists());
    assert!(!root.join("app/target").exists());
    succeeds(&root.join("util"), &["check", "--locked"]);
    fails(
        root,
        &["add", "--path", "util"],
        "a workspace manifest is not a package",
    );
    fails(
        root,
        &["graph"],
        "run `plenty graph` in one member's directory",
    );
    succeeds(root, &["clean"]);
    assert!(!root.join("target").exists());

    write(
        &root.join("plenty.toml"),
        "manifest-version = 1\n\n[workspace]\nmembers = [\"util\", \"missing\"]\n",
    );
    fails(root, &["check"], "member missing has no plenty.toml");
    write(
        &root.join("plenty.toml"),
        "manifest-version = 1\n\n[workspace]\nmembers = [\"../elsewhere\"]\n",
    );
    fails(root, &["check"], "relative directories below the workspace");
}

#[test]
fn plentyc_compiles_through_a_package_graph() {
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "util",
        "",
        &[(
            "src/lib.plenty",
            "pub def twice(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x * 2)\n",
        )],
    );
    let app = twice_application(dir.path(), "util = { path = \"../util\" }\n");
    write(&app.join("graph.toml"), &succeeds(&app, &["graph"]));
    let plentyc = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_plentyc"))
            .args(args)
            .current_dir(&app)
            .output()
            .unwrap()
    };
    let run = plentyc(&["--package-graph", "graph.toml", "src/main.plenty"]);
    assert_eq!(
        run.stdout,
        b"42\n",
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        plentyc(&[
            "--package-graph",
            "graph.toml",
            "--check",
            "src/main.plenty"
        ])
        .status
        .success()
    );
    let compile = plentyc(&[
        "--package-graph",
        "graph.toml",
        "--compile",
        "src/main.plenty",
        "-o",
        "out",
    ]);
    assert!(compile.status.success());
    assert_eq!(
        Command::new(app.join("out")).output().unwrap().stdout,
        b"42\n"
    );

    // Without the graph, `util` is just a missing module.
    assert!(!plentyc(&["--check", "src/main.plenty"]).status.success());
    let both = plentyc(&[
        "--package-graph",
        "graph.toml",
        "--module-root",
        "src",
        "--check",
        "src/main.plenty",
    ]);
    assert!(String::from_utf8_lossy(&both.stderr).contains("excludes --module-root"));
}

#[test]
fn check_all_methods_checks_unused_methods_of_every_target() {
    // A library's generic method is checked for the instance its own code
    // creates only under `--check-all-methods` (issue #517).
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "cells",
        "",
        &[(
            "src/lib.plenty",
            "class Cell[T]:
    value: T
    def get(self) -> T:
        self.value

def lists() -> Result[Cell[list[i64]], AllocError]:
    Ok(Cell([1]?))
",
        )],
    );
    let cells = dir.path().join("cells");
    succeeds(&cells, &["check"]);
    fails(
        &cells,
        &["check", "--check-all-methods"],
        "lib.plenty:6:23: `Cell[list[i64]].get` copies `T`, but `list[i64]` cannot be copied",
    );
    fails(
        &cells,
        &["build", "--check-all-methods"],
        "unexpected argument \"--check-all-methods\"",
    );
}

#[test]
fn check_all_methods_skips_types_from_dependencies() {
    // Only the checked package's own generic types have every method checked
    // (issue #517); a dependency's author chose which methods each instance
    // supports.
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "cells",
        "",
        &[(
            "src/lib.plenty",
            "pub class Cell[T]:
    pub value: T
    pub def get(self) -> T:
        self.value

pub def lists() -> Result[Cell[list[i64]], AllocError]:
    Ok(Cell([1]?))
",
        )],
    );
    let main = "import cells

def main() -> Result[(), AllocError]:
    drop(cells.lists()?)
    Ok(())
";
    package(
        dir.path(),
        "app",
        "cells = { path = \"../cells\" }\n",
        &[("src/main.plenty", main)],
    );
    let app = dir.path().join("app");
    succeeds(&app, &["check", "--check-all-methods"]);
    write(
        &app.join("src/main.plenty"),
        &format!(
            "{main}
class Own[T]:
    value: T
    def get(self) -> T:
        self.value

def own() -> Result[Own[list[i64]], AllocError]:
    Ok(Own([1]?))
"
        ),
    );
    succeeds(&app, &["check"]);
    fails(
        &app,
        &["check", "--check-all-methods"],
        "main.plenty:12:21: `Own[list[i64]].get` copies `T`, but `list[i64]` cannot be copied",
    );
}

/// `plenty check` checks the files each target's import graph reaches, so it
/// names every other `.plenty` file below `src/` instead of implying that a
/// successful check covered the whole package (issue #805).
#[test]
fn check_reports_unimported_sources() {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("tools");
    write(
        &app.join("plenty.toml"),
        "manifest-version = 1\n[package]\nname = \"tools\"\nversion = \"1.0.0\"\n\n[lib]\n\n[[bin]]\nname = \"one\"\npath = \"src/one.plenty\"\n\n[[bin]]\nname = \"two\"\npath = \"src/two.plenty\"\n",
    );
    let main = "def main() -> ():\n    pass\n";
    write(
        &app.join("src/lib.plenty"),
        "pub def size() -> i64:\n    1\n",
    );
    write(&app.join("src/one.plenty"), main);
    // Reached only from the second executable, through a subdirectory.
    write(
        &app.join("src/two.plenty"),
        &format!("from util.helper import twice\n{main}"),
    );
    write(
        &app.join("src/util/helper.plenty"),
        "pub def twice(n: i64) -> i64:\n    n\n",
    );
    // Neither an orphan nor a target: fixtures outside src/, tests/ (which
    // `plenty test` checks), and non-source files below src/.
    let broken = "def broken( -> i64:\n";
    write(&app.join("tests/fixtures/bad.plenty"), broken);
    write(&app.join("fixtures/bad.plenty"), broken);
    write(&app.join("src/notes.txt"), broken);
    let lines = status_lines(&app, &["check"]);
    assert!(!lines.contains("warning"), "{lines}");
    assert!(
        lines.contains("Checked tools v1.0.0: library and 2 executables; 4 of 4 files in src/"),
        "{lines}"
    );

    write(&app.join("src/util/orphan.plenty"), broken);
    let lines = status_lines(&app, &["check"]);
    assert!(
        lines.contains(
            "warning: src/util/orphan.plenty is not imported by any target of `tools`; it was not checked"
        ),
        "{lines}"
    );
    assert_eq!(lines.matches("warning").count(), 1, "{lines}");
    assert!(
        lines.contains("Checked tools v1.0.0: library and 2 executables; 4 of 5 files in src/"),
        "{lines}"
    );

    // An excluded file lives outside src/; moving it there ends the warning.
    std::fs::rename(
        app.join("src/util/orphan.plenty"),
        app.join("fixtures/orphan.plenty"),
    )
    .unwrap();
    let lines = status_lines(&app, &["check"]);
    assert!(!lines.contains("warning"), "{lines}");
}

/// Issue #788: a package's `include_str` and `include_bytes` read below its
/// `src/` or `assets/`, and nowhere else. Asset files are build inputs.
#[test]
fn packages_include_files_from_their_directory() {
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "util",
        "",
        &[
            (
                "src/lib.plenty",
                "pub const BANNER: str = include_str(\"../data/banner.txt\")\n",
            ),
            ("data/banner.txt", "util banner"),
        ],
    );
    package(
        dir.path(),
        "app",
        "util = { path = \"../util\" }\n",
        &[
            (
                "src/main.plenty",
                "import util\nfrom ui.view import ICON\nconst GREETING: str = include_str(\"../data/greeting.txt\")\n\ndef main() -> Result[(), IoError]:\n    print(GREETING)?\n    print(ICON)?\n    print(util.BANNER)?\n    Ok(())\n",
            ),
            (
                "src/ui/view.plenty",
                "pub const ICON: bytes = include_bytes(\"../../data/icons/icon.bin\")\n",
            ),
            ("data/greeting.txt", "hello"),
            ("data/icons/icon.bin", "\u{1}PNG"),
            (
                "tests/greeting.plenty",
                "const GREETING: str = include_str(\"../data/greeting.txt\")\n\ndef main() -> i32:\n    0 if GREETING == \"hello\" else 1\n",
            ),
        ],
    );
    let app = dir.path().join("app");
    assert_eq!(
        succeeds(&app, &["run"]),
        "hello\nb\"\\x01PNG\"\nutil banner\n"
    );
    assert!(succeeds(&app, &["test"]).contains("test greeting ... ok"));

    // Build reuse rehashes the files the last build read, so an edit to an
    // included file rebuilds and an unread file does not.
    assert!(status_lines(&app, &["build"]).contains("Fresh"));
    write(&dir.path().join("util/data/banner.txt"), "new banner");
    assert!(status_lines(&app, &["build"]).contains("Compiling"));
    let expected = "hello\nb\"\\x01PNG\"\nnew banner\n";
    assert_eq!(succeeds(&app, &["run"]), expected);
    write(&app.join("data/unused.txt"), "");
    assert!(status_lines(&app, &["build"]).contains("Fresh"));
    // A test target rereads only its own inputs; the greeting test reads no
    // icon.
    write(&app.join("data/icons/icon.bin"), "\u{2}PNG");
    assert!(status_lines(&app, &["test"]).contains("Fresh"));
    assert!(status_lines(&app, &["build"]).contains("Compiling"));
    write(&app.join("data/icons/icon.bin"), "\u{1}PNG");
    assert!(status_lines(&app, &["build"]).contains("Compiling"));
    assert!(status_lines(&app, &["build"]).contains("Fresh"));

    // `plentyc` derives each package's directory from its root, also when
    // a bare relative path names a graph with relative roots.
    write(&app.join("graph.toml"), &succeeds(&app, &["graph"]));
    let base = dir.path().canonicalize().unwrap();
    let relative = std::fs::read_to_string(app.join("graph.toml"))
        .unwrap()
        .replace(&format!("{}/", base.join("app").display()), "")
        .replace(&format!("{}/", base.display()), "../");
    assert!(relative.contains("root = \"src\""), "{relative}");
    write(&app.join("relative.toml"), &relative);
    for graph in ["graph.toml", "relative.toml"] {
        let run = Command::new(env!("CARGO_BIN_EXE_plentyc"))
            .args(["--package-graph", graph, "src/main.plenty"])
            .current_dir(&app)
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            expected,
            "{graph}: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    // A relocated package resolves the same relative paths.
    let moved = tempfile::tempdir().unwrap();
    for name in ["util", "app"] {
        std::fs::rename(dir.path().join(name), moved.path().join(name)).unwrap();
    }
    let app = moved.path().join("app");
    assert_eq!(succeeds(&app, &["run"]), expected);

    // A file is outside when `..` or a symbolic link leaves the package
    // directory, even into another package.
    write(&moved.path().join("secret.txt"), "secret");
    std::os::unix::fs::symlink(moved.path().join("secret.txt"), app.join("data/link.txt")).unwrap();
    let boundary = app.canonicalize().unwrap();
    for path in [
        "../data/link.txt",
        "../../util/data/banner.txt",
        "../data/../../secret.txt",
    ] {
        write(
            &app.join("src/main.plenty"),
            &format!("const S: str = include_str({path:?})\n\ndef main() -> ():\n    pass\n"),
        );
        for command in ["check", "build"] {
            fails(
                &app,
                &[command],
                &format!(
                    "included path `{path}` is outside the package directory {}",
                    boundary.display()
                ),
            );
        }
    }
    // A dependency's directory linked to the application's reads outside
    // the dependency.
    write(
        &app.join("src/main.plenty"),
        "import util\n\ndef main() -> Result[(), IoError]:\n    print(util.BANNER)?\n    Ok(())\n",
    );
    let util = moved.path().join("util");
    std::fs::remove_dir_all(util.join("data")).unwrap();
    std::os::unix::fs::symlink("../app/data", util.join("data")).unwrap();
    write(
        &util.join("src/lib.plenty"),
        "pub const BANNER: str = include_str(\"../data/greeting.txt\")\n",
    );
    let message = format!(
        "included path `../data/greeting.txt` is outside the package directory {}",
        util.canonicalize().unwrap().display()
    );
    for command in ["check", "build"] {
        fails(&app, &[command], &message);
    }
}

/// Runs `plentyc --package-graph graph.toml src/main.plenty` in `app`.
fn plentyc_graph(app: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plentyc"))
        .args(["--package-graph", "graph.toml", "src/main.plenty"])
        .current_dir(app)
        .output()
        .unwrap()
}

#[test]
fn a_symbolic_source_root_includes_its_own_files_only() {
    // `util/src` links to `app/vend`. Its own files are those below `util/`
    // and below `app/vend`; `app/data` stays the application's, in manifest
    // and graph mode alike. Whether such a link is allowed at all is issue
    // #840.
    let dir = tempfile::tempdir().unwrap();
    package(dir.path(), "util", "", &[("data/u.txt", "util data")]);
    let util = dir.path().join("util");
    package(
        dir.path(),
        "app",
        "util = { path = \"../util\" }\n",
        &[
            (
                "src/main.plenty",
                "import util\n\ndef main() -> Result[(), IoError]:\n    print(util.OWN)?\n    print(util.DATA)?\n    Ok(())\n",
            ),
            ("vend/own.txt", "own"),
            ("data/secret.txt", "secret"),
        ],
    );
    let app = dir.path().join("app");
    let lib = |include: &str| {
        format!(
            "pub const OWN: str = include_str(\"own.txt\")\npub const DATA: str = include_str(\"../../util/data/u.txt\")\n{include}"
        )
    };
    write(&app.join("vend/lib.plenty"), &lib(""));
    std::os::unix::fs::symlink("../app/vend", util.join("src")).unwrap();
    assert_eq!(succeeds(&app, &["run"]), "own\nutil data\n");
    write(&app.join("graph.toml"), &succeeds(&app, &["graph"]));
    let run = plentyc_graph(&app);
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "own\nutil data\n",
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    write(
        &app.join("vend/lib.plenty"),
        &lib("const SECRET: str = include_str(\"../data/secret.txt\")\n"),
    );
    let message = format!(
        "included path `../data/secret.txt` is outside the package directory {}",
        util.canonicalize().unwrap().display()
    );
    for command in ["check", "build"] {
        fails(&app, &[command], &message);
    }
    let run = plentyc_graph(&app);
    assert!(!run.status.success());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains(&message), "{stderr}");

    // The application cannot include the dependency's source root.
    write(&app.join("vend/lib.plenty"), &lib(""));
    write(
        &app.join("src/main.plenty"),
        "import util\nconst S: str = include_str(\"../vend/own.txt\")\n\ndef main() -> ():\n    pass\n",
    );
    fails(
        &app,
        &["check"],
        "included path `../vend/own.txt` belongs to package `util`; package `app` may include only its own files",
    );
}

#[test]
fn graph_packages_with_one_parent_include_only_their_own_sources() {
    // Both roots have the parent `work/`, which is each package's directory.
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");
    write(
        &work.join("a_src/main.plenty"),
        "import b\nconst S: str = include_str(\"../b_src/private.txt\")\n\ndef main() -> ():\n    pass\n",
    );
    write(&work.join("b_src/lib.plenty"), "pub const N: i64 = 1\n");
    write(&work.join("b_src/private.txt"), "private");
    write(&work.join("shared.txt"), "shared");
    write(
        &work.join("graph.toml"),
        "graph-version = 1\n\n[[package]]\nidentity = \"a\"\nroot = \"a_src\"\ndependencies = { b = \"b\" }\n\n[[package]]\nidentity = \"b\"\nroot = \"b_src\"\nlibrary = \"b_src/lib.plenty\"\ndependencies = {}\n",
    );
    let check = || {
        Command::new(env!("CARGO_BIN_EXE_plentyc"))
            .args([
                "--package-graph",
                "graph.toml",
                "--check",
                "a_src/main.plenty",
            ])
            .current_dir(&work)
            .output()
            .unwrap()
    };
    let output = check();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "included path `../b_src/private.txt` belongs to package `b`; package `a` may include only its own files"
        ),
        "{stderr}"
    );
    // A file in the shared directory is each package's own.
    write(
        &work.join("a_src/main.plenty"),
        "import b\nconst S: str = include_str(\"../shared.txt\")\n\ndef main() -> ():\n    pass\n",
    );
    let output = check();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn reuse_rechecks_where_included_files_may_come_from() {
    // Neither build reuse nor the program cache may keep an include that a
    // build now would refuse.
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "app",
        "",
        &[
            (
                "src/main.plenty",
                "const SUB: str = include_str(\"../data/sub/x.txt\")\nconst LINK: str = include_str(\"../data/link.txt\")\n\ndef main() -> Result[(), IoError]:\n    print(SUB)?\n    print(LINK)?\n    Ok(())\n",
            ),
            ("data/sub/x.txt", "x"),
            ("data/y.txt", "y"),
        ],
    );
    let app = dir.path().join("app");
    std::os::unix::fs::symlink("y.txt", app.join("data/link.txt")).unwrap();
    let fingerprints = app.join("target/optimized/.fingerprint");
    succeeds(&app, &["build"]);
    assert!(status_lines(&app, &["build"]).contains("Fresh"));

    // A new nested package takes `data/sub/` over.
    write(&app.join("data/sub/plenty.toml"), "");
    let owned = format!(
        "included path `../data/sub/x.txt` belongs to the package in {}",
        app.join("data/sub").canonicalize().unwrap().display()
    );
    fails(&app, &["build"], &owned);
    std::fs::remove_file(app.join("data/sub/plenty.toml")).unwrap();
    succeeds(&app, &["build"]);
    assert!(fingerprints.is_dir());
    // Without a fingerprint the program cache would serve the object.
    write(&app.join("data/sub/plenty.toml"), "");
    std::fs::remove_dir_all(&fingerprints).unwrap();
    fails(&app, &["build"], &owned);
    std::fs::remove_file(app.join("data/sub/plenty.toml")).unwrap();
    succeeds(&app, &["build"]);

    // A link retargeted outside the package, to a file with the same bytes.
    write(&dir.path().join("y.txt"), "y");
    std::fs::remove_file(app.join("data/link.txt")).unwrap();
    std::os::unix::fs::symlink("../../y.txt", app.join("data/link.txt")).unwrap();
    fails(
        &app,
        &["build"],
        &format!(
            "included path `../data/link.txt` is outside the package directory {}",
            app.canonicalize().unwrap().display()
        ),
    );
}

#[test]
fn a_package_includes_only_its_own_files() {
    // `ws/` holds `game`, whose member `levels` is at `ws/levels/`, whose
    // dependency `deep` is at `ws/levels/deep/`. Each includes its own files
    // anywhere in its directory; none includes a file inside a nested
    // package's directory.
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    let levels = ws.join("levels");
    let deep = levels.join("deep");
    write(
        &ws.join("plenty.toml"),
        "manifest-version = 1\n[package]\nname = \"game\"\nversion = \"0.1.0\"\n\n[dependencies]\nlevels = { path = \"levels\" }\n\n[workspace]\nmembers = [\"levels\"]\n",
    );
    write(
        &levels.join("plenty.toml"),
        "manifest-version = 1\n[package]\nname = \"levels\"\nversion = \"0.1.0\"\n\n[dependencies]\ndeep = { path = \"deep\" }\n",
    );
    let lib = |include: &str| {
        format!(
            "import deep\nconst DATA: str = include_str(\"data.txt\")\nconst LEVEL: str = include_str(\"../data/lvl.txt\")\nconst MANIFEST: str = include_str(\"../plenty.toml\")\n{include}pub const N: i64 = deep.N if DATA == \"data\" and LEVEL == \"level\" and MANIFEST != \"\" else 0\n"
        )
    };
    write(&levels.join("src/lib.plenty"), &lib(""));
    write(&levels.join("src/data.txt"), "data");
    write(&levels.join("data/lvl.txt"), "level");
    write(&levels.join("tests/f.txt"), "fixture");
    write(
        &levels.join("tests/fixture.plenty"),
        "const F: str = include_str(\"f.txt\")\nconst LEVEL: str = include_str(\"../data/lvl.txt\")\n\ndef main() -> i32:\n    0 if F == \"fixture\" and LEVEL == \"level\" else 1\n",
    );
    write(
        &deep.join("plenty.toml"),
        "manifest-version = 1\n[package]\nname = \"deep\"\nversion = \"0.1.0\"\n",
    );
    write(
        &deep.join("src/lib.plenty"),
        "const ONE: str = include_str(\"../data/one.txt\")\npub const N: i64 = 1 if ONE == \"one\" else 0\n",
    );
    write(&deep.join("data/one.txt"), "one");
    // Not in the graph, but its manifest makes it a package.
    write(
        &ws.join("vendor/other/plenty.toml"),
        "manifest-version = 1\n[package]\nname = \"other\"\nversion = \"0.1.0\"\n",
    );
    write(&ws.join("vendor/other/x.txt"), "x");
    std::fs::create_dir(ws.join("data")).unwrap();
    std::os::unix::fs::symlink("../levels/src/data.txt", ws.join("data/l.txt")).unwrap();
    let main = |include: &str| {
        format!(
            "import levels\n{include}\ndef main() -> Result[(), IoError]:\n    print(levels.N)?\n    Ok(())\n"
        )
    };
    write(&ws.join("src/main.plenty"), &main(""));
    assert_eq!(succeeds(&ws, &["run"]), "1\n");
    for command in ["check", "build"] {
        succeeds(&levels, &[command]);
    }
    assert!(succeeds(&levels, &["test"]).ends_with("test result: ok. 1 passed; 0 failed\n"));

    let other = ws.join("vendor/other").canonicalize().unwrap();
    for (path, owner) in [
        ("../levels/src/lib.plenty", "package `levels`".to_string()),
        ("../levels/data/lvl.txt", "package `levels`".into()),
        ("../data/l.txt", "package `levels`".into()),
        ("../levels/tests/f.txt", "package `levels`".into()),
        ("../levels/plenty.toml", "package `levels`".into()),
        ("../levels/deep/data/one.txt", "package `deep`".into()),
        (
            "../vendor/other/x.txt",
            format!("the package in {}", other.display()),
        ),
    ] {
        write(
            &ws.join("src/main.plenty"),
            &main(&format!("const S: str = include_str({path:?})\n")),
        );
        for command in ["check", "build"] {
            fails(
                &ws,
                &[command],
                &format!(
                    "included path `{path}` belongs to {owner}; package `game` may include only its own files"
                ),
            );
        }
    }
    write(&ws.join("src/main.plenty"), &main(""));
    write(
        &levels.join("src/lib.plenty"),
        &lib("const S: str = include_str(\"../deep/plenty.toml\")\n"),
    );
    fails(
        &levels,
        &["check"],
        "included path `../deep/plenty.toml` belongs to package `deep`; package `levels` may include only its own files",
    );
}
