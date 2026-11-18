//! Application C adapters retain the library ABI without a library/host entry.
#![cfg(target_os = "linux")]
use plenty::CompileOptions;
use std::{
    path::Path,
    process::{Command, Output},
};

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "{}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

const APP: &str = r#"
import native
class State:
    value: i32
    def __del__(self) -> ():
        drop(print("DESTROYED"))
enum Error:
    Invalid
export def create(fail: bool) -> Result[State, AllocError] = "proof_create":
    if fail:
        return Err(AllocError.CapacityOverflow)
    Ok(State(4i32))
export def step(state: &mut State, fail: bool) -> Result[i32, Error] = "proof_step":
    state.value = 9i32
    if fail:
        return Err(Error.Invalid)
    Ok(state.value)
export def consume(state: State) -> i32 = "proof_consume":
    state.value
export def scalar(value: &mut i32) -> Result[bool, Error] = "proof_scalar":
    *value = 42i32
    Ok(True)
export def divide(value: i32) -> i32 = "proof_divide":
    unchecked:
        100i32 // value
export def unused() -> u32 = "proof_unused":
    1u32
pub def hidden() -> i32:
    123i32
def main() -> i32:
    native.drive(native.callback())
"#;
const C: &str = r#"
#include "proof.h"
#include <assert.h>
int32_t drive(void *callback) {
    assert(callback == (void *)proof_step);
    proof_State *state = NULL;
    uint32_t error = 99;
    assert(proof_create(true, &state, &error) == 1 && error == 1 && state == NULL);
    assert(proof_create(false, &state, &error) == 0 && state != NULL);
    int32_t value = -1;
    assert(proof_step(state, true, &value, &error) == 1);
    assert(error == proof_Error_Invalid && value == -1);
    assert(proof_step(state, false, &value, &error) == 0 && value == 9);
    assert(proof_consume(state) == 9); /* consumed, including its original destructor */
    state = NULL;
    assert(proof_create(false, &state, &error) == 0);
    proof_State_destroy(state);
    proof_State_destroy(NULL);
    bool ok = false;
    assert(proof_scalar(&value, &ok, &error) == 0 && ok && value == 42);
    return 0;
}
"#;
fn fixture(root: &Path) -> std::path::PathBuf {
    std::fs::write(root.join("native.plentyi"), "pub opaque Pointer\npub extern def callback() -> Pointer = address \"proof_step\"\npub extern def drive(callback: Pointer) -> i32 = \"drive\"\n").unwrap();
    let source = root.join("app.plenty");
    std::fs::write(&source, APP).unwrap();
    source
}

#[test]
fn executable_exports_callbacks_ownership_errors_visibility_and_gc() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = fixture(root);
    std::fs::write(root.join("drive.c"), C).unwrap();
    for optimize in [false, true] {
        let options = CompileOptions {
            executable_exports: Some("proof".into()),
            optimize,
            strip: false,
            ..Default::default()
        };
        let object = root.join("app.o");
        plenty::compile_file_to_object_with_options(&source, &object, None, &options).unwrap();
        let symbols = success(
            Command::new("readelf")
                .args(["-Ws"])
                .arg(&object)
                .output()
                .unwrap(),
        );
        let symbols = String::from_utf8(symbols.stdout).unwrap();
        assert!(
            symbols
                .lines()
                .any(|line| line.contains("GLOBAL PROTECTED") && line.ends_with("proof_step")),
            "{symbols}"
        );
        success(
            Command::new("cc")
                .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-c"])
                .arg(root.join("drive.c"))
                .arg("-o")
                .arg(root.join("drive.o"))
                .output()
                .unwrap(),
        );
        let executable = root.join("app");
        let options = CompileOptions {
            link_args: vec![root.join("drive.o").into_os_string()],
            ..options
        };
        plenty::compile_file_to_executable_with_options(&source, &executable, None, &options)
            .unwrap();
        let output = success(Command::new(&executable).output().unwrap());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "DESTROYED\nDESTROYED\n"
        );
        let symbols = success(Command::new("nm").arg(&executable).output().unwrap());
        let symbols = String::from_utf8(symbols.stdout).unwrap();
        assert!(
            !symbols.lines().any(|line| line.ends_with("proof_unused")),
            "unreferenced adapter must be stripped"
        );
        assert!(!symbols.lines().any(|line| line.ends_with(" hidden")));
    }
}

/// Even checked private bodies and destructor/type-metadata edges must vanish
/// with unused C adapters. The native dependency deliberately has no definition.
#[test]
fn executable_exports_strip_unused_checked_callbacks_and_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::write(
        root.join("native.plentyi"),
        "pub extern def unavailable() -> i32 = \"unused_widget_dependency\"\n",
    )
    .unwrap();
    let source = root.join("unused_checked.plenty");
    std::fs::write(
        &source,
        r#"import native
class Unused:
    value: i32
    def __del__(self) -> ():
        unchecked:
            drop(native.unavailable() // self.value)
export def unused(value: i32) -> i32 = "proof_unused_checked":
    unchecked:
        native.unavailable() // value
export def create_unused() -> Result[Unused, AllocError] = "proof_create_unused":
    Ok(Unused(1i32))
def main() -> i32:
    0i32
"#,
    )
    .unwrap();
    let unused_symbols = [
        "proof_unused_checked",
        "__plenty_fn_unused",
        "proof_create_unused",
        "proof_Unused_destroy",
        "__plenty_fn___plenty_class_Unused.__del__",
        "__plenty_drop___plenty_class_Unused.__del__",
    ];
    for optimize in [false, true] {
        let options = CompileOptions {
            executable_exports: Some("proof".into()),
            optimize,
            strip: false,
            ..Default::default()
        };
        let object = root.join("unused_checked.o");
        plenty::compile_file_to_object_with_options(&source, &object, None, &options).unwrap();
        let symbols = success(Command::new("nm").arg(&object).output().unwrap());
        let symbols = String::from_utf8(symbols.stdout).unwrap();
        for unused in unused_symbols {
            assert!(
                symbols.contains(unused),
                "missing GC witness {unused}: {symbols}"
            );
        }
        let executable = root.join("unused_checked");
        plenty::compile_file_to_executable_with_options(&source, &executable, None, &options)
            .unwrap();
        success(Command::new(&executable).output().unwrap());
        let symbols = success(
            Command::new("nm")
                .arg("--defined-only")
                .arg(&executable)
                .output()
                .unwrap(),
        );
        let symbols = String::from_utf8(symbols.stdout).unwrap();
        for unused in unused_symbols {
            assert!(!symbols.contains(unused), "retained {unused}: {symbols}");
        }
        // Linkers may leave unused undefined entries in .symtab. No live
        // relocation may still depend on the unavailable native implementation.
        let relocations = success(
            Command::new("readelf")
                .arg("-rW")
                .arg(&executable)
                .output()
                .unwrap(),
        );
        let relocations = String::from_utf8(relocations.stdout).unwrap();
        assert!(
            !relocations.contains("unused_widget_dependency"),
            "{relocations}"
        );
    }
}

#[test]
fn executable_exports_live_native_callbacks_and_destructor_checks_report_source() {
    const SOURCE: &str = r#"import native
class State:
    value: i32
    def __del__(self) -> ():
        unchecked:
            drop(100i32 // self.value)
export def create(value: i32) -> Result[State, AllocError] = "proof_create":
    Ok(State(value))
export def checked(value: i32) -> i32 = "proof_checked":
    unchecked:
        100i32 // value
def main() -> i32:
    native.drive()
"#;
    const DRIVER: &str = r#"
#include "proof.h"
#include <assert.h>
int32_t drive(void) {
    assert(proof_checked(4) == 25);
    proof_State *state = NULL;
    uint32_t error = 99;
    assert(proof_create(4, &state, &error) == 0 && state != NULL);
    proof_State_destroy(state);
#if DESTRUCTOR
    state = NULL;
    assert(proof_create(0, &state, &error) == 0 && state != NULL);
    proof_State_destroy(state); /* follows the originating type-metadata edge */
#else
    (void)proof_checked(0); /* an actual native relocation retains this body */
#endif
    return 99; /* the failed check must not return */
}
"#;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = root.join("live_checks.plenty");
    std::fs::write(&source, SOURCE).unwrap();
    std::fs::write(
        root.join("native.plentyi"),
        "pub extern def drive() -> i32 = \"drive\"\n",
    )
    .unwrap();
    std::fs::write(root.join("drive.c"), DRIVER).unwrap();
    for optimize in [false, true] {
        let options = CompileOptions {
            executable_exports: Some("proof".into()),
            optimize,
            strip: false,
            ..Default::default()
        };
        plenty::compile_file_to_object_with_options(&source, &root.join("app.o"), None, &options)
            .unwrap();
        for destructor in [false, true] {
            success(
                Command::new("cc")
                    .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-c"])
                    .arg(format!("-DDESTRUCTOR={}", u8::from(destructor)))
                    .arg(root.join("drive.c"))
                    .arg("-o")
                    .arg(root.join("drive.o"))
                    .output()
                    .unwrap(),
            );
            let executable = root.join("live_checks");
            plenty::compile_file_to_executable_with_options(
                &source,
                &executable,
                None,
                &CompileOptions {
                    link_args: vec![root.join("drive.o").into_os_string()],
                    ..options.clone()
                },
            )
            .unwrap();
            let output = Command::new(&executable).output().unwrap();
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert_eq!(output.status.code(), Some(1), "{stderr}");
            assert!(stderr.contains("division by zero"), "{stderr}");
            let check = if destructor {
                "drop(100i32 // self.value)"
            } else {
                "100i32 // value"
            };
            let line = SOURCE
                .lines()
                .position(|line| line.contains(check))
                .unwrap()
                + 1;
            assert!(
                stderr.contains(&format!("live_checks.plenty:{line}:")),
                "wrong source diagnostic: {stderr}"
            );
        }
    }
}

#[test]
fn executable_exports_validate_modes_names_abi_and_publication() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = fixture(root);
    let object = root.join("app.o");
    let options = CompileOptions {
        executable_exports: Some("proof".into()),
        ..Default::default()
    };
    let error =
        plenty::compile_file_to_object_with_options(&source, &root.join("proof.h"), None, &options)
            .unwrap_err()
            .to_string();
    assert!(error.contains("conflicts"), "{error}");
    std::fs::write(root.join("proof.h"), "old header").unwrap();
    // On a case-insensitive filesystem these two paths already name one file.
    let alias = root.join("Proof.H");
    let old_alias = std::fs::read(&alias).ok();
    let error = plenty::compile_file_to_object_with_options(&source, &alias, None, &options)
        .unwrap_err()
        .to_string();
    assert!(error.contains("conflicts"), "{error}");
    assert_eq!(std::fs::read(&alias).ok(), old_alias);
    assert_eq!(std::fs::read(root.join("proof.h")).unwrap(), b"old header");
    let invalid = CompileOptions {
        executable_exports: Some("bad-name".into()),
        ..options.clone()
    };
    assert!(plenty::compile_file_to_object_with_options(&source, &object, None, &invalid).is_err());
    let cpu = CompileOptions {
        cpu: plenty::CpuLevel::X86_64V3,
        ..options.clone()
    };
    assert!(
        plenty::compile_file_to_object_with_options(&source, &object, None, &cpu)
            .unwrap_err()
            .to_string()
            .contains("baseline")
    );
    for declaration in [
        "export def bad(x: list[i32]) -> () = \"proof_bad\":\n    pass\n",
        "export def bad() -> i32 = exact \"main\":\n    0i32\n",
        "export def bad() -> i32 = exact \"plenty_runtime_start\":\n    0i32\n",
        "export def bad() -> i32 = \"other_bad\":\n    0i32\n",
    ] {
        std::fs::write(
            &source,
            format!("{declaration}\ndef main() -> ():\n    pass\n"),
        )
        .unwrap();
        assert!(
            plenty::compile_file_to_object_with_options(&source, &object, None, &options).is_err()
        );
        assert_eq!(
            std::fs::read_to_string(root.join("proof.h")).unwrap(),
            "old header"
        );
        assert!(!object.exists());
    }
    for symbol in ["main", "plenty_runtime_start"] {
        std::fs::write(root.join("raw.plentyi"), format!(
            "pub opaque Pointer\npub extern def descriptor() -> Pointer = object \"{symbol}\"(size=8, align=8):\n    0: u64 = 0\n"
        )).unwrap();
        std::fs::write(&source,
            "import raw\nexport def descriptor() -> raw.Pointer = \"proof_descriptor\":\n    raw.descriptor()\ndef main() -> ():\n    pass\n"
        ).unwrap();
        let error = plenty::compile_file_to_object_with_options(&source, &object, None, &options)
            .unwrap_err()
            .to_string();
        assert!(error.contains("namespace"), "{error}");
    }
    // Header publication must not overwrite an input, including imported interfaces.
    std::fs::write(root.join("proof.h"), "source sentinel").unwrap();
    std::fs::write(&source, APP).unwrap();
    let same_source = CompileOptions {
        executable_exports: Some("proof".into()),
        ..Default::default()
    };
    assert!(
        plenty::compile_file_to_object_with_options(&source, &source, None, &same_source).is_err()
    );
    assert_eq!(std::fs::read_to_string(&source).unwrap(), APP);
    // Link failure occurs after successful emission, but neither destination
    // should change. The app intentionally has no native drive implementation.
    let executable = root.join("app");
    std::fs::write(&executable, "old executable").unwrap();
    let error =
        plenty::compile_file_to_executable_with_options(&source, &executable, None, &options)
            .unwrap_err();
    assert!(error.to_string().contains("drive"), "{error}");
    assert_eq!(
        std::fs::read_to_string(&executable).unwrap(),
        "old executable"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("proof.h")).unwrap(),
        "source sentinel"
    );
    for mode in ["--check", "--shared-library", "--emit-runtime", "--help"] {
        let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
            .args(["--executable-exports", "proof", mode])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("requires --emit-object or --compile")
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args([
            "--executable-exports",
            "proof",
            "--executable-exports",
            "proof",
            "--emit-object",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn executable_exports_link_one_cargo_runtime_archive_without_gui_dependencies() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = fixture(root);
    let object = root.join("app.o");
    std::fs::write(root.join("drive.c"), C).unwrap();
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("plenty-runtime");
    std::fs::write(
        root.join("Cargo.toml"),
        format!(
            r#"
[package]
name = "native-support-proof"
version = "0.0.0"
edition = "2024"
[workspace]
[lib]
path = "support.rs"
crate-type = ["staticlib"]
[dependencies]
plenty-runtime = {{ path = {runtime:?}, features = ["executable"] }}
"#
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("support.rs"),
        r#"
unsafe extern "C" {
    fn plenty_actor_heap(op: i64, argument: i64) -> i64;
    fn plenty_actor_run(heap: i64, storage: *mut u8,
        entry: unsafe extern "C" fn(*mut u8) -> *mut u8) -> i64;
    fn proof_divide(value: i32) -> i32;
}
unsafe extern "C" fn allocated(storage: *mut u8) -> *mut u8 {
    let mut bytes = Vec::<u8>::new();
    bytes.try_reserve_exact(4096).unwrap();
    assert!(unsafe { plenty_actor_heap(3, 0) } >= 4096);
    drop(bytes);
    storage
}
unsafe extern "C" fn trapped(storage: *mut u8) -> *mut u8 {
    unsafe { proof_divide(0); }
    storage
}
#[unsafe(no_mangle)]
unsafe extern "C" fn main(argc: i32, argv: *const *const u8) -> i32 {
    // Test actual executable-feature dispatch/router, not Cargo's cfg(test) library.
    unsafe {
        // Enter ordinary Plenty startup before exercising native callbacks.
        let status = plenty_runtime::main(argc, argv);
        assert_eq!(status, 0);
        assert_eq!(plenty_actor_heap(7, 0), 0); // PREPARE
        let heap = plenty_actor_heap(0, 65536); // CREATE
        assert!(heap >= 0);
        assert_eq!(plenty_actor_run(heap, std::ptr::null_mut(), allocated), 0);
        assert_eq!(plenty_actor_run(heap, std::ptr::null_mut(), trapped), 3);
        assert_eq!(plenty_actor_heap(8, 0), 3); // Arithmetic trap contained
        assert_eq!(plenty_actor_heap(1, heap), 0); // DISCARD
        assert_eq!(plenty_actor_heap(5, 0), 0); // LIVE_COUNT
        status
    }
}
"#,
    )
    .unwrap();
    let cargo = |args: &[&str]| {
        let mut command = Command::new("cargo");
        command
            .args(args)
            .arg("--manifest-path")
            .arg(root.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", root.join("cargo-target"))
            .env_remove("CARGO_BUILD_TARGET");
        command
    };
    success(cargo(&["generate-lockfile", "--offline"]).output().unwrap());
    let tree = success(
        cargo(&[
            "tree",
            "--locked",
            "--offline",
            "--color",
            "never",
            "--edges",
            "normal",
        ])
        .output()
        .unwrap(),
    );
    let tree = String::from_utf8(tree.stdout).unwrap();
    assert!(
        !tree.contains("gpui v") && !tree.contains("xkbcommon"),
        "{tree}"
    );
    // This pin was reviewed for APP, not derived from the object being linked.
    // Ownership is in the target-specific contract, not the C prototype.
    const GUARD: &str =
        "proof_plenty_contract_v1_a03115ebc69e215339e8dfb0d970769cc8e0915c75866584309a1550c0cd7804";
    let qualified = plenty::native_target() == "x86_64-unknown-linux-gnu";
    let manifest = root.join("Cargo.toml");
    let driver = root.join("drive.o");
    let executable = root.join("unified");
    let link = || {
        let mut command = Command::new("python3");
        command
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/link_native_executable.py"))
            .arg("--manifest-path")
            .arg(&manifest)
            .args(["--lib-name", "native_support_proof"])
            .arg("--target-dir")
            .arg(root.join("cargo-target"))
            .env_remove("CARGO_BUILD_TARGET")
            .env("CARGO_NET_OFFLINE", "true")
            .env("CARGO_TERM_COLOR", "always");
        if qualified {
            command.args(["--require-contract", GUARD]);
        }
        command
            .arg(&object)
            .arg(&driver)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap()
    };
    let emit = |optimize: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_plenty"));
        command
            .arg("--emit-object")
            .arg(&source)
            .args(["--executable-exports", "proof"]);
        if !optimize {
            command.arg("--fast-compile");
        }
        success(command.arg("-o").arg(&object).output().unwrap());
        success(
            Command::new("cc")
                .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-c"])
                .arg(root.join("drive.c"))
                .arg("-o")
                .arg(&driver)
                .output()
                .unwrap(),
        );
        let header = std::fs::read_to_string(root.join("proof.h")).unwrap();
        assert!(header.contains(
            "uint32_t proof_step(proof_State * p0, bool p1, int32_t *out_ok, uint32_t *out_error)"
        ));
    };
    for optimize in [false, true] {
        std::fs::write(&source, APP).unwrap();
        emit(optimize);
        success(link());
        let output = success(Command::new(&executable).output().unwrap());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "DESTROYED\nDESTROYED\n"
        );
        let needed = success(
            Command::new("readelf")
                .args(["-d"])
                .arg(&executable)
                .output()
                .unwrap(),
        );
        let needed = String::from_utf8(needed.stdout).unwrap();
        assert!(
            !needed.contains("gpui") && !needed.contains("xcb"),
            "{needed}"
        );
        if qualified {
            for mismatch in [
                APP.replace("step(state: &mut State", "step(state: State")
                    .replace("    state.value = 9i32\n", ""),
                format!("{APP}\nexport def extra() -> u32 = \"proof_extra\":\n    0u32\n"),
            ] {
                // Rebuild the C driver against the new matching header. A successful
                // compile cannot prove that this caller still owns the handle.
                std::fs::write(&source, mismatch).unwrap();
                emit(optimize);
                let failed = link();
                assert!(!failed.status.success(), "linked incompatible contract");
                assert!(
                    String::from_utf8_lossy(&failed.stderr).contains(GUARD),
                    "{}",
                    String::from_utf8_lossy(&failed.stderr)
                );
            }
        }
    }
}

/// The final link, not the visibility of the adapter, controls whether its
/// checked body needs the optional engine (#1000).
#[test]
fn executable_exports_engine_roots_include_native_objects_and_preserve_opaque_inputs() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let source = root.join("engine.plenty");
    let output = root.join("app");
    let engine = root.join("libplenty_dataframe_v1.so");
    let missing = root.join("missing.so");
    std::fs::write(
        root.join("engine.c"),
        "int pldf_v1_fixture(void) { return 42; }",
    )
    .unwrap();
    success(
        Command::new("cc")
            .args(["-shared", "-fPIC", "-Wl,-soname,libplenty_dataframe_v1.so"])
            .arg(root.join("engine.c"))
            .arg("-o")
            .arg(&engine)
            .output()
            .unwrap(),
    );
    std::fs::write(root.join("raw.plentyi"), "pub extern def engine() -> i32 = \"pldf_v1_fixture\"\npub extern def drive() -> i32 = \"drive\"\n").unwrap();
    let app = |expr| {
        std::fs::write(&source, format!("import raw\nexport def check(x: i32) -> i32 = \"proof_check\":\n    unchecked:\n        raw.engine() // x\ndef main() -> i32:\n    {expr}\n")).unwrap()
    };
    let object = root.join("drive.o");
    let driver = |body| {
        std::fs::write(root.join("drive.c"), format!("#include \"proof.h\"\nextern int pldf_v1_fixture(void);\nint drive(void) {{ return {body}; }}")).unwrap();
        success(
            Command::new("cc")
                .arg("-c")
                .arg(root.join("drive.c"))
                .arg("-o")
                .arg(&object)
                .output()
                .unwrap(),
        );
    };
    let needs_engine = || {
        let elf = success(
            Command::new("readelf")
                .arg("-d")
                .arg(&output)
                .output()
                .unwrap(),
        );
        String::from_utf8(elf.stdout)
            .unwrap()
            .lines()
            .any(|l| l.contains("NEEDED") && l.contains("libplenty_dataframe_v1.so"))
    };
    for optimize in [false, true] {
        let compile = |selected: &Path, args: &[std::ffi::OsString]| {
            plenty::compile_file_to_executable_with_options(
                &source,
                &output,
                None,
                &CompileOptions {
                    executable_exports: Some("proof".into()),
                    dataframe_engine: Some(selected.into()),
                    link_args: args.to_vec(),
                    optimize,
                    ..Default::default()
                },
            )
        };
        app("0i32");
        for selected in [&engine, &missing] {
            compile(selected, &[]).unwrap();
            assert!(!needs_engine());
            success(Command::new(&output).output().unwrap());
        }
        app("raw.drive()");
        driver("0");
        let object_arg = [object.clone().into_os_string()];
        compile(&missing, &object_arg).unwrap();
        assert!(!needs_engine());
        // With identical Plenty code, changing only the native caller now
        // retains the callback. A direct engine import is also a possible root.
        for body in ["proof_check(2)", "pldf_v1_fixture() / 2"] {
            driver(body);
            assert!(
                compile(&missing, &object_arg)
                    .unwrap_err()
                    .to_string()
                    .contains("std.dataframe requires")
            );
            compile(&engine, &object_arg).unwrap();
            assert!(needs_engine());
            assert_eq!(Command::new(&output).status().unwrap().code(), Some(21));
        }
        // No engine symbols in the Plenty object at all. The native object
        // alone must still select its engine (also with an opaque driver flag).
        std::fs::write(&source, "import raw\nexport def check(x: i32) -> i32 = \"proof_check\":\n    x\ndef main() -> i32:\n    raw.drive()\n").unwrap();
        compile(
            &engine,
            &[object_arg[0].clone(), "-Wl,--gc-sections".into()],
        )
        .unwrap();
        assert!(needs_engine());
        assert_eq!(Command::new(&output).status().unwrap().code(), Some(21));
        app("check(2i32)");
        assert!(
            compile(&missing, &[])
                .unwrap_err()
                .to_string()
                .contains("std.dataframe requires")
        );
        compile(&engine, &[]).unwrap();
        assert!(needs_engine());
        assert_eq!(Command::new(&output).status().unwrap().code(), Some(21));
        app("0i32");
        // -u can retain an adapter without an undefined reference in any .o.
        assert!(
            compile(&missing, &["-Wl,-u,proof_check".into()])
                .unwrap_err()
                .to_string()
                .contains("std.dataframe requires")
        );
        compile(&engine, &["-Wl,-u,proof_check".into()]).unwrap();
        assert!(needs_engine());
    }
}
