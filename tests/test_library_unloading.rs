//! Physical unload/reload, including allocations on persistent foreign threads.
use std::process::Command;

fn supported() -> bool {
    cfg!(all(
        target_os = "linux",
        target_env = "gnu",
        target_arch = "x86_64"
    ))
}

fn options() -> plenty::LibraryOptions {
    let mut options = plenty::LibraryOptions::new("cycle", plenty::LibraryKind::Shared);
    options.unloadable = true;
    options
}

#[test]
fn repeated_unload_with_allocations_on_live_host_threads() {
    if !supported() {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::write(
        root.join("raw.plentyi"),
        r#"
pub opaque Pointer
pub extern def report() -> () = "probe_report_write"
pub extern def counter() -> Pointer = storage 8
pub extern def read(p: Pointer, offset: u64) -> i64 = intrinsic "load"
pub extern def write(p: Pointer, offset: u64, value: i64) -> () = intrinsic "store"
"#,
    )
    .unwrap();
    let source = root.join("source.plenty");
    std::fs::write(
        &source,
        r#"
import raw
import std.bytes as byte_buffers
def unused() -> ():
    print("unreachable IO does not block unloading").unwrap()
class Owner:
    items: list[i64]
export def create(value: i64) -> Result[Owner, AllocError] = "cycle_create":
    Ok(Owner([value]?))
export def read(owner: &Owner) -> i64 = "cycle_read":
    owner.items[0]
@thread_safe
export def entry(value: i64) -> i64 = exact "entry":
    mut items = [value].unwrap()
    items.append(value).unwrap()
    text = f"memory {items[0]}".unwrap()
    buffer = byte_buffers.from_text(&text).unwrap()
    table = {buffer.to_text().unwrap(): items[0]}.unwrap()
    keys = {value}.unwrap()
    if value in keys:
        return table[&text]
    0
export def report_once() -> () = exact "report_once":
    raw.report()
export def generation() -> i64 = exact "generation":
    p = raw.counter()
    value = raw.read(p, 0u64)
    unchecked:
        raw.write(p, 0u64, value + 1)
    value
"#,
    )
    .unwrap();
    // Exercise the actual ReportWrite runtime branch, not just the dormant
    // branch in a memory Formatter. The test-only bridge uses the runtime's
    // collection entry ABI (three u128 inputs, result, 32 scratch bytes).
    let bridge = root.join("report.c");
    std::fs::write(
        &bridge,
        r#"
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
extern void plenty_op_510(const __uint128_t *, const void *, const void *);
void probe_report_write(void) {
    static const char message[] = "unload report\n";
    __uint128_t words[6] = {0};
    words[0] = (__uint128_t)(uintptr_t)message | ((__uint128_t)(sizeof(message) - 1) << 64);
    plenty_op_510(words, NULL, NULL);
    assert(words[3] == sizeof(message) - 1);
}
"#,
    )
    .unwrap();
    let bridge_object = root.join("report.o");
    assert!(
        Command::new("cc")
            .args(["-fPIC", "-c"])
            .arg(bridge)
            .arg("-o")
            .arg(&bridge_object)
            .status()
            .unwrap()
            .success()
    );
    let mut certified_options = options();
    certified_options
        .compile
        .link_args
        .push(bridge_object.clone().into_os_string());
    let library = root.join("libcycle.so");
    let artifacts =
        plenty::compile_file_to_library(&source, &library, None, &certified_options).unwrap();
    assert!(
        std::fs::read_to_string(artifacts.header)
            .unwrap()
            .contains("unloadable-library profile v1")
    );
    let certified = std::fs::read_to_string(&artifacts.interface).unwrap();
    assert!(certified.contains("# unloadable-profile: linux-gnu-x86_64-v1"));
    plenty::verify_library_interface(&library, &artifacts.interface).unwrap();
    let mut ordinary = plenty::LibraryOptions::new("cycle", plenty::LibraryKind::Shared);
    ordinary
        .compile
        .link_args
        .push(bridge_object.into_os_string());
    let ordinary_library = root.join("ordinary.so");
    plenty::compile_file_to_library(&source, &ordinary_library, None, &ordinary).unwrap();
    // Publication replaced the companion with the ordinary contract. It must
    // not be accepted against the certified library despite identical exports.
    assert!(plenty::verify_library_interface(&library, &artifacts.interface).is_err());
    std::fs::write(&artifacts.interface, certified).unwrap();
    let host = root.join("host.c");
    std::fs::write(&host, r#"
#define _GNU_SOURCE
#include <assert.h>
#include <dlfcn.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "cycle.h"
#define THREADS 4
#define CYCLES 24
static pthread_barrier_t begin, done;
static int64_t (*loaded_entry)(int64_t);
static void *worker(void *arg) {
    int64_t value = (int64_t)(intptr_t)arg;
    for (int cycle = 0; cycle < CYCLES; ++cycle) {
        pthread_barrier_wait(&begin);
        for (int call = 0; call < 100; ++call) assert(loaded_entry(value) == value);
        pthread_barrier_wait(&done);
    }
    return NULL;
}
static int mapped(const char *path) {
    FILE *f = fopen("/proc/self/maps", "r");
    assert(f);
    char *line = NULL;
    size_t capacity = 0;
    int found = 0;
    while (getline(&line, &capacity, f) > 0) if (strstr(line, path)) found = 1;
    free(line);
    fclose(f);
    return found;
}
int main(int argc, char **argv) {
    assert(argc == 2);
    pthread_t threads[THREADS];
    assert(!pthread_barrier_init(&begin, NULL, THREADS + 1));
    assert(!pthread_barrier_init(&done, NULL, THREADS + 1));
    for (int i = 0; i < THREADS; ++i) assert(!pthread_create(&threads[i], NULL, worker, (void *)(intptr_t)(42 + i)));
    for (int cycle = 0; cycle < CYCLES; ++cycle) {
        void *library = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
        if (!library) { fprintf(stderr, "%s\n", dlerror()); return 2; }
        loaded_entry = dlsym(library, "entry");
        int64_t (*generation)(void) = dlsym(library, "generation");
        uint32_t (*create)(int64_t, cycle_Owner **, uint32_t *) = dlsym(library, "cycle_create");
        int64_t (*read_owner)(const cycle_Owner *) = dlsym(library, "cycle_read");
        void (*destroy)(cycle_Owner *) = dlsym(library, "cycle_Owner_destroy");
        assert(loaded_entry && generation && create && read_owner && destroy);
        assert(mapped(argv[1]));
        void (*report)(void) = dlsym(library, "report_once");
        assert(report);
        report();
        assert(generation() == 0 && generation() == 1);
        cycle_Owner *owner = NULL;
        uint32_t error = 0;
        assert(create(123, &owner, &error) == 0 && owner);
        assert(read_owner(owner) == 123);
        pthread_barrier_wait(&begin);
        pthread_barrier_wait(&done);
        destroy(owner);
        // Threads remain alive and blocked at begin, with no Plenty frame.
        assert(!dlclose(library));
        assert(!mapped(argv[1]));
    }
    for (int i = 0; i < THREADS; ++i) assert(!pthread_join(threads[i], NULL));
    pthread_barrier_destroy(&begin);
    pthread_barrier_destroy(&done);
    return 0;
}
"#).unwrap();
    let executable = root.join("host");
    let output = Command::new("cc")
        .args(["-Wall", "-Wextra", "-Werror"])
        .arg(&host)
        .args(["-ldl", "-pthread", "-o"])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(executable).arg(library).output().unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn unload_profile_follows_helpers_and_destructors() {
    if !supported() {
        return;
    }
    // B28: these runtime families need separate teardown contracts before
    // admission to the unloadable profile; ordinary library builds still work.
    for source in [
        "def unsafe_target() -> ():\n    print(1).unwrap()\nexport def entry() -> () = \"cycle_entry\":\n    callback: Callable[[], ()] = unsafe_target\n    callback()\n",
        "def helper() -> ():\n    print(1).unwrap()\nexport def entry() -> () = \"cycle_entry\":\n    helper()\n",
        "class Owner:\n    value: i64\n    def __del__(self) -> ():\n        print(self.value).unwrap()\nexport def create() -> Result[Owner, AllocError] = \"cycle_create\":\n    Ok(Owner(1))\n",
        "import std.time as time\nexport def entry() -> () = \"cycle_entry\":\n    drop(time.monotonic_now())\n",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source_path = temp.path().join("source.plenty");
        std::fs::write(&source_path, source).unwrap();
        let output = temp.path().join("library.so");
        let error = plenty::compile_file_to_library(&source_path, &output, None, &options())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("unloadable library:") && error.contains("unsupported operation"),
            "{error}"
        );
        assert!(!output.exists());
    }
}

#[test]
fn unload_profile_is_opt_in_and_shared_only() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.plenty");
    std::fs::write(
        &source,
        "export def entry() -> () = \"cycle_entry\":\n    print(1).unwrap()\n",
    )
    .unwrap();
    let output = temp.path().join("library.a");
    let mut options = plenty::LibraryOptions::new("cycle", plenty::LibraryKind::Static);
    plenty::compile_file_to_library(&source, &output, None, &options).unwrap();
    options.unloadable = true;
    let error = plenty::compile_file_to_library(&source, &output, None, &options)
        .unwrap_err()
        .to_string();
    assert!(error.contains("require shared library"), "{error}");
    let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args(["--check", "--unloadable-library"])
        .arg(source)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires --shared-library"));
}
