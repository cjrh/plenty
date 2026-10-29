//! Distribution CLI, exact engine selection, provenance, and app-only relocation.
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn compiler() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_plenty"));
    cmd.env_remove("PLENTY_DATAFRAME_ENGINE")
        .env_remove("PLENTY_LINALG_ENGINE");
    cmd
}

fn success(output: Output) -> Output {
    assert!(output.status.success(), "{output:?}");
    output
}

fn failure(output: Output, message: &str) {
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{output:?}"
    );
}

fn run(executable: &Path, cwd: &Path) -> Output {
    Command::new(executable)
        .current_dir(cwd)
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .output()
        .unwrap()
}

fn dynamic(executable: &Path) -> String {
    String::from_utf8(
        success(
            Command::new("readelf")
                .arg("-d")
                .arg(executable)
                .output()
                .unwrap(),
        )
        .stdout,
    )
    .unwrap()
}

fn source(root: &Path, calls: &str) -> PathBuf {
    std::fs::write(root.join("native.plentyi"), "extern def dataframe_raw() -> i64 = \"pldf_v1_fixture\"\nextern def linalg_raw() -> i64 = \"plla_v1_fixture\"\npub def dataframe() -> i64:\n    dataframe_raw()\npub def linalg() -> i64:\n    linalg_raw()\n").unwrap();
    let source = root.join("analysis file.plenty");
    std::fs::write(&source, format!("import native\ndef unused() -> i64:\n    native.linalg()\ndef main() -> Result[(), IoError]:\n    print({calls})?\n    Ok(())\n")).unwrap();
    source
}

/// Small real ELF libraries isolate bundling from the optional Rust engines.
/// They intentionally implement fixture-only symbols, not dataframe/faer behavior.
fn engine(root: &Path, name: &str, prefix: &str, value: i32) -> PathBuf {
    let package = root.join(format!("{name} package"));
    let lib = package.join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    let library = lib.join(format!("libplenty_{name}_v1.so"));
    let code = package.join("fixture.c");
    std::fs::write(
        &code,
        format!("long {prefix}_v1_fixture(void) {{ return {value}; }}"),
    )
    .unwrap();
    success(
        Command::new("cc")
            .args(["-shared", "-fPIC"])
            .arg(format!("-Wl,-soname,libplenty_{name}_v1.so"))
            .arg(&code)
            .arg("-o")
            .arg(&library)
            .output()
            .unwrap(),
    );
    let manifest = serde_json::json!({
        "format": 1, "plenty_version": env!("CARGO_PKG_VERSION"),
        "target": plenty::native_target(), "abi_generation": 1,
        "library": format!("lib/libplenty_{name}_v1.so"),
        "minimum_glibc": "2.28", "source_commit": "fixture-only",
        "library_sha256": format!("{:x}", Sha256::digest(std::fs::read(&library).unwrap()))
    });
    std::fs::write(package.join("engine.json"), manifest.to_string()).unwrap();
    for name in ["LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.md"] {
        std::fs::write(package.join(name), format!("fixture {name}")).unwrap();
    }
    std::fs::create_dir_all(package.join("licenses/rust")).unwrap();
    std::fs::write(package.join("licenses/rust/LICENSE.txt"), "fixture license").unwrap();
    library
}

fn bundle(source: &Path, dest: &Path, dataframe: &Path, linalg: &Path) -> Output {
    compiler()
        .env("PLENTY_DATAFRAME_ENGINE", dataframe)
        .env("PLENTY_LINALG_ENGINE", linalg)
        .arg("--compile")
        .arg(source)
        .arg("--dist")
        .arg(dest)
        .output()
        .unwrap()
}

fn assert_no_stage(root: &Path) {
    assert!(!std::fs::read_dir(root).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".plenty-dist-")
    }));
}

#[test]
fn no_engine_bundle_relocates_with_unused_imports_and_missing_engines() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("hello world.plenty");
    std::fs::write(&source, "import std.dataframe\nimport std.linalg\ndef main() -> Result[(), IoError]:\n    print(42)?\n    Ok(())\n").unwrap();
    let dist = temp.path().join("new parent/app bundle");
    success(bundle(
        &source,
        &dist,
        Path::new("/missing/dataframe"),
        Path::new("/missing/linalg"),
    ));
    assert_eq!(std::fs::read_dir(&dist).unwrap().count(), 1);
    let relocated = temp.path().join("renamed app");
    std::fs::rename(dist, &relocated).unwrap();
    assert_eq!(
        success(run(&relocated.join("hello world"), Path::new("/"))).stdout,
        b"42\n"
    );
    assert!(!dynamic(&relocated.join("hello world")).contains("libplenty_"));
}

#[test]
fn exact_reached_engines_notices_and_origin_only_lookup() {
    let temp = tempfile::tempdir().unwrap();
    let dataframe = engine(temp.path(), "dataframe", "pldf", 11);
    let linalg = engine(temp.path(), "linalg", "plla", 22);
    for (calls, names, expected) in [
        ("native.dataframe()", vec!["dataframe"], "11\n"),
        ("native.linalg()", vec!["linalg"], "22\n"),
        (
            "native.dataframe() + native.linalg()",
            vec!["dataframe", "linalg"],
            "33\n",
        ),
    ] {
        let src = source(temp.path(), calls);
        let dist = temp.path().join(format!("bundle-{}", names.join("-")));
        success(bundle(&src, &dist, &dataframe, &linalg));
        assert_eq!(
            std::fs::read_dir(&dist).unwrap().count(),
            1 + 2 * names.len()
        );
        for name in &names {
            let library = format!("libplenty_{name}_v1.so");
            let original = temp.path().join(format!("{name} package"));
            assert_eq!(
                std::fs::read(dist.join(&library)).unwrap(),
                std::fs::read(original.join("lib").join(&library)).unwrap()
            );
            for file in [
                "engine.json",
                "LICENSE-MIT",
                "LICENSE-APACHE",
                "THIRD-PARTY-NOTICES.md",
                "licenses/rust/LICENSE.txt",
            ] {
                assert_eq!(
                    std::fs::read(dist.join(format!("{name}-notices")).join(file)).unwrap(),
                    std::fs::read(original.join(file)).unwrap()
                );
            }
        }
        let relocated = dist.with_extension("moved");
        std::fs::rename(&dist, &relocated).unwrap();
        let executable = relocated.join("analysis file");
        assert_eq!(
            success(run(&executable, Path::new("/"))).stdout,
            expected.as_bytes()
        );
        let headers = dynamic(&executable);
        assert!(headers.contains("[$ORIGIN]"), "{headers}");
        assert!(
            !headers.contains(&temp.path().display().to_string()),
            "{headers}"
        );
        // Original libraries remain available: a fallback would mask this error.
        let missing = relocated.join(format!("libplenty_{}_v1.so", names[0]));
        std::fs::remove_file(missing).unwrap();
        failure(
            run(&executable, Path::new("/")),
            "cannot open shared object file",
        );
    }
}

#[test]
fn installed_notices_layout_and_ordinary_compile_are_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let original = engine(temp.path(), "linalg", "plla", 22);
    let installed = temp.path().join("installed");
    std::fs::create_dir(&installed).unwrap();
    let library = installed.join("libplenty_linalg_v1.so");
    std::fs::copy(&original, &library).unwrap();
    std::fs::rename(
        original.parent().unwrap().parent().unwrap(),
        installed.join("linalg-notices"),
    )
    .unwrap();
    let src = source(temp.path(), "native.linalg()");
    let dist = temp.path().join("bundle");
    success(bundle(&src, &dist, Path::new("/missing"), &library));
    assert_eq!(
        success(run(&dist.join("analysis file"), Path::new("/"))).stdout,
        b"22\n"
    );
    std::fs::remove_dir_all(installed.join("linalg-notices")).unwrap();
    let ordinary = temp.path().join("ordinary");
    success(
        compiler()
            .env("PLENTY_LINALG_ENGINE", &library)
            .arg("--compile")
            .arg(&src)
            .arg("-o")
            .arg(&ordinary)
            .output()
            .unwrap(),
    );
    assert_eq!(success(run(&ordinary, Path::new("/"))).stdout, b"22\n");
    assert!(dynamic(&ordinary).contains(&installed.display().to_string()));
    failure(
        bundle(
            &src,
            &temp.path().join("missing notices"),
            Path::new("/missing"),
            &library,
        ),
        "cannot bundle std.linalg",
    );
}

#[test]
fn cli_rejects_incompatible_modes_and_bad_values() {
    for binary in [env!("CARGO_BIN_EXE_plenty"), env!("CARGO_BIN_EXE_plentyc")] {
        for args in [
            vec!["--dist"],
            vec!["--compile", "a.plenty", "--dist", ""],
            vec!["--compile", "a.plenty", "--dist", "one", "--dist", "two"],
            vec!["--compile", "a.plenty", "--dist", "one", "-o", "out"],
            vec!["--legacy", "--compile", "a.plenty", "--dist", "one"],
            vec![
                "--compile",
                "a.plenty",
                "--dist",
                "one",
                "--link-arg",
                "-Wl,-rpath,/tmp",
            ],
        ] {
            failure(Command::new(binary).args(args).output().unwrap(), "--dist");
        }
        for mode in [
            "--check",
            "--emit-object",
            "--fuzz",
            "--shared-library",
            "--static-library",
            "--emit-runtime",
            "--version",
        ] {
            failure(
                Command::new(binary)
                    .args([mode, "input", "--dist", "output"])
                    .output()
                    .unwrap(),
                "--dist requires --compile",
            );
        }
        assert!(
            String::from_utf8(success(Command::new(binary).arg("--help").output().unwrap()).stdout)
                .unwrap()
                .contains("--dist")
        );
    }
}

#[test]
fn existing_destinations_collisions_and_failed_builds_leave_no_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let src = source(temp.path(), "42");
    let dest = temp.path().join("bundle");
    for kind in ["directory", "file", "dangling symlink"] {
        match kind {
            "directory" => std::fs::create_dir(&dest).unwrap(),
            "file" => std::fs::write(&dest, "keep").unwrap(),
            _ => std::os::unix::fs::symlink("missing", &dest).unwrap(),
        }
        failure(
            bundle(&src, &dest, Path::new("/missing"), Path::new("/missing")),
            "already exists",
        );
        if kind == "directory" {
            std::fs::remove_dir(&dest).unwrap();
        } else {
            std::fs::remove_file(&dest).unwrap();
        }
    }
    for name in ["libplenty_dataframe_v1.so.plenty", "linalg-notices.plenty"] {
        let collision = temp.path().join(name);
        std::fs::copy(&src, &collision).unwrap();
        failure(
            bundle(
                &collision,
                &dest,
                Path::new("/missing"),
                Path::new("/missing"),
            ),
            "conflicts",
        );
        assert!(!dest.exists());
    }
    failure(
        compiler()
            .arg("--compile")
            .arg(&src)
            .arg("--dist")
            .arg(&dest)
            .args(["--linker", "/bin/false"])
            .output()
            .unwrap(),
        "linker driver",
    );
    assert!(!dest.exists());
    assert_no_stage(temp.path());
    std::fs::write(&src, "not valid Plenty").unwrap();
    assert!(
        !bundle(&src, &dest, Path::new("/missing"), Path::new("/missing"))
            .status
            .success()
    );
    assert!(!dest.exists());
    assert_no_stage(temp.path());
}

#[test]
fn missing_artifacts_and_mismatched_provenance_fail_without_partial_output() {
    let temp = tempfile::tempdir().unwrap();
    let lib = engine(temp.path(), "dataframe", "pldf", 11);
    let package = lib.parent().unwrap().parent().unwrap();
    let src = source(temp.path(), "native.dataframe()");
    let dest = temp.path().join("bundle");
    for artifact in [
        "engine.json",
        "LICENSE-MIT",
        "LICENSE-APACHE",
        "THIRD-PARTY-NOTICES.md",
        "licenses",
        "lib/libplenty_dataframe_v1.so",
    ] {
        let path = package.join(artifact);
        let backup = package.join("saved");
        std::fs::rename(&path, &backup).unwrap();
        failure(bundle(&src, &dest, &lib, Path::new("/missing")), artifact);
        assert!(!dest.exists());
        assert_no_stage(temp.path());
        std::fs::rename(backup, path).unwrap();
    }
    let manifest_path = package.join("engine.json");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    for field in [
        "format",
        "plenty_version",
        "target",
        "abi_generation",
        "library",
        "source_commit",
        "minimum_glibc",
        "library_sha256",
    ] {
        let mut bad = manifest.clone();
        bad[field] = serde_json::json!("");
        std::fs::write(&manifest_path, bad.to_string()).unwrap();
        failure(bundle(&src, &dest, &lib, Path::new("/missing")), field);
        assert!(!dest.exists());
        assert_no_stage(temp.path());
    }
    std::fs::write(&manifest_path, manifest.to_string()).unwrap();
    // Matching version text is insufficient: the selected bytes must match too.
    std::fs::write(&lib, b"replaced library").unwrap();
    failure(
        bundle(&src, &dest, &lib, Path::new("/missing")),
        "library_sha256",
    );
    assert!(!dest.exists());
    assert_no_stage(temp.path());
}

#[test]
fn invalid_soname_and_symlinked_notices_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let library = engine(temp.path(), "linalg", "plla", 22);
    let package = library.parent().unwrap().parent().unwrap();
    let src = source(temp.path(), "native.linalg()");
    let dest = temp.path().join("bundle");
    let license = package.join("licenses/rust/LICENSE.txt");
    std::fs::remove_file(&license).unwrap();
    std::os::unix::fs::symlink(package.join("LICENSE-MIT"), &license).unwrap();
    failure(
        bundle(&src, &dest, Path::new("/missing"), &library),
        "regular distribution artifact",
    );
    assert!(!dest.exists());
    std::fs::remove_file(&license).unwrap();
    std::fs::write(license, "fixture license").unwrap();

    // An absent/wrong SONAME would embed the staging path in DT_NEEDED.
    for soname in [None, Some("wrong-engine.so")] {
        let mut cc = Command::new("cc");
        cc.args(["-shared", "-fPIC"]);
        if let Some(soname) = soname {
            cc.arg(format!("-Wl,-soname,{soname}"));
        }
        success(
            cc.arg(package.join("fixture.c"))
                .arg("-o")
                .arg(&library)
                .output()
                .unwrap(),
        );
        let manifest_path = package.join("engine.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["library_sha256"] = serde_json::json!(format!(
            "{:x}",
            Sha256::digest(std::fs::read(&library).unwrap())
        ));
        std::fs::write(manifest_path, manifest.to_string()).unwrap();
        failure(
            bundle(&src, &dest, Path::new("/missing"), &library),
            "SONAME",
        );
        assert!(!dest.exists());
        assert_no_stage(temp.path());
    }
}

#[test]
fn linker_injected_fallback_is_rejected_before_publication() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let src = source(temp.path(), "42");
    let dest = temp.path().join("bundle");
    let driver = temp.path().join("linker wrapper");
    std::fs::write(
        &driver,
        "#!/bin/sh\nexec cc \"$@\" -Wl,-rpath,/development-machine\n",
    )
    .unwrap();
    std::fs::set_permissions(&driver, std::fs::Permissions::from_mode(0o755)).unwrap();
    failure(
        compiler()
            .arg("--compile")
            .arg(&src)
            .arg("--dist")
            .arg(&dest)
            .arg("--linker")
            .arg(&driver)
            .output()
            .unwrap(),
        "non-$ORIGIN RUNPATH",
    );
    assert!(!dest.exists());
    assert_no_stage(temp.path());
}
