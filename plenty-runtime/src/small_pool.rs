//! Size classes for small System allocations (issue #814). See "Small-block
//! pool" in book/src/design/toolchain/02-architecture/memory.md.
//!
//! - Every System-path block whose layout fits a class (at most 256 bytes,
//!   alignment at most 16) comes from here. The choice depends only on the
//!   layout, which `dealloc` and `realloc` also receive, so blocks carry no
//!   header: glibc's 8-byte chunk header and 16-byte rounding made a 64-byte
//!   box an 80-byte chunk.
//! - Blocks are carved from 64 KiB slabs aligned to their size, so a block's
//!   slab header is at its address rounded down.
//! - Each thread keeps a bounded cache per class. A freed block joins the
//!   freeing thread's cache, whichever thread allocated it. A full cache
//!   returns half its blocks to their slabs under one lock; thread exit
//!   returns all of them.
//! - A slab counts the blocks it has handed out, including those waiting in
//!   thread caches. When all have come back it joins the empty slabs, which
//!   any class reuses before mapping a new slab. An empty slab unused for
//!   `DECAY_MS` is unmapped the next time a slab empties, so a program that
//!   frees and rebuilds a large structure does not map and unmap its slabs
//!   each time.
//!
//! The router calls this only for System allocations: actor heaps and arenas
//! keep their own free lists, and the accounting allocator wraps the router,
//! so allocation checks and failure injection see every reuse.

use std::alloc::Layout;
use std::cell::UnsafeCell;
use std::ptr::null_mut;
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, Ordering};

const QUANTUM: usize = 16;
const MAX_BYTES: usize = 256;
const CLASSES: usize = MAX_BYTES / QUANTUM;
const SLAB_BYTES: usize = 1 << 16;
/// Bytes before a slab's first block: the header, rounded so blocks stay
/// 16-byte aligned.
const HEADER_BYTES: usize = size_of::<Slab>().next_multiple_of(QUANTUM);
// One cache line, which also keeps the slab's last-block waste small.
const _: () = assert!(HEADER_BYTES <= 64);
/// A thread caches at most about this many bytes per class.
const CACHE_BYTES: usize = 16 << 10;
/// Milliseconds an empty slab stays mapped without reuse.
const DECAY_MS: u64 = 1000;

/// The class serving `layout`, or `None` for System.
#[inline]
pub(crate) fn class_of(layout: Layout) -> Option<usize> {
    (layout.size() <= MAX_BYTES && layout.align() <= QUANTUM)
        .then(|| layout.size().saturating_sub(1) / QUANTUM)
}

const fn block_bytes(class: usize) -> usize {
    (class + 1) * QUANTUM
}

/// Blocks a thread may cache for `class`; a full cache returns half.
const fn cache_limit(class: usize) -> usize {
    CACHE_BYTES / block_bytes(class)
}

/// `cache_limit` by class, so a free does not divide.
const LIMITS: [usize; CLASSES] = {
    let mut limits = [0; CLASSES];
    let mut class = 0;
    while class < CLASSES {
        limits[class] = cache_limit(class);
        class += 1;
    }
    limits
};

#[repr(C)]
struct Slab {
    class: u32,
    listed: bool,
    /// Blocks handed out and not yet returned, whether in use or cached.
    live: usize,
    /// Returned blocks, each linking the next through its first word.
    free: *mut u8,
    /// The first byte never handed out.
    bump: *mut u8,
    /// The end of the last whole block.
    end: *mut u8,
    /// Links in the class's partial list while `listed`, or in the empty
    /// list once `live` is zero.
    prev: *mut Slab,
    next: *mut Slab,
    /// When the slab last became empty, in coarse monotonic milliseconds.
    emptied: u64,
}

impl Slab {
    fn first(slab: *mut Slab) -> *mut u8 {
        slab.cast::<u8>().wrapping_add(HEADER_BYTES)
    }
    fn of(block: *mut u8) -> *mut Slab {
        ((block as usize) & !(SLAB_BYTES - 1)) as *mut Slab
    }
}

struct Central {
    /// Per class, the slabs that can hand out a block, most recently listed
    /// first.
    partial: [*mut Slab; CLASSES],
    /// Empty slabs, most recently emptied first.
    newest: *mut Slab,
    oldest: *mut Slab,
}
// SAFETY: the slab pointers are reached only under `CENTRAL`'s lock.
unsafe impl Send for Central {}

static CENTRAL: Mutex<Central> = Mutex::new(Central::new());

#[cfg(test)]
thread_local! {
    /// Replaces this thread's clock, so decay tests do not depend on how
    /// long the test thread runs between steps (issue #889).
    static FIXED_MS: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Coarse monotonic milliseconds: a vDSO read with no system call.
fn now_ms() -> u64 {
    #[cfg(test)]
    if let Some(now) = FIXED_MS.with(std::cell::Cell::get) {
        return now;
    }
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: writes only `time`.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC_COARSE, &mut time) };
    time.tv_sec as u64 * 1000 + time.tv_nsec as u64 / 1_000_000
}

fn unmap(slab: *mut Slab) {
    // SAFETY: an empty slab is a whole mapping no block points into.
    unsafe { libc::munmap(slab.cast(), SLAB_BYTES) };
}

fn lock() -> std::sync::MutexGuard<'static, Central> {
    // Nothing panics while holding the lock, but a poisoned lock still
    // guards consistent lists.
    CENTRAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
thread_local! {
    /// Makes this thread's slab mappings fail.
    static REFUSE_MAPPING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A fresh `SLAB_BYTES` mapping aligned to its size, or null.
fn map_slab() -> *mut Slab {
    #[cfg(test)]
    if REFUSE_MAPPING.with(std::cell::Cell::get) {
        return null_mut();
    }
    // SAFETY: anonymous private mappings; every unmapped range lies inside
    // the mapping just made.
    unsafe {
        let map = |bytes| {
            libc::mmap(
                null_mut(),
                bytes,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        let first = map(SLAB_BYTES);
        if first == libc::MAP_FAILED {
            return null_mut();
        }
        let slab = if (first as usize).is_multiple_of(SLAB_BYTES) {
            first.cast::<u8>()
        } else {
            // Successive mappings usually land next to each other, so the
            // first one is often aligned; otherwise map twice the size and
            // trim it to an aligned slab.
            libc::munmap(first, SLAB_BYTES);
            let wide = map(2 * SLAB_BYTES);
            if wide == libc::MAP_FAILED {
                return null_mut();
            }
            let start = wide as usize;
            let aligned = start.next_multiple_of(SLAB_BYTES);
            if aligned > start {
                libc::munmap(wide, aligned - start);
            }
            let tail = start + 2 * SLAB_BYTES - (aligned + SLAB_BYTES);
            if tail > 0 {
                libc::munmap((aligned + SLAB_BYTES) as *mut libc::c_void, tail);
            }
            aligned as *mut u8
        };
        slab.cast()
    }
}

impl Central {
    const fn new() -> Self {
        Central {
            partial: [null_mut(); CLASSES],
            newest: null_mut(),
            oldest: null_mut(),
        }
    }

    fn link(&mut self, slab: *mut Slab) {
        // SAFETY: `slab` is a live slab, reached under the lock.
        unsafe {
            let head = &mut self.partial[(*slab).class as usize];
            (*slab).prev = null_mut();
            (*slab).next = *head;
            if !head.is_null() {
                (**head).prev = slab;
            }
            *head = slab;
            (*slab).listed = true;
        }
    }

    fn unlink(&mut self, slab: *mut Slab) {
        // SAFETY: as in `link`; `slab` is listed.
        unsafe {
            let (prev, next) = ((*slab).prev, (*slab).next);
            if prev.is_null() {
                self.partial[(*slab).class as usize] = next;
            } else {
                (*prev).next = next;
            }
            if !next.is_null() {
                (*next).prev = prev;
            }
            (*slab).listed = false;
        }
    }

    /// Add an emptied slab to the empty list, then unmap the slabs that have
    /// stayed empty for `DECAY_MS` before `now`.
    fn retire(&mut self, slab: *mut Slab, now: u64) {
        // SAFETY: empty slabs are reached only under the lock.
        unsafe {
            (*slab).emptied = now;
            (*slab).prev = null_mut();
            (*slab).next = self.newest;
            if self.newest.is_null() {
                self.oldest = slab;
            } else {
                (*self.newest).prev = slab;
            }
            self.newest = slab;
        }
        self.trim(now);
    }

    fn trim(&mut self, now: u64) {
        // SAFETY: as in `retire`.
        unsafe {
            while !self.oldest.is_null() && now.saturating_sub((*self.oldest).emptied) >= DECAY_MS {
                let slab = self.oldest;
                self.oldest = (*slab).prev;
                if self.oldest.is_null() {
                    self.newest = null_mut();
                } else {
                    (*self.oldest).next = null_mut();
                }
                unmap(slab);
            }
        }
    }

    /// The most recently emptied slab, whose pages are likeliest resident.
    fn reuse(&mut self) -> *mut Slab {
        let slab = self.newest;
        if !slab.is_null() {
            // SAFETY: as in `retire`.
            unsafe {
                self.newest = (*slab).next;
                if self.newest.is_null() {
                    self.oldest = null_mut();
                } else {
                    (*self.newest).prev = null_mut();
                }
            }
        }
        slab
    }

    /// A slab of `class` that can hand out a block, or null when none can be
    /// mapped.
    fn available(&mut self, class: usize) -> *mut Slab {
        let listed = self.partial[class];
        if !listed.is_null() {
            return listed;
        }
        let empty = self.reuse();
        let slab = if empty.is_null() { map_slab() } else { empty };
        if slab.is_null() {
            return slab;
        }
        // SAFETY: a fresh mapping, or an empty slab whose blocks have all
        // come back, so none is reachable and the slab can start over in
        // any class.
        unsafe {
            slab.write(Slab {
                class: class as u32,
                listed: false,
                live: 0,
                free: null_mut(),
                bump: Slab::first(slab),
                end: Slab::first(slab).wrapping_add(
                    (SLAB_BYTES - HEADER_BYTES) / block_bytes(class) * block_bytes(class),
                ),
                prev: null_mut(),
                next: null_mut(),
                emptied: 0,
            });
        }
        self.link(slab);
        slab
    }

    /// Move up to `want` blocks of `class` into `bin`, fewer only when no
    /// slab can be mapped. Each slab's count changes once per batch.
    fn fill(&mut self, class: usize, bin: &mut Bin, want: usize) {
        let size = block_bytes(class);
        let mut moved = 0;
        while moved < want {
            let slab = self.available(class);
            if slab.is_null() {
                return;
            }
            // SAFETY: a listed slab of `class` has returned blocks or room
            // past `bump`; both hold at least one word for the cache link.
            unsafe {
                let start = moved;
                while moved < want && !(*slab).free.is_null() {
                    let block = (*slab).free;
                    (*slab).free = block.cast::<*mut u8>().read();
                    bin.push(block);
                    moved += 1;
                }
                let mut bump = (*slab).bump;
                while moved < want && bump < (*slab).end {
                    bin.push(bump);
                    bump = bump.add(size);
                    moved += 1;
                }
                (*slab).bump = bump;
                (*slab).live += moved - start;
                if (*slab).free.is_null() && bump == (*slab).end {
                    self.unlink(slab);
                }
            }
        }
    }

    /// One block of `class`, or null when no slab can be mapped.
    fn take(&mut self, class: usize) -> *mut u8 {
        let mut bin = Bin {
            head: null_mut(),
            count: 0,
        };
        self.fill(class, &mut bin, 1);
        bin.head
    }

    /// Return `block`, which `take` handed out, to its slab.
    fn give(&mut self, block: *mut u8) {
        let slab = Slab::of(block);
        // SAFETY: the block lies in a live slab, whose header is at the
        // slab's aligned start; the block is at least one word long.
        unsafe {
            block.cast::<*mut u8>().write((*slab).free);
            (*slab).free = block;
            (*slab).live -= 1;
            if (*slab).live != 0 {
                if !(*slab).listed {
                    self.link(slab);
                }
                return;
            }
            if (*slab).listed {
                self.unlink(slab);
            }
        }
        self.retire(slab, now_ms());
    }
}

#[derive(Clone, Copy)]
struct Bin {
    head: *mut u8,
    count: usize,
}

struct Cache([Bin; CLASSES]);

impl Bin {
    fn pop(&mut self) -> *mut u8 {
        let block = self.head;
        // SAFETY: a cached block links the next through its first word.
        self.head = unsafe { block.cast::<*mut u8>().read() };
        self.count -= 1;
        block
    }
    fn push(&mut self, block: *mut u8) {
        // SAFETY: the caller owns the free block, at least one word long.
        unsafe { block.cast::<*mut u8>().write(self.head) };
        self.head = block;
        self.count += 1;
    }
    /// Return `count` cached blocks to their slabs.
    fn give_back(&mut self, central: &mut Central, count: usize) {
        for _ in 0..count {
            central.give(self.pop());
        }
    }
}

impl Drop for Cache {
    /// Thread exit returns the cache, so its blocks can be reused by other
    /// threads and its slabs can empty.
    fn drop(&mut self) {
        self.give_all(&mut lock());
    }
}

impl Cache {
    fn give_all(&mut self, central: &mut Central) {
        for bin in &mut self.0 {
            let count = bin.count;
            bin.give_back(central, count);
        }
    }
}

thread_local! {
    static CACHE: UnsafeCell<Cache> = const {
        UnsafeCell::new(Cache([Bin { head: null_mut(), count: 0 }; CLASSES]))
    };
}

/// Set only in `--test-hooks` builds. Their failure injection wraps malloc,
/// which pooled blocks bypass, so the hook object registers a gate that
/// spends the same per-thread budget for each pooled allocation.
static GATE: AtomicPtr<()> = AtomicPtr::new(null_mut());

type Gate = extern "C" fn() -> i8;

/// Called by `plenty-runtime/native/test_hooks.c` when a test first arms
/// allocation failures.
#[unsafe(no_mangle)]
pub extern "C" fn plenty_small_pool_gate(gate: Gate) {
    GATE.store(gate as *mut (), Ordering::Relaxed);
}

/// True when an injected failure refuses this allocation. Spends one unit of
/// the hook's budget, as each wrapped malloc or realloc does.
#[inline]
pub(crate) fn refused() -> bool {
    let gate = GATE.load(Ordering::Relaxed);
    !gate.is_null() && spend(gate)
}

#[cold]
#[inline(never)]
fn spend(gate: *mut ()) -> bool {
    // SAFETY: only `plenty_small_pool_gate` stores here, from a `Gate`.
    let gate: Gate = unsafe { std::mem::transmute::<*mut (), Gate>(gate) };
    gate() != 0
}

/// A block of `class` for this thread, or null when no slab can be mapped
/// or an injected failure refuses it.
#[inline]
pub(crate) fn alloc(class: usize) -> *mut u8 {
    if refused() {
        return null_mut();
    }
    CACHE
        .try_with(|cache| {
            // SAFETY: only this thread reaches its cache, and nothing here
            // reenters the allocator; `class_of` returns classes in range.
            let bin = unsafe { (*cache.get()).0.get_unchecked_mut(class) };
            if bin.head.is_null() {
                refill(bin, class)
            } else {
                bin.pop()
            }
        })
        // During thread exit the cache is gone: use the slabs directly.
        .unwrap_or_else(|_| lock().take(class))
}

/// Fill an empty bin with half its limit under one lock and pop a block.
#[cold]
#[inline(never)]
fn refill(bin: &mut Bin, class: usize) -> *mut u8 {
    let mut central = lock();
    central.fill(class, bin, cache_limit(class) / 2);
    // After `fill`, which prefers an empty slab to a new mapping: a program
    // that keeps allocating without emptying slabs still returns old ones.
    central.trim(now_ms());
    drop(central);
    if bin.head.is_null() {
        null_mut()
    } else {
        bin.pop()
    }
}

/// Free `block`, which `alloc(class)` returned on any thread.
#[inline]
pub(crate) fn free(block: *mut u8, class: usize) {
    #[cfg(feature = "allocation-checks")]
    check_free(block, class);
    let cached = CACHE.try_with(|cache| {
        // SAFETY: as in `alloc`.
        let bin = unsafe { (*cache.get()).0.get_unchecked_mut(class) };
        bin.push(block);
        // SAFETY: as above.
        if bin.count > unsafe { *LIMITS.get_unchecked(class) } {
            flush(bin);
        }
    });
    if cached.is_err() {
        lock().give(block);
    }
}

/// glibc tolerated a free with a layout other than the allocation's; here it
/// would put the block on another class's list. Abort instead.
#[cfg(feature = "allocation-checks")]
fn check_free(block: *mut u8, class: usize) {
    let slab = Slab::of(block);
    // SAFETY: a live block's slab is mapped, and its class and end do not
    // change until every block has come back.
    let (owner, end) = unsafe { ((*slab).class as usize, (*slab).end) };
    let offset = (block as usize).wrapping_sub(Slab::first(slab) as usize);
    if owner != class || block >= end || !offset.is_multiple_of(block_bytes(class)) {
        std::process::abort();
    }
}

/// Return half of a full bin under one lock.
#[cold]
#[inline(never)]
fn flush(bin: &mut Bin) {
    let half = bin.count / 2;
    let mut central = lock();
    central.trim(now_ms());
    bin.give_back(&mut central, half);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_cover_small_layouts_only() {
        let layout = |size, align| Layout::from_size_align(size, align).unwrap();
        assert_eq!(class_of(layout(1, 1)), Some(0));
        assert_eq!(class_of(layout(16, 16)), Some(0));
        assert_eq!(class_of(layout(17, 8)), Some(1));
        assert_eq!(class_of(layout(256, 16)), Some(CLASSES - 1));
        assert_eq!(class_of(layout(257, 8)), None);
        assert_eq!(class_of(layout(64, 32)), None);
    }

    #[test]
    fn blocks_are_distinct_aligned_and_reused() {
        let class = class_of(Layout::from_size_align(48, 16).unwrap()).unwrap();
        let blocks: Vec<_> = (0..5000).map(|_| alloc(class)).collect();
        let mut sorted = blocks.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), blocks.len());
        for &block in &blocks {
            assert!((block as usize).is_multiple_of(16));
            // SAFETY: a live block of 48 bytes.
            unsafe { block.write_bytes(0xA5, 48) };
        }
        for &block in &blocks {
            free(block, class);
        }
        let again = alloc(class);
        assert!(
            blocks.contains(&again),
            "the thread cache reuses freed blocks"
        );
        free(again, class);
    }

    /// Fixes this thread's clock until dropped. The thread's allocations
    /// also reach the shared pool, so a panicking test must not leave its
    /// thread-exit `give_all` stamping shared slabs with the fixed time.
    struct FixedClock;

    impl FixedClock {
        fn at(now: u64) -> Self {
            FIXED_MS.with(|fixed| fixed.set(Some(now)));
            Self
        }
    }

    impl Drop for FixedClock {
        fn drop(&mut self) {
            FIXED_MS.with(|fixed| fixed.set(None));
        }
    }

    fn empty_list(central: &Central) -> Vec<*mut Slab> {
        let mut slabs = Vec::new();
        let mut slab = central.newest;
        while !slab.is_null() {
            slabs.push(slab);
            // SAFETY: a private pool's empty slabs stay mapped until trimmed.
            slab = unsafe { (*slab).next };
        }
        slabs
    }

    /// A private central pool, so other tests' allocations cannot change
    /// the slabs observed.
    #[test]
    fn empty_slabs_are_reused_by_any_class_then_unmapped() {
        let class = 3;
        let per_slab = (SLAB_BYTES - HEADER_BYTES) / block_bytes(class);
        let mut central = Central::new();
        // Every retire stamps and trims at this time, so no slab decays
        // before the explicit trims below.
        let now = 1 << 40;
        let _clock = FixedClock::at(now);
        let blocks: Vec<_> = (0..per_slab + 1).map(|_| central.take(class)).collect();
        let first = Slab::of(blocks[0]);
        let second = Slab::of(blocks[per_slab]);
        assert_ne!(first, second);
        assert!(blocks[..per_slab].iter().all(|&b| Slab::of(b) == first));
        // A full slab leaves the partial list; the second slab is on it.
        assert_eq!(central.partial[class], second);

        // A cache that returns its blocks at thread exit empties both slabs.
        let mut cache = Cache(
            [Bin {
                head: null_mut(),
                count: 0,
            }; CLASSES],
        );
        for &block in &blocks {
            cache.0[class].push(block);
        }
        cache.give_all(&mut central);
        assert_eq!(cache.0[class].count, 0);
        assert!(central.partial[class].is_null());
        let empty = empty_list(&central);
        assert_eq!(empty.len(), 2);
        assert!(empty.contains(&first) && empty.contains(&second));

        // Another class reuses the most recently emptied slab before mapping.
        let other = central.take(9);
        assert_eq!(Slab::of(other), empty[0]);
        // SAFETY: the slab was just reinitialized for class 9.
        assert_eq!(unsafe { (*Slab::of(other)).class }, 9);
        assert_eq!(empty_list(&central), [empty[1]]);
        central.give(other);
        assert_eq!(empty_list(&central).len(), 2);

        // Slabs empty for `DECAY_MS` are unmapped.
        central.trim(now + DECAY_MS - 1);
        assert_eq!(empty_list(&central).len(), 2);
        central.trim(now + DECAY_MS);
        assert!(empty_list(&central).is_empty());
        assert!(central.oldest.is_null());
    }

    #[test]
    fn a_failed_slab_mapping_returns_null() {
        let mut central = Central::new();
        REFUSE_MAPPING.with(|refuse| refuse.set(true));
        let block = central.take(5);
        REFUSE_MAPPING.with(|refuse| refuse.set(false));
        assert!(block.is_null());
        assert!(central.partial[5].is_null());
    }

    /// Thread-local destructors run in an unspecified order, so one that
    /// frees after the cache is gone must reach the slabs directly.
    #[test]
    fn frees_during_thread_exit_reach_the_slabs() {
        struct Late(Vec<u8>);
        impl Drop for Late {
            fn drop(&mut self) {
                let more = vec![1u8; 48];
                std::mem::take(&mut self.0);
                drop(more);
            }
        }
        thread_local! {
            static LATE: std::cell::RefCell<Option<Late>> = const { std::cell::RefCell::new(None) };
        }
        let class = 7;
        std::thread::spawn(move || {
            for _ in 0..3 * cache_limit(class) {
                free(alloc(class), class);
            }
            LATE.with(|late| *late.borrow_mut() = Some(Late(vec![2u8; 100])));
        })
        .join()
        .unwrap();
    }

    #[test]
    fn realloc_keeps_contents_across_classes_and_system() {
        // The router is the global allocator for runtime tests, so a Vec
        // reaches the pool's System path.
        let mut bytes: Vec<u8> = Vec::with_capacity(40);
        bytes.extend(0..40u8);
        let in_class = |v: &Vec<u8>, class: usize| {
            // SAFETY: a pooled block's slab header is mapped while it lives.
            unsafe { (*Slab::of(v.as_ptr().cast_mut())).class as usize == class }
        };
        assert!(in_class(&bytes, 2));
        let at = bytes.as_ptr();
        bytes.reserve_exact(8);
        assert_eq!(bytes.as_ptr(), at, "growth within a class keeps the block");
        bytes.reserve_exact(100);
        assert!(in_class(
            &bytes,
            class_of(Layout::array::<u8>(bytes.capacity()).unwrap()).unwrap()
        ));
        bytes.extend(40..200u8);
        bytes.reserve_exact(4000);
        bytes.extend((0..1000).map(|i| i as u8));
        bytes.truncate(30);
        bytes.shrink_to_fit();
        assert!(in_class(&bytes, 1));
        assert_eq!(bytes, (0..30u8).collect::<Vec<_>>());
    }
}
