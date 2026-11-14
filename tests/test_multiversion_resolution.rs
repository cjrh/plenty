//! Exact source identity, owner-edge pins, and lock migration with local fixtures.
mod support;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use support::write_file as write;

fn plenty(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plenty"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap()
}
fn ok(directory: &Path, args: &[&str]) -> String {
    let output = plenty(directory, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn fail(directory: &Path, args: &[&str], expected: &str) -> String {
    let output = plenty(directory, args);
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(!output.status.success(), "{args:?} succeeded");
    assert!(error.contains(expected), "wanted {expected:?}, got {error}");
    error
}
fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
        ])
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn manifest(name: &str, deps: &str) -> String {
    format!(
        "manifest-version = 1\n[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n[dependencies]\n{deps}"
    )
}
fn package(path: &Path, name: &str, deps: &str, body: &str) {
    write(&path.join("plenty.toml"), &manifest(name, deps));
    write(&path.join("src/lib.plenty"), body);
}
fn repo(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    git(path, &["init", "--quiet"]);
}
fn commit(path: &Path) -> String {
    git(path, &["add", "--all"]);
    git(path, &["commit", "--quiet", "-m", "fixture"]);
    git(path, &["rev-parse", "HEAD"])
}
fn edge(url: &Path, selector: &str) -> String {
    format!(
        "{{ git = \"file://{}\", tag = \"{selector}\" }}",
        url.display()
    )
}
fn lock(path: &Path) -> String {
    std::fs::read_to_string(path.join("plenty.lock")).unwrap()
}
const VALUE: &str = "pub def value() -> i64:\n    1\n";
fn chain_fixture(root: &Path, repo_path: &Path) -> PathBuf {
    package(
        &root.join("a"),
        "a",
        &format!("c = {}\n", edge(repo_path, "current")),
        "import c\npub def value() -> i64:\n    c.value()\n",
    );
    package(
        &root.join("b"),
        "b",
        &format!("c = {}\n", edge(repo_path, "current")),
        "import c\npub def value() -> i64:\n    c.value()\n",
    );
    let app = root.join("app");
    package(
        &app,
        "app",
        "a = { path = \"../a\" }\nb = { path = \"../b\" }\n",
        VALUE,
    );
    write(
        &app.join("src/main.plenty"),
        "import a\nimport b\ndef main() -> Result[(), IoError]:\n    print(a.value())?\n    print(b.value())?\n    Ok(())\n",
    );
    app
}
#[test]
fn same_tag_edges_split_on_targeted_update_and_preserve_the_other_pin() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let c = root.join("c-repo");
    repo(&c);
    package(&c, "c", "", VALUE);
    let first = commit(&c);
    git(&c, &["tag", "current"]);
    let app = chain_fixture(root, &c);
    assert_eq!(ok(&app, &["run"]), "1\n1\n");
    write(
        &c.join("src/lib.plenty"),
        "pub def value() -> i64:\n    2\n",
    );
    let second = commit(&c);
    git(&c, &["tag", "-f", "current"]);
    ok(&app, &["update", "a/c"]);
    let text = lock(&app);
    assert!(text.contains(&first) && text.contains(&second), "{text}");
    assert_eq!(ok(&app, &["run", "--locked"]), "2\n1\n");
    let before = lock(&app);
    fail(&app, &["update", "c"], "ambiguous package `c`");
    assert_eq!(before, lock(&app));
    let duplicates = ok(&app, &["graph", "--duplicates"]);
    assert!(
        duplicates.contains(&first) && duplicates.contains(&second),
        "{duplicates}"
    );
    // Unselected mutable selectors are never refreshed by an ordinary build.
    assert_eq!(ok(&app, &["run", "--offline", "--locked"]), "2\n1\n");
}
#[test]
fn replacement_git_parent_recomputes_snapshot_path_chain_but_retains_independent_pin() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let independent = root.join("independent");
    repo(&independent);
    package(&independent, "other", "", VALUE);
    let old_other = commit(&independent);
    git(&independent, &["tag", "current"]);
    let mono = root.join("mono");
    repo(&mono);
    let deps = format!(
        "c = {{ path = \"../c\" }}\nother = {}\n",
        edge(&independent, "current")
    );
    package(
        &mono.join("a"),
        "a",
        &deps,
        "import c\nimport other\npub def value() -> i64:\n    c.value()\npub def independent() -> i64:\n    other.value()\n",
    );
    package(
        &mono.join("c"),
        "c",
        "d = { path = \"../d\" }\n",
        "import d\npub def value() -> i64:\n    d.value()\n",
    );
    package(&mono.join("d"), "d", "", VALUE);
    let r1 = commit(&mono);
    git(&mono, &["tag", "current"]);
    package(
        &root.join("b"),
        "b",
        &format!(
            "c = {{ git = \"file://{}\", subdir = \"c\", rev = \"{r1}\" }}\n",
            mono.display()
        ),
        "import c\npub def value() -> i64:\n    c.value()\n",
    );
    let app = root.join("app");
    package(
        &app,
        "app",
        &format!(
            "a = {{ git = \"file://{}\", subdir = \"a\", tag = \"current\" }}\nb = {{ path = \"../b\" }}\n",
            mono.display()
        ),
        VALUE,
    );
    ok(&app, &["fetch"]);
    write(
        &mono.join("d/src/lib.plenty"),
        "pub def value() -> i64:\n    2\n",
    );
    let r2 = commit(&mono);
    git(&mono, &["tag", "-f", "current"]);
    write(
        &independent.join("src/lib.plenty"),
        "pub def value() -> i64:\n    9\n",
    );
    let new_other = commit(&independent);
    git(&independent, &["tag", "-f", "current"]);
    ok(&app, &["update", "a"]);
    let text = lock(&app);
    assert!(text.contains(&r2) && text.contains(&r1), "{text}");
    assert!(
        text.contains(&old_other) && !text.contains(&new_other),
        "{text}"
    );
    let error = fail(&app, &["update", "a/c/d"], "owning Git dependency");
    assert!(
        error.contains("::a") && error.contains("snapshot-relative"),
        "{error}"
    );
    assert_eq!(text, lock(&app));
    write(
        &app.join("src/main.plenty"),
        "import a\nimport b\ndef main() -> Result[(), IoError]:\n    print(a.value())?\n    print(a.independent())?\n    print(b.value())?\n    Ok(())\n",
    );
    assert_eq!(ok(&app, &["run", "--locked"]), "2\n1\n1\n");
}
#[test]
fn repeated_names_on_different_source_chain_are_valid_but_identity_cycles_fail() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    package(&root.join("inner"), "same", "", VALUE);
    package(
        &root.join("middle"),
        "middle",
        "inner = { path = \"../inner\" }\n",
        VALUE,
    );
    package(
        &root.join("outer"),
        "same",
        "middle = { path = \"../middle\" }\n",
        VALUE,
    );
    ok(&root.join("outer"), &["check"]);
    package(
        &root.join("inner"),
        "same",
        "outer = { path = \"../outer\" }\n",
        VALUE,
    );
    fail(&root.join("outer"), &["check"], "dependency cycle");
}
#[test]
fn v1_migration_preserves_full_commit_and_unknown_or_malformed_ids_fail_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let c = root.join("c-repo");
    repo(&c);
    package(&c, "c", "", VALUE);
    let first = commit(&c);
    git(&c, &["tag", "current"]);
    let app = root.join("app");
    package(
        &app,
        "app",
        &format!("c = {}\n", edge(&c, "current")),
        VALUE,
    );
    ok(&app, &["fetch"]);
    let old = format!(
        "lock-version = 1\n[[package]]\nname = \"app\"\nversion = \"1.0.0\"\nsource = \"root\"\ndependencies = [{{ alias = \"c\", package = \"c\" }}]\n[[package]]\nname = \"c\"\nversion = \"1.0.0\"\nsource = \"git+file://{}\"\ncommit = \"{first}\"\nselectors = [\"tag=current\"]\ndependencies = []\n",
        c.display()
    );
    write(&app.join("plenty.lock"), &old);
    write(
        &c.join("src/lib.plenty"),
        "pub def value() -> i64:\n    2\n",
    );
    let second = commit(&c);
    git(&c, &["tag", "-f", "current"]);
    fail(&app, &["fetch", "--locked"], "migrate its existing commits");
    assert_eq!(old, lock(&app));
    ok(&app, &["fetch", "--offline"]);
    let migrated = lock(&app);
    assert!(
        migrated.contains("lock-version = 2")
            && migrated.contains(&first)
            && !migrated.contains(&second)
    );
    ok(&app, &["fetch", "--locked"]);
    let invalid = migrated.replace("lock-version = 2", "lock-version = 99");
    write(&app.join("plenty.lock"), &invalid);
    fail(&app, &["update"], "unsupported lock-version");
    assert_eq!(invalid, lock(&app));
    let invalid = migrated.replace("identity = \"path:.\"", "identity = \"path:forged\"");
    write(&app.join("plenty.lock"), &invalid);
    fail(&app, &["fetch"], "malformed package identity");
    assert_eq!(invalid, lock(&app));
}
#[test]
fn path_lock_relocation_and_live_changes_keep_stable_identity() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().join("original");
    package(&base.join("dep"), "dep", "", VALUE);
    package(
        &base.join("app"),
        "app",
        "dep = { path = \"../dep\" }\n",
        VALUE,
    );
    ok(&base.join("app"), &["fetch"]);
    let before = lock(&base.join("app"));
    let moved = temp.path().join("moved");
    std::fs::rename(&base, &moved).unwrap();
    ok(&moved.join("app"), &["fetch", "--locked"]);
    assert_eq!(before, lock(&moved.join("app")));
    write(
        &moved.join("dep/src/lib.plenty"),
        "pub def value() -> i64:\n    2\n",
    );
    ok(&moved.join("app"), &["check", "--locked"]);
}
#[test]
fn each_owner_rejects_distinct_direct_names_even_when_not_the_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    package(&root.join("c1"), "c", "", VALUE);
    package(&root.join("c2"), "c", "", VALUE);
    package(
        &root.join("a"),
        "a",
        "old = { path = \"../c1\" }\nnew = { path = \"../c2\" }\n",
        VALUE,
    );
    package(&root.join("app"), "app", "a = { path = \"../a\" }\n", VALUE);
    let error = fail(
        &root.join("app"),
        &["fetch"],
        "package `a`: two different packages are named `c`",
    );
    assert!(error.contains("`old`") && error.contains("`new`"));
    assert!(!root.join("app/plenty.lock").exists());
}
#[test]
fn script_root_has_separate_identity_and_lock_reuses_pins() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let c = root.join("c-repo");
    repo(&c);
    package(&c, "c", "", VALUE);
    let first = commit(&c);
    git(&c, &["tag", "current"]);
    package(
        root,
        "library",
        &format!("c = {}\n", edge(&c, "current")),
        "import c\npub def value() -> i64:\n    c.value()\n",
    );
    write(
        &root.join("scripts/app.plenty"),
        "---plenty\n[dependencies]\nlib = { path = \"..\" }\n---\nimport lib\ndef main() -> Result[(), IoError]:\n    print(lib.value())?\n    Ok(())\n",
    );
    assert_eq!(
        ok(
            root,
            &["script", "--lockfile", "script.lock", "scripts/app.plenty"]
        ),
        "1\n"
    );
    let before = std::fs::read_to_string(root.join("script.lock")).unwrap();
    assert!(
        before.contains("script:app.plenty") && before.contains("path:.."),
        "{before}"
    );
    write(
        &c.join("src/lib.plenty"),
        "pub def value() -> i64:\n    2\n",
    );
    commit(&c);
    git(&c, &["tag", "-f", "current"]);
    assert_eq!(
        ok(
            root,
            &[
                "script",
                "--lockfile",
                "script.lock",
                "--offline",
                "--locked",
                "scripts/app.plenty"
            ]
        ),
        "1\n"
    );
    assert!(before.contains(&first));
    assert_eq!(
        before,
        std::fs::read_to_string(root.join("script.lock")).unwrap()
    );
}
#[test]
fn parent_source_requirement_change_retains_matching_independent_child_pin() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let c = root.join("c-repo");
    repo(&c);
    package(&c, "c", "", VALUE);
    let first = commit(&c);
    git(&c, &["tag", "current"]);
    let a = root.join("a-repo");
    repo(&a);
    package(&a, "a", &format!("c = {}\n", edge(&c, "current")), VALUE);
    commit(&a);
    git(&a, &["tag", "one"]);
    let app = root.join("app");
    package(&app, "app", &format!("a = {}\n", edge(&a, "one")), VALUE);
    ok(&app, &["fetch"]);
    write(
        &c.join("src/lib.plenty"),
        "pub def value() -> i64:\n    2\n",
    );
    let second = commit(&c);
    git(&c, &["tag", "-f", "current"]);
    write(
        &a.join("src/lib.plenty"),
        "pub def value() -> i64:\n    9\n",
    );
    commit(&a);
    git(&a, &["tag", "two"]);
    write(
        &app.join("plenty.toml"),
        &manifest("app", &format!("a = {}\n", edge(&a, "two"))),
    );
    ok(&app, &["fetch"]);
    let text = lock(&app);
    assert!(text.contains(&first) && !text.contains(&second), "{text}");
}
#[test]
fn updating_shared_outgoing_mapping_reports_and_updates_all_consumers() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let d = root.join("d-repo");
    repo(&d);
    package(&d, "d", "", VALUE);
    commit(&d);
    git(&d, &["tag", "current"]);
    package(
        &root.join("c"),
        "c",
        &format!("d = {}\n", edge(&d, "current")),
        "import d\npub def value() -> i64:\n    d.value()\n",
    );
    for name in ["a", "b"] {
        package(
            &root.join(name),
            name,
            "c = { path = \"../c\" }\n",
            "import c\npub def value() -> i64:\n    c.value()\n",
        );
    }
    let app = root.join("app");
    package(
        &app,
        "app",
        "a = { path = \"../a\" }\nb = { path = \"../b\" }\n",
        VALUE,
    );
    write(
        &app.join("src/main.plenty"),
        "import a\nimport b\ndef main() -> Result[(), IoError]:\n    print(a.value(), b.value())?\n    Ok(())\n",
    );
    assert_eq!(ok(&app, &["run"]), "1 1\n");
    write(
        &d.join("src/lib.plenty"),
        "pub def value() -> i64:\n    2\n",
    );
    commit(&d);
    git(&d, &["tag", "-f", "current"]);
    let output = plenty(&app, &["update", "a/c/d"]);
    assert!(output.status.success());
    let report = String::from_utf8(output.stderr).unwrap();
    for expected in ["affects all consumers", "path:../a::c", "path:../b::c"] {
        assert!(report.contains(expected), "{report}");
    }
    assert_eq!(ok(&app, &["run", "--locked"]), "2 2\n");
}
#[test]
fn failed_selected_update_keeps_old_lock_and_identical_commit_tags_share() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let c = root.join("c-repo");
    repo(&c);
    package(&c, "c", "", VALUE);
    let first = commit(&c);
    git(&c, &["tag", "one"]);
    git(&c, &["tag", "two"]);
    let app = root.join("app");
    package(
        &app,
        "app",
        &format!("one = {}\ntwo = {}\n", edge(&c, "one"), edge(&c, "two")),
        VALUE,
    );
    ok(&app, &["fetch"]);
    assert_eq!(lock(&app).matches("[[package]]").count(), 2);
    // Version requirements are validated after source resolution, without a solver.
    package(
        &app,
        "app",
        &format!(
            "c = {{ git = \"file://{}\", tag = \"one\", version = \"=1.0.0\" }}\n",
            c.display()
        ),
        VALUE,
    );
    ok(&app, &["fetch"]);
    let before = lock(&app);
    write(
        &c.join("plenty.toml"),
        &manifest("c", "").replace("1.0.0", "2.0.0"),
    );
    commit(&c);
    git(&c, &["tag", "-f", "one"]);
    fail(&app, &["update", "c"], "requires `c` =1.0.0");
    assert_eq!(before, lock(&app));
    assert!(before.contains(&first));
    ok(&app, &["fetch", "--offline", "--locked"]);
}
#[test]
#[cfg(unix)]
fn native_canonical_local_and_git_directories_keep_distinct_lossless_identities() {
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let first = PathBuf::from(std::ffi::OsString::from_vec(b"c-\xff".to_vec()));
    let second = PathBuf::from(std::ffi::OsString::from_vec(b"c-\xfe".to_vec()));
    for (directory, alias) in [(&first, "first"), (&second, "second")] {
        package(&root.join(directory), "c", "", VALUE);
        symlink(directory, root.join(alias)).unwrap();
    }
    for (name, alias) in [("a", "first"), ("b", "second")] {
        package(
            &root.join(name),
            name,
            &format!("c = {{ path = \"../{alias}\" }}\n"),
            VALUE,
        );
    }
    let app = root.join("app");
    package(
        &app,
        "app",
        "a = { path = \"../a\" }\nb = { path = \"../b\" }\n",
        VALUE,
    );
    ok(&app, &["fetch"]);
    let text = lock(&app);
    assert_eq!(
        text.matches("identity = \"path-bytes:").count(),
        2,
        "{text}"
    );
    ok(&app, &["check", "--locked"]);
    assert_eq!(text, lock(&app));
    // Git's tree paths retain the same native bytes; UTF-8 aliases locate them.
    let mono = root.join("mono");
    repo(&mono);
    for (directory, alias) in [(&first, "first"), (&second, "second")] {
        package(&mono.join(directory), "c", "", VALUE);
        symlink(directory, mono.join(alias)).unwrap();
    }
    package(&mono.join("a"), "a", "c = { path = \"../first\" }\n", VALUE);
    package(
        &mono.join("b"),
        "b",
        "c = { path = \"../second\" }\n",
        VALUE,
    );
    commit(&mono);
    git(&mono, &["tag", "current"]);
    package(
        &app,
        "app",
        &format!(
            "a = {{ git = \"file://{}\", subdir = \"a\", tag = \"current\" }}\nb = {{ git = \"file://{}\", subdir = \"b\", tag = \"current\" }}\n",
            mono.display(),
            mono.display()
        ),
        VALUE,
    );
    ok(&app, &["fetch"]);
    let text = lock(&app);
    assert_eq!(text.matches("identity = \"git-bytes:").count(), 2, "{text}");
    assert_eq!(text.matches("subdir-bytes =").count(), 2, "{text}");
    ok(&app, &["check", "--locked"]);
    assert_eq!(text, lock(&app));
}
#[test]
#[cfg(unix)]
fn git_subdirectory_symlink_and_changed_version_guard_preserve_existing_pin() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mono = root.join("mono");
    repo(&mono);
    package(&mono.join("real"), "c", "", VALUE);
    symlink("real", mono.join("alias")).unwrap();
    let first = commit(&mono);
    git(&mono, &["tag", "current"]);
    let app = root.join("app");
    let dependency = format!(
        "c = {{ git = \"file://{}\", subdir = \"alias\", tag = \"current\" }}\n",
        mono.display()
    );
    package(&app, "app", &dependency, VALUE);
    ok(&app, &["fetch"]);
    ok(&app, &["check", "--locked"]);
    write(
        &mono.join("real/src/lib.plenty"),
        "pub def value() -> i64:\n    2\n",
    );
    let second = commit(&mono);
    git(&mono, &["tag", "-f", "current"]);
    package(
        &app,
        "app",
        &dependency.replace(
            "tag = \"current\"",
            "tag = \"current\", version = \">=1.0.0\"",
        ),
        VALUE,
    );
    ok(&app, &["fetch"]);
    let text = lock(&app);
    assert!(text.contains(&first) && !text.contains(&second), "{text}");
    ok(&app, &["check", "--locked", "--offline"]);
}
