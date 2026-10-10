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
fn recursive_owners_keep_the_opaque_c_handle_and_generated_wrapper_contract() {
    let source = r#"
class Node:
    value: i64
    next: Option[Box[Node]]
    def __del__(self) -> ():
        print(self.value).unwrap()
export def create(value: i64) -> Result[Node, AllocError] = "calc_create":
    child = Node(value, Nothing)
    Ok(Node(value + 1, Some(Box(child)?)))
export def read(owner: &Node) -> i64 = "calc_read":
    owner.value
"#;
    for kind in [LibraryKind::Static, LibraryKind::Shared] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let (library, artifacts) = build(root, source, kind);
        let header = std::fs::read_to_string(&artifacts.header).unwrap();
        assert!(header.contains("const calc_Node * p0"));
        let c = root.join("recursive.c");
        std::fs::write(
            &c,
            r#"
#include "calc.h"
#include <assert.h>
int main(void) {
    calc_Node *owner = 0;
    uint32_t error = 99;
    assert(calc_create(41, &owner, &error) == 0);
    assert(calc_read(owner) == 42);
    calc_Node_destroy(owner);
    return 0;
}
"#,
        )
        .unwrap();
        let executable = root.join("caller");
        let args = std::fs::read_to_string(artifacts.link_args).unwrap();
        success(
            Command::new("cc")
                .args(["-Wall", "-Wextra", "-Werror"])
                .arg(&c)
                .arg(&library)
                .args(args.lines())
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap(),
        );
        assert_eq!(
            success(Command::new(&executable).output().unwrap()).stdout,
            b"42\n41\n"
        );
        let app = root.join("main.plenty");
        std::fs::write(&app, "import calc\ndef main() -> Result[(), Failure]:\n    owner = calc.create(41)?\n    print(calc.read(&owner))?\n    Ok(())\n").unwrap();
        let options = plenty::CompileOptions {
            link_args: vec![library.into_os_string()],
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&app, &executable, None, &options).unwrap();
        assert_eq!(
            success(Command::new(&executable).output().unwrap()).stdout,
            b"42\n42\n41\n"
        );
    }
}

#[test]
fn static_and_shared_exports_work_from_c_cpp_and_plenty() {
    for kind in [LibraryKind::Static, LibraryKind::Shared] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let (library, artifacts) = build(root, SCALARS, kind);
        let header = std::fs::read_to_string(&artifacts.header).unwrap();
        assert!(header.contains("No ownership crosses"));
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
fn result_exports_preserve_payloads_and_inactive_outputs() {
    let source = r#"
export def checked(x: i32) -> Result[f32, i16] = "calc_checked":
    if x < 0:
        Err(-123)
    else:
        Ok(1.25)
export def touch(x: &mut i32) -> Result[(), u64] = "calc_touch":
    *x = *x + 1
    Err(18000000000)
export def allocation(x: i32) -> Result[(), AllocError] = "calc_allocation":
    if x == 0:
        Ok(())
    elif x == 1:
        Err(AllocError.OutOfMemory)
    else:
        Err(AllocError.CapacityOverflow)
export def parse() -> Result[i64, ParseError] = "calc_parse":
    Err(ParseError.OutOfRange)
export def failed() -> Result[(), Failure] = "calc_failed":
    Err(Failure.Unspecified)
"#;
    for kind in [LibraryKind::Static, LibraryKind::Shared] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let (library, artifacts) = build(root, source, kind);
        let c = root.join("result.c");
        std::fs::write(
            &c,
            r#"
#include "calc.h"
#include <assert.h>
int main(void) {
    float value = 8.0f;
    int16_t error = 90;
    assert(calc_checked(0, &value, &error) == 0);
    assert(value == 1.25f && error == 90);
    assert(calc_checked(-1, &value, &error) == 1);
    assert(value == 1.25f && error == -123);
    int32_t x = 40;
    uint64_t large = 0;
    assert(calc_touch(&x, &large) == 1);
    assert(x == 41 && large == UINT64_C(18000000000));
    uint32_t code = 99;
    assert(calc_allocation(0, &code) == 0 && code == 99);
    assert(calc_allocation(1, &code) == 1 && code == 0);
    assert(calc_allocation(2, &code) == 1 && code == 1);
    int64_t unused = 42;
    assert(calc_parse(&unused, &code) == 1 && code == 1 && unused == 42);
    assert(calc_failed(&code) == 1 && code == 0);
    return 0;
}
"#,
        )
        .unwrap();
        let executable = root.join("caller");
        let args = std::fs::read_to_string(artifacts.link_args).unwrap();
        for compiler in ["cc", "c++"] {
            success(
                Command::new(compiler)
                    .args(["-Wall", "-Wextra", "-Werror"])
                    .arg(&c)
                    .arg(&library)
                    .args(args.lines())
                    .arg("-o")
                    .arg(&executable)
                    .output()
                    .unwrap(),
            );
            success(Command::new(&executable).output().unwrap());
        }
        let app = root.join("main.plenty");
        std::fs::write(&app, "import calc\ndef main() -> Result[(), Failure]:\n    print(calc.checked(0))?\n    print(calc.checked(-1))?\n    mut x = 40i32\n    print(calc.touch(&mut x))?\n    print(x)?\n    print(calc.allocation(0))?\n    print(calc.allocation(1))?\n    print(calc.allocation(2))?\n    print(calc.parse())?\n    print(calc.failed())?\n    Ok(())\n").unwrap();
        let options = plenty::CompileOptions {
            link_args: vec![library.into_os_string()],
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&app, &executable, None, &options).unwrap();
        let output = success(Command::new(executable).output().unwrap());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "Result[f32, i16].Ok(1.25)\nResult[f32, i16].Err(-123)\nResult[(), u64].Err(18000000000)\n41\nResult[(), AllocError].Ok(())\nResult[(), AllocError].Err(AllocError.OutOfMemory)\nResult[(), AllocError].Err(AllocError.CapacityOverflow)\nResult[i64, ParseError].Err(ParseError.OutOfRange)\nResult[(), Failure].Err(Failure.Unspecified)\n");
    }
}

#[test]
fn owned_exports_generate_matching_destruction_and_plenty_cleanup() {
    let source = r#"
class Resource:
    value: i64
    def __del__(self) -> ():
        print(self.value).unwrap()
export def create(value: i64) -> Result[Resource, AllocError] = "calc_create":
    if value < 0:
        return Err(AllocError.CapacityOverflow)
    Ok(Resource(value))
export def limit() -> () = "calc_limit":
    print("__test_warm_io__").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
export def restore() -> () = "calc_restore":
    print("__test_restore_allocations__").unwrap()
"#;
    for kind in [LibraryKind::Static, LibraryKind::Shared] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let (library, artifacts) = build(root, source, kind);
        let header = std::fs::read_to_string(artifacts.header).unwrap();
        assert!(header.contains("Release it exactly once with calc_Resource_destroy"));
        let c = root.join("owner.c");
        std::fs::write(
            &c,
            r#"
#include "calc.h"
#include <assert.h>
int main(void) {
    calc_Resource *owner = 0;
    uint32_t error = 99;
    calc_Resource_destroy(0);
    assert(calc_create(42, &owner, &error) == 0 && owner && error == 99);
    calc_Resource *original = owner;
    assert(calc_create(-1, &owner, &error) == 1 && owner == original && error == 1);
    calc_Resource_destroy(owner);
    return 0;
}
"#,
        )
        .unwrap();
        let executable = root.join("caller");
        let args = std::fs::read_to_string(artifacts.link_args).unwrap();
        for compiler in ["cc", "c++"] {
            success(
                Command::new(compiler)
                    .args(["-Wall", "-Wextra", "-Werror"])
                    .arg(&c)
                    .arg(&library)
                    .args(args.lines())
                    .arg("-o")
                    .arg(&executable)
                    .output()
                    .unwrap(),
            );
            let output = success(Command::new(&executable).output().unwrap());
            assert_eq!(output.stdout, b"42\n");
        }
        let app = root.join("main.plenty");
        std::fs::write(&app, "import calc\ndef work() -> Result[(), AllocError]:\n    first = calc.create(11)?\n    second = calc.create(22)?\n    calc.create(-1)?\n    Ok(())\ndef main() -> Result[(), Failure]:\n    print(work())?\n    Ok(())\n").unwrap();
        let options = plenty::CompileOptions {
            link_args: vec![library.into_os_string()],
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&app, &executable, None, &options).unwrap();
        let output = success(Command::new(&executable).output().unwrap());
        assert_eq!(
            output.stdout,
            b"22\n11\nResult[(), AllocError].Err(AllocError.CapacityOverflow)\n"
        );
        if cfg!(feature = "runtime-checks") {
            // The library allocates the handle and the wrapper allocates
            // nothing. A shared library keeps its own runtime, so a failure
            // injected by the application does not reach it.
            for (limit, expected) in [
                (
                    "print(\"__test_fail_allocations_after_0__\").unwrap()",
                    if matches!(kind, LibraryKind::Shared) {
                        "created\n33"
                    } else {
                        "AllocError.OutOfMemory"
                    },
                ),
                ("calc.limit()", "AllocError.OutOfMemory"),
            ] {
                std::fs::write(&app, format!("import calc\ndef main() -> Result[(), Failure]:\n    {limit}\n    result = calc.create(33)\n    calc.restore()\n    print(\"__test_restore_allocations__\").unwrap()\n    match result:\n        case Ok(owner):\n            print(\"created\")?\n        case Err(error):\n            print(error)?\n    Ok(())\n")).unwrap();
                plenty::compile_file_to_executable_with_options(&app, &executable, None, &options)
                    .unwrap();
                let output = success(Command::new(&executable).output().unwrap());
                let visible = String::from_utf8(output.stdout)
                    .unwrap()
                    .lines()
                    .filter(|line| !line.starts_with("__test_"))
                    .collect::<Vec<_>>()
                    .join("\n");
                assert_eq!(visible, expected);
            }
        }
    }
}

#[test]
fn borrowed_handle_exports_preserve_mutation_and_loans() {
    let source = r#"
class Resource:
    value: i64
    def __del__(self) -> ():
        print(self.value).unwrap()
export def create(value: i64) -> Result[Resource, AllocError] = "calc_create":
    Ok(Resource(value))
export def read(a: &Resource, b: &Resource) -> i64 = "calc_read":
    a.value + b.value
export def replace(owner: &mut Resource, value: i64) -> Result[(), AllocError] = "calc_replace":
    owner.value = value
    Err(AllocError.CapacityOverflow)
"#;
    for kind in [LibraryKind::Static, LibraryKind::Shared] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let (library, artifacts) = build(root, source, kind);
        let header = std::fs::read_to_string(&artifacts.header).unwrap();
        assert!(header.contains("fields may change even on Err"));
        assert!(header.contains("const calc_Resource * p0"));
        let c = root.join("loans.c");
        std::fs::write(
            &c,
            r#"
#include "calc.h"
#include <assert.h>
int main(void) {
    calc_Resource *owner = 0;
    uint32_t error = 99;
    assert(calc_create(11, &owner, &error) == 0);
    assert(calc_read(owner, owner) == 22);
    assert(calc_replace(owner, 42, &error) == 1 && error == 1);
    assert(calc_read(owner, owner) == 84);
    calc_Resource_destroy(owner);
    return 0;
}
"#,
        )
        .unwrap();
        let executable = root.join("caller");
        let args = std::fs::read_to_string(artifacts.link_args).unwrap();
        for compiler in ["cc", "c++"] {
            success(
                Command::new(compiler)
                    .args(["-Wall", "-Wextra", "-Werror"])
                    .arg(&c)
                    .arg(&library)
                    .args(args.lines())
                    .arg("-o")
                    .arg(&executable)
                    .output()
                    .unwrap(),
            );
            assert_eq!(
                success(Command::new(&executable).output().unwrap()).stdout,
                b"42\n"
            );
        }
        let app = root.join("main.plenty");
        std::fs::write(&app, "import calc\ndef main() -> Result[(), Failure]:\n    mut owner = calc.create(11)?\n    print(calc.read(&owner, &owner))?\n    print(calc.replace(&mut owner, 42))?\n    print(calc.read(&owner, &owner))?\n    Ok(())\n").unwrap();
        let options = plenty::CompileOptions {
            link_args: vec![library.into_os_string()],
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&app, &executable, None, &options).unwrap();
        assert_eq!(
            success(Command::new(executable).output().unwrap()).stdout,
            b"22\nResult[(), AllocError].Err(AllocError.CapacityOverflow)\n84\n42\n"
        );
        std::fs::write(&app, "import calc\ndef main() -> Result[(), Failure]:\n    mut owner = calc.create(11)?\n    loan = &owner\n    calc.replace(&mut owner, 42)?\n    print(calc.read(loan, loan))?\n    Ok(())\n").unwrap();
        let error = plenty::check_file(&app, None).unwrap_err().to_string();
        assert!(error.contains("borrow"), "{error}");
    }
}

#[test]
fn consuming_handle_exports_transfer_on_success_error_and_wrapper_failure() {
    let source = r#"
class Resource:
    value: i64
    def __del__(self) -> ():
        print("released").unwrap()
export def create(value: i64) -> Result[Resource, AllocError] = "calc_create":
    Ok(Resource(value))
export def consume(owner: Resource, fail: i32) -> Result[i64, i32] = "calc_consume":
    if fail != 0:
        Err(-1)
    else:
        Ok(owner.value)
export def discard(owner: Resource) -> () = "calc_discard":
    drop(owner)
export def identity(owner: Resource) -> Result[Resource, AllocError] = "calc_identity":
    Ok(owner)
"#;
    for kind in [LibraryKind::Static, LibraryKind::Shared] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let (library, artifacts) = build(root, source, kind);
        let header = std::fs::read_to_string(&artifacts.header).unwrap();
        assert!(header.contains("Consumes p0 on entry, on both Ok and Err"));
        let c = root.join("transfer.c");
        std::fs::write(
            &c,
            r#"
#include "calc.h"
#include <assert.h>
int main(void) {
    calc_Resource *owner = 0;
    uint32_t alloc = 99;
    int64_t value = 90;
    int32_t error = 50;
    assert(calc_create(11, &owner, &alloc) == 0);
    assert(calc_consume(owner, 0, &value, &error) == 0 && value == 11 && error == 50);
    assert(calc_create(22, &owner, &alloc) == 0);
    assert(calc_consume(owner, 1, &value, &error) == 1 && value == 11 && error == -1);
    assert(calc_create(33, &owner, &alloc) == 0);
    calc_discard(owner);
    assert(calc_create(44, &owner, &alloc) == 0);
    calc_Resource *returned = 0;
    // The returned owner is a new handle; the consumed one is gone.
    assert(calc_identity(owner, &returned, &alloc) == 0 && returned != 0);
    calc_Resource_destroy(returned);
    return 0;
}
"#,
        )
        .unwrap();
        let executable = root.join("caller");
        let args = std::fs::read_to_string(artifacts.link_args).unwrap();
        for compiler in ["cc", "c++"] {
            success(
                Command::new(compiler)
                    .args(["-Wall", "-Wextra", "-Werror"])
                    .arg(&c)
                    .arg(&library)
                    .args(args.lines())
                    .arg("-o")
                    .arg(&executable)
                    .output()
                    .unwrap(),
            );
            assert_eq!(
                success(Command::new(&executable).output().unwrap()).stdout,
                b"released\nreleased\nreleased\nreleased\n"
            );
        }
        let app = root.join("main.plenty");
        std::fs::write(&app, "import calc\ndef main() -> Result[(), Failure]:\n    print(calc.consume(calc.create(11)?, 0))?\n    print(calc.consume(calc.create(22)?, 1))?\n    calc.discard(calc.create(33)?)\n    drop(calc.identity(calc.create(44)?)?)\n    Ok(())\n").unwrap();
        let options = plenty::CompileOptions {
            link_args: vec![library.into_os_string()],
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&app, &executable, None, &options).unwrap();
        assert_eq!(
            success(Command::new(&executable).output().unwrap()).stdout,
            b"released\nResult[i64, i32].Ok(11)\nreleased\nResult[i64, i32].Err(-1)\nreleased\nreleased\n"
        );
        if cfg!(feature = "runtime-checks") {
            std::fs::write(&app, "import calc\ndef main() -> Result[(), Failure]:\n    owner = calc.create(55)?\n    print(\"__test_fail_allocations_after_0__\").unwrap()\n    result = calc.identity(owner)\n    print(\"__test_restore_allocations__\").unwrap()\n    match result:\n        case Ok(unexpected):\n            print(\"unexpected success\")?\n        case Err(error):\n            print(error)?\n    Ok(())\n").unwrap();
            // Warm the producer's output before injecting process allocation failure.
            let text = std::fs::read_to_string(&app).unwrap().replace(
                "    owner =",
                "    calc.discard(calc.create(0)?)\n    owner =",
            );
            std::fs::write(&app, text).unwrap();
            plenty::compile_file_to_executable_with_options(&app, &executable, None, &options)
                .unwrap();
            let output = success(Command::new(&executable).output().unwrap());
            let visible = String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .filter(|line| !line.starts_with("__test_"))
                .collect::<Vec<_>>()
                .join("\n");
            // Only a static library shares the application's injected failure.
            assert_eq!(
                visible,
                if matches!(kind, LibraryKind::Shared) {
                    "released\nunexpected success\nreleased"
                } else {
                    "released\nreleased\nAllocError.OutOfMemory"
                }
            );
        }
        std::fs::write(&app, "import calc\ndef main() -> Result[(), Failure]:\n    owner = calc.create(11)?\n    calc.discard(owner)\n    calc.discard(owner)\n    Ok(())\n").unwrap();
        let error = plenty::check_file(&app, None).unwrap_err().to_string();
        assert!(error.contains("moved"), "{error}");
    }
}

#[test]
fn generated_library_names_reject_collisions_before_publication() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.plenty");
    let output = temp.path().join("out.a");
    std::fs::write(&output, "previous library").unwrap();
    std::fs::write(
        temp.path().join("foreign.plentyi"),
        "pub extern def release(value: i64) -> () = \"calc_Foo_destroy\"\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("discovery.plentyi"),
        "pub extern def discover() -> () = \"calc_plenty_interface_v1\"\n",
    )
    .unwrap();
    for name in ["first", "second"] {
        std::fs::write(
            temp.path().join(format!("{name}.plenty")),
            "pub class Item:\n    pub value: i64\n",
        )
        .unwrap();
    }
    for (text, expected) in [
        ("class Foo:\n    value: i64\nclass Foo_destroy:\n    value: i64\nexport def a(x: &Foo) -> i64 = \"calc_a\":\n    x.value\nexport def b(x: &Foo_destroy) -> i64 = \"calc_b\":\n    x.value\n", "identifier collision"),
        ("import foreign\nclass Foo:\n    value: i64\nexport def create() -> Result[Foo, AllocError] = \"calc_create\":\n    Ok(Foo(1))\n", "imported C symbol"),
        ("import discovery\nexport def answer() -> i32 = \"calc_answer\":\n    42\n", "imported C symbol"),
        ("import first\nimport second\nexport def a(x: &first.Item) -> i64 = \"calc_a\":\n    x.value\nexport def b(x: &second.Item) -> i64 = \"calc_b\":\n    x.value\n", "conflicting interface name"),
        ("export def _plenty_require_contract() -> () = \"calc_bad\":\n    pass\n", "reserved"),
        ("class plenty_metadata:\n    value: i64\nexport def read(x: &plenty_metadata) -> i64 = \"calc_read\":\n    x.value\n", "reserved C metadata"),
        ("export def guard() -> () = \"calc_plenty_contract_other\":\n    pass\n", "reserved"),
    ] {
        std::fs::write(&source, text).unwrap();
        let error = plenty::compile_file_to_library(&source, &output, None, &LibraryOptions::new("calc", LibraryKind::Static)).unwrap_err().to_string();
        assert!(error.contains(expected), "{expected}: {error}");
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "previous library");
        assert!(!temp.path().join("calc.h").exists());
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
