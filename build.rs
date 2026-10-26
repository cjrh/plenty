//! Package a target-specific native runtime once when building the compiler.
//! Invoke rustc directly on the runtime crate and its vendored dependencies:
//! invoking Cargo recursively from a build script would contend for Cargo's
//! package locks.
use std::path::{Path, PathBuf};

#[path = "plenty-runtime/native/brotli_build.rs"]
mod brotli_build;
use std::process::Command;

/// The runtime's only third-party code: RustCrypto digests, HMAC and
/// constant-time comparison for `std.crypto`, zlib-rs for `std.compress`, and
/// rustls with its ring provider for `std.tls`, and SipHash for collections,
/// copied unmodified from
/// crates.io into `plenty-runtime/vendor` (tests, benchmarks and other
/// platforms' pregenerated assembly omitted). Versions match `Cargo.lock`,
/// which the Cargo build of the same crate uses; `tests/test_std_crypto.rs`
/// checks they agree.
/// Each entry: directory, crate name, edition, features, extra cfgs, externs.
/// An extern written `alias=crate` is a dependency renamed in the manifest.
type Vendored = (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
);
const VENDORED: &[Vendored] = &[
    ("siphasher-1.0.4", "siphasher", "2018", &[], &[], &[]),
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
    (
        "sha1-0.10.6",
        "sha1",
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
    // `std.tls`. libc's build script sets no cfg for x86_64 Linux GNU.
    ("libc-0.2.186", "libc", "2021", &[], &[], &[]),
    (
        "getrandom-0.2.17",
        "getrandom",
        "2018",
        &[],
        &[],
        &["cfg_if", "libc"],
    ),
    ("untrusted-0.9.0", "untrusted", "2018", &[], &[], &[]),
    ("zeroize-1.9.1", "zeroize", "2024", &["alloc"], &[], &[]),
    (
        "once_cell-1.21.4",
        "once_cell",
        "2021",
        &["alloc", "race", "std"],
        &[],
        &[],
    ),
    (
        "ring-0.17.14",
        "ring",
        "2021",
        &["alloc", "default", "dev_urandom_fallback"],
        &[],
        &["cfg_if", "getrandom", "untrusted"],
    ),
    (
        "rustls-pki-types-1.15.1",
        "rustls_pki_types",
        "2021",
        &["alloc", "default", "std"],
        &[],
        &["zeroize"],
    ),
    (
        "rustls-webpki-0.103.15",
        "webpki",
        "2021",
        &["alloc", "ring", "std"],
        &[],
        &["pki_types=rustls_pki_types", "ring", "untrusted"],
    ),
    (
        "rustls-0.23.46",
        "rustls",
        "2021",
        &["ring", "std", "tls12"],
        &[],
        &[
            "once_cell",
            "pki_types=rustls_pki_types",
            "ring",
            "subtle",
            "webpki",
            "zeroize",
        ],
    ),
];
const RUNTIME_EXTERNS: &[&str] = &[
    "sha2",
    "sha1",
    "hmac",
    "subtle",
    "zlib_rs",
    "rustls",
    "siphasher",
];

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
    brotli_build::build(Path::new("plenty-runtime/native"), &vendor);
    build_ring(&vendor);
    build_runtime(&out, &vendor, true);
    build_runtime(&out, &vendor, false);
    build_native_object(&out, TEST_HOOKS, "plenty_test_hooks.o");
    build_native_object(&out, FUZZ_DRIVER, "plenty_fuzz_driver.o");
}

/// The libFuzzer entry linked only into `plenty --fuzz` executables.
const FUZZ_DRIVER: &str = "plenty-runtime/native/fuzz_driver.c";

/// Failure injection linked only into `plenty test` executables, outside both
/// runtime archives so shipped programs are unchanged.
const TEST_HOOKS: &str = "plenty-runtime/native/test_hooks.c";

fn build_native_object(out: &Path, source: &str, object: &str) {
    println!("cargo:rerun-if-changed={source}");
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    let output = Command::new(&cc)
        .args(["-c", "-O2", "-fPIC", "-g0", "-Wall", "-Werror"])
        .args(["-ffunction-sections", "-fdata-sections"])
        .arg(format!("-ffile-prefix-map={}=/plenty", manifest.display()))
        .arg(source)
        .arg("-o")
        .arg(out.join(object))
        .output()
        .expect("invoke the C compiler for a native object");
    assert!(
        output.status.success(),
        "compiling {source} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The SQLite amalgamation behind `std.db.sqlite`, unmodified C from
/// sqlite.org. Both runtime archives bundle it. Function and data sections
/// let `--gc-sections` drop it from programs that never reach `std.db.sqlite`.
/// See book/src/design/stdlib/sqlite.md for the options and update policy.
const SQLITE: &str = "plenty-runtime/native/sqlite-3.54.0";
const SQLITE_ARCHIVE: &str = "plenty_sqlite";
const SQLITE_OPTIONS: &[&str] = &[
    "-DSQLITE_THREADSAFE=1",
    "-DSQLITE_DEFAULT_MEMSTATUS=0",
    "-DSQLITE_DQS=0",
    "-DSQLITE_LIKE_DOESNT_MATCH_BLOBS",
    "-DSQLITE_OMIT_DEPRECATED",
    "-DSQLITE_ENABLE_FTS5",
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
    let archive = dir.join(format!("lib{SQLITE_ARCHIVE}.a"));
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

/// ring's C and assembly for x86_64 Linux, as its build script selects them,
/// with the pregenerated `-elf.S` files its crates.io package ships in place
/// of the Perl generators. The archive name is the `links` value ring's
/// symbol prefix expects.
const RING: &str = "plenty-runtime/vendor/ring-0.17.14";
const RING_ARCHIVE: &str = "ring_core_0_17_14_";
const RING_SOURCES: &[&str] = &[
    "crypto/curve25519/curve25519.c",
    "crypto/fipsmodule/aes/aes_nohw.c",
    "crypto/fipsmodule/bn/montgomery.c",
    "crypto/fipsmodule/bn/montgomery_inv.c",
    "crypto/fipsmodule/ec/ecp_nistz.c",
    "crypto/fipsmodule/ec/gfp_p256.c",
    "crypto/fipsmodule/ec/gfp_p384.c",
    "crypto/fipsmodule/ec/p256.c",
    "crypto/limbs/limbs.c",
    "crypto/mem.c",
    "crypto/poly1305/poly1305.c",
    "crypto/crypto.c",
    "crypto/cpu_intel.c",
    "crypto/curve25519/curve25519_64_adx.c",
    "crypto/fipsmodule/ec/p256-nistz.c",
    "third_party/fiat/asm/fiat_curve25519_adx_mul.S",
    "third_party/fiat/asm/fiat_curve25519_adx_square.S",
    "pregenerated/chacha-x86_64-elf.S",
    "pregenerated/aes-gcm-avx2-x86_64-elf.S",
    "pregenerated/aesni-gcm-x86_64-elf.S",
    "pregenerated/aesni-x86_64-elf.S",
    "pregenerated/ghash-x86_64-elf.S",
    "pregenerated/vpaes-x86_64-elf.S",
    "pregenerated/x86_64-mont-elf.S",
    "pregenerated/x86_64-mont5-elf.S",
    "pregenerated/p256-x86_64-asm-elf.S",
    "pregenerated/sha512-x86_64-elf.S",
    "pregenerated/sha256-x86_64-elf.S",
    "pregenerated/chacha20_poly1305_x86_64-elf.S",
];

fn build_ring(dir: &Path) {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    let objects: Vec<PathBuf> = RING_SOURCES
        .iter()
        .map(|source| dir.join(format!("ring-{}.o", source.replace('/', "-"))))
        .collect();
    // A few compilers at a time: the build shares the machine with Cargo's jobs.
    for (sources, objects) in RING_SOURCES.chunks(4).zip(objects.chunks(4)) {
        let children: Vec<_> = sources
            .iter()
            .zip(objects)
            .map(|(source, object)| {
                Command::new(&cc)
                    .args(["-c", "-O2", "-fPIC", "-g0", "-w"])
                    .args(["-ffunction-sections", "-fdata-sections"])
                    .args(["-fvisibility=hidden", "-std=c1x", "-DNDEBUG"])
                    .arg(format!("-ffile-prefix-map={}=/plenty", manifest.display()))
                    .arg(format!("-I{RING}/include"))
                    .arg(format!("-I{RING}/pregenerated"))
                    .arg(format!("{RING}/{source}"))
                    .arg("-o")
                    .arg(object)
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .expect("invoke the C compiler for ring; std.tls needs `cc`")
            })
            .collect();
        for (child, source) in children.into_iter().zip(sources) {
            let output = child.wait_with_output().expect("wait for the C compiler");
            assert!(
                output.status.success(),
                "compiling ring's {source} failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    let archive = dir.join(format!("lib{RING_ARCHIVE}.a"));
    let _ = std::fs::remove_file(&archive);
    let ar = std::env::var_os("AR").unwrap_or_else(|| "ar".into());
    let output = Command::new(&ar)
        .arg("crsD")
        .arg(&archive)
        .args(&objects)
        .output()
        .expect("invoke `ar` to archive ring");
    assert!(
        output.status.success(),
        "archiving ring failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The rustc sysroot, queried once: `remap` runs for every vendored crate and
/// runtime build, and each query starts a rustc process.
fn sysroot() -> &'static str {
    static SYSROOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SYSROOT.get_or_init(|| {
        let output = Command::new(std::env::var_os("RUSTC").unwrap())
            .args(["--print", "sysroot"])
            .output()
            .expect("query rustc sysroot");
        assert!(output.status.success(), "querying rustc sysroot failed");
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    })
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
    rustc.arg(format!("--remap-path-prefix={}=/rust-toolchain", sysroot()));
    // Separate, explicit flags allow runtime sanitizer builds without silently
    // leaking compiler-only flags (coverage/link arguments/metadata) into an ABI library.
    if let Ok(flags) = std::env::var("PLENTY_RUNTIME_RUSTFLAGS") {
        rustc.args(flags.split_whitespace());
    }
}

fn extern_arg(dir: &Path, name: &str) -> String {
    let (alias, name) = name.split_once('=').unwrap_or((name, name));
    format!(
        "--extern={alias}={}",
        dir.join(format!("lib{name}.rlib")).display()
    )
}

/// The `CARGO_PKG_*` values a crate may read with `env!`; ring derives its
/// native symbol prefix from them. Directories are named `<name>-<version>`.
fn package_env(rustc: &mut Command, path: &str) {
    let (name, version) = path.rsplit_once('-').unwrap();
    let (version, pre) = version.split_once('-').unwrap_or((version, ""));
    let mut parts = version.split('.');
    rustc
        .env("CARGO_PKG_NAME", name)
        .env("CARGO_PKG_VERSION", version)
        .env("CARGO_PKG_VERSION_MAJOR", parts.next().unwrap())
        .env("CARGO_PKG_VERSION_MINOR", parts.next().unwrap())
        .env("CARGO_PKG_VERSION_PATCH", parts.next().unwrap())
        .env("CARGO_PKG_VERSION_PRE", pre);
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
        package_env(&mut rustc, path);
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

/// The one global symbol in `archive`'s symbol table named `name`, either
/// unmangled or as a v0-mangled item of the compiler's `__rustc` crate. The
/// mangled name changes with the Rust toolchain, so it is read, not written.
fn rust_symbol(archive: &Path, name: &str) -> String {
    let bytes = std::fs::read(archive).unwrap();
    assert!(
        bytes.starts_with(b"!<arch>\n") && bytes.len() >= 68,
        "runtime archive must be in GNU ar format"
    );
    let header = &bytes[8..68];
    let word = match &header[..16] {
        b"/               " => 4,
        b"/SYM64/         " => 8,
        _ => panic!("runtime archive must start with a symbol table"),
    };
    let size: usize = std::str::from_utf8(&header[48..58])
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let table = &bytes[68..68 + size];
    let count = table[..word]
        .iter()
        .fold(0usize, |n, &b| n << 8 | usize::from(b));
    let mut found: Vec<&str> = table[word * (count + 1)..]
        .split(|&b| b == 0)
        .filter_map(|symbol| std::str::from_utf8(symbol).ok())
        .filter(|symbol| {
            *symbol == name
                || (symbol.starts_with("_R")
                    && symbol.contains("7___rustc")
                    && symbol.ends_with(name))
        })
        .collect();
    found.sort_unstable();
    found.dedup();
    assert!(
        found.len() == 1,
        "runtime archive must define one `{name}`, found {found:?}"
    );
    found[0].to_string()
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
        "--edition=2024",
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
        "--print",
        "native-static-libs",
        "-o",
    ]);
    rustc.arg(&archive);
    rustc.arg(format!("-lstatic={SQLITE_ARCHIVE}"));
    rustc.arg(format!("-lstatic={RING_ARCHIVE}"));
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
    // behind its own `plenty_op_<N>` entries, stay out of programs.
    let libs = if application {
        let panic = rust_symbol(&archive, "rust_begin_unwind");
        let oom = rust_symbol(&archive, "__rust_alloc_error_handler");
        // Executables take the runtime's panic handlers instead of std's, which
        // would link the backtrace symbolizer (see plenty-runtime/src/panics.rs).
        format!(
            "-Wl,--gc-sections -Wl,--defsym={panic}=plenty_rust_panic \
             -Wl,--defsym={oom}=plenty_rust_alloc_error {libs}"
        )
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
