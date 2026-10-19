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
    let build = || {
        success(
            Command::new(env!("CARGO_BIN_EXE_plenty"))
                .current_dir(&project)
                .arg("build")
                .env("PATH", &tools)
                .env("DRIVER_LOG", &log)
                .output()
                .unwrap(),
        );
        let calls = std::fs::read_to_string(&log).unwrap();
        std::fs::write(&log, "").unwrap();
        calls
    };
    let first = build();
    assert_eq!(first.lines().count(), 2, "{first}");
    assert!(build().is_empty());
    support::write_script(&tools.join("ld.mold"), "#!/bin/sh\nexit 0\n");
    let changed = build();
    assert_eq!(changed.lines().count(), 3, "{changed}");
    assert_eq!(changed.matches("--version").count(), 1, "{changed}");
    assert!(changed.lines().all(|line| line.contains("-fuse-ld=mold")));
    let fresh = build();
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
        target: Some("x86_64-pc-windows-msvc".into()),
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
        format!("{} (CPU baseline: x86-64/SSE2)", plenty::native_target())
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
    let temp = tempfile::tempdir().unwrap();
    let mut snapshots = Vec::new();
    for index in 0..3 {
        let dir = temp.path().join(format!("build-{index}"));
        std::fs::create_dir(&dir).unwrap();
        // The file name is part of the source tree: trap reports print it.
        let source = dir.join("source.plenty");
        std::fs::write(
            &source,
            "import helper\nexport def answer(x: i64) -> i64 = \"fixture_answer\":\n    helper.shift(x)\ndef main() -> i32:\n    i32(answer(4))\n",
        ).unwrap();
        std::fs::write(
            dir.join("helper.plenty"),
            "pub def shift(x: i64) -> i64:\n    x * 9\n",
        )
        .unwrap();
        let mut artifacts = Vec::new();
        for (mode, name) in [
            ("--emit-object", "app.o"),
            ("--compile", "app"),
            ("--static-library", "fixture.a"),
            ("--shared-library", "fixture.so"),
        ] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_plenty"));
            command
                .current_dir(&dir)
                .arg(mode)
                .arg(&source)
                .arg("-o")
                .arg(dir.join(name));
            if mode.ends_with("library") {
                command.args(["--library-name", "fixture"]);
            }
            success(command.output().unwrap());
            artifacts.push(std::fs::read(dir.join(name)).unwrap());
        }
        for name in ["fixture.h", "fixture.plentyi", "fixture.link-args.txt"] {
            artifacts.push(std::fs::read(dir.join(name)).unwrap());
        }
        let runtime = dir.join("runtime");
        success(
            Command::new(env!("CARGO_BIN_EXE_plenty"))
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
            assert_eq!(first, next, "artifact {index} differs");
        }
    }
}
