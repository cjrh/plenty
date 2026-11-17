use super::*;
use crate::allocator::Provider;
use std::cell::RefCell;
use std::ptr;
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

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
