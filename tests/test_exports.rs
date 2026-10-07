//! Exercise generated libraries using real C/C++ and Plenty consumers.
use plenty::{LibraryKind, LibraryOptions};
use std::path::Path;
use std::process::{Command, Output};

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "status {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn build(
    root: &Path,
    source: &str,
    kind: LibraryKind,
) -> (std::path::PathBuf, plenty::LibraryArtifacts) {
    let path = root.join("source.plenty");
    std::fs::write(&path, source).unwrap();
    let output = root.join(if kind == LibraryKind::Static {
        "libcalc.a"
    } else {
        "libcalc.so"
    });
    let mut options = LibraryOptions::new("calc", kind);
    options.compile.link_args.push("-Wl,--gc-sections".into());
    let artifacts = plenty::compile_file_to_library(&path, &output, None, &options).unwrap();
    (output, artifacts)
}

const SCALARS: &str = r#"
export def add(a: i32, b: i32) -> i32 = "calc_add":
    "Add two values. Text such as */ must remain inside the comment."
    a + b
export def narrow(a: i8, b: u8, c: i16, d: u16, e: i32, f: u32, g: i64, h: u64) -> i64 = "calc_narrow":
    i64(a) + i64(b) + i64(c) + i64(d) + i64(e) + i64(f) + g + i64(h)
export def single(x: f32) -> f32 = "calc_single":
    x * 2.0
export def double(x: f64) -> f64 = "calc_double":
    x / 2.0
export def empty() -> () = "calc_empty":
    pass
pub def hidden() -> i32:
    99
"#;

#[test]
fn static_and_shared_exports_work_from_c_cpp_and_plenty() {
    for kind in [LibraryKind::Static, LibraryKind::Shared] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let (library, artifacts) = build(root, SCALARS, kind);
        let header = std::fs::read_to_string(&artifacts.header).unwrap();
        assert!(header.contains("no ownership crosses"));
        assert!(header.contains("* / must remain inside"));
        let interface = std::fs::read(&artifacts.interface).unwrap();
        assert!(std::fs::read(&library)
            .unwrap()
            .windows(interface.len())
            .any(|w| w == interface));
        if kind == LibraryKind::Shared {
            let symbols = success(
                Command::new("nm")
                    .args(["-D", "--defined-only"])
                    .arg(&library)
                    .output()
                    .unwrap(),
            );
            let symbols = String::from_utf8(symbols.stdout).unwrap();
            assert!(
                !symbols.contains(" plenty_")
                    && !symbols.contains(" main")
                    && !symbols.contains(" hidden"),
                "{symbols}"
            );
            success(
                Command::new("strip")
                    .arg("--strip-all")
                    .arg(&library)
                    .output()
                    .unwrap(),
            );
            assert!(std::fs::read(&library)
                .unwrap()
                .windows(interface.len())
                .any(|w| w == interface));
        }
        let c = root.join("caller.c");
        std::fs::write(&c, r##"
#include "calc.h"
#include <assert.h>
#include <string.h>
int main(void) {
    assert(calc_add(20, 22) == 42);
    assert(calc_narrow(-5, 250, -300, 60000, -70000, 80000, -90000, 100000) == 79945);
    assert(calc_single(1.25f) == 2.5f);
    assert(calc_double(7.0) == 3.5);
    calc_empty();
    size_t length = 0;
    const uint8_t *contract = calc_plenty_interface_v1(&length);
    assert(length > 100);
    assert(memcmp(contract, "# plenty-interface-format: 1\n", sizeof("# plenty-interface-format: 1\n") - 1) == 0);
    return 0;
}
"##).unwrap();
        let args = std::fs::read_to_string(artifacts.link_args).unwrap();
        for compiler in ["cc", "c++"] {
            let app = root.join("caller");
            success(
                Command::new(compiler)
                    .args(["-Wall", "-Wextra", "-Werror"])
                    .arg(&c)
                    .arg(&library)
                    .args(args.lines())
                    .arg("-o")
                    .arg(&app)
                    .output()
                    .unwrap(),
            );
            success(Command::new(app).output().unwrap());
        }
        let app = root.join("main.plenty");
        std::fs::write(
            &app,
            "import calc\ndef main() -> i32:\n    calc.add(20, 22)\n",
        )
        .unwrap();
        let executable = root.join("plenty-caller");
        let options = plenty::CompileOptions {
            link_args: vec![library.into_os_string(), "-Wl,--gc-sections".into()],
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&app, &executable, None, &options).unwrap();
        if kind == LibraryKind::Static {
            // This caller never invokes discovery. The retained section still
            // survives archive extraction and native section garbage collection.
            assert!(std::fs::read(&executable)
                .unwrap()
                .windows(interface.len())
                .any(|w| w == interface));
        }
        assert_eq!(Command::new(executable).status().unwrap().code(), Some(42));
    }
}

const BORROWS: &str = r#"
export def update(a: &mut i8, b: &mut u8, c: &mut i16, d: &mut u16, e: &mut i32, f: &mut u32, g: &mut i64, h: &mut u64, x: &mut f32, y: &mut f64) -> () = "calc_update":
    *a = -12
    *b = 254
    *c = -30000
    *d = 60000
    *e = -2000000000
    *f = 4000000000
    *g = -9000000000
    *h = 18000000000
    *x = *x * 2.0
    *y = *y / 2.0
export def shared(a: &i8, b: &i8) -> i8 = "calc_shared":
    *a + *b
export def increment(a: &mut i8) -> i8 = "calc_increment":
    *a = *a + 1
    *a
"#;

#[test]
fn scalar_borrows_use_exact_c_storage_and_preserve_contracts() {
    for kind in [LibraryKind::Static, LibraryKind::Shared] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let (library, artifacts) = build(root, BORROWS, kind);
        let header = std::fs::read_to_string(&artifacts.header).unwrap();
        for contract in [
            "non-null, aligned, initialized",
            "borrowed exclusively",
            "must not overlap",
            "borrowed read-only",
            "never retained",
            "final value is written back",
        ] {
            assert!(header.contains(contract), "missing {contract}");
        }
        assert!(header.contains("const int8_t * p0"));
        let c = root.join("borrow.c");
        std::fs::write(&c, r#"
#include "calc.h"
#include <assert.h>
#include <sys/mman.h>
#include <unistd.h>
/* Put each argument against a guard page to catch even read-only overreads. */
static void *scalar(size_t size) {
    long page = sysconf(_SC_PAGESIZE);
    assert(page > 0);
    void *base = mmap(0, (size_t)page * 2, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    assert(base != MAP_FAILED);
    assert(mprotect((char *)base + page, (size_t)page, PROT_NONE) == 0);
    return (char *)base + page - size;
}
int main(void) {
    int8_t *a = scalar(sizeof(*a)); *a = -3;
    uint8_t *b = scalar(sizeof(*b)); *b = 3;
    int16_t *c = scalar(sizeof(*c)); *c = -3;
    uint16_t *d = scalar(sizeof(*d)); *d = 3;
    int32_t *e = scalar(sizeof(*e)); *e = -3;
    uint32_t *f = scalar(sizeof(*f)); *f = 3;
    int64_t *g = scalar(sizeof(*g)); *g = -3;
    uint64_t *h = scalar(sizeof(*h)); *h = 3;
    float *x = scalar(sizeof(*x)); *x = 1.25f;
    double *y = scalar(sizeof(*y)); *y = 7.0;
    assert(calc_shared(a, a) == -6);
    assert(*a == -3);
    assert(calc_increment(a) == -2 && *a == -2);
    calc_update(a, b, c, d, e, f, g, h, x, y);
    assert(*a == -12 && *b == 254 && *c == -30000 && *d == 60000);
    assert(*e == -2000000000 && *f == UINT32_C(4000000000));
    assert(*g == -INT64_C(9000000000) && *h == UINT64_C(18000000000));
    assert(*x == 2.5f && *y == 3.5);
    return 0;
}
"#).unwrap();
        let args = std::fs::read_to_string(artifacts.link_args).unwrap();
        let app = root.join("borrow");
        success(
            Command::new("cc")
                .args(["-Wall", "-Wextra", "-Werror"])
                .arg(&c)
                .arg(&library)
                .args(args.lines())
                .arg("-o")
                .arg(&app)
                .output()
                .unwrap(),
        );
        success(Command::new(app).output().unwrap());
        let source = root.join("main.plenty");
        std::fs::write(&source, "import calc\ndef main() -> i32:\n    mut value = 40i8\n    calc.increment(&mut value)\n    i32(calc.increment(&mut value))\n").unwrap();
        let executable = root.join("plenty-borrows");
        let options = plenty::CompileOptions {
            link_args: vec![library.into_os_string()],
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&source, &executable, None, &options)
            .unwrap();
        assert_eq!(Command::new(executable).status().unwrap().code(), Some(42));
        std::fs::write(&source, "import calc\ndef main() -> ():\n    mut value = 40i8\n    loan = &value\n    calc.increment(&mut value)\n    calc.shared(loan, loan)\n    pass\n").unwrap();
        let error = plenty::check_file(&source, None).unwrap_err().to_string();
        assert!(error.contains("borrow"), "{error}");
    }
}

#[test]
fn export_diagnostics_reject_unsupported_and_ambiguous_interfaces() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.plenty");
    for (source, expected) in [
        ("export def bad[T](x: T) -> T = \"calc_bad\":\n    x\n", "cannot be generic"),
        ("export def bad(x: str) -> str = \"calc_bad\":\n    x\n", "numeric scalar"),
        ("export def bad() -> bool = \"calc_bad\":\n    True\n", "numeric scalar"),
        ("export def a() -> () = \"calc_same\":\n    pass\nexport def b() -> () = \"calc_same\":\n    pass\n", "duplicate or imported"),
    ] {
        std::fs::write(&path, source).unwrap();
        let error = plenty::check_module_file(&path, None).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
    std::fs::write(
        &path,
        "export def good() -> i32 = \"wrong_prefix\":\n    1\n",
    )
    .unwrap();
    let output = temp.path().join("out.a");
    std::fs::write(&output, "previous output").unwrap();
    let error = plenty::compile_file_to_library(
        &path,
        &output,
        None,
        &LibraryOptions::new("calc", LibraryKind::Static),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("must start with"), "{error}");
    assert_eq!(std::fs::read_to_string(output).unwrap(), "previous output");
}

#[test]
fn cli_builds_libraries_without_an_application_entrypoint() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.plenty");
    std::fs::write(
        &source,
        "export def value() -> i32 = \"calc_value\":\n    42\n",
    )
    .unwrap();
    for mode in ["--static-library", "--shared-library"] {
        let output = temp.path().join("artifact");
        success(
            Command::new(env!("CARGO_BIN_EXE_plenty"))
                .arg(mode)
                .arg(&source)
                .args(["--library-name", "calc", "-o"])
                .arg(&output)
                .output()
                .unwrap(),
        );
        assert!(output.is_file());
        assert!(temp.path().join("calc.h").is_file());
        assert!(temp.path().join("calc.plentyi").is_file());
    }
    let result = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg("--shared-library")
        .arg(&source)
        .arg("-o")
        .arg(temp.path().join("bad"))
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("requires --library-name"));
}
