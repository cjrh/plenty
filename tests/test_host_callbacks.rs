//! Host callback permission follows the entry context, including generated interfaces.
use std::fs;

const NATIVE: &str = r#"
@thread_shareable
pub opaque Pointer
@host_callback
pub extern def call(function: Pointer, data: Pointer) -> i64 = function
@thread_safe
pub extern def pointer() -> Pointer = "native_pointer"
"#;

fn check(source: &str, native: &str) -> Result<(), String> {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("native.plentyi"), native).unwrap();
    let path = root.path().join("source.plenty");
    fs::write(&path, source).unwrap();
    plenty::check_module_file(&path, None)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

const ENTRY: &str = r#"
import native
@thread_safe
export def entry(p: native.Pointer) -> i64 = exact "entry":
    native.call(p, p)
"#;

#[test]
fn host_callback_permission_does_not_escape_into_workers() {
    check(ENTRY, NATIVE).unwrap();
    let missing = check(ENTRY, &NATIVE.replace("@host_callback\n", "")).unwrap_err();
    assert!(missing.contains("function address"), "{missing}");
    for body in [
        "    native.call(native.pointer(), native.pointer())",
        "    entry(native.pointer())",
        "    helper = entry\n    helper(native.pointer())",
        "    resource = Resource(native.pointer())\n    drop(resource)\n    0",
    ] {
        let source = format!(
            r#"{ENTRY}
@thread_safe
export def cleanup(p: native.Pointer) -> () = exact "cleanup":
    resource = Resource(p)
    drop(resource)
class Resource:
    pointer: native.Pointer
    def __del__(self) -> ():
        drop(native.call(self.pointer, self.pointer))
def worker() -> i64:
{body}
def main() -> Result[(), SpawnError[()]]:
    with spawn(worker).drop_job()? as task:
        drop(task.join())
    Ok(())
"#
        );
        let error = check(&source, NATIVE).unwrap_err();
        assert!(error.contains("ordinary Plenty workers"), "{error}");
    }
}

#[test]
fn executor_and_parallel_roots_do_not_inherit_host_permission() {
    for operation in [
        "        drop(pool.submit(work, 1i64).drop_job()?.result()?)",
        "        rows = [1i64, 2i64]?\n        drop(iter(rows).par(&pool).map(work).collect()?)",
    ] {
        let source = format!(
            r#"{ENTRY}
def work(value: i64) -> i64:
    drop(value)
    entry(native.pointer())
def main() -> Result[(), AllocError | PoolError | SubmitError[()] | FutureError | PoolMapError]:
    with ThreadPoolExecutor(1, 1)? as pool:
{operation}
    Ok(())
"#
        );
        let error = check(&source, NATIVE).unwrap_err();
        assert!(error.contains("ordinary Plenty workers"), "{error}");
    }
}

#[test]
fn generated_interface_does_not_restore_worker_permission() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("native.plentyi"), NATIVE).unwrap();
    let source = root.path().join("source.plenty");
    fs::write(
        &source,
        r#"
import native
@thread_safe
export def entry() -> i64 = exact "entry":
    native.call(native.pointer(), native.pointer())
"#,
    )
    .unwrap();
    let artifacts = plenty::compile_file_to_library(
        &source,
        &root.path().join("libcallbacks.a"),
        None,
        &plenty::LibraryOptions::new("callbacks", plenty::LibraryKind::Static),
    )
    .unwrap();
    assert_eq!(artifacts.interface, root.path().join("callbacks.plentyi"));
    fs::write(
        &source,
        r#"
import callbacks
def worker() -> i64:
    callbacks.entry()
def main() -> Result[(), SpawnError[()]]:
    with spawn(worker).drop_job()? as task:
        drop(task.join())
    Ok(())
"#,
    )
    .unwrap();
    let error = plenty::check_file(&source, None).unwrap_err().to_string();
    assert!(
        error.contains("no worker-thread effect contract"),
        "{error}"
    );
}

#[test]
fn host_callback_annotation_requires_an_indirect_target() {
    for (declaration, expected) in [
        (
            "@host_callback\npub extern def bad() -> () = \"bad\"",
            "function-address parameter",
        ),
        (
            "@host_callback\npub extern def bad() -> Pointer = storage 8",
            "function-address parameter",
        ),
        (
            "@host_callback\n@thread_safe\npub extern def bad(p: Pointer) -> () = p",
            "mutually exclusive",
        ),
        ("@host_callback\npub opaque Bad", "indirect `extern def`"),
        (
            "@host_callback\nexport def bad() -> ():\n    pass",
            "indirect `extern def`",
        ),
    ] {
        let native = format!("pub opaque Pointer\n{declaration}\n");
        let error = check("import native\n", &native).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn native_record_callbacks_concurrent_and_handoff() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(
        root.join("native.plentyi"),
        r#"
@thread_movable
pub opaque Pointer
@thread_safe
pub extern def read(p: Pointer, offset: u64) -> Pointer = intrinsic "load"
@host_callback
pub extern def count(function: Pointer, data: Pointer) -> u32 = function
@host_callback
pub extern def get(function: Pointer, data: Pointer, index: u32) -> i64 = function
@host_callback
pub extern def notify(function: Pointer, data: Pointer, value: i64) -> () = function
"#,
    )
    .unwrap();
    let source = root.join("source.plenty");
    fs::write(
        &source,
        r#"
import native
@thread_safe
export def entry(p: native.Pointer) -> i64 = exact "host_entry":
    count = native.read(p, 0u64)
    get = native.read(p, 8u64)
    notify = native.read(p, 16u64)
    mut total = 0i64
    for i in range(native.count(count, p)):
        unchecked:
            total += native.get(get, p, i)
    native.notify(notify, p, total)
    total
@thread_safe
export def pure(value: i64) -> i64 = exact "pure":
    value
"#,
    )
    .unwrap();
    let library = root.join("libcallbacks.a");
    let artifacts = plenty::compile_file_to_library(
        &source,
        &library,
        None,
        &plenty::LibraryOptions::new("callbacks", plenty::LibraryKind::Static),
    )
    .unwrap();
    plenty::check_module_file(&artifacts.interface, None).unwrap();
    let interface = fs::read_to_string(&artifacts.interface).unwrap();
    assert!(
        interface.contains("# Requires the foreign host protocol callback context"),
        "{interface}"
    );
    assert!(
        interface.contains("@thread_safe\nextern def _plenty_c_pure"),
        "{interface}"
    );
    assert!(
        !interface.contains("@thread_safe\nextern def _plenty_c_entry"),
        "{interface}"
    );
    let host = root.join("host.c");
    fs::write(
        &host,
        r#"
#include <assert.h>
#include <pthread.h>
#include "callbacks.h"
struct record {
    uint32_t (*count)(void *);
    int64_t (*get)(void *, uint32_t);
    void (*notify)(void *, int64_t);
    int64_t base;
    int64_t last;
};
static uint32_t count(void *p) { (void)p; return 3; }
static int64_t get(void *p, uint32_t i) { return ((struct record *)p)->base + i; }
static void notify(void *p, int64_t value) { ((struct record *)p)->last = value; }
static void *run(void *p) {
    struct record *r = p;
    for (int i = 0; i < 100; ++i) {
        assert(host_entry(p) == 3*r->base + 3);
        assert(r->last == 3*r->base + 3);
        r->base++;
    }
    return 0;
}
int main(void) {
    struct record a = { count, get, notify, 10, 0 };
    struct record b = { count, get, notify, 1000, 0 };
    pthread_t first, second;
    assert(!pthread_create(&first, 0, run, &a));
    assert(!pthread_create(&second, 0, run, &b));
    assert(!pthread_join(first, 0));
    assert(!pthread_join(second, 0));
    /* Join transfers each record back before handing it to a new thread. */
    assert(!pthread_create(&first, 0, run, &a));
    assert(!pthread_join(first, 0));
    assert(a.base == 210 && b.base == 1100);
    assert(pure(2) == 2);
    return 0;
}
"#,
    )
    .unwrap();
    let executable = root.join("host");
    let args = fs::read_to_string(artifacts.link_args).unwrap();
    let output = std::process::Command::new("cc")
        .args(["-Wall", "-Wextra", "-Werror", "-pthread"])
        .arg(host)
        .arg(library)
        .args(args.lines())
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        std::process::Command::new(executable)
            .status()
            .unwrap()
            .success()
    );
}
