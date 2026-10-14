//! Opt-in test instrumentation; absent from normal compiler/runtime builds.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Accounting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static BASELINE: AtomicUsize = AtomicUsize::new(0);
#[global_allocator]
static ALLOCATOR: Accounting = Accounting;

// Region baselines and failure budgets belong to the calling thread, so worker
// markers cannot reset a parent's region or attribute worker allocations to it.
// Constant TLS initializers do not allocate; live bytes remain process-wide
// because heap owners can move between threads.
thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    static REGION_START: Cell<usize> = const { Cell::new(0) };
    static FAIL_AFTER: Cell<Option<usize>> = const { Cell::new(None) };
}
// Regions mean "no memory is obtained": only successful allocations count, so
// an injected or real failure inside a region does not trip it (issue #120).
fn record_allocation() {
    ALLOCATIONS.with(|count| count.set(count.get().wrapping_add(1)));
}
pub(crate) fn fail_after(successes: Option<usize>) {
    FAIL_AFTER.with(|budget| budget.set(successes));
}
fn injected_failure() -> bool {
    FAIL_AFTER.with(|budget| match budget.get() {
        None => false,
        Some(0) => true,
        Some(n) => {
            budget.set(Some(n - 1));
            false
        }
    })
}

// SAFETY: layouts/pointers are forwarded unchanged to System. Counters neither
// allocate nor hold locks, and realloc changes accounting only on success.
unsafe impl GlobalAlloc for Accounting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if injected_failure() {
            return std::ptr::null_mut();
        }
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            record_allocation();
            LIVE.fetch_add(layout.size(), Relaxed);
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if injected_failure() {
            return std::ptr::null_mut();
        }
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            record_allocation();
            LIVE.fetch_add(layout.size(), Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        unsafe {
            System.dealloc(p, layout);
        }
    }
    unsafe fn realloc(&self, p: *mut u8, old: Layout, size: usize) -> *mut u8 {
        if injected_failure() {
            return std::ptr::null_mut();
        }
        let p = unsafe { System.realloc(p, old, size) };
        if !p.is_null() {
            record_allocation();
            LIVE.fetch_add(size, Relaxed);
            LIVE.fetch_sub(old.size(), Relaxed);
        }
        p
    }
}
pub(crate) fn begin_no_allocations() {
    REGION_START.with(|start| start.set(ALLOCATIONS.with(Cell::get)));
}
pub(crate) fn end_no_allocations() {
    if ALLOCATIONS.with(Cell::get) != REGION_START.with(Cell::get) {
        crate::fail("unexpected successful allocation in allocation-free region");
    }
}
#[cfg(plenty_runtime_embedded)]
pub(crate) fn start() {
    // Exclude process-owned standard I/O buffers, but count every runtime-owned
    // allocation: raw values, Vec buffers, and temporary renderings. Type
    // descriptors are immutable compiler-emitted static data.
    drop(std::io::stdout().lock());
    drop(std::io::stderr().lock());
    drop(std::io::stdin().lock());
    BASELINE.store(LIVE.load(Relaxed), Relaxed);
}
pub(crate) fn checkpoint() {
    // Workers must be idle or joined; their retained heap counts here too.
    let live = LIVE.load(Relaxed).saturating_sub(BASELINE.load(Relaxed));
    if live > 4096 {
        crate::fail(&format!("out-of-scope values retained: {live} bytes"));
    }
}
#[cfg(plenty_runtime_embedded)]
pub(crate) fn finish() {
    let live = LIVE.load(Relaxed);
    let baseline = BASELINE.load(Relaxed);
    if live != baseline {
        crate::fail(&format!(
            "runtime allocation imbalance: {live} bytes live, {baseline} bytes baseline"
        ));
    }
}
