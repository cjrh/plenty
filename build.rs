//! Package a target-specific native runtime once when building the compiler.
//! Invoke rustc directly on the runtime crate and its vendored dependencies:
//! invoking Cargo recursively from a build script would contend for Cargo's
//! package locks.
use std::path::{Path, PathBuf};
use std::process::Command;

/// The runtime's only third-party code: RustCrypto digests, HMAC and
/// constant-time comparison for `std.crypto`, and zlib-rs for `std.compress`,
/// copied unmodified from crates.io into `plenty-runtime/vendor`. Versions match `Cargo.lock`, which the Cargo
/// build of the same crate uses; `tests/test_std_crypto.rs` checks they agree.
/// Each entry: directory, crate name, edition, features, extra cfgs, externs.
type Vendored = (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
);
const VENDORED: &[Vendored] = &[
    ("cfg-if-1.0.4", "cfg_if", "2018", &[], &[], &[]),
    ("typenum-1.20.1", "typenum", "2018", &[], &[], &[]),
    (
        "generic-array-0.14.7",
        "generic_array",
        "2015",
        &["more_lengths"],
        // Its build script enables this on every supported rustc.
        &["relaxed_coherence"],
        &["typenum"],
    ),
    (
        "crypto-common-0.1.7",
        "crypto_common",
        "2018",
        &[],
        &[],
        &["generic_array", "typenum"],
    ),
    (
        "block-buffer-0.10.4",
        "block_buffer",
        "2018",
        &[],
        &[],
        &["generic_array"],
    ),
    ("subtle-2.6.1", "subtle", "2018", &[], &[], &[]),
    (
        "digest-0.10.7",
        "digest",
        "2018",
        &["core-api", "block-buffer", "mac", "subtle"],
        &[],
        &["crypto_common", "block_buffer", "subtle"],
    ),
    ("cpufeatures-0.2.17", "cpufeatures", "2018", &[], &[], &[]),
    (
        "sha2-0.10.9",
        "sha2",
        "2018",
        &[],
        &[],
        &["cfg_if", "digest", "cpufeatures"],
    ),
    ("hmac-0.12.1", "hmac", "2018", &[], &[], &["digest"]),
    (
        "zlib-rs-0.6.8",
        "zlib_rs",
        "2021",
        // Cargo's `std` feature implies `rust-allocator`.
        &["std", "rust-allocator", "__internal-api"],
        &[],
        &[],
    ),
];
const RUNTIME_EXTERNS: &[&str] = &["sha2", "hmac", "subtle", "zlib_rs"];

fn main() {
    println!("cargo:rerun-if-changed=plenty-runtime/src");
    println!("cargo:rerun-if-changed=plenty-runtime/Cargo.toml");
    println!("cargo:rerun-if-env-changed=PLENTY_RUNTIME_RUSTFLAGS");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    println!(
        "cargo:rustc-env=PLENTY_RUNTIME_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
    let vendor = build_vendored(&out);
    build_sqlite(&vendor);
    build_runtime(&out, &vendor, true);
    build_runtime(&out, &vendor, false);
    build_test_hooks(&out);
}

/// Failure injection linked only into `plenty test` executables, outside both
/// runtime archives so shipped programs are unchanged.
const TEST_HOOKS: &str = "plenty-runtime/native/test_hooks.c";

fn build_test_hooks(out: &Path) {
    println!("cargo:rerun-if-changed={TEST_HOOKS}");
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    let output = Command::new(&cc)
        .args(["-c", "-O2", "-fPIC", "-g0", "-Wall", "-Werror"])
        .args(["-ffunction-sections", "-fdata-sections"])
        .arg(format!("-ffile-prefix-map={}=/plenty", manifest.display()))
        .arg(TEST_HOOKS)
        .arg("-o")
        .arg(out.join("plenty_test_hooks.o"))
        .output()
        .expect("invoke the C compiler for the test hooks");
    assert!(
        output.status.success(),
        "compiling the test hooks failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The SQLite amalgamation behind `std.db.sqlite`, unmodified C from
/// sqlite.org. Both runtime archives bundle it. Function and data sections
/// let `--gc-sections` drop it from programs that never call `plenty_sqlite`.
/// See book/src/design/stdlib/sqlite.md for the options and update policy.
const SQLITE: &str = "plenty-runtime/native/sqlite-3.54.0";
const SQLITE_OPTIONS: &[&str] = &[
    "-DSQLITE_THREADSAFE=1",
    "-DSQLITE_DEFAULT_MEMSTATUS=0",
    "-DSQLITE_DQS=0",
    "-DSQLITE_LIKE_DOESNT_MATCH_BLOBS",
    "-DSQLITE_OMIT_DEPRECATED",
    "-DSQLITE_OMIT_LOAD_EXTENSION",
    "-DSQLITE_OMIT_SHARED_CACHE",
];

fn build_sqlite(dir: &Path) {
    println!("cargo:rerun-if-changed={SQLITE}");
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=AR");
    let object = dir.join("sqlite3.o");
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    let output = Command::new(&cc)
        .args(["-c", "-O2", "-fPIC", "-g0", "-w"])
        .args(["-ffunction-sections", "-fdata-sections"])
        .arg(format!("-ffile-prefix-map={}=/plenty", manifest.display()))
        .args(SQLITE_OPTIONS)
        .arg(format!("{SQLITE}/sqlite3.c"))
        .arg("-o")
        .arg(&object)
        .output()
        .expect("invoke the C compiler for SQLite; std.db.sqlite needs `cc`");
    assert!(
        output.status.success(),
        "compiling SQLite failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let archive = dir.join("libplenty_sqlite.a");
    let _ = std::fs::remove_file(&archive);
    let ar = std::env::var_os("AR").unwrap_or_else(|| "ar".into());
    let output = Command::new(&ar)
        .arg("crsD")
        .arg(&archive)
        .arg(&object)
        .output()
        .expect("invoke `ar` to archive SQLite");
    assert!(
        output.status.success(),
        "archiving SQLite failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn remap(rustc: &mut Command, out: &Path) {
    // Precompiled standard-library objects remain toolchain inputs.
    for (path, replacement) in [
        (
            PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()),
            "/plenty",
        ),
        (out.to_path_buf(), "/plenty-build"),
    ] {
        rustc.arg(format!(
            "--remap-path-prefix={}={replacement}",
            path.display()
        ));
    }
    let sysroot = Command::new(std::env::var_os("RUSTC").unwrap())
        .args(["--print", "sysroot"])
        .output()
        .expect("query rustc sysroot");
    assert!(sysroot.status.success(), "querying rustc sysroot failed");
    rustc.arg(format!(
        "--remap-path-prefix={}=/rust-toolchain",
        String::from_utf8(sysroot.stdout).unwrap().trim()
    ));
    // Separate, explicit flags allow runtime sanitizer builds without silently
    // leaking compiler-only flags (coverage/link arguments/metadata) into an ABI library.
    if let Ok(flags) = std::env::var("PLENTY_RUNTIME_RUSTFLAGS") {
        rustc.args(flags.split_whitespace());
    }
}

fn extern_arg(dir: &Path, name: &str) -> String {
    format!(
        "--extern={name}={}",
        dir.join(format!("lib{name}.rlib")).display()
    )
}

/// Compile the vendored crates once into rlibs shared by both runtime builds.
fn build_vendored(out: &Path) -> PathBuf {
    println!("cargo:rerun-if-changed=plenty-runtime/vendor");
    let dir = out.join("vendor");
    std::fs::create_dir_all(&dir).unwrap();
    for (path, name, edition, features, cfgs, externs) in VENDORED {
        let mut rustc = Command::new(std::env::var_os("RUSTC").unwrap());
        rustc
            .arg(format!("plenty-runtime/vendor/{path}/src/lib.rs"))
            .args(["--crate-name", name, "--crate-type", "rlib"])
            .arg(format!("--edition={edition}"))
            .args(["--target", &std::env::var("TARGET").unwrap()])
            .args(["-Copt-level=2", "-Cpanic=abort", "-Ccodegen-units=1"])
            .arg("-Cembed-bitcode=yes")
            .arg(format!("-Cmetadata=plenty-runtime-{path}"))
            .args(["--cap-lints", "allow"])
            .arg(format!("-Ldependency={}", dir.display()))
            .arg("--out-dir")
            .arg(&dir);
        for feature in *features {
            rustc.args(["--cfg", &format!("feature=\"{feature}\"")]);
        }
        for cfg in *cfgs {
            rustc.args(["--cfg", cfg]);
        }
        for name in *externs {
            rustc.arg(extern_arg(&dir, name));
        }
        remap(&mut rustc, out);
        let output = rustc.output().expect("invoke rustc for a vendored crate");
        assert!(
            output.status.success(),
            "building vendored {path} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    dir
}

fn build_runtime(out: &Path, vendor: &Path, application: bool) {
    let archive = out.join(if application {
        "libplenty_runtime.a"
    } else {
        "libplenty_library_runtime.a"
    });
    let mut rustc = Command::new(std::env::var_os("RUSTC").unwrap());
    rustc.args([
        "plenty-runtime/src/lib.rs",
        "--crate-name",
        "plenty_runtime",
        "--crate-type",
        "staticlib",
        "--edition=2021",
        "--target",
        &std::env::var("TARGET").unwrap(),
        "-Copt-level=2",
        "-Cpanic=abort",
        "-Clto=thin",
        "-Ccodegen-units=1",
        "--check-cfg",
        "cfg(plenty_runtime_embedded)",
        "--check-cfg",
        "cfg(feature, values(\"allocation-checks\"))",
        "--check-cfg",
        "cfg(plenty_runtime_sqlite)",
        "--cfg",
        "plenty_runtime_sqlite",
        "-lstatic=plenty_sqlite",
        "--print",
        "native-static-libs",
        "-o",
    ]);
    rustc.arg(&archive);
    rustc.arg(format!("-Ldependency={}", vendor.display()));
    rustc.arg(format!("-Lnative={}", vendor.display()));
    for name in RUNTIME_EXTERNS {
        rustc.arg(extern_arg(vendor, name));
    }
    remap(&mut rustc, out);
    if application {
        rustc.args(["--cfg", "plenty_runtime_embedded"]);
    }
    if std::env::var_os("CARGO_FEATURE_RUNTIME_CHECKS").is_some() {
        rustc.args(["--cfg", "feature=\"allocation-checks\""]);
    }
    let output = rustc
        .output()
        .expect("invoke rustc to build plenty-runtime");
    let diagnostics = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "building plenty-runtime failed:\n{diagnostics}"
    );
    let libs = diagnostics
        .lines()
        .find_map(|line| line.strip_prefix("note: native-static-libs: "))
        .expect("rustc must report the runtime's native link dependencies");
    // Unreferenced runtime sections, such as `std.crypto`'s hashing code
    // behind its own `plenty_crypto` entry, stay out of programs.
    let libs = if application {
        format!("-Wl,--gc-sections {libs}")
    } else {
        libs.to_string()
    };
    std::fs::write(
        out.join(if application {
            "runtime-link-args.txt"
        } else {
            "library-runtime-link-args.txt"
        }),
        libs,
    )
    .unwrap();
}
