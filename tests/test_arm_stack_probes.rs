//! AArch64 loop probes leave SP unchanged; registered PCs locate their frontier.
#![cfg(all(target_os = "linux", target_arch = "aarch64"))]

use rstest::rstest;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;

#[rstest]
fn unrolled_and_loop_probes_report_with_cold_and_warm_function_caches(
    #[values(2, 4, 20, 40)] locals: usize,
    #[values(false, true)] optimize: bool,
) {
    let fields: String = (0..100).map(|i| format!("    f{i}: i64\n")).collect();
    let arguments = vec!["n"; 100].join(", ");
    let values: String = (0..locals)
        .map(|i| format!("    a{i} = Wide({arguments})\n"))
        .collect();
    let sum = (0..locals)
        .map(|i| format!("a{i}.f{}", i % 100))
        .collect::<Vec<_>>()
        .join(" + ");
    let source = format!(
        "class Wide:\n{fields}\ndef depth(n: i64) -> i64:\n    if n == 0:\n        return 0\n{values}    below = depth(n - 1)\n    below + {sum}\ndef main() -> Result[(), IoError]:\n    print(depth(1000000))?\n    Ok(())\n"
    );
    let workspace = tempfile::tempdir().unwrap();
    let input = workspace.path().join("probe.plenty");
    std::fs::write(&input, source).unwrap();
    let options = plenty::CompileOptions {
        optimize,
        cache: plenty::cache::CacheMode::Directory(workspace.path().join("cache")),
        // Always place the functions again, including the warm build's probes.
        program_cache: false,
        ..Default::default()
    };
    for build in ["cold", "warm"] {
        let executable = workspace.path().join(build);
        plenty::compile_file_to_executable_with_options(&input, &executable, None, &options)
            .unwrap();
        let output = Command::new("sh")
            .args([
                "-c",
                "ulimit -c 0; ulimit -s 256; exec \"$1\"",
                "probe-test",
            ])
            .arg(&executable)
            .output()
            .unwrap();
        assert_eq!(
            output.status.signal(),
            Some(6),
            "{build}: {:?}",
            output.status
        );
        assert_eq!(output.stderr, b"error: stack overflow\n", "{build}");
        assert!(output.stdout.is_empty(), "{build}: {:?}", output.stdout);
    }
}

#[test]
fn an_unregistered_below_sp_write_keeps_the_default_fault() {
    let workspace = tempfile::tempdir().unwrap();
    let native = workspace.path().join("native.c");
    std::fs::write(
        &native,
        r#"
#define _GNU_SOURCE
#include <stdint.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <unistd.h>
void unrelated_below_sp(void) {
    uintptr_t sp;
    __asm__ volatile("mov %0, sp" : "=r"(sp));
    const uintptr_t page = (uintptr_t)sysconf(_SC_PAGESIZE);
    void *base = (void *)((sp & ~(page - 1)) - 4 * page);
    if (mmap(base, 4 * page, PROT_READ | PROT_WRITE,
             MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0) != base ||
        mprotect(base, page, PROT_NONE) != 0) {
        _Exit(77);
    }
    // Even the probe registers and exact store instruction are insufficient:
    // this PC belongs to external native code, not a registered Plenty probe.
    __asm__ volatile("neg x16, %0\nmov x17, %0\nstr wzr, [sp, x16]"
                     : : "r"(4 * page) : "x16", "x17", "memory");
    _Exit(78);
}
"#,
    )
    .unwrap();
    let object = workspace.path().join("native.o");
    let compiled = Command::new("cc")
        .args(["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-c"])
        .arg(native)
        .arg("-o")
        .arg(&object)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    std::fs::write(
        workspace.path().join("native.plentyi"),
        "pub extern def fault() -> () = \"unrelated_below_sp\"\n",
    )
    .unwrap();
    let source = workspace.path().join("main.plenty");
    std::fs::write(
        &source,
        "import native\ndef main() -> ():\n    native.fault()\n",
    )
    .unwrap();
    let executable = workspace.path().join("fault");
    let options = plenty::CompileOptions {
        link_args: vec![object.into_os_string()],
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&source, &executable, None, &options).unwrap();
    let output = Command::new("sh")
        .args(["-c", "ulimit -c 0; exec \"$1\"", "unrelated-fault"])
        .arg(executable)
        .output()
        .unwrap();
    assert_eq!(output.status.signal(), Some(11), "{:?}", output.status);
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
