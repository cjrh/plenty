//! Retained-provider registration is rejected before allocating or taking state
//! from an ambient actor/arena context. Run heap-owning probes in a subprocess.
use super::*;
use crate::allocator::{Allocator, Block, CreateError, Provider};
use crate::memory::AllocError;
use std::alloc::Layout;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Candidate {
    value: usize,
    drops: Arc<AtomicUsize>,
}
impl Provider for Candidate {
    fn allocate(&mut self, layout: Layout) -> Result<Block, AllocError> {
        Allocator::system().allocate(layout)
    }
    fn release(&mut self, block: Block) {
        drop(block);
    }
}
impl Drop for Candidate {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}
fn reject(candidate: Candidate) -> Candidate {
    #[cfg(feature = "allocation-checks")]
    crate::accounting::fail_after(Some(0));
    let result = Allocator::new(&Allocator::system(), candidate);
    #[cfg(feature = "allocation-checks")]
    crate::accounting::fail_after(None);
    let Err(rejected) = result else {
        panic!("registration admitted ambient storage");
    };
    assert_eq!(rejected.error, CreateError::UnsupportedContext);
    assert_eq!(rejected.provider.value, 42);
    assert_eq!(rejected.provider.drops.load(Ordering::Relaxed), 0);
    rejected.provider
}
unsafe extern "C" fn entry(storage: *mut u8) -> *mut u8 {
    // SAFETY: the probe retains this Option across the actor entry.
    let state = unsafe { &mut *storage.cast::<Option<Candidate>>() };
    *state = Some(reject(state.take().unwrap()));
    // Runtime helpers may temporarily switch allocation routing to System;
    // that does not remove the still-active actor recovery frame.
    *state = Some(outside(|| reject(state.take().unwrap())));
    storage
}
unsafe extern "C" fn binding_entry(storage: *mut u8) -> *mut u8 {
    // Temporary system routing must not bypass a live actor recovery frame.
    outside(|| unsafe {
        let mut args = [1u128; 6];
        crate::aggregates::operation(
            *storage.cast::<i64>(),
            args.as_mut_ptr(),
            std::ptr::dangling(),
            std::ptr::null(),
        );
    });
    std::ptr::null_mut()
}

unsafe extern "C" fn block_binding_entry(storage: *mut u8) -> *mut u8 {
    // The opcode lives on the probe's stack across this actor invocation.
    let op = unsafe { *storage.cast::<i64>() };
    outside(|| unsafe {
        // Invalid operands must never be inspected while recovery is active.
        crate::allocator::bindings::block_operation(op, [0; 4]);
    });
    std::ptr::null_mut()
}

#[test]
fn probe() {
    if std::env::var_os("PLENTY_PROVIDER_REGISTRATION_PROBE").is_none() {
        return;
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let mut candidate = Candidate {
        value: 42,
        drops: drops.clone(),
    };
    let mut arena = Arena::new(4096).unwrap();
    candidate = within(&mut arena, || reject(candidate)).unwrap();
    let heap = native::create(65536);
    assert!(heap >= 0);
    assert_eq!(native::operate(SWITCH, heap), -1);
    candidate = reject(candidate); // allocation target active, no recovery frame
    std::thread::spawn(|| assert!(allocator_registration_allowed()))
        .join()
        .unwrap();
    assert_eq!(native::operate(SWITCH, -1), heap);
    let mut state = Some(candidate);
    let status =
        unsafe { native::plenty_actor_run(heap, std::ptr::from_mut(&mut state).cast(), entry) };
    assert_eq!(status, 0);
    assert_eq!(native::discard(heap), 0);
    assert!(allocator_registration_allowed());
    let accepted = Allocator::new(&Allocator::system(), state.take().unwrap())
        .unwrap_or_else(|_| panic!("registration not restored outside ambient context"));
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    drop(accepted);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    use crate::collection_opcodes::{
        ALLOCATOR_BYTES_SLICE, ALLOCATOR_CLIST_SLICE, ALLOCATOR_COPY, ALLOCATOR_KEY_VIEW_TO_SET,
        ALLOCATOR_LIST_SLICE, ALLOCATOR_SET_SYMMETRIC_DIFFERENCE, ALLOCATOR_SET_UNION,
        ALLOCATOR_SYSTEM, ALLOCATOR_TEXT_CONCAT, ALLOCATOR_TEXT_FINISH, ALLOCATOR_TEXT_JOIN,
        ALLOCATOR_TEXT_REPEAT, ALLOCATOR_TEXT_SLICE,
    };
    for mut op in [
        ALLOCATOR_SYSTEM,
        ALLOCATOR_COPY,
        ALLOCATOR_KEY_VIEW_TO_SET,
        ALLOCATOR_LIST_SLICE,
        ALLOCATOR_BYTES_SLICE,
        ALLOCATOR_TEXT_CONCAT,
        ALLOCATOR_TEXT_JOIN,
        ALLOCATOR_TEXT_FINISH,
        ALLOCATOR_TEXT_REPEAT,
        ALLOCATOR_TEXT_SLICE,
        ALLOCATOR_CLIST_SLICE,
    ]
    .into_iter()
    .chain(ALLOCATOR_SET_UNION..=ALLOCATOR_SET_SYMMETRIC_DIFFERENCE)
    {
        let heap = native::create(65536);
        assert!(heap >= 0);
        let status = unsafe {
            native::plenty_actor_run(heap, std::ptr::from_mut(&mut op).cast(), binding_entry)
        };
        assert_ne!(status, 0, "allocator opcode {op} admitted actor recovery");
        assert_eq!(native::discard(heap), 0);
        assert!(allocator_registration_allowed());
    }
    for mut op in crate::collection_opcodes::ALLOCATOR_BLOCK
        ..=crate::collection_opcodes::ALLOCATOR_BLOCK_SPLIT
    {
        let heap = native::create(65536);
        assert!(heap >= 0);
        let status = unsafe {
            native::plenty_actor_run(
                heap,
                std::ptr::from_mut(&mut op).cast(),
                block_binding_entry,
            )
        };
        assert_ne!(status, 0, "block opcode {op} admitted actor recovery");
        assert_eq!(native::discard(heap), 0);
        assert!(allocator_registration_allowed());
    }
}
#[test]
fn ambient_context_rejection_returns_provider_without_allocating() {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "actor_heap::registration_tests::probe",
            "--nocapture",
        ])
        .env("PLENTY_PROVIDER_REGISTRATION_PROBE", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("registration probe timed out");
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
}
