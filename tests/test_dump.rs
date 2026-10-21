//! `plenty --dump` (issue #360): per-function text for each compiler stage,
//! an index, readable symbols, scope and filter control, and output that does
//! not depend on the cache.
use std::path::Path;
use std::process::Command;

const PROGRAM: &str = "def total(n: i64) -> i64:
    mut sum = 0
    for i in range(n):
        sum += i
    sum

def twice(v: i64) -> i64:
    v * 2

def main() -> Result[(), AllocError | IoError]:
    print(total(10))?
    print(twice(4))?
    Ok(())
";

/// Run `plenty` with `args` in `dir`, with the compile cache in `cache`
/// (`None`: off).
fn plenty(dir: &Path, cache: Option<&Path>, args: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_plenty"));
    command.current_dir(dir).env_remove("PLENTY_CACHE_LIMIT");
    match cache {
        Some(cache) => command
            .env_remove("PLENTY_CACHE")
            .env("PLENTY_CACHE_DIR", cache),
        None => command
            .env_remove("PLENTY_CACHE_DIR")
            .env("PLENTY_CACHE", "off"),
    };
    let output = command.args(args).output().unwrap();
    assert!(
        output.status.success(),
        "plenty {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn setup() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("demo.plenty"), PROGRAM).unwrap();
    dir
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap()
}

#[test]
fn dump_writes_an_index_and_a_file_per_stage() {
    let dir = setup();
    plenty(
        dir.path(),
        None,
        &["--dump", "all", "demo.plenty", "-o", "out"],
    );
    let out = dir.path().join("out");
    let index = read(&out, "index.txt");
    assert!(index.contains("# stages ops,clif,clif-opt,asm"), "{index}");
    assert!(index.contains("# scope user\n"), "{index}");
    let row = index
        .lines()
        .find(|line| line.ends_with(" total"))
        .unwrap_or_else(|| panic!("no row for total in {index}"));
    assert!(row.contains("demo.plenty:1 "), "{row}");
    assert!(row.contains("__plenty_fn_total"), "{row}");
    assert!(
        !index.contains("std/"),
        "std functions in the user scope: {index}"
    );

    for stage in ["ops", "clif", "clif-opt", "asm"] {
        let text = read(&out, &format!("{stage}.txt"));
        assert!(
            text.contains(
                "==== total  symbol=__plenty_fn_total  kind=function  source=demo.plenty:1"
            ),
            "{stage}: {text}"
        );
        assert!(text.contains("==== main  "), "{stage}");
    }
    let ops = read(&out, "ops.txt");
    assert!(
        ops.contains("source=demo.plenty:1\nlocal 0: i64\n"),
        "{ops}"
    );
    let clif = read(&out, "clif.txt");
    assert!(clif.contains("    ; line 3\n"), "{clif}");
    assert!(clif.contains("; plenty_trap_overflow"), "{clif}");
    let asm = read(&out, "asm.txt");
    assert!(asm.contains("# line 3\n"), "{asm}");
    // Calls name their targets rather than relocation indices.
    assert!(asm.contains("# __plenty_fn_total"), "{asm}");
    assert!(asm.contains("# plenty_trap_overflow"), "{asm}");
}

#[test]
fn dump_selects_stages_scope_and_functions() {
    let dir = setup();
    let output = plenty(
        dir.path(),
        None,
        &[
            "--dump",
            "asm",
            "--dump-filter",
            "twice",
            "demo.plenty",
            "-o",
            "-",
        ],
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("#### index.txt\n"), "{text}");
    assert!(text.contains("#### asm.txt\n"), "{text}");
    assert!(!text.contains("#### clif.txt"), "{text}");
    assert!(text.contains("# filter \"twice\""), "{text}");
    assert!(text.contains("# functions 1\n"), "{text}");
    assert!(text.contains("==== twice  "), "{text}");
    assert!(!text.contains("==== total  "), "{text}");

    std::fs::write(
        dir.path().join("format.plenty"),
        "from std.fmt import FormatError

def main() -> Result[(), AllocError | IoError | FormatError]:
    print(f\"{3:>5}\"?)?
    Ok(())
",
    )
    .unwrap();
    plenty(
        dir.path(),
        None,
        &["--dump", "ops", "format.plenty", "-o", "out"],
    );
    let index = read(&dir.path().join("out"), "index.txt");
    assert!(!index.contains(" std  std/"), "{index}");
    plenty(
        dir.path(),
        None,
        &["--dump", "ops", "--dump-std", "format.plenty", "-o", "out"],
    );
    let index = read(&dir.path().join("out"), "index.txt");
    assert!(index.contains("# scope user+std"), "{index}");
    assert!(index.contains(" std  std/"), "{index}");
}

#[test]
fn dump_is_the_same_for_every_cache_state() {
    let dir = setup();
    let args = |out| ["--dump", "all", "--release", "demo.plenty", "-o", out];
    plenty(dir.path(), None, &args("off"));
    let cache = dir.path().join("cache");
    let cache = Some(cache.as_path());
    // The first run fills the cache through an ordinary build; the dump after
    // it must still compile every function.
    plenty(
        dir.path(),
        cache,
        &["--compile", "demo.plenty", "-o", "demo"],
    );
    plenty(dir.path(), cache, &args("warm"));
    plenty(dir.path(), None, &args("again"));
    for name in [
        "index.txt",
        "ops.txt",
        "clif.txt",
        "clif-opt.txt",
        "asm.txt",
    ] {
        let expected = read(&dir.path().join("off"), name);
        assert_eq!(expected, read(&dir.path().join("warm"), name), "{name}");
        assert_eq!(expected, read(&dir.path().join("again"), name), "{name}");
    }
}

#[test]
fn dump_rejects_unknown_stages_and_misplaced_options() {
    let dir = setup();
    for args in [
        &["--dump", "bogus", "demo.plenty", "-o", "out"][..],
        &["--dump-std", "demo.plenty"][..],
        &[
            "--dump",
            "all",
            "--linker",
            "cc",
            "demo.plenty",
            "-o",
            "out",
        ][..],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?} succeeded");
    }
}
