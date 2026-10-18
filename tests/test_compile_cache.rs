//! The compiled-function cache (issue #144), through the `plenty` binary:
//! objects built from it are identical to uncached ones, concurrent compilers
//! share a directory safely, damage degrades to compiling, and the controls
//! work. Every test uses a directory of its own: nothing here depends on the
//! suite's shared cache, and nothing here changes it.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The compiler, with the suite's own cache settings removed: each test sets
/// exactly the ones it is about.
fn plenty() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_plenty"));
    for variable in ["PLENTY_CACHE", "PLENTY_CACHE_DIR", "PLENTY_CACHE_LIMIT"] {
        command.env_remove(variable);
    }
    command
}

const PROGRAM: &str = "import std.json

def total(items: &list[i64]) -> i64:
    mut sum = 0
    for item in items:
        sum = sum + item
    sum

def main() -> Result[(), Failure]:
    items = [1, 2, 3]?
    print(total(&items))?
    table: dict[str, i64] = {}?
    print(table)?
    Ok(())
";

/// A compiler run: how the cache was configured, and what it produced.
struct Run {
    output: Output,
    object: Vec<u8>,
}

impl Run {
    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.output.stderr).into_owned()
    }

    /// `(hits, misses)` from `--cache-stats`. Functions of one program can
    /// share a stencil, so even a cold run has hits: one worker's entry
    /// serves another's identical function. The sum is the function count.
    fn counts(&self) -> (u64, u64) {
        let stderr = self.stderr();
        let line = stderr
            .lines()
            .find(|line| line.starts_with("compile cache: "))
            .unwrap_or_else(|| panic!("no cache stats in {stderr:?}"));
        let number = |suffix: &str| -> u64 {
            line.split(", ")
                .map(|part| part.trim_start_matches("compile cache: "))
                .find_map(|part| part.strip_suffix(suffix))
                .unwrap_or_else(|| panic!("no {suffix:?} in {line:?}"))
                .parse()
                .unwrap()
        };
        (number(" hits"), number(" misses"))
    }
}

/// Emit an object for `source` with the cache in `cache` (`None`: off).
fn emit(dir: &Path, source: &str, cache: Option<&Path>, args: &[&str]) -> Run {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    // The source file's name reaches the object, so every run uses the same
    // name in a directory of its own.
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let run = dir.join(format!("run{n}"));
    std::fs::create_dir(&run).unwrap();
    let program = run.join("program.plenty");
    let object = run.join("program.o");
    std::fs::write(&program, source).unwrap();
    let mut command = plenty();
    command.args(["--cache-stats", "--emit-object"]);
    command.arg(&program).arg("-o").arg(&object).args(args);
    match cache {
        Some(cache) => command.env("PLENTY_CACHE_DIR", cache),
        None => command.env("PLENTY_CACHE", "off"),
    };
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let object = std::fs::read(&object).unwrap();
    Run { output, object }
}

fn entries(cache: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for shard in std::fs::read_dir(cache.join("fn")).unwrap().flatten() {
        for file in std::fs::read_dir(shard.path()).unwrap().flatten() {
            found.push(file.path());
        }
    }
    found.sort();
    found
}

#[test]
fn warm_cache_reproduces_the_uncached_object_without_misses() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let off = emit(dir.path(), PROGRAM, None, &[]);
    assert!(off.stderr().contains("compile cache: off"));
    let cold = emit(dir.path(), PROGRAM, Some(&cache), &[]);
    let (hits, misses) = cold.counts();
    assert!(misses > hits, "{}", cold.stderr());
    assert!(entries(&cache).len() as u64 <= misses);
    let warm = emit(dir.path(), PROGRAM, Some(&cache), &[]);
    assert_eq!(warm.counts(), (hits + misses, 0), "{}", warm.stderr());
    assert!(off.object == cold.object, "a cold cache changed the object");
    assert!(off.object == warm.object, "a warm cache changed the object");

    // Entries are read-only data: the warm run wrote nothing.
    let stderr = warm.stderr();
    assert!(stderr.contains(" 0 B written"), "{stderr}");
}

/// An edit recompiles the edited function and reuses the rest, including
/// `std`, and the edited program's object is still the uncached one.
#[test]
fn an_edit_misses_only_what_changed() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let first = emit(dir.path(), PROGRAM, Some(&cache), &[]);
    let (first_hits, first_misses) = first.counts();
    let functions = first_hits + first_misses;
    let edited = PROGRAM.replace("sum = sum + item", "sum = sum + item * 2");
    let off = emit(dir.path(), &edited, None, &[]);
    let warm = emit(dir.path(), &edited, Some(&cache), &[]);
    let (hits, misses) = warm.counts();
    assert!(off.object == warm.object);
    assert!(misses >= 1 && misses < functions / 2, "{}", warm.stderr());
    assert_eq!(hits + misses, functions);
}

/// Optimized code is keyed apart from unoptimized code.
#[test]
fn optimization_level_is_part_of_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let debug = emit(dir.path(), PROGRAM, Some(&cache), &[]);
    let (debug_hits, debug_misses) = debug.counts();
    let before = entries(&cache).len() as u64;
    let source = dir.path().join("release.plenty");
    std::fs::write(&source, PROGRAM).unwrap();
    let object = dir.path().join("release.o");
    // `--release` needs `--compile`; the object is extracted by linking
    // nothing else, so compile to an executable instead.
    let output = plenty()
        .args(["--cache-stats", "--release", "--compile"])
        .arg(&source)
        .arg("-o")
        .arg(&object)
        .env("PLENTY_CACHE_DIR", &cache)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let release = Run {
        output,
        object: Vec::new(),
    };
    let (hits, misses) = release.counts();
    assert_eq!(hits + misses, debug_hits + debug_misses, "{stderr}");
    // Every optimized function missed, apart from the intra-program sharing
    // a cold run has: the unoptimized entries were never candidates, so the
    // optimized ones were all added. Workers missing the same new key at
    // once write one entry between them, so the counts vary with timing.
    assert!(misses > hits, "{stderr}");
    let added = entries(&cache).len() as u64 - before;
    assert!(
        added <= misses && added >= before / 2,
        "{added} added, {stderr}"
    );
}

#[test]
fn flags_and_environment_turn_the_cache_off() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let flag = emit(dir.path(), PROGRAM, Some(&cache), &["--no-cache"]);
    assert!(flag.stderr().contains("compile cache: off"));
    assert!(!cache.exists(), "--no-cache created the cache");
    let program = dir.path().join("env.plenty");
    std::fs::write(&program, PROGRAM).unwrap();
    let output = plenty()
        .args(["--cache-stats", "--emit-object"])
        .arg(&program)
        .arg("-o")
        .arg(dir.path().join("env.o"))
        .env("PLENTY_CACHE", "off")
        .env("PLENTY_CACHE_DIR", &cache)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("compile cache: off"));
    assert!(!cache.exists());
}

/// Library builds honor the cache flags too (issue #160).
#[test]
fn library_builds_honor_cache_flags() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let source = dir.path().join("lib.plenty");
    std::fs::write(
        &source,
        "export def value() -> i32 = \"calc_value\":\n    42\n",
    )
    .unwrap();
    let build = |args: &[&str]| {
        let output = plenty()
            .arg("--static-library")
            .arg(&source)
            .args(["--library-name", "calc", "-o"])
            .arg(dir.path().join("libcalc.a"))
            .args(args)
            .env("PLENTY_CACHE_DIR", &cache)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(output.status.success(), "{stderr}");
        stderr
    };
    let off = build(&["--no-cache", "--cache-stats"]);
    assert!(off.contains("compile cache: off"), "{off}");
    assert!(!cache.exists(), "--no-cache created the cache");
    let on = build(&["--cache-stats"]);
    assert!(on.contains(" misses, "), "{on}");
    assert!(cache.exists());
}

/// Many compilers writing the same entries at once, then reading them.
#[test]
fn concurrent_compilers_share_one_directory() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let reference = emit(dir.path(), PROGRAM, None, &[]);
    for round in 0..2 {
        let runs: Vec<Run> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| emit(dir.path(), PROGRAM, Some(&cache), &[])))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for run in &runs {
            assert!(run.object == reference.object, "round {round}");
            assert!(!run.stderr().contains("warning"), "{}", run.stderr());
        }
        if round == 1 {
            for run in &runs {
                assert_eq!(run.counts().1, 0, "{}", run.stderr());
            }
        }
    }
    assert!(entries(&cache)
        .iter()
        .all(|path| path.extension().is_none()));
}

#[test]
fn damaged_entries_are_replaced_and_the_object_is_unaffected() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let reference = emit(dir.path(), PROGRAM, None, &[]);
    let cold = emit(dir.path(), PROGRAM, Some(&cache), &[]);
    let entries = entries(&cache);
    for (i, path) in entries.iter().enumerate() {
        match i % 3 {
            0 => std::fs::write(path, b"garbage").unwrap(),
            1 => {
                let bytes = std::fs::read(path).unwrap();
                std::fs::write(path, &bytes[..bytes.len() / 2]).unwrap();
            }
            _ => {
                let mut bytes = std::fs::read(path).unwrap();
                let last = bytes.len() - 1;
                bytes[last] ^= 0xff;
                std::fs::write(path, &bytes).unwrap();
            }
        }
    }
    let repaired = emit(dir.path(), PROGRAM, Some(&cache), &[]);
    assert!(repaired.object == reference.object);
    let (hits, misses) = repaired.counts();
    assert_eq!(hits + misses, cold.counts().0 + cold.counts().1);
    assert!(misses > hits, "{}", repaired.stderr());
    assert!(!repaired.stderr().contains("warning"));
    for path in &entries {
        assert_ne!(std::fs::read(path).unwrap(), b"garbage");
    }
    let warm = emit(dir.path(), PROGRAM, Some(&cache), &[]);
    assert_eq!(warm.counts().1, 0);
}

/// An unusable directory warns once and compiles without the cache.
#[test]
fn unusable_directory_warns_once() {
    let dir = tempfile::tempdir().unwrap();
    let reference = emit(dir.path(), PROGRAM, None, &[]);
    let file = dir.path().join("not-a-directory");
    std::fs::write(&file, b"").unwrap();
    let run = emit(dir.path(), PROGRAM, Some(&file), &[]);
    assert!(run.object == reference.object);
    let stderr = run.stderr();
    assert_eq!(
        stderr.matches("warning: compile cache disabled").count(),
        1,
        "{stderr}"
    );
    assert!(stderr.contains("compile cache: off"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn read_only_directory_still_serves_hits() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let reference = emit(dir.path(), PROGRAM, None, &[]);
    let cold = emit(dir.path(), PROGRAM, Some(&cache), &[]);
    let read_only = std::fs::Permissions::from_mode(0o555);
    let mut locked = vec![cache.clone(), cache.join("fn")];
    locked.extend(
        entries(&cache)
            .iter()
            .map(|p| p.parent().unwrap().to_path_buf()),
    );
    locked.sort();
    locked.dedup();
    for path in &locked {
        std::fs::set_permissions(path, read_only.clone()).unwrap();
    }
    let edited = PROGRAM.replace("sum = sum + item", "sum = sum + item + 1");
    let warm = emit(dir.path(), PROGRAM, Some(&cache), &[]);
    let changed = emit(dir.path(), &edited, Some(&cache), &[]);
    let writable = std::fs::Permissions::from_mode(0o755);
    for path in &locked {
        std::fs::set_permissions(path, writable.clone()).unwrap();
    }
    assert!(warm.object == reference.object);
    let (cold_hits, cold_misses) = cold.counts();
    assert_eq!(
        warm.counts(),
        (cold_hits + cold_misses, 0),
        "{}",
        warm.stderr()
    );
    assert!(changed.object == emit(dir.path(), &edited, None, &[]).object);
    let stderr = changed.stderr();
    // Root may write anywhere; then nothing is disabled.
    if stderr.contains("warning") {
        assert_eq!(
            stderr.matches("warning: compile cache disabled").count(),
            1,
            "{stderr}"
        );
    }
}

/// Code generation never reaches an erroneous program, so the cache cannot
/// change diagnostics (issue #126); check it anyway, with and without entries.
#[test]
fn diagnostics_do_not_depend_on_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    emit(dir.path(), PROGRAM, Some(&cache), &[]);
    let broken = PROGRAM.replace("sum = sum + item", "sum = sum + \"item\"");
    let program = dir.path().join("broken.plenty");
    std::fs::write(&program, &broken).unwrap();
    let compile = |env: (&str, &Path)| {
        plenty()
            .arg("--emit-object")
            .arg(&program)
            .arg("-o")
            .arg(dir.path().join("broken.o"))
            .env(env.0, env.1)
            .output()
            .unwrap()
    };
    let off = compile(("PLENTY_CACHE", Path::new("off")));
    let on = compile(("PLENTY_CACHE_DIR", &cache));
    assert!(!off.status.success());
    assert_eq!(off.status.code(), on.status.code());
    assert_eq!(off.stderr, on.stderr);
    assert!(String::from_utf8_lossy(&off.stderr).contains("error"));
}

#[test]
fn cache_commands_report_and_empty_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    emit(dir.path(), PROGRAM, Some(&cache), &[]);
    let functions = entries(&cache).len();
    let stats = plenty()
        .args(["cache", "stats"])
        .env("PLENTY_CACHE_DIR", &cache)
        .output()
        .unwrap();
    assert!(stats.status.success());
    let text = String::from_utf8_lossy(&stats.stdout);
    assert!(
        text.contains(&format!("directory: {}", cache.display())),
        "{text}"
    );
    assert!(text.contains(&format!("entries:   {functions}")), "{text}");
    assert!(text.contains("limit:     1.00 GiB"), "{text}");

    let clean = plenty()
        .args(["cache", "clean"])
        .env("PLENTY_CACHE_DIR", &cache)
        .output()
        .unwrap();
    assert!(clean.status.success());
    assert!(
        String::from_utf8_lossy(&clean.stderr).contains(&format!("Removed {functions} entries"))
    );
    assert!(entries(&cache).is_empty());

    let off = plenty()
        .args(["cache", "stats"])
        .env("PLENTY_CACHE", "off")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&off.stdout), "compile cache: off\n");
    let usage = plenty().args(["cache", "prune"]).output().unwrap();
    assert!(!usage.status.success());
    assert!(String::from_utf8_lossy(&usage.stderr).contains("usage: plenty cache"));
}

/// A compile that wrote entries trims the directory to `PLENTY_CACHE_LIMIT`
/// when no sweep ran recently.
#[test]
fn the_size_limit_trims_old_entries() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    emit(dir.path(), PROGRAM, Some(&cache), &[]);
    let functions = entries(&cache).len();
    let total: u64 = entries(&cache)
        .iter()
        .map(|p| std::fs::metadata(p).unwrap().len())
        .sum();
    assert!(cache.join("sweep").exists(), "the first run sweeps");

    // A second program adds entries; with the limit at the first program's
    // size, the sweep (due again once the marker is old) removes the oldest.
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 60 * 60);
    std::fs::File::options()
        .write(true)
        .open(cache.join("sweep"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    let other = PROGRAM.replace("sum = sum + item", "sum = sum - item");
    let program = dir.path().join("other.plenty");
    std::fs::write(&program, &other).unwrap();
    let output = plenty()
        .args(["--cache-stats", "--emit-object"])
        .arg(&program)
        .arg("-o")
        .arg(dir.path().join("other.o"))
        .env("PLENTY_CACHE_DIR", &cache)
        .env("PLENTY_CACHE_LIMIT", total.to_string())
        .output()
        .unwrap();
    assert!(output.status.success());
    let remaining: u64 = entries(&cache)
        .iter()
        .map(|p| std::fs::metadata(p).unwrap().len())
        .sum();
    assert!(remaining <= total, "{remaining} > {total}");
    assert!(entries(&cache).len() <= functions);
    assert!(!entries(&cache).is_empty());
}
