//! Subprocess tests use real actor/arena recovery frames. No compiled provider
//! bridge is involved; failures exercise the runtime entry points it will use.
use super::*;
use crate::allocator::{Allocator, Block, Provider};
use crate::memory::AllocError;
use std::alloc::Layout;
use std::sync::{Arc, Mutex};

struct Policy {
    mode: String,
    recursive: Arc<Mutex<Option<Allocator>>>,
}
impl Provider for Policy {
    fn allocate(&mut self, layout: Layout) -> Result<Block, AllocError> {
        match self.mode.as_str() {
            "allocate" => crate::fail("provider allocation trap"),
            "short" => return Allocator::system().allocate(Layout::new::<u8>()),
            "recursive" => {
                return self
                    .recursive
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .allocate(layout);
            }
            "poison" => panic!("poison provider for probe"),
            "error" => return Err(AllocError::OutOfMemory),
            "arena" | "actor-arena" => allocation_failure(),
            "inner-arena" => {
                let mut arena = Arena::new(4096).unwrap();
                let _: Option<()> = within(&mut arena, || allocation_failure());
                panic!("callback's nested arena recovered");
            }
            _ => {}
        }
        Allocator::system().allocate(layout)
    }
    fn release(&mut self, block: Block) {
        if self.mode == "release" {
            crate::fail("provider release trap");
        }
        drop(block);
    }
}
impl Drop for Policy {
    fn drop(&mut self) {
        if self.mode == "destroy" {
            crate::fail("provider destruction trap");
        }
    }
}
fn allocation_failure() -> ! {
    unsafe extern "Rust" {
        fn plenty_rust_alloc_error(size: usize, align: usize) -> !;
    }
    // SAFETY: invoke the actual runtime failure handler with a valid layout.
    // This deliberately exercises recovery, not the host's Rust OOM hook.
    unsafe { plenty_rust_alloc_error(8192, 16) }
}
struct Context {
    allocator: Option<Allocator>,
    block: Option<Block>,
    mode: String,
    arena: Arena,
}
unsafe extern "C" fn entry(storage: *mut u8) -> *mut u8 {
    // SAFETY: the probe keeps Context live across the actor call.
    let ctx = unsafe { &mut *storage.cast::<Context>() };
    match ctx.mode.as_str() {
        "destroy" => drop(ctx.allocator.take()),
        "release" => drop(ctx.block.take()),
        "actor-arena" => {
            let allocator = ctx.allocator.as_ref().unwrap();
            let _ = within(&mut ctx.arena, || allocator.allocate(Layout::new::<u64>()));
        }
        "error" => {
            assert!(matches!(
                ctx.allocator
                    .as_ref()
                    .unwrap()
                    .allocate(Layout::new::<u64>()),
                Err(AllocError::OutOfMemory)
            ));
            // The returned error must have restored ordinary actor recovery.
            crate::fail("ordinary actor trap after allocation error");
        }
        "success" => {
            drop(
                ctx.allocator
                    .as_ref()
                    .unwrap()
                    .allocate(Layout::new::<u64>())
                    .unwrap(),
            );
            drop(ctx.allocator.take());
            crate::fail("ordinary actor trap after successful callbacks");
        }
        _ => {
            drop(
                ctx.allocator
                    .as_ref()
                    .unwrap()
                    .allocate(Layout::new::<u64>()),
            );
        }
    }
    storage
}

#[test]
fn probe() {
    let Ok(mode) = std::env::var("PLENTY_ALLOCATOR_RECOVERY_PROBE") else {
        return;
    };
    // Failure-hook cases abort. Do not leave core dumps behind.
    unsafe {
        libc::setrlimit(
            libc::RLIMIT_CORE,
            &libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            },
        );
    }
    let recursive = Arc::new(Mutex::new(None));
    let allocator = Allocator::new(
        &Allocator::system(),
        Policy {
            mode: mode.clone(),
            recursive: recursive.clone(),
        },
    )
    .unwrap_or_else(|_| panic!("bootstrap failed"));
    if mode == "recursive" {
        *recursive.lock().unwrap() = Some(allocator.clone());
    }
    if mode == "poison" {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            allocator.allocate(Layout::new::<u64>())
        }));
        assert!(result.is_err());
        assert!(!crate::allocator::callback_active());
    }
    let block = (mode == "release").then(|| allocator.allocate(Layout::new::<u64>()).unwrap());
    let mut ctx = Context {
        allocator: Some(allocator),
        block,
        mode: mode.clone(),
        arena: Arena::new(4096).unwrap(),
    };
    if mode == "arena" {
        let allocator = ctx.allocator.as_ref().unwrap();
        let _ = within(&mut ctx.arena, || allocator.allocate(Layout::new::<u64>()));
        panic!("arena recovered through callback");
    }
    let heap = native::create(65536);
    assert!(heap >= 0);
    let status =
        unsafe { native::plenty_actor_run(heap, std::ptr::from_mut(&mut ctx).cast(), entry) };
    assert!(
        mode == "error" || mode == "success",
        "callback trap recovered: {mode}, {status}"
    );
    assert_eq!(status, TrapKind::Other as i64);
    assert!(!crate::allocator::callback_active());
    assert_eq!(native::discard(heap), 0);
    // Arena recovery also works again after the callback returns normally.
    let result: Option<()> = within(&mut ctx.arena, || {
        abandon_arena_call();
        panic!("ordinary arena did not recover");
    });
    assert!(result.is_none());
}

#[test]
fn callbacks_cannot_escape_through_actor_or_arena_recovery() {
    use std::os::unix::process::ExitStatusExt;
    for (mode, message, abort) in [
        ("allocate", "provider allocation trap", false),
        ("release", "provider release trap", false),
        ("destroy", "provider destruction trap", false),
        ("short", "insufficient extent or alignment", false),
        ("recursive", "reentrant allocator callback", false),
        ("poison", "poisoned allocator provider", false),
        ("arena", "memory allocation of 8192 bytes failed", true),
        (
            "actor-arena",
            "memory allocation of 8192 bytes failed",
            true,
        ),
        (
            "inner-arena",
            "memory allocation of 8192 bytes failed",
            true,
        ),
        ("error", "ordinary actor trap after allocation error", false),
        (
            "success",
            "ordinary actor trap after successful callbacks",
            false,
        ),
    ] {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "actor_heap::allocator_tests::probe",
                "--nocapture",
            ])
            .env("PLENTY_ALLOCATOR_RECOVERY_PROBE", mode)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("{mode}: callback deadlocked");
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let output = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(message), "{mode}: {output:?}");
        if mode == "error" || mode == "success" {
            assert!(output.status.success(), "{mode}: {output:?}");
            assert!(stderr.contains("contained in an actor"));
            assert!(!stderr.contains("callback traps are process-fatal"));
        } else {
            if abort {
                assert_eq!(
                    output.status.signal(),
                    Some(libc::SIGABRT),
                    "{mode}: {output:?}"
                );
            } else {
                assert_eq!(output.status.code(), Some(1), "{mode}: {output:?}");
            }
            assert!(
                stderr.contains("allocator callback traps are process-fatal"),
                "{mode}: {output:?}"
            );
            assert!(
                !stderr.contains("contained in an actor"),
                "{mode}: {output:?}"
            );
        }
    }
}
