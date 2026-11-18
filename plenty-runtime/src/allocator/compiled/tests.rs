use super::*;
use crate::aggregates::{Variant, release};
use crate::ranges;
use std::cell::RefCell;
use std::sync::Arc;

static INTEGER: Type = Type {
    kind: b'4',
    affine: false,
    reflexive: true,
    inline_range: false,
    inline_bytes: 0,
    key: None,
    value: None,
    name: "",
    variants: &[],
    drop: None,
    wrapper: None,
    plan: None,
    redacted: false,
    transparent: false,
    debug: None,
    display: None,
    quantity: None,
    finite: None,
    destroy: None,
    array: None,
};
static BOX: Type = Type {
    kind: b'O',
    affine: true,
    key: Some(&INTEGER),
    ..INTEGER
};
static RANGE: Type = Type {
    kind: b'R',
    ..INTEGER
};
static RECORD: Type = Type {
    kind: b'C',
    affine: true,
    inline_bytes: 96,
    variants: &[
        Variant {
            name: "id",
            fields: &[&INTEGER],
        },
        Variant {
            name: "span",
            fields: &[&RANGE],
        },
        Variant {
            name: "child",
            fields: &[&BOX],
        },
    ],
    drop: Some(record_drop),
    ..INTEGER
};
static RECORD_BOX: Type = Type {
    key: Some(&RECORD),
    ..BOX
};
thread_local! { static DROPS: RefCell<Vec<u128>> = const { RefCell::new(Vec::new()) }; }
unsafe extern "C" fn record_drop(receiver: *mut u128) {
    // The receiver is the virtual slot preceding the flat record. The state
    // word is at +1; id at +2; the range word at +3 and its payload at +4.
    unsafe {
        let id = receiver.add(2).read();
        let range = receiver.add(3).read() as *const ranges::Range;
        assert_eq!(range.cast::<u128>(), receiver.add(4));
        assert_eq!((*range).start as u128, id);
        DROPS.with(|drops| drops.borrow_mut().push(id));
    }
}
unsafe fn record(id: u64, storage: &mut [u128; 6]) {
    unsafe {
        storage[0] = 1; // armed custom destructor
        storage[1] = id as u128;
        storage[2] = storage.as_ptr().add(3) as u128;
        storage
            .as_mut_ptr()
            .add(3)
            .cast::<ranges::Range>()
            .write(ranges::Range::new(id, id + 3, 1, true));
        storage[5] =
            allocator_owners::box_new_in(&Allocator::system(), &BOX, id as u128).unwrap() as u128;
    }
}

struct Budget {
    remaining: usize,
    calls: Arc<AtomicUsize>,
    releases: Arc<AtomicUsize>,
}
impl Provider for Budget {
    fn allocate(&mut self, layout: Layout) -> Result<Block, AllocError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.remaining == 0 {
            return Err(AllocError::OutOfMemory);
        }
        self.remaining -= 1;
        Allocator::system().allocate(layout)
    }
    fn release(&mut self, block: Block) {
        self.releases.fetch_add(1, Ordering::Relaxed);
        assert!(block.bytes().iter().all(|byte| *byte == 0));
        drop(block);
    }
}
unsafe extern "C" fn allocate(_: *mut Header, size: u64, align: u64, out: *mut u64) -> u64 {
    assert!(callback_active());
    let block = Allocator::system()
        .allocate(Layout::from_size_align(size as usize, align as usize).unwrap())
        .unwrap();
    unsafe { bindings::write_block(out as u128, block) };
    PROVIDER_OK
}
unsafe extern "C" fn release_block(_: *mut Header, words: *const u64) {
    assert!(callback_active());
    unsafe {
        drop(ManuallyDrop::into_inner(bindings::read_block(
            words as u128,
        )))
    };
}
fn operands(bootstrap: &Allocator, source: &[u128; 6]) -> [u128; 4] {
    [
        bootstrap.0.map_or(0, |p| p.as_ptr() as u128),
        source.as_ptr() as u128,
        allocate as *const () as usize as u128,
        release_block as *const () as usize as u128,
    ]
}

#[test]
fn compiled_registration_has_exactly_two_bootstrap_allocations_and_transactional_rollback() {
    for budget in 0..=2 {
        DROPS.with(|drops| drops.borrow_mut().clear());
        let calls = Arc::new(AtomicUsize::new(0));
        let releases = Arc::new(AtomicUsize::new(0));
        let bootstrap = Allocator::new(
            &Allocator::system(),
            Budget {
                remaining: budget,
                calls: calls.clone(),
                releases: releases.clone(),
            },
        )
        .unwrap_or_else(|_| panic!("bootstrap setup"));
        let mut source = [0; 6];
        unsafe {
            record(73, &mut source);
            let before = source;
            let result = register(operands(&bootstrap, &source), &RECORD_BOX);
            assert_eq!(source, before, "registration mutated original inline P");
            assert_eq!(calls.load(Ordering::Relaxed), (budget + 1).min(2));
            DROPS.with(|drops| assert!(drops.borrow().is_empty()));
            if budget < 2 {
                assert_eq!(
                    result,
                    (1u128 << 64) | u128::from(REGISTRATION_OUT_OF_MEMORY)
                );
                assert_eq!(releases.load(Ordering::Relaxed), budget);
                // The original still has its nested child and relocated range.
                release(source.as_ptr() as u128, &RECORD);
            } else {
                assert_eq!(result >> 64, 0);
                source.fill(0); // std commit: no destructor, sole owner is box
                let allocator = Allocator(NonNull::new(result as *mut ProviderHeader));
                drop(allocator.allocate(Layout::new::<u64>()).unwrap());
                drop(allocator);
                assert_eq!(releases.load(Ordering::Relaxed), 2);
            }
        }
        DROPS.with(|drops| assert_eq!(&*drops.borrow(), &[73]));
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[test]
fn compiled_registration_rejects_context_before_allocation_or_source_access() {
    let mut arena = crate::actor_heap::Arena::new(4096).unwrap();
    let result = crate::actor_heap::within(&mut arena, || unsafe {
        // Intentionally invalid operands: none may be accessed on rejection.
        register([0, 1, 1, 1], std::ptr::null())
    })
    .unwrap();
    assert_eq!(
        result,
        (1u128 << 64) | u128::from(REGISTRATION_UNSUPPORTED_CONTEXT)
    );
    assert_eq!(arena.high_water(), 0);
    DROPS.with(|drops| drops.borrow_mut().clear());
    let mut source = [0; 6];
    unsafe {
        record(91, &mut source);
        let before = source;
        let result = crate::actor_heap::within(&mut arena, || {
            register(operands(&Allocator::system(), &source), &RECORD_BOX)
        })
        .unwrap();
        assert_eq!(
            result,
            (1u128 << 64) | u128::from(REGISTRATION_UNSUPPORTED_CONTEXT)
        );
        assert_eq!(source, before);
        DROPS.with(|drops| assert!(drops.borrow().is_empty()));
        release(source.as_ptr() as u128, &RECORD);
    }
    DROPS.with(|drops| assert_eq!(&*drops.borrow(), &[91]));
}

#[test]
fn compiled_states_owning_selected_text_share_one_protected_walk() {
    static TEXT: Type = Type {
        kind: b's',
        ..INTEGER
    };
    static STATE: Type = Type {
        kind: b'C',
        affine: true,
        inline_bytes: 32,
        variants: &[Variant {
            name: "child",
            fields: &[&TEXT],
        }],
        drop: Some(state_hook),
        ..INTEGER
    };
    static STATE_BOX: Type = Type {
        key: Some(&STATE),
        ..BOX
    };
    thread_local! {
        static DEPTHS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
        static PROTECTED_HOOKS: Cell<usize> = const { Cell::new(0) };
    }
    unsafe extern "C" fn state_hook(_: *mut u128) {
        PROTECTED_HOOKS.with(|n| n.set(n.get() + usize::from(callback_active())));
    }
    unsafe extern "C" fn release(_: *mut Header, words: *const u64) {
        DEPTHS.with(|d| d.borrow_mut().push(memory::queue_depth()));
        unsafe { release_block(ptr::null_mut(), words) };
    }
    // Measure nesting directly, not by making a recursive implementation run
    // out of stack. Each state has exactly one child, with a distinct provider.
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| unsafe {
            let mut text = 1; // empty inline string
            for _ in 0..8 {
                let state = [1, text]; // armed hook, then child
                let handle = register(
                    [
                        0,
                        state.as_ptr() as u128,
                        allocate as *const () as u128,
                        release as *const () as u128,
                    ],
                    &STATE_BOX,
                );
                assert_eq!(handle >> 64, 0);
                let allocator = Allocator(NonNull::new(handle as *mut ProviderHeader));
                text = crate::strings::try_stepped_slice_range_in(
                    "long text",
                    (0, 1, 9),
                    Some(&allocator),
                )
                .unwrap() as u128;
            }
            plenty_release(text as *mut Header);
            // First callback is outside a state walk. All remaining releases are
            // isolated callbacks within the same walk, not a fresh walk per edge.
            DEPTHS.with(|d| assert_eq!(&*d.borrow(), &[1, 2, 2, 2, 2, 2, 2, 2]));
            assert_eq!(PROTECTED_HOOKS.with(Cell::get), 8);
            assert!(!callback_active());
            assert_eq!(memory::queue_depth(), 0);
        })
        .unwrap()
        .join()
        .unwrap();
}
