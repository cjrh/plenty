//! Standalone unit tests for the fault boundary, usable before the full runtime
//! builds on a new host. Pass a matching libc rlib with `rustc --test --extern`.
//! Cross-target type checks can use `--emit=metadata` and libc's rmeta instead.
#![allow(dead_code)]
#![cfg(any(target_os = "linux", target_os = "macos", windows))]

#[repr(C)]
struct Site;

// Trap-table tests compare helper addresses without invoking a diagnostic.
// Keep those symbols local to the harness; production uses src/io.rs.
mod io {
    macro_rules! helpers {
        ($($name:ident),*) => { $(pub extern "C" fn $name() { std::process::exit(77) })* };
    }
    helpers!(
        plenty_trap_overflow,
        plenty_trap_div_zero,
        plenty_trap_shift,
        plenty_trap_cast_range,
        plenty_trap_unwrap
    );
}

#[path = "../src/signal_context.rs"]
mod signal_context;
#[path = "../src/stack_overflow.rs"]
mod stack_overflow;
#[path = "../src/traps.rs"]
mod traps;

#[test]
fn guarded_boundary_returns_the_body_result() {
    let _install: fn() = stack_overflow::install;
    assert_eq!(stack_overflow::guarded(|| 42), 42);
}

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows_faults {
    use super::*;
    use std::process::Command;

    // A known label avoids depending on compiler-generated function prologues.
    std::arch::global_asm!(
        ".text",
        ".globl plenty_test_illegal",
        "plenty_test_illegal:",
        "ud2",
    );
    unsafe extern "C" {
        fn plenty_test_illegal();
        fn plenty_traps_register(table: *const std::ffi::c_void);
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetErrorMode(mode: u32) -> u32;
    }

    #[repr(C)]
    struct TrapTable {
        count: u32,
        sites: i32,
        address: i32,
        entry: u32,
    }
    static mut TABLE: TrapTable = TrapTable {
        count: 1,
        sites: 0,
        address: 0,
        entry: 0xfffffff0,
    };

    fn child(mode: &str) -> std::process::Output {
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "windows_faults::native_fault_child",
                "--nocapture",
            ])
            .env("PLENTY_NATIVE_FAULT", mode)
            .output()
            .unwrap()
    }

    #[test]
    fn registered_illegal_instruction_enters_the_checked_helper() {
        let output = child("checked");
        assert_eq!(output.status.code(), Some(77), "{output:?}");
    }

    #[test]
    fn unrelated_illegal_instruction_keeps_the_windows_disposition() {
        let output = child("unregistered");
        assert_eq!(
            output.status.code().map(|code| code as u32),
            Some(signal_context::ILLEGAL_INSTRUCTION),
            "{output:?}"
        );
    }

    #[test]
    fn exhausted_guarded_thread_reports_stack_overflow() {
        let output = child("overflow");
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("error: stack overflow"),
            "{output:?}"
        );
    }

    #[test]
    fn unrelated_fault_reaches_an_existing_host_handler() {
        let output = child("host");
        assert_eq!(output.status.code(), Some(78), "{output:?}");
    }

    unsafe extern "system" fn host_handler(_: *mut signal_context::ExceptionPointers) -> i32 {
        std::process::exit(78)
    }

    #[inline(never)]
    #[allow(unconditional_recursion)]
    fn exhaust_stack() {
        let mut frame = [0u8; 4096];
        std::hint::black_box(&mut frame);
        exhaust_stack();
        std::hint::black_box(&frame);
    }

    #[test]
    fn native_fault_child() {
        let Ok(mode) = std::env::var("PLENTY_NATIVE_FAULT") else {
            return;
        };
        // SAFETY: disables modal crash dialogs only inside this subprocess.
        unsafe {
            SetErrorMode(0x0001 | 0x0002);
        }
        if mode == "host" {
            // SAFETY: the callback lives until this subprocess exits.
            assert!(
                !unsafe { signal_context::AddVectoredExceptionHandler(1, host_handler) }.is_null()
            );
        }
        if mode == "overflow" {
            stack_overflow::install();
            std::thread::Builder::new()
                .stack_size(256 * 1024)
                .spawn(|| {
                    stack_overflow::guarded(exhaust_stack);
                })
                .unwrap()
                .join()
                .unwrap();
        } else {
            // SAFETY: the static table and machine-code label live forever.
            // Both are in this executable image, within a signed 32-bit offset.
            unsafe {
                let field = &raw mut TABLE.address;
                *field = i32::try_from(
                    (plenty_test_illegal as *const () as usize).wrapping_sub(field as usize)
                        as isize,
                )
                .unwrap();
                if mode == "unregistered" || mode == "host" {
                    TABLE.count = 0;
                }
                plenty_traps_register((&raw const TABLE).cast());
                plenty_test_illegal();
            }
        }
        panic!("native fault unexpectedly returned");
    }
}
