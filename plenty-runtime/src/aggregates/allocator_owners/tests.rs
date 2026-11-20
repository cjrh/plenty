use super::*;
use crate::allocator::Provider;
use std::cell::RefCell;
use std::ptr;
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[test]
#[cfg(feature = "allocation-checks")]
fn compiled_binding_reports_failures_and_cleans_partial_lists() {
    use crate::aggregates::wrap;
    use crate::allocator::bindings::operation;
    use crate::collection_opcodes::*;
    unsafe {
        crate::accounting::fail_after(Some(0));
        let root = operation(ALLOCATOR_SYSTEM, 0, 0, ptr::null());
        assert_eq!(root, 0);
        assert_eq!(operation(ALLOCATOR_SHARE, root, 0, ptr::null()), 0);
        operation(ALLOCATOR_RELEASE, root, 0, ptr::null());
        crate::accounting::fail_after(None);
        assert_eq!(
            operation(ALLOCATOR_LIST, root, u64::MAX as u128, &LIST),
            wrap(wrap(0, AllocError::CapacityOverflow as u64), 1)
        );
        for successes in 0..=3 {
            crate::accounting::fail_after(Some(successes));
            let result = operation(ALLOCATOR_LIST, root, 8, &LIST);
            crate::accounting::fail_after(None);
            if successes < 3 {
                assert_eq!(result, wrap(wrap(0, AllocError::OutOfMemory as u64), 1));
            } else {
                assert_ne!(result, 0);
                assert_eq!(result >> 64, 0);
                plenty_release(result as *mut Header);
            }
        }
    }
}

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
static LIST: Type = Type {
    kind: b'L',
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
static RECORD_LIST: Type = Type {
    key: Some(&RECORD),
    ..LIST
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
        storage[5] = try_box_new_in(&Allocator::system(), &BOX, id as u128).unwrap() as u128;
    }
}

struct Counts {
    calls: AtomicUsize,
    releases: AtomicUsize,
    drops: AtomicUsize,
    budget: AtomicIsize,
    events: Arc<Mutex<Vec<&'static str>>>,
}
impl Counts {
    fn new(events: Arc<Mutex<Vec<&'static str>>>) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            releases: AtomicUsize::new(0),
            drops: AtomicUsize::new(0),
            budget: AtomicIsize::new(-1),
            events,
        })
    }
}
struct Policy {
    counts: Arc<Counts>,
    label: &'static str,
}
impl Provider for Policy {
    fn allocate(&mut self, layout: Layout) -> Result<Block, AllocError> {
        self.counts.calls.fetch_add(1, Ordering::Relaxed);
        let budget = self.counts.budget.load(Ordering::Relaxed);
        if budget == 0 {
            return Err(AllocError::OutOfMemory);
        }
        if budget > 0 {
            self.counts.budget.fetch_sub(1, Ordering::Relaxed);
        }
        Allocator::system().allocate(layout)
    }
    fn release(&mut self, block: Block) {
        assert!(block.bytes().iter().all(|byte| *byte == 0));
        self.counts.releases.fetch_add(1, Ordering::Relaxed);
        self.counts.events.lock().unwrap().push(self.label);
        drop(block);
    }
}
impl Drop for Policy {
    fn drop(&mut self) {
        self.counts.drops.fetch_add(1, Ordering::Relaxed);
    }
}
fn allocator(counts: &Arc<Counts>, label: &'static str) -> Allocator {
    Allocator::new(
        &Allocator::system(),
        Policy {
            counts: counts.clone(),
            label,
        },
    )
    .unwrap_or_else(|_| panic!("bootstrap failed"))
}
fn counts() -> Arc<Counts> {
    Counts::new(Arc::new(Mutex::new(Vec::new())))
}

#[test]
fn box_uses_selected_storage_and_drops_nested_owned_record_once() {
    DROPS.with(|drops| drops.borrow_mut().clear());
    let counts = counts();
    let allocator = allocator(&counts, "box");
    let mut source = [0; 6];
    unsafe {
        record(7, &mut source);
        let boxed = try_box_new_in(&allocator, &RECORD_BOX, source.as_ptr() as u128).unwrap();
        source.fill(0); // invalidate caller storage: the payload must have moved
        drop(allocator);
        assert_eq!(counts.drops.load(Ordering::Relaxed), 0);
        plenty_release(boxed.cast());
    }
    DROPS.with(|drops| assert_eq!(&*drops.borrow(), &[7]));
    assert_eq!(counts.calls.load(Ordering::Relaxed), 1);
    assert_eq!(counts.releases.load(Ordering::Relaxed), 1);
    assert_eq!(counts.drops.load(Ordering::Relaxed), 1);
}

#[test]
fn failed_box_allocation_leaves_input_owned_by_caller() {
    DROPS.with(|drops| drops.borrow_mut().clear());
    let counts = counts();
    let allocator = allocator(&counts, "box");
    counts.budget.store(0, Ordering::Relaxed);
    unsafe {
        let mut source = [0; 6];
        record(9, &mut source);
        assert!(matches!(
            try_box_new_in(&allocator, &RECORD_BOX, source.as_ptr() as u128),
            Err(AllocError::OutOfMemory)
        ));
        DROPS.with(|drops| assert!(drops.borrow().is_empty()));
        release(source.as_ptr() as u128, &RECORD);
    }
    DROPS.with(|drops| assert_eq!(&*drops.borrow(), &[9]));
    assert_eq!(counts.releases.load(Ordering::Relaxed), 0);
}

#[test]
fn selected_box_waits_for_child_even_with_fast_release_descriptor() {
    unsafe extern "C" fn fast_release(slot: *mut u128) {
        unsafe { plenty_release(slot.read() as *mut Header) };
    }
    static OUTER: Type = Type {
        key: Some(&BOX),
        drop: Some(fast_release),
        ..BOX
    };
    let events = Arc::new(Mutex::new(Vec::new()));
    let parent = Counts::new(events.clone());
    let child = Counts::new(events.clone());
    let a = allocator(&parent, "parent");
    let b = allocator(&child, "child");
    unsafe {
        let inner = try_box_new_in(&b, &BOX, 42).unwrap();
        let outer = try_box_new_in(&a, &OUTER, inner as u128).unwrap();
        drop(a);
        drop(b);
        plenty_release(outer.cast());
    }
    assert_eq!(&*events.lock().unwrap(), &["child", "parent"]);
    assert_eq!(parent.drops.load(Ordering::Relaxed), 1);
    assert_eq!(child.drops.load(Ordering::Relaxed), 1);
}

#[test]
fn list_object_growth_and_failed_growth_use_one_selected_provider() {
    let counts = counts();
    let allocator = allocator(&counts, "list");
    unsafe {
        let list = try_list_new_in(&allocator, &LIST, 2).unwrap();
        (*list).entries.push(Entry { key: 11, value: 0 });
        (*list).entries.push(Entry { key: 22, value: 0 });
        assert_eq!(counts.calls.load(Ordering::Relaxed), 3); // state + rows + object
        (*list).try_reserve(10).unwrap();
        assert_eq!(counts.calls.load(Ordering::Relaxed), 4);
        assert_eq!(counts.releases.load(Ordering::Relaxed), 1); // old row block
        let capacity = (*list).entries.capacity();
        let rows = plenty_list_rows(list.cast());
        counts.budget.store(0, Ordering::Relaxed);
        assert_eq!((*list).try_reserve(100), Err(AllocError::OutOfMemory));
        assert_eq!((*list).entries.capacity(), capacity);
        assert_eq!(plenty_list_rows(list.cast()), rows);
        assert_eq!((*list).len(), 2);
        assert_eq!((*list).entries.get(0).key, 11);
        assert_eq!((*list).entries.get(1).key, 22);
        drop(allocator);
        assert_eq!(counts.drops.load(Ordering::Relaxed), 0);
        plenty_release(list.cast());
    }
    assert_eq!(counts.releases.load(Ordering::Relaxed), 4);
    assert_eq!(counts.drops.load(Ordering::Relaxed), 1);
}

#[test]
fn list_growth_relocates_nested_payloads_and_pop_transfers_destruction() {
    DROPS.with(|drops| drops.borrow_mut().clear());
    let counts = counts();
    let allocator = allocator(&counts, "list");
    unsafe {
        let list = try_list_new_in(&allocator, &RECORD_LIST, 0).unwrap();
        for id in 0..6 {
            (*list).try_reserve(1).unwrap();
            let mut source = [0; 6];
            record(id, &mut source);
            (*list).entries.push(Entry {
                key: source.as_ptr() as u128,
                value: 0,
            });
            source.fill(0);
        }
        let extracted = (*list).entries.remove(2);
        let mut snapshot = [0u128; 7];
        ranges::store(snapshot.as_mut_ptr(), extracted.key, &RECORD);
        (*list).try_reserve(20).unwrap();
        release(snapshot[0], &RECORD);
        drop(allocator);
        plenty_release(list.cast());
    }
    DROPS.with(|drops| assert_eq!(&*drops.borrow(), &[2, 0, 1, 3, 4, 5]));
    assert_eq!(
        counts.calls.load(Ordering::Relaxed),
        counts.releases.load(Ordering::Relaxed)
    );
    assert_eq!(counts.drops.load(Ordering::Relaxed), 1);
}

#[test]
fn failed_list_construction_releases_only_successful_partial_allocations() {
    for successes in 0..3 {
        let counts = counts();
        let allocator = allocator(&counts, "list");
        counts.budget.store(successes, Ordering::Relaxed);
        assert!(matches!(
            try_list_new_in(&allocator, &RECORD_LIST, 4),
            Err(AllocError::OutOfMemory)
        ));
        assert_eq!(counts.releases.load(Ordering::Relaxed), successes as usize);
        drop(allocator);
        assert_eq!(counts.drops.load(Ordering::Relaxed), 1);
    }
    let counts = counts();
    let allocator = allocator(&counts, "overflow");
    assert!(matches!(
        try_list_new_in(&allocator, &LIST, usize::MAX),
        Err(AllocError::CapacityOverflow)
    ));
    assert_eq!(counts.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn empty_list_retains_provider_and_box_can_be_taken_without_destroying_content() {
    let counts = counts();
    let allocator = allocator(&counts, "owner");
    unsafe {
        let list = try_list_new_in(&allocator, &LIST, 0).unwrap();
        let boxed = try_box_new_in(&allocator, &BOX, 55).unwrap();
        assert_eq!(Boxed::content(boxed, &INTEGER).read(), 55);
        box_take(boxed, &BOX);
        assert_eq!(counts.releases.load(Ordering::Relaxed), 1);
        drop(allocator);
        assert_eq!(counts.drops.load(Ordering::Relaxed), 0);
        plenty_release(list.cast());
    }
    assert_eq!(counts.calls.load(Ordering::Relaxed), 3);
    assert_eq!(counts.releases.load(Ordering::Relaxed), 3);
    assert_eq!(counts.drops.load(Ordering::Relaxed), 1);
}

#[test]
fn final_builtin_owners_can_drop_on_worker() {
    let counts = counts();
    let allocator = allocator(&counts, "worker");
    let (list, boxed) = unsafe {
        (
            try_list_new_in(&allocator, &LIST, 1)
                .unwrap()
                .expose_provenance(),
            try_box_new_in(&allocator, &BOX, 8)
                .unwrap()
                .expose_provenance(),
        )
    };
    drop(allocator);
    std::thread::spawn(move || unsafe {
        plenty_release(ptr::with_exposed_provenance_mut(list));
        plenty_release(ptr::with_exposed_provenance_mut(boxed));
    })
    .join()
    .unwrap();
    assert_eq!(counts.releases.load(Ordering::Relaxed), 4);
    assert_eq!(counts.drops.load(Ordering::Relaxed), 1);
}

#[test]
fn selected_text_slices_keep_one_block_until_last_worker_release() {
    let counts = counts();
    let out = allocator(&counts, "text");
    counts.budget.store(1, Ordering::Relaxed);
    let slice =
        |source, positions| strings::try_stepped_slice_range_in(source, positions, Some(&out));
    let short = slice("界ab🙂", (3, -3, 2)).unwrap();
    assert!(strings::is_inline(short));
    assert_eq!(unsafe { strings::text(short) }, "🙂界");
    assert_eq!(counts.calls.load(Ordering::Relaxed), 0);
    let long = slice("界ab🙂", (3, -1, 4)).unwrap();
    assert_eq!(slice("abcdefgh", (0, 1, 8)), Err(AllocError::OutOfMemory));
    drop(out);
    // Ordinary slices copy into ordinary storage even when the source's
    // provider is now exhausted. Retained word copies keep the same prefix.
    let ordinary = unsafe {
        crate::memory::plenty_retain(long.cast());
        let text = strings::try_stepped_slice(long, (3, -1, 4)).unwrap();
        strings::release_text(long);
        text
    };
    assert_eq!(counts.calls.load(Ordering::Relaxed), 2);
    assert_eq!(counts.drops.load(Ordering::Relaxed), 0);
    let address = long.expose_provenance();
    std::thread::spawn(move || unsafe {
        let text = ptr::with_exposed_provenance_mut(address);
        assert_eq!(strings::text(text), "🙂ba界");
        strings::release_text(text);
    })
    .join()
    .unwrap();
    assert_eq!(counts.releases.load(Ordering::Relaxed), 1);
    assert_eq!(counts.drops.load(Ordering::Relaxed), 1);
    unsafe {
        assert_eq!(strings::text(ordinary), "界ab🙂");
        strings::release_text(ordinary);
    }
}

#[test]
fn selected_lists_keep_dense_scalar_widths_and_compiler_offsets() {
    static BYTE: Type = Type {
        kind: b'5',
        ..INTEGER
    };
    static SHORT: Type = Type {
        kind: b'6',
        ..INTEGER
    };
    static FLOAT: Type = Type {
        kind: b'f',
        ..INTEGER
    };
    static BYTE_LIST: Type = Type {
        key: Some(&BYTE),
        ..LIST
    };
    static SHORT_LIST: Type = Type {
        key: Some(&SHORT),
        ..LIST
    };
    static FLOAT_LIST: Type = Type {
        key: Some(&FLOAT),
        ..LIST
    };
    for (ty, width) in [
        (&BYTE_LIST, 1),
        (&SHORT_LIST, 2),
        (&FLOAT_LIST, 4),
        (&LIST, 8),
    ] {
        unsafe {
            let list = try_list_new_in(&Allocator::system(), ty, 0).unwrap();
            for value in 1..=19 {
                (*list).try_reserve(1).unwrap();
                (*list).entries.push(Entry {
                    key: value,
                    value: 0,
                });
            }
            assert_eq!(plenty_list_row_bytes(list.cast()), width);
            assert_eq!(plenty_list_len(list.cast()), 19);
            let rows = list
                .cast::<u8>()
                .add(crate::list_layout::ROWS as usize)
                .cast::<*mut u128>()
                .read();
            assert_eq!(rows, plenty_list_rows(list.cast()));
            for index in 0..19 {
                assert_eq!((*list).entries.get(index).key, index as u128 + 1);
            }
            plenty_release(list.cast());
        }
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn instrumented_system_failures_release_partial_lists_without_fallback() {
    struct Counting(Arc<AtomicUsize>);
    impl Provider for Counting {
        fn allocate(&mut self, layout: Layout) -> Result<Block, AllocError> {
            Allocator::system().allocate(layout)
        }
        fn release(&mut self, block: Block) {
            self.0.fetch_add(1, Ordering::Relaxed);
            drop(block); // no logging allocation while failures are armed
        }
    }
    for successes in 0..3 {
        let releases = Arc::new(AtomicUsize::new(0));
        let allocator = Allocator::new(&Allocator::system(), Counting(releases.clone()))
            .unwrap_or_else(|_| panic!("bootstrap failed"));
        crate::accounting::fail_after(Some(successes));
        let result = try_list_new_in(&allocator, &LIST, 4);
        crate::accounting::fail_after(None);
        assert!(matches!(result, Err(AllocError::OutOfMemory)));
        assert_eq!(releases.load(Ordering::Relaxed), successes);
        unsafe {
            let list = try_list_new_in(&allocator, &LIST, 1).unwrap();
            (*list).entries.push(Entry { key: 17, value: 0 });
            crate::accounting::fail_after(Some(0));
            let grown = (*list).try_reserve(100);
            let boxed = try_box_new_in(&allocator, &BOX, 42);
            crate::accounting::fail_after(None);
            assert_eq!(grown, Err(AllocError::OutOfMemory));
            assert!(matches!(boxed, Err(AllocError::OutOfMemory)));
            assert_eq!((*list).entries.get(0).key, 17);
            plenty_release(list.cast());
        }
    }
}

#[test]
fn selected_box_graph_destruction_remains_iterative() {
    static NODE: Type = Type {
        kind: b'C',
        affine: true,
        inline_bytes: 32,
        variants: &[
            Variant {
                name: "id",
                fields: &[&INTEGER],
            },
            Variant {
                name: "child",
                fields: &[&NODE_BOX],
            },
        ],
        ..INTEGER
    };
    static NODE_BOX: Type = Type {
        key: Some(&NODE),
        ..BOX
    };
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let counts = counts();
            let allocator = allocator(&counts, "node");
            let mut child = 0;
            for id in 1..=10_000 {
                // Zero child is the runtime's empty/moved-out owner sentinel.
                let source = [id, child];
                child = unsafe {
                    try_box_new_in(&allocator, &NODE_BOX, source.as_ptr() as u128).unwrap() as u128
                };
            }
            drop(allocator);
            unsafe { plenty_release(child as *mut Header) };
            assert_eq!(counts.releases.load(Ordering::Relaxed), 10_000);
            assert_eq!(counts.drops.load(Ordering::Relaxed), 1);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn selected_box_binding_preserves_failure_input_and_encodes_success() {
    use crate::allocator::bindings::operation;
    use crate::collection_opcodes::ALLOCATOR_BOX;
    DROPS.with(|drops| drops.borrow_mut().clear());
    unsafe {
        let mut source = [0; 6];
        record(93, &mut source);
        #[cfg(feature = "allocation-checks")]
        {
            let before = source;
            crate::accounting::fail_after(Some(0));
            let result = operation(ALLOCATOR_BOX, 0, source.as_ptr() as u128, &RECORD_BOX);
            crate::accounting::fail_after(None);
            assert_eq!(result, (1 + 2 * AllocError::OutOfMemory as u128) << 64);
            assert_eq!(source, before);
            DROPS.with(|drops| assert!(drops.borrow().is_empty()));
        }
        let result = operation(ALLOCATOR_BOX, 0, source.as_ptr() as u128, &RECORD_BOX);
        assert_ne!(result, 0);
        assert_eq!(result >> 64, 0);
        source.fill(0);
        plenty_release(result as *mut Header);
    }
    DROPS.with(|drops| assert_eq!(&*drops.borrow(), &[93]));
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[test]
fn allocator_dispatch_rejects_arena_before_operand_access() {
    use crate::collection_opcodes::{
        ALLOCATOR_BOX, ALLOCATOR_BYTES_SLICE, ALLOCATOR_CLIST_SLICE, ALLOCATOR_COPY,
        ALLOCATOR_DICT_SET, ALLOCATOR_KEY_VIEW_TO_SET, ALLOCATOR_LIST, ALLOCATOR_LIST_SLICE,
        ALLOCATOR_SET_SYMMETRIC_DIFFERENCE, ALLOCATOR_SET_UNION, ALLOCATOR_SYSTEM,
        ALLOCATOR_TEXT_CASE, ALLOCATOR_TEXT_CONCAT, ALLOCATOR_TEXT_FINISH, ALLOCATOR_TEXT_JOIN,
        ALLOCATOR_TEXT_REPEAT, ALLOCATOR_TEXT_REPLACE, ALLOCATOR_TEXT_SLICE, ALLOCATOR_TEXT_SPLIT,
        ALLOCATOR_TEXT_SPLIT_WHITESPACE, ALLOCATOR_TEXT_STRIP,
    };
    const PROBE: &str = "PLENTY_SELECTED_BOX_ARENA_PROBE";
    if let Ok(probe) = std::env::var(PROBE) {
        let (op, entry) = probe.split_once(':').unwrap();
        let op = op.parse::<i64>().unwrap();
        let mut arena = crate::actor_heap::Arena::new(4096).unwrap();
        let _ = crate::actor_heap::within(&mut arena, || unsafe {
            // Issue #994: a direct binding call misses earlier descriptor reads.
            // The argument buffer is valid; none of its pointer operands are.
            if entry == "operation" {
                let mut args = [1u128; 6];
                let args = if matches!(
                    op,
                    ALLOCATOR_COPY
                        | ALLOCATOR_KEY_VIEW_TO_SET
                        | ALLOCATOR_LIST_SLICE
                        | ALLOCATOR_BYTES_SLICE
                        | ALLOCATOR_TEXT_CONCAT
                        | ALLOCATOR_TEXT_JOIN
                        | ALLOCATOR_TEXT_FINISH
                        | ALLOCATOR_TEXT_REPEAT
                        | ALLOCATOR_TEXT_REPLACE
                        | ALLOCATOR_TEXT_STRIP
                        | ALLOCATOR_TEXT_CASE
                        | ALLOCATOR_TEXT_SPLIT
                        | ALLOCATOR_TEXT_SPLIT_WHITESPACE
                        | ALLOCATOR_TEXT_SLICE
                        | ALLOCATOR_CLIST_SLICE
                ) {
                    // Reject before loading even the allocator handle.
                    ptr::dangling()
                } else {
                    args.as_mut_ptr()
                };
                crate::aggregates::operation(op, args, ptr::dangling(), ptr::null());
            } else {
                crate::aggregates::collection_at(op, 1, 1, 0, ptr::dangling(), ptr::null());
            }
        });
        panic!("allocator arena guard did not terminate");
    }
    for op in (ALLOCATOR_SYSTEM..=ALLOCATOR_LIST)
        .chain([
            ALLOCATOR_BOX,
            ALLOCATOR_DICT_SET,
            ALLOCATOR_COPY,
            ALLOCATOR_KEY_VIEW_TO_SET,
            ALLOCATOR_LIST_SLICE,
            ALLOCATOR_BYTES_SLICE,
            ALLOCATOR_TEXT_CONCAT,
            ALLOCATOR_TEXT_JOIN,
            ALLOCATOR_TEXT_FINISH,
            ALLOCATOR_TEXT_REPEAT,
            ALLOCATOR_TEXT_REPLACE,
            ALLOCATOR_TEXT_STRIP,
            ALLOCATOR_TEXT_CASE,
            ALLOCATOR_TEXT_SPLIT,
            ALLOCATOR_TEXT_SPLIT_WHITESPACE,
            ALLOCATOR_TEXT_SLICE,
            ALLOCATOR_CLIST_SLICE,
        ])
        .chain(ALLOCATOR_SET_UNION..=ALLOCATOR_SET_SYMMETRIC_DIFFERENCE)
    {
        // List slicing has no collection_at entry; byte slicing defensively
        // rejects that entry before reading a descriptor.
        let entries: &[_] = if op == ALLOCATOR_LIST_SLICE {
            &["operation"]
        } else {
            &["operation", "collection_at"]
        };
        for entry in entries {
            let probe = format!("{op}:{entry}");
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "aggregates::allocator_owners::tests::allocator_dispatch_rejects_arena_before_operand_access",
                    "--nocapture",
                ])
                .env(PROBE, &probe)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while child.try_wait().unwrap().is_none() {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let output = child.wait_with_output().unwrap();
                    panic!("{probe} timed out: {output:?}");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let output = child.wait_with_output().unwrap();
            assert_eq!(output.status.code(), Some(1), "{probe}: {output:?}");
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .contains("retained allocator use is unsupported in actor or arena contexts"),
                "{probe}: {output:?}"
            );
        }
    }
}

mod finite_sets;
mod hash_storage;
mod key_view_to_set;
mod list_slice;
mod recursive_copy;
mod set_algebra;

#[test]
fn selected_clist_retains_one_block_and_checks_growth_before_provider_calls() {
    static ROW: Type = Type {
        wrapper: Some(&Variant {
            name: "n",
            fields: &[&INTEGER],
        }),
        ..INTEGER
    };
    static COLUMNS: Type = Type {
        kind: b'Q',
        key: Some(&ROW),
        ..INTEGER
    };
    let counts = counts();
    let allocator = allocator(&counts, "columns");
    unsafe {
        // A selected empty has a real owner, unlike an ordinary empty slice.
        let mut owner = clist::slice(0, [0, 0, -1, 0], &COLUMNS, Some(&allocator)).unwrap();
        assert_ne!(owner, 0);
        drop(allocator);
        let slot = ptr::from_mut(&mut owner) as u128;
        let reserve = |n: i64| clist::call(521, [slot, n as u128, 0, 0], &COLUMNS, ptr::null());
        assert_eq!(
            reserve(i64::MAX),
            wrap(wrap(0, AllocError::CapacityOverflow as u64), 1)
        );
        assert_eq!(counts.calls.load(Ordering::Relaxed), 1);
        assert_eq!(reserve(8), 0);
        assert_eq!(counts.releases.load(Ordering::Relaxed), 1);
        let before = owner;
        counts.budget.store(0, Ordering::Relaxed);
        assert_eq!(reserve(9), wrap(wrap(0, AllocError::OutOfMemory as u64), 1));
        assert_eq!(owner, before);
        assert_eq!(counts.releases.load(Ordering::Relaxed), 1);
        assert_eq!(
            clist::slice(owner, [0, 0, 1, 0], &COLUMNS, None).unwrap(),
            0
        );
        plenty_release(owner as *mut Header);
    }
    assert_eq!(counts.releases.load(Ordering::Relaxed), 2);
    assert_eq!(counts.drops.load(Ordering::Relaxed), 1);
}
