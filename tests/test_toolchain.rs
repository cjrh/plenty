//! Native artifact and driver contracts. Each test owns its environment.
use std::process::Command;

mod support;
use support::assert_success as success;

#[test]
fn default_linker_links_and_runs_without_cargos_compiler_path() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("main.plenty");
    let executable = temp.path().join("app");
    std::fs::write(&source, "def main() -> i32:\n    23\n").unwrap();
    let output = success(
        Command::new(env!("CARGO_BIN_EXE_plenty"))
            .env_remove("COMPILER_PATH")
            .arg("--compile")
            .arg(source)
            .arg("-o")
            .arg(&executable)
            .arg("--verbose")
            .output()
            .unwrap(),
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("link: "));
    assert_eq!(Command::new(executable).status().unwrap().code(), Some(23));
}

#[cfg(unix)]
#[test]
fn package_build_probes_once_and_rebuilds_when_linker_selection_changes() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let tools = temp.path().join("tools");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::create_dir(&tools).unwrap();
    std::fs::write(
        project.join("plenty.toml"),
        concat!(
            "manifest-version = 1\n[package]\nname = \"fixture\"\nversion = \"1.0.0\"\n",
            "[[bin]]\nname = \"one\"\npath = \"src/one.plenty\"\n",
            "[[bin]]\nname = \"two\"\npath = \"src/two.plenty\"\n"
        ),
    )
    .unwrap();
    for name in ["one", "two"] {
        std::fs::write(
            project.join(format!("src/{name}.plenty")),
            "def main() -> ():\n    pass\n",
        )
        .unwrap();
    }
    let log = temp.path().join("driver.log");
    support::write_script(
        &tools.join("cc"),
        r##"#!/bin/sh
printf '%s\n' "$*" >> "$DRIVER_LOG"
case "$*" in *--version*) exit 0;; esac
while [ "$#" -gt 0 ]; do
    if [ "$1" = -o ]; then shift; : > "$1"; exit 0; fi
    shift
done
exit 1
"##,
    );
    let cache = temp.path().join("cache");
    let build_with = |cache_mode: &str| {
        success(
            Command::new(env!("CARGO_BIN_EXE_plenty"))
                .current_dir(&project)
                .arg("build")
                .env("PATH", &tools)
                .env("DRIVER_LOG", &log)
                .env("PLENTY_CACHE_DIR", &cache)
                .env("PLENTY_CACHE", cache_mode)
                .output()
                .unwrap(),
        );
        let calls = std::fs::read_to_string(&log).unwrap();
        std::fs::write(&log, "").unwrap();
        calls
    };
    let build = || build_with("");
    let first = build();
    assert_eq!(first.lines().count(), 2, "{first}");
    assert!(build().is_empty());
    support::write_script(&tools.join("ld.mold"), "#!/bin/sh\nexit 0\n");
    let changed = build();
    assert_eq!(changed.lines().count(), 3, "{changed}");
    assert_eq!(changed.matches("--version").count(), 1, "{changed}");
    assert!(changed.lines().all(|line| line.contains("-fuse-ld=mold")));
    // A new process reuses the probe recorded in the cache directory.
    assert!(build().is_empty());
    // Without the cache, each process probes once.
    let fresh = build_with("off");
    assert_eq!(fresh.lines().count(), 1, "{fresh}");
    assert!(fresh.contains("--version"));
}

#[cfg(unix)]
#[test]
fn automatic_linker_selection_preserves_overrides_and_falls_back() {
    for (candidates, reject, args, expected, probes) in [
        (vec!["mold", "lld"], "", vec![], Some("mold"), 1),
        (vec!["lld"], "", vec![], Some("lld"), 1),
        (vec![], "", vec![], None, 0),
        (vec!["mold", "lld"], "mold", vec![], Some("lld"), 2),
        (vec!["mold", "lld"], "all", vec![], None, 2),
        (vec!["mold"], "", vec!["--linker", "cc"], None, 0),
        (vec!["mold"], "", vec!["--link-arg", "-lm"], None, 0),
        (
            vec!["mold"],
            "",
            vec!["--link-arg", "-fuse-ld=bfd"],
            Some("bfd"),
            0,
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("driver.log");
        support::write_script(
            &temp.path().join("cc"),
            r##"#!/bin/sh
printf '%s\n' "$*" >> "$DRIVER_LOG"
case "$*" in
  *--version*)
    case "$REJECT:$1" in all:*|mold:-fuse-ld=mold) exit 1;; esac
    exit 0;;
esac
exit 42
"##,
        );
        for candidate in candidates {
            support::write_script(
                &temp.path().join(format!("ld.{candidate}")),
                "#!/bin/sh\nexit 0\n",
            );
        }
        let source = temp.path().join("main.plenty");
        std::fs::write(&source, "def main() -> ():\n    pass\n").unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_plenty"))
            .env("PATH", temp.path())
            .env("DRIVER_LOG", &log)
            .env("REJECT", reject)
            .arg("--compile")
            .arg(source)
            .arg("-o")
            .arg(temp.path().join("app"))
            .arg("--verbose")
            .args(&args)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(!result.status.success(), "{stderr}");
        assert!(
            stderr.contains("42") && stderr.contains("link: "),
            "{stderr}"
        );
        let calls = std::fs::read_to_string(log).unwrap();
        let lines: Vec<_> = calls.lines().collect();
        assert_eq!(lines.len(), probes + 1, "{calls}");
        let actual = lines
            .last()
            .unwrap()
            .split_whitespace()
            .find_map(|arg| arg.strip_prefix("-fuse-ld="));
        assert_eq!(actual, expected, "{calls}");
    }
}

#[test]
fn configurable_driver_links_and_runs() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("main.plenty");
    std::fs::write(&source, "def main() -> i32:\n    23\n").unwrap();
    let output = success(
        Command::new(env!("CARGO_BIN_EXE_plenty"))
            .args(["--compile", source.to_str().unwrap(), "-o"])
            .arg(temp.path().join("app"))
            .args(["--linker", "cc", "--link-arg", "-lm"])
            .output()
            .unwrap(),
    );
    assert!(output.stdout.is_empty());
    assert_eq!(
        Command::new(temp.path().join("app"))
            .status()
            .unwrap()
            .code(),
        Some(23)
    );
}

#[cfg(unix)]
#[test]
fn driver_receives_literal_arguments_and_reports_failure() {
    let temp = tempfile::tempdir().unwrap();
    let driver = temp.path().join("custom driver");
    // This mock implements no link flags: it only reports argv and fails.
    support::write_script(&driver, "#!/bin/sh\nprintf '<%s>\\n' \"$@\" >&2\nexit 42\n");
    let options = plenty::CompileOptions {
        linker: driver.clone(),
        link_args: vec!["a b;$(false)".into(), "-lfixture".into()],
        ..Default::default()
    };
    let error = plenty::compile_source_to_executable_with_options(
        "def main() -> ():\n    pass\n",
        &temp.path().join("out file"),
        &options,
    )
    .unwrap_err()
    .to_string();
    for part in [
        "custom driver",
        "42",
        "<a b;$(false)>",
        "<-lfixture>",
        "<-o>",
        "out file",
    ] {
        assert!(error.contains(part), "{error}");
    }
    let object = error.lines().find(|l| l.ends_with("program.o>")).unwrap();
    assert!(!std::path::Path::new(object.trim_matches(['<', '>'])).exists());
    let options = plenty::CompileOptions {
        linker: temp.path().join("missing driver"),
        ..options
    };
    let error = plenty::compile_source_to_executable_with_options(
        "def main() -> ():\n    pass\n",
        &temp.path().join("out"),
        &options,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("failed to invoke linker driver"), "{error}");
}

#[test]
fn unsupported_target_fails_before_source_or_codegen() {
    let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args([
            "--emit-object",
            "missing.plenty",
            "-o",
            "unused.o",
            "--target",
            "i686-unknown-linux-gnu",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported native target"));
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("untouched");
    std::fs::write(&output, "original").unwrap();
    let options = plenty::CompileOptions {
        target: Some("x86_64-unknown-linux-musl".into()),
        ..Default::default()
    };
    assert!(
        plenty::compile_source_to_executable_with_options("invalid source", &output, &options)
            .unwrap_err()
            .to_string()
            .contains("unsupported native target")
    );
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "original");
    let result = success(
        Command::new(env!("CARGO_BIN_EXE_plenty"))
            .arg("--print-target")
            .output()
            .unwrap(),
    );
    assert_eq!(
        String::from_utf8(result.stdout).unwrap().trim(),
        format!(
            "{} (CPU baseline: {})",
            plenty::native_target(),
            plenty::native_cpu_baseline()
        )
    );
    plenty::validate_target(Some(plenty::native_target())).unwrap();
}

#[test]
fn version_reports_package_version_and_target() {
    for (binary, name) in [
        (env!("CARGO_BIN_EXE_plenty"), "plenty"),
        (env!("CARGO_BIN_EXE_plentyc"), "plentyc"),
    ] {
        for flag in ["--version", "-V"] {
            let result = success(Command::new(binary).arg(flag).output().unwrap());
            assert_eq!(
                String::from_utf8(result.stdout).unwrap(),
                format!(
                    "{name} {} ({})\n",
                    env!("CARGO_PKG_VERSION"),
                    plenty::native_target()
                )
            );
        }
    }
    // Only the bare flag reports; `plenty add --version REQ` is a requirement.
    let result = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args(["--version", "extra"])
        .output()
        .unwrap();
    assert!(!result.status.success());
}

#[test]
fn cli_rejects_missing_or_unused_link_options() {
    for args in [
        vec!["--linker"],
        vec!["--link-arg"],
        vec!["--check", "missing.plenty", "--linker", "cc"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("reading"));
    }
    for args in [
        vec!["--version", "--link-arg", "x"],
        vec!["-V", "--linker", "cc"],
        vec!["--print-target", "--linker", "cc"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
            .args(&args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("linker options require compilation or execution"),
            "{args:?}: {stderr}"
        );
    }
}

#[test]
fn application_object_links_with_extracted_runtime() {
    let temp = tempfile::tempdir().unwrap();
    let object = temp.path().join("program.o");
    plenty::compile_source_to_object(
        "def main() -> i32:\n    print(42).unwrap()\n    7\n",
        &object,
    )
    .unwrap();
    let runtime = plenty::emit_runtime(&temp.path().join("runtime")).unwrap();
    let executable = temp.path().join("app");
    success(
        Command::new("cc")
            .arg(&object)
            .arg(runtime.archive)
            .args(runtime.link_args)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap(),
    );
    let output = Command::new(executable).output().unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout, b"42\n");
}

#[test]
fn cli_emits_objects_with_imports_without_any_toolchain_on_path() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("main.plenty");
    std::fs::write(
        &source,
        "import value\ndef main() -> i32:\n    value.answer()\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("value.plenty"),
        "pub def answer() -> i32:\n    42\n",
    )
    .unwrap();
    let object = temp.path().join("program.o");
    success(
        Command::new(env!("CARGO_BIN_EXE_plenty"))
            .env("PATH", "")
            .arg("--emit-object")
            .arg(&source)
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap(),
    );
    let runtime = temp.path().join("runtime");
    success(
        Command::new(env!("CARGO_BIN_EXE_plenty"))
            .env("PATH", "")
            .arg("--emit-runtime")
            .arg(&runtime)
            .output()
            .unwrap(),
    );
    let args = std::fs::read_to_string(runtime.join("link-args.txt")).unwrap();
    let executable = temp.path().join("app");
    success(
        Command::new("cc")
            .arg(&object)
            .arg(runtime.join("libplenty_runtime.a"))
            .args(args.lines())
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap(),
    );
    assert_eq!(Command::new(executable).status().unwrap().code(), Some(42));
    let error = plenty::compile_source_to_object("def helper() -> ():\n    pass\n", &object)
        .unwrap_err()
        .to_string();
    assert!(error.contains("main"), "{error}");
}

#[test]
fn artifacts_are_reproducible_across_processes_and_paths() {
    use std::time::{Duration, SystemTime};
    // The compiler runs one worker per CPU the process may use
    // (src/parallel.rs), so one build pinned to a single CPU varies the count.
    // Unit tests in src/codegen/output.rs compare object bytes for fixed
    // counts; this covers the affinity-derived count in every artifact.
    let taskset = Command::new("taskset")
        .args(["-c", "0", "true"])
        .status()
        .is_ok_and(|status| status.success());
    if !taskset {
        eprintln!("taskset is unavailable; the code-generation worker count is not varied");
    }
    // Functions are checked and compiled in parallel, so a chain of them
    // gives every worker some.
    let mut helper = String::from("pub def shift(x: i64) -> i64:\n    step_0(x)\n");
    for step in 0..63 {
        helper.push_str(&format!(
            "def step_{step}(x: i64) -> i64:\n    step_{}(x + {step})\n",
            step + 1
        ));
    }
    helper.push_str("def step_63(x: i64) -> i64:\n    x * 9\n");
    let temp = tempfile::tempdir().unwrap();
    let mut snapshots = Vec::new();
    for index in 0..3 {
        let dir = temp.path().join(format!("build-{index}"));
        let out = dir.join(format!("out-{index}"));
        let user = ["alice", "bob", "carol"][index];
        let home = dir.join(user);
        std::fs::create_dir_all(&out).unwrap();
        std::fs::create_dir(&home).unwrap();
        // The file name is part of the source tree: trap reports print it.
        let source = dir.join("source.plenty");
        std::fs::write(
            &source,
            "import helper\nexport def answer(x: i64) -> i64 = \"fixture_answer\":\n    helper.shift(x)\ndef main() -> i32:\n    i32(answer(4))\n",
        ).unwrap();
        std::fs::write(dir.join("helper.plenty"), &helper).unwrap();
        let modified = match index {
            0 => SystemTime::UNIX_EPOCH + Duration::from_secs(315_532_800), // 1980
            1 => SystemTime::now(),
            _ => SystemTime::UNIX_EPOCH + Duration::from_secs(4_102_444_800), // 2100
        };
        for file in [source.clone(), dir.join("helper.plenty")] {
            std::fs::File::options()
                .write(true)
                .open(file)
                .unwrap()
                .set_modified(modified)
                .unwrap();
        }
        let plenty = || {
            let mut command = if index == 0 && taskset {
                let mut command = Command::new("taskset");
                command.args(["-c", "0", env!("CARGO_BIN_EXE_plenty")]);
                command
            } else {
                Command::new(env!("CARGO_BIN_EXE_plenty"))
            };
            // Each build runs fresh code generation; test_compile_cache
            // compares cached builds with uncached ones.
            command
                .current_dir(&dir)
                .env("PLENTY_CACHE", "off")
                .env("USER", user)
                .env("LOGNAME", user)
                .env("HOME", &home);
            match index {
                0 => command.env_remove("SOURCE_DATE_EPOCH"),
                1 => command.env("SOURCE_DATE_EPOCH", "0"),
                _ => command.env("SOURCE_DATE_EPOCH", "1700000000"),
            };
            command
        };
        let mut artifacts = Vec::new();
        for (mode, name) in [
            (&["--emit-object"][..], format!("app-{index}.o")),
            (&["--compile"], format!("app-{index}")),
            (&["--compile", "--no-strip"], format!("unstripped-{index}")),
            (&["--static-library"], format!("fixture-{index}.a")),
            (&["--shared-library"], format!("fixture-{index}.so")),
        ] {
            let mut command = plenty();
            command
                .args(mode)
                .arg(&source)
                .arg("-o")
                .arg(out.join(&name));
            // The library name is an input: it names the exports and the
            // header, interface, and link-argument files written beside OUT.
            if mode[0].ends_with("library") {
                command.args(["--library-name", "fixture"]);
            }
            success(command.output().unwrap());
            artifacts.push(std::fs::read(out.join(name)).unwrap());
        }
        for name in ["fixture.h", "fixture.plentyi", "fixture.link-args.txt"] {
            artifacts.push(std::fs::read(out.join(name)).unwrap());
        }
        let runtime = out.join(format!("runtime-{index}"));
        success(
            plenty()
                .arg("--emit-runtime")
                .arg(&runtime)
                .output()
                .unwrap(),
        );
        for name in ["libplenty_runtime.a", "link-args.txt", "target.txt"] {
            artifacts.push(std::fs::read(runtime.join(name)).unwrap());
        }
        snapshots.push(artifacts);
    }
    for snapshot in &snapshots[1..] {
        for (index, (first, next)) in snapshots[0].iter().zip(snapshot).enumerate() {
            assert!(first == next, "artifact {index} differs");
        }
    }
}

/// Section names of the ELF file at `path`, and whether it has a symbol table.
fn sections(path: &std::path::Path) -> (Vec<String>, bool) {
    use object::{Object, ObjectSection};
    let data = std::fs::read(path).unwrap();
    let file = object::File::parse(&*data).unwrap();
    let names = file
        .sections()
        .filter_map(|section| section.name().ok().map(str::to_owned))
        .collect();
    (names, file.symbol_table().is_some())
}

/// Executables and shared libraries drop the symbol table and debug sections
/// unless `--no-strip` asks to keep them (issue #500). Trap reports read the
/// compiler's site tables, not symbols, so a stripped program still reports
/// where it failed; shared libraries keep their dynamic exports.
#[test]
fn outputs_are_stripped_unless_no_strip_is_given() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("main.plenty");
    std::fs::write(
        &source,
        "export def answer(x: i64) -> i64 = \"fixture_answer\":\n    x + 1\n\ndef main() -> Result[(), AllocError | IoError]:\n    big: i8 = 127\n    print(big + 1i8)?\n    Ok(())\n",
    )
    .unwrap();
    let build = |args: &[&str], output: &str| {
        let path = temp.path().join(output);
        let mut command = Command::new(env!("CARGO_BIN_EXE_plenty"));
        command.args(args).arg(&source).arg("-o").arg(&path);
        if args[0] == "--shared-library" {
            command.args(["--library-name", "fixture"]);
        }
        success(command.output().unwrap());
        path
    };
    for (args, kept) in [
        (&["--compile"][..], false),
        (&["--compile", "--no-strip"], true),
    ] {
        let program = build(args, if kept { "kept" } else { "stripped" });
        let (names, symbols) = sections(&program);
        let debug = names.iter().any(|name| name.starts_with(".debug_"));
        assert_eq!((debug, symbols), (kept, kept), "{args:?}: {names:?}");
        let output = Command::new(&program).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("6:11: integer overflow"),
            "{args:?}: {stderr}"
        );
    }
    for (args, kept) in [
        (&["--shared-library"][..], false),
        (&["--shared-library", "--no-strip"], true),
    ] {
        let library = build(args, if kept { "kept.so" } else { "stripped.so" });
        let (names, symbols) = sections(&library);
        let debug = names.iter().any(|name| name.starts_with(".debug_"));
        assert_eq!((debug, symbols), (kept, kept), "{args:?}: {names:?}");
        let exports = success(
            Command::new("nm")
                .args(["-D", "--defined-only"])
                .arg(&library)
                .output()
                .unwrap(),
        );
        assert!(String::from_utf8_lossy(&exports.stdout).contains(" fixture_answer\n"));
    }
    for mode in ["--emit-object", "--check", "--fuzz"] {
        let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
            .args([mode, "--no-strip"])
            .arg(&source)
            .args(["-o", "unused"])
            .output()
            .unwrap();
        assert!(!output.status.success(), "{mode}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("--no-strip requires --compile or --shared-library"),
            "{mode}"
        );
    }
}

/// Global flags must be removed before checking the compilation mode (#590).
#[test]
fn release_and_no_strip_are_order_independent() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("main.plenty"),
        "def main() -> i32:\n    0\n",
    )
    .unwrap();
    let permutations = [
        ["--release", "--no-strip", "--compile", "main.plenty"],
        ["--no-strip", "--release", "--compile", "main.plenty"],
        ["--release", "--compile", "--no-strip", "main.plenty"],
        ["--no-strip", "--compile", "--release", "main.plenty"],
        ["--release", "--compile", "main.plenty", "--no-strip"],
        ["--no-strip", "--compile", "main.plenty", "--release"],
        ["--compile", "--release", "--no-strip", "main.plenty"],
        ["--compile", "--no-strip", "--release", "main.plenty"],
        ["--compile", "--release", "main.plenty", "--no-strip"],
        ["--compile", "--no-strip", "main.plenty", "--release"],
        ["--compile", "main.plenty", "--release", "--no-strip"],
        ["--compile", "main.plenty", "--no-strip", "--release"],
    ];
    for binary in [env!("CARGO_BIN_EXE_plenty"), env!("CARGO_BIN_EXE_plentyc")] {
        for args in permutations {
            let program = temp.path().join("program");
            success(
                Command::new(binary)
                    .current_dir(temp.path())
                    .args(args)
                    .arg("-o")
                    .arg(&program)
                    .output()
                    .unwrap(),
            );
            let (names, symbols) = sections(&program);
            assert!(symbols, "{binary} {args:?}: {names:?}");
            success(Command::new(&program).output().unwrap());
            std::fs::remove_file(program).unwrap();
        }
        // Normalization must not weaken either flag's mode restrictions.
        for flags in [["--release", "--no-strip"], ["--no-strip", "--release"]] {
            for (mode, diagnostic) in [
                (
                    "--check",
                    "--release requires FILE, --compile, --fuzz, or --dump",
                ),
                (
                    "--shared-library",
                    "--release requires FILE, --compile, --fuzz, or --dump",
                ),
                (
                    "--dump",
                    "--no-strip requires --compile or --shared-library",
                ),
            ] {
                let output = Command::new(binary)
                    .current_dir(temp.path())
                    .args(flags)
                    .args([mode, "main.plenty"])
                    .output()
                    .unwrap();
                assert!(!output.status.success(), "{binary} {flags:?} {mode}");
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(
                    stderr.contains(diagnostic),
                    "{binary} {flags:?} {mode}: {stderr}"
                );
            }
        }
    }
}

#[test]
fn check_all_methods_flag_is_documented_and_parsed_by_both_binaries() {
    // Issue #517.
    let temp = tempfile::tempdir().unwrap();
    let module = temp.path().join("cells.plenty");
    std::fs::write(
        &module,
        "class Cell[T]:
    value: T
    def get(self) -> T:
        self.value

def lists() -> Result[Cell[list[i64]], AllocError]:
    Ok(Cell([1]?))
",
    )
    .unwrap();
    for binary in [env!("CARGO_BIN_EXE_plenty"), env!("CARGO_BIN_EXE_plentyc")] {
        let help = success(Command::new(binary).arg("--help").output().unwrap());
        assert!(
            String::from_utf8_lossy(&help.stdout).contains("--check-all-methods: with --check"),
            "{binary}"
        );
        success(
            Command::new(binary)
                .arg("--check-module")
                .arg(&module)
                .output()
                .unwrap(),
        );
        // The flag may precede or follow the mode.
        for args in [
            ["--check-module", "--check-all-methods"],
            ["--check-all-methods", "--check-module"],
        ] {
            let output = Command::new(binary)
                .args(args)
                .arg(&module)
                .output()
                .unwrap();
            assert!(!output.status.success(), "{binary} {args:?}");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains(":6:23: `Cell[list[i64]].get` copies `T`")
                    && stderr.contains("is checked because of --check-all-methods"),
                "{binary} {args:?}: {stderr}"
            );
        }
    }
    let help = success(Command::new(env!("CARGO_BIN_EXE_plenty")).output().unwrap());
    assert!(
        String::from_utf8_lossy(&help.stdout)
            .contains("plenty check [--locked] [--offline] [--check-all-methods]")
    );
}
