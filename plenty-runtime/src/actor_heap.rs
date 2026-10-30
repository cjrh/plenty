//! Per-actor heaps and trap containment behind `std.actor`; see
//! book/src/design/stdlib/actor.md for the contract and its limits.
//!
//! - Allocation routing: while a handler runs, every Rust global allocation
//!   on that thread comes from the current actor heap, a fixed slot of one
//!   reserved virtual range. A pointer's owner is found from its address
//!   alone, so frees and reallocations return to the heap that made them,
//!   whichever heap is current. Exceeding a heap's limit returns null, which
//!   the runtime reports as `AllocError`.
//! - Trap containment: `fail_at`, `fail` and the allocation-failure handler
//!   resume the innermost `plenty_actor_run` instead of exiting. The handler's
//!   frames are abandoned without destructors; `std.actor` then discards its
//!   heap in one call, closing the files it still owned.
//!
//! The heap table belongs to one thread: heaps are created, run and discarded
//! on the thread that made the first heap, and heap operations from any other
//! thread fail. Other threads only ever allocate from System, because their
//! `CURRENT` is zero. A block in the range freed by another thread is left in
//! place, and reallocating one copies it to System.
//!
//! Programs that never create a heap pay one load of `BASE` per allocation and
//! one compare per free; `BASE` stays null and every call goes to System.

/// `plenty_actor_heap` operations; `stdlib/std/actor.plenty` mirrors them.
const CREATE: i64 = 0;
const DISCARD: i64 = 1;
const USED: i64 = 2;
const USED_TOTAL: i64 = 3;
const COMMITTED_TOTAL: i64 = 4;
const LIVE_COUNT: i64 = 5;
const SWITCH: i64 = 6;
/// Reserves the heap range and initializes runtime globals; 0 or a `CREATE`
/// failure.
const PREPARE: i64 = 7;
/// The kind of the last trap `plenty_actor_run` contained on this thread.
const TRAP_KIND: i64 = 8;

/// Why a handler trapped. `plenty_actor_run` returns the number, and
/// `std.actor.TrapKind` lists the variants in this order from 1.
#[derive(Clone, Copy)]
pub(crate) enum TrapKind {
    /// An allocation the runtime could not report as `AllocError`.
    Memory = 1,
    /// A list, text or clist index out of bounds, or a missing dictionary key.
    Index = 2,
    /// Integer overflow, division by zero, a shift or cast out of range.
    Arithmetic = 3,
    /// `unwrap()` on `Nothing` or `Err`.
    Unwrap = 4,
    /// Any other runtime failure.
    Other = 5,
}

/// `CREATE` failures.
const BAD_LIMIT: i64 = -1;
const TABLE_FULL: i64 = -2;
const RESERVE_FAILED: i64 = -3;
const UNSUPPORTED: i64 = -4;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64"),
    feature = "allocation-checks"
))]
pub(crate) use native::Router;
#[cfg(feature = "allocation-checks")]
pub(crate) use native::in_heap;
pub(crate) use native::{
    Arena, abandon_arena_call, foreign, outside, recover, running, track_file, untrack_file, within,
};

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native {
    use super::*;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::{Cell, UnsafeCell};
    use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

    /// Virtual bytes reserved per heap; also the largest limit.
    pub(super) const SLOT_BYTES: usize = 1 << 26;
    pub(super) const MAX_HEAPS: usize = 1024;
    /// Pages are made writable in steps of this many bytes.
    const COMMIT_STEP: usize = 1 << 16;
    static PAGE_BYTES: AtomicUsize = AtomicUsize::new(4096);
    const MIN_SHIFT: u32 = 4;
    /// Power-of-two size classes 16 B ..= SLOT_BYTES.
    const CLASSES: usize = (SLOT_BYTES.trailing_zeros() - MIN_SHIFT + 1) as usize;
    /// Open file owners allocated inside actor heaps. An owner opened while
    /// the registry is full is not closed when its heap is discarded.
    const MAX_FILES: usize = 4096;

    const PROT_NONE: i32 = 0;
    const PROT_READ_WRITE: i32 = 3;
    const MAP_PRIVATE_ANONYMOUS_NORESERVE: i32 = 0x02 | 0x20 | 0x4000;
    const MADV_DONTNEED: i32 = 4;

    unsafe extern "C" {
        fn mmap(
            address: *mut u8,
            length: usize,
            protection: i32,
            flags: i32,
            fd: i32,
            offset: i64,
        ) -> *mut u8;
        fn mprotect(address: *mut u8, length: usize, protection: i32) -> i32;
        fn madvise(address: *mut u8, length: usize, advice: i32) -> i32;
    }

    struct Heap {
        live: bool,
        limit: usize,
        /// Offset of the first never-used byte; never exceeds `limit`.
        bump: usize,
        /// Writable prefix of the slot.
        committed: usize,
        /// Bytes in live blocks, counted at their class size.
        used: usize,
        /// Freed blocks by class; each free block's first word links the next.
        free: [*mut u8; CLASSES],
    }

    const EMPTY_HEAP: Heap = Heap {
        live: false,
        limit: 0,
        bump: 0,
        committed: 0,
        used: 0,
        free: [std::ptr::null_mut(); CLASSES],
    };

    struct Table {
        heaps: [Heap; MAX_HEAPS],
        files: [*mut crate::files::File; MAX_FILES],
    }

    struct Shared(UnsafeCell<Table>);
    // SAFETY: see the module comment; only the owning thread reaches the
    // table. Other threads route to System because their CURRENT is zero, and
    // a pointer inside the range can only have been allocated by the owner.
    unsafe impl Sync for Shared {}

    static TABLE: Shared = Shared(UnsafeCell::new(Table {
        heaps: [EMPTY_HEAP; MAX_HEAPS],
        files: [std::ptr::null_mut(); MAX_FILES],
    }));
    /// Start of the reserved range, or null before the first heap exists.
    static BASE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
    /// The owning thread's `TOKEN`, set with `BASE`.
    static OWNER: AtomicUsize = AtomicUsize::new(0);
    /// The last thread token handed out.
    static TOKENS: AtomicUsize = AtomicUsize::new(0);

    thread_local! {
        /// One plus the index of the heap that receives this thread's
        /// allocations, or zero for System.
        static CURRENT: Cell<usize> = const { Cell::new(0) };
        /// The register save area of the running `plenty_actor_run`, or null.
        static RECOVERY: Cell<*mut Resume> = const { Cell::new(std::ptr::null_mut()) };
        /// This thread's unique nonzero token, or 0 before its first use.
        static TOKEN: Cell<usize> = const { Cell::new(0) };
        /// The `TrapKind` of the last contained trap, or 0 before the first.
        static LAST_TRAP: Cell<i64> = const { Cell::new(0) };
    }

    fn token() -> usize {
        TOKEN.with(|token| {
            if token.get() == 0 {
                token.set(TOKENS.fetch_add(1, Ordering::Relaxed) + 1);
            }
            token.get()
        })
    }

    /// Whether this thread owns the heap table.
    fn owning() -> bool {
        OWNER.load(Ordering::Acquire) == token()
    }

    fn claim_owner(owner: &AtomicUsize, claimant: usize) -> bool {
        let observed = owner.load(Ordering::Acquire);
        if observed != 0 {
            return observed == claimant;
        }
        match owner.compare_exchange(0, claimant, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => true,
            Err(owner) => owner == claimant,
        }
    }

    /// Claim ownership before any shared table access, including initialization.
    fn check_owner() {
        if !claim_owner(&OWNER, token()) {
            crate::fail("std.actor: actor heaps belong to the thread that created the first one");
        }
    }

    fn table() -> *mut Table {
        TABLE.0.get()
    }

    /// The heap whose slot contains `pointer`.
    fn owner(pointer: *const u8) -> Option<usize> {
        let base = BASE.load(Ordering::Acquire);
        if base.is_null() {
            return None;
        }
        let offset = (pointer as usize).wrapping_sub(base as usize);
        (offset < SLOT_BYTES * MAX_HEAPS).then_some(offset / SLOT_BYTES)
    }

    /// Whether `pointer` lies in an actor heap or the routed arena, whose
    /// blocks are released whole rather than freed one by one.
    #[cfg(feature = "allocation-checks")]
    pub(crate) fn in_heap(pointer: *const u8) -> bool {
        owner(pointer).is_some() || in_arena(pointer)
    }

    /// Whether `pointer` lies outside the current allocation target: in
    /// another actor heap, or in any heap while System is current, or in
    /// System while a heap is current. A copy that crosses heaps duplicates
    /// such shared text instead of retaining it.
    pub(crate) fn foreign(pointer: *const u8) -> bool {
        if BASE.load(Ordering::Relaxed).is_null() {
            return false;
        }
        let current = CURRENT.with(Cell::get);
        match owner(pointer) {
            Some(index) => current != index + 1,
            None => current != 0,
        }
    }

    fn slot(index: usize) -> *mut u8 {
        BASE.load(Ordering::Acquire)
            .wrapping_add(index * SLOT_BYTES)
    }

    fn class_of(layout: Layout) -> Option<(usize, usize)> {
        let size = layout
            .size()
            .max(layout.align())
            .max(1 << MIN_SHIFT)
            .checked_next_power_of_two()?;
        (size <= SLOT_BYTES).then(|| (size, (size.trailing_zeros() - MIN_SHIFT) as usize))
    }

    impl Heap {
        /// A block of `layout` from this heap, or null when it would pass
        /// `limit`. Bytes past `committed` are made writable first.
        ///
        /// # Safety
        /// The caller has exclusive access to this heap, `base` is its start,
        /// and `base..base + limit` is reserved for it.
        unsafe fn take(&mut self, base: *mut u8, layout: Layout) -> *mut u8 {
            let Some((size, class)) = class_of(layout) else {
                return std::ptr::null_mut();
            };
            let head = self.free[class];
            if !head.is_null() && (head as usize).is_multiple_of(layout.align()) {
                // SAFETY: a free block holds its successor in its first word.
                self.free[class] = unsafe { head.cast::<*mut u8>().read() };
                self.used += size;
                return head;
            }
            let align = layout.align().max(1 << MIN_SHIFT);
            let start = self.bump.next_multiple_of(align);
            let Some(end) = start.checked_add(size).filter(|&end| end <= self.limit) else {
                return std::ptr::null_mut();
            };
            if end > self.committed {
                let target = end.next_multiple_of(COMMIT_STEP).min(
                    self.limit
                        .next_multiple_of(PAGE_BYTES.load(Ordering::Relaxed)),
                );
                // SAFETY: the range lies inside this heap's reservation.
                let status = unsafe {
                    mprotect(
                        base.add(self.committed),
                        target - self.committed,
                        PROT_READ_WRITE,
                    )
                };
                if status != 0 {
                    return std::ptr::null_mut();
                }
                self.committed = target;
            }
            self.bump = end;
            self.used += size;
            base.wrapping_add(start)
        }

        /// # Safety
        /// `pointer` came from `take(_, layout)` on this heap and is live.
        unsafe fn give(&mut self, pointer: *mut u8, layout: Layout) {
            if !self.live {
                // A discarded heap has no writable pages; its blocks are gone.
                return;
            }
            let Some((size, class)) = class_of(layout) else {
                return;
            };
            // SAFETY: the block is at least 16 bytes and writable while live.
            unsafe { pointer.cast::<*mut u8>().write(self.free[class]) };
            self.free[class] = pointer;
            self.used -= size;
        }
    }

    /// # Safety
    /// `index` names a live heap of the calling (owning) thread.
    pub(super) unsafe fn heap_alloc(index: usize, layout: Layout) -> *mut u8 {
        // SAFETY: the owning thread has exclusive access to its heap entries,
        // and the slot is reserved for this heap.
        unsafe { (*table()).heaps[index].take(slot(index), layout) }
    }

    /// # Safety
    /// `pointer` came from `heap_alloc(index, layout)` and is still live.
    pub(super) unsafe fn heap_free(index: usize, pointer: *mut u8, layout: Layout) {
        // SAFETY: as for `heap_alloc`.
        unsafe { (*table()).heaps[index].give(pointer, layout) }
    }

    /// Routes allocations to the thread's arena or current actor heap, and
    /// frees to their owner.
    pub(crate) struct Router;

    /// One plus the current heap's index, or 0 for System. Reads no
    /// thread-local before the first heap exists, so programs without actors
    /// pay one load and a branch per allocation.
    #[inline]
    fn current() -> usize {
        if BASE.load(Ordering::Relaxed).is_null() {
            return 0;
        }
        CURRENT.with(Cell::get)
    }

    // SAFETY: arena and heap blocks are disjoint, aligned to the layout, and
    // at least the requested size; System handles every other pointer. An
    // arena block is freed or grown only while its arena is routed: see
    // `within`.
    unsafe impl GlobalAlloc for Router {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let arena = routed();
            if !arena.is_null() {
                // SAFETY: a routed arena belongs to this thread's `within`.
                return unsafe { (*arena).take(layout) };
            }
            match current() {
                // SAFETY: as GlobalAlloc requires of the caller.
                0 => unsafe { System.alloc(layout) },
                // SAFETY: CURRENT only ever names a live heap of this thread.
                current => unsafe { heap_alloc(current - 1, layout) },
            }
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let arena = routed();
            let pointer = if !arena.is_null() {
                // SAFETY: as in `alloc`.
                unsafe { (*arena).take(layout) }
            } else {
                match current() {
                    // SAFETY: as GlobalAlloc requires; keeps calloc's fresh pages.
                    0 => return unsafe { System.alloc_zeroed(layout) },
                    // SAFETY: as in `alloc`.
                    current => unsafe { heap_alloc(current - 1, layout) },
                }
            };
            if !pointer.is_null() {
                // SAFETY: a reused block may be dirty; it holds at least
                // `layout.size()` bytes.
                unsafe { pointer.write_bytes(0, layout.size()) };
            }
            pointer
        }
        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            let arena = routed();
            // SAFETY: as in `alloc`; the arena made this block with `layout`.
            if !arena.is_null() && unsafe { (*arena).contains(pointer) } {
                return unsafe { (*arena).give(pointer, layout) };
            }
            match owner(pointer) {
                // SAFETY: the owner made this block with this layout.
                Some(index) if owning() => unsafe { heap_free(index, pointer, layout) },
                // Another thread must not touch the free lists; the block is
                // reclaimed when its heap is discarded.
                Some(_) => {}
                // SAFETY: as GlobalAlloc requires of the caller.
                None => unsafe { System.dealloc(pointer, layout) },
            }
        }
        unsafe fn realloc(&self, pointer: *mut u8, old: Layout, size: usize) -> *mut u8 {
            let arena = routed();
            // SAFETY: as in `dealloc`.
            if !arena.is_null() && unsafe { (*arena).contains(pointer) } {
                let Ok(new) = Layout::from_size_align(size, old.align()) else {
                    return std::ptr::null_mut();
                };
                if class_of(old).map(|c| c.0) == class_of(new).map(|c| c.0) {
                    return pointer;
                }
                // SAFETY: both blocks belong to the routed arena and hold the
                // copied prefix.
                unsafe {
                    let moved = (*arena).take(new);
                    if !moved.is_null() {
                        std::ptr::copy_nonoverlapping(pointer, moved, old.size().min(size));
                        (*arena).give(pointer, old);
                    }
                    return moved;
                }
            }
            let Some(index) = owner(pointer) else {
                // Memory allocated outside every heap stays outside, even when
                // a handler grows it: the actor does not own it.
                // SAFETY: as GlobalAlloc requires of the caller.
                return unsafe { System.realloc(pointer, old, size) };
            };
            let Ok(new) = Layout::from_size_align(size, old.align()) else {
                return std::ptr::null_mut();
            };
            if !owning() {
                // As in `dealloc`: copy out to System and leave the block.
                // SAFETY: `new` has a nonzero size and a valid alignment.
                let moved = unsafe { System.alloc(new) };
                if !moved.is_null() {
                    // SAFETY: both blocks hold the copied prefix.
                    unsafe { std::ptr::copy_nonoverlapping(pointer, moved, old.size().min(size)) };
                }
                return moved;
            }
            if class_of(old).map(|c| c.0) == class_of(new).map(|c| c.0) {
                return pointer;
            }
            // Growth stays in the owning heap, whichever heap is current.
            // SAFETY: the owner is live: blocks of discarded heaps are
            // unreachable once their owners are forgotten.
            let moved = unsafe { heap_alloc(index, new) };
            if !moved.is_null() {
                // SAFETY: both blocks are live and hold the copied prefix.
                unsafe {
                    std::ptr::copy_nonoverlapping(pointer, moved, old.size().min(size));
                    heap_free(index, pointer, old);
                }
            }
            moved
        }
    }

    // ------------------------------------------------------------- arenas

    /// Arenas start on this boundary; a larger alignment is refused.
    const ARENA_ALIGN: usize = 64;

    /// Whether any thread has routed an arena. Each thread reads only its own
    /// `ARENA`, which it set itself, so a stale `false` on another thread
    /// still finds that thread's null `ARENA`.
    static ARENAS: AtomicBool = AtomicBool::new(false);

    thread_local! {
        /// The arena receiving this thread's allocations, or null.
        static ARENA: Cell<*mut Arena> = const { Cell::new(std::ptr::null_mut()) };
        /// The register save area of the running `within`, or null.
        static ARENA_RESUME: Cell<*mut Resume> = const { Cell::new(std::ptr::null_mut()) };
    }

    #[inline]
    fn routed() -> *mut Arena {
        if !ARENAS.load(Ordering::Relaxed) {
            return std::ptr::null_mut();
        }
        ARENA.with(Cell::get)
    }

    /// A fixed block that receives one thread's Rust allocations while
    /// `within` runs, for library code that cannot report allocation failure.
    /// The block itself comes from the current allocation target, so an
    /// actor's arena counts against its heap limit. Dropping the arena zeroes
    /// and frees the whole block without visiting its allocations.
    pub(crate) struct Arena {
        block: std::ptr::NonNull<Block>,
    }

    /// The start of an arena's block: its free lists, then the bytes it hands
    /// out. Keeping them in the block keeps `Arena` one pointer wide.
    #[repr(C, align(64))]
    struct Block {
        heap: Heap,
    }
    const _: () = assert!(align_of::<Block>() == ARENA_ALIGN);

    // SAFETY: the block is plain memory that only its owner reaches; routing
    // state is per thread and set only during `within`. Dropping on another
    // thread frees a System block normally. A block in an actor heap cannot
    // move: values in a heap stay on its owning thread.
    unsafe impl Send for Arena {}

    impl Arena {
        /// Reserves `size` bytes for allocations, plus the free lists, or
        /// `None` when the allocation fails.
        pub(crate) fn new(size: usize) -> Option<Self> {
            let total = size.checked_add(size_of::<Block>())?;
            let layout = Layout::from_size_align(total, ARENA_ALIGN).ok()?;
            // SAFETY: the layout has a nonzero size.
            let block = std::ptr::NonNull::new(unsafe { std::alloc::alloc(layout) })?.cast();
            // SAFETY: a fresh block of at least `size_of::<Block>()` bytes,
            // aligned for `Block`.
            unsafe {
                block.write(Block {
                    heap: Heap {
                        live: true,
                        limit: size,
                        // The block is writable throughout, so `take` never
                        // calls mprotect.
                        committed: size,
                        ..EMPTY_HEAP
                    },
                });
            }
            Some(Self { block })
        }

        fn heap(&mut self) -> &mut Heap {
            // SAFETY: the block holds an initialized `Block` until drop, and
            // only this arena reaches it.
            unsafe { &mut (*self.block.as_ptr()).heap }
        }

        fn limit(&self) -> usize {
            // SAFETY: as in `heap`.
            unsafe { (*self.block.as_ptr()).heap.limit }
        }

        pub(super) fn data(&self) -> *mut u8 {
            self.block.as_ptr().wrapping_add(1).cast()
        }

        /// Bytes from the start of the data to the end of the last block ever
        /// handed out: the size this arena's work needed.
        #[cfg(test)]
        pub(crate) fn high_water(&mut self) -> usize {
            self.heap().bump
        }

        fn contains(&self, pointer: *const u8) -> bool {
            (pointer as usize).wrapping_sub(self.data() as usize) < self.limit()
        }

        unsafe fn take(&mut self, layout: Layout) -> *mut u8 {
            if layout.align() > ARENA_ALIGN {
                return std::ptr::null_mut();
            }
            let data = self.data();
            // SAFETY: the routing thread has exclusive access, and the data
            // is writable through `limit`.
            unsafe { self.heap().take(data, layout) }
        }

        unsafe fn give(&mut self, pointer: *mut u8, layout: Layout) {
            // SAFETY: as GlobalAlloc's dealloc requires of its caller.
            unsafe { self.heap().give(pointer, layout) }
        }
    }

    impl Drop for Arena {
        fn drop(&mut self) {
            let used = self.heap().bump;
            let limit = self.limit();
            let data = self.data();
            // SAFETY: the data holds `limit` writable bytes, and nothing past
            // `bump` was ever handed out. Volatile writes keep the scrub of
            // key material that ring frees without zeroing. The layout is the
            // one `new` allocated.
            unsafe {
                for offset in 0..used {
                    std::ptr::write_volatile(data.add(offset), 0);
                }
                std::alloc::dealloc(
                    self.block.as_ptr().cast(),
                    Layout::from_size_align_unchecked(limit + size_of::<Block>(), ARENA_ALIGN),
                );
            }
        }
    }

    #[cfg(feature = "allocation-checks")]
    fn in_arena(pointer: *const u8) -> bool {
        let arena = routed();
        // SAFETY: a routed arena is live while `within` runs.
        !arena.is_null() && unsafe { (*arena).contains(pointer) }
    }

    struct Call<F, R> {
        job: Option<F>,
        result: Option<R>,
    }

    unsafe extern "C" fn run_call<F: FnOnce() -> R, R>(storage: *mut u8) -> *mut u8 {
        // SAFETY: `within` passes its live `Call<F, R>`.
        let call = unsafe { &mut *storage.cast::<Call<F, R>>() };
        if let Some(job) = call.job.take() {
            call.result = Some(job());
        }
        std::ptr::null_mut()
    }

    /// Runs `job` with every Rust allocation on this thread taken from
    /// `arena`. Returns `None` when an allocation fails, which abandons
    /// `job`'s frames without running their destructors; their blocks are
    /// all in `arena`, which the caller then drops whole.
    ///
    /// `job` must leave nothing outside the arena that a destructor would
    /// have to repair: no lock, no partly updated outside state, and no
    /// allocation it hands back. Anything it allocates must stay in the arena
    /// and be freed, if ever, inside another `within` on the same arena.
    pub(crate) fn within<F: FnOnce() -> R, R>(arena: &mut Arena, job: F) -> Option<R> {
        if !ARENA.with(Cell::get).is_null() {
            crate::fail("internal: an arena call cannot nest");
        }
        let mut call = Call {
            job: Some(job),
            result: None,
        };
        let mut resume = Resume([0; RESUME_WORDS]);
        ARENAS.store(true, Ordering::Relaxed);
        ARENA.with(|slot| slot.set(arena));
        ARENA_RESUME.with(|slot| slot.set(&mut resume));
        // SAFETY: `run_call` reads the `Call` it is given; `resume` and `call`
        // outlive the call.
        let status = unsafe {
            plenty_actor_enter(
                &mut resume,
                run_call::<F, R>,
                std::ptr::addr_of_mut!(call).cast(),
            )
        };
        ARENA_RESUME.with(|slot| slot.set(std::ptr::null_mut()));
        ARENA.with(|slot| slot.set(std::ptr::null_mut()));
        if status != 0 {
            return None;
        }
        call.result
    }

    /// Called by the allocation-failure handler: inside `within`, the failed
    /// allocation was the arena's, and `within` returns `None`. Returns only
    /// outside `within`.
    pub(crate) fn abandon_arena_call() {
        let resume = ARENA_RESUME.with(Cell::get);
        if !resume.is_null() {
            // SAFETY: `resume` was filled by the enter call of the `within`
            // still on this thread's stack.
            unsafe { plenty_actor_resume(resume, 1) }
        }
    }

    // The accounting allocator wraps the router when allocation checks are on.
    #[cfg(not(feature = "allocation-checks"))]
    #[global_allocator]
    static ALLOCATOR: Router = Router;

    /// Runs `f` with System current, for runtime state that outlives every
    /// handler, such as the child-process table.
    pub(crate) fn outside<R>(f: impl FnOnce() -> R) -> R {
        let current = CURRENT.with(|current| current.replace(0));
        let result = f();
        CURRENT.with(|slot| slot.set(current));
        result
    }

    // -------------------------------------------------------------- files

    /// Records a file owner allocated inside an actor heap, so discarding the
    /// heap closes its descriptor. Owners outside every heap are not recorded.
    pub(crate) fn track_file(file: *mut crate::files::File) {
        if owner(file.cast()).is_none() {
            return;
        }
        // SAFETY: owning thread; see the module comment.
        let files = unsafe { &mut (*table()).files };
        if let Some(entry) = files.iter_mut().find(|entry| entry.is_null()) {
            *entry = file;
        }
    }

    /// Forgets a file owner whose descriptor is closed or being dropped.
    pub(crate) fn untrack_file(file: *mut crate::files::File) {
        if owner(file.cast()).is_none() {
            return;
        }
        // SAFETY: owning thread; see the module comment.
        let files = unsafe { &mut (*table()).files };
        if let Some(entry) = files.iter_mut().find(|entry| **entry == file) {
            *entry = std::ptr::null_mut();
        }
    }

    // -------------------------------------------------------------- traps

    /// Callee-saved registers, stack pointer and return address of the
    /// `plenty_actor_enter` call that `plenty_actor_run` makes.
    #[repr(C)]
    struct Resume([u64; RESUME_WORDS]);
    #[cfg(target_arch = "x86_64")]
    const RESUME_WORDS: usize = 8;
    // x19-x30, SP, padding, and the preserved low halves of v8-v15.
    #[cfg(target_arch = "aarch64")]
    const RESUME_WORDS: usize = 22;

    unsafe extern "C" {
        /// Saves the caller's state in `resume`, then calls `entry(storage)`.
        /// Returns 0 when `entry` returns, or the status passed to
        /// `plenty_actor_resume`.
        fn plenty_actor_enter(
            resume: *mut Resume,
            entry: unsafe extern "C" fn(*mut u8) -> *mut u8,
            storage: *mut u8,
        ) -> u64;
        /// Returns `status` from the `plenty_actor_enter` call that filled
        /// `resume`, discarding every frame above it.
        fn plenty_actor_resume(resume: *const Resume, status: u64) -> !;
    }

    // SysV x86-64: rbx, rbp and r12-r15 are callee-saved, and the call that
    // enters leaves rsp 8 below a 16-byte boundary. Resuming restores those
    // registers and returns to the caller of `enter` exactly as `ret` would.
    #[cfg(target_arch = "x86_64")]
    std::arch::global_asm!(
        ".text",
        ".p2align 4",
        ".globl plenty_actor_enter",
        ".hidden plenty_actor_enter",
        ".type plenty_actor_enter,@function",
        "plenty_actor_enter:",
        "mov qword ptr [rdi], rbx",
        "mov qword ptr [rdi + 8], rbp",
        "mov qword ptr [rdi + 16], r12",
        "mov qword ptr [rdi + 24], r13",
        "mov qword ptr [rdi + 32], r14",
        "mov qword ptr [rdi + 40], r15",
        "lea rax, [rsp + 8]",
        "mov qword ptr [rdi + 48], rax",
        "mov rax, qword ptr [rsp]",
        "mov qword ptr [rdi + 56], rax",
        "sub rsp, 8",
        "mov rdi, rdx",
        "call rsi",
        "add rsp, 8",
        "xor eax, eax",
        "ret",
        ".size plenty_actor_enter, . - plenty_actor_enter",
        ".p2align 4",
        ".globl plenty_actor_resume",
        ".hidden plenty_actor_resume",
        ".type plenty_actor_resume,@function",
        "plenty_actor_resume:",
        "mov rbx, qword ptr [rdi]",
        "mov rbp, qword ptr [rdi + 8]",
        "mov r12, qword ptr [rdi + 16]",
        "mov r13, qword ptr [rdi + 24]",
        "mov r14, qword ptr [rdi + 32]",
        "mov r15, qword ptr [rdi + 40]",
        "mov rsp, qword ptr [rdi + 48]",
        "mov rax, rsi",
        "jmp qword ptr [rdi + 56]",
        ".size plenty_actor_resume, . - plenty_actor_resume",
    );

    // AAPCS64 preserves x19-x29, SP and the low 64 bits of v8-v15.
    // Save LR too: BLR overwrites it even on the ordinary return path.
    #[cfg(target_arch = "aarch64")]
    std::arch::global_asm!(
        ".text",
        ".p2align 2",
        ".globl plenty_actor_enter",
        ".hidden plenty_actor_enter",
        ".type plenty_actor_enter,%function",
        "plenty_actor_enter:",
        "stp x19, x20, [x0, #0]",
        "stp x21, x22, [x0, #16]",
        "stp x23, x24, [x0, #32]",
        "stp x25, x26, [x0, #48]",
        "stp x27, x28, [x0, #64]",
        "stp x29, x30, [x0, #80]",
        "mov x9, sp",
        "str x9, [x0, #96]",
        "stp d8, d9, [x0, #112]",
        "stp d10, d11, [x0, #128]",
        "stp d12, d13, [x0, #144]",
        "stp d14, d15, [x0, #160]",
        // A real frame keeps the ordinary return independent of actor memory.
        "stp x29, x30, [sp, #-16]!",
        "mov x29, sp",
        "mov x0, x2",
        "blr x1",
        "ldp x29, x30, [sp], #16",
        "mov x0, #0",
        "ret",
        ".size plenty_actor_enter, . - plenty_actor_enter",
        ".p2align 2",
        ".globl plenty_actor_resume",
        ".hidden plenty_actor_resume",
        ".type plenty_actor_resume,%function",
        "plenty_actor_resume:",
        "ldp x19, x20, [x0, #0]",
        "ldp x21, x22, [x0, #16]",
        "ldp x23, x24, [x0, #32]",
        "ldp x25, x26, [x0, #48]",
        "ldp x27, x28, [x0, #64]",
        "ldp x29, x30, [x0, #80]",
        "ldr x9, [x0, #96]",
        "ldp d8, d9, [x0, #112]",
        "ldp d10, d11, [x0, #128]",
        "ldp d12, d13, [x0, #144]",
        "ldp d14, d15, [x0, #160]",
        "mov sp, x9",
        "mov x0, x1",
        "ret",
        ".size plenty_actor_resume, . - plenty_actor_resume",
    );

    /// Called after a trap's message is printed. Returns only when no handler
    /// is running on this thread, and the process then exits as before.
    pub(crate) fn recover(kind: TrapKind) {
        let resume = RECOVERY.with(Cell::get);
        if resume.is_null() {
            return;
        }
        eprintln!("note: contained in an actor; its heap is discarded");
        // SAFETY: `resume` was filled by the enter call that is still on this
        // thread's stack, below every frame being abandoned.
        unsafe { plenty_actor_resume(resume, kind as u64) }
    }

    fn live(heap: i64) -> Option<usize> {
        let index = usize::try_from(heap).ok().filter(|&i| i < MAX_HEAPS)?;
        // SAFETY: owning thread; see the module comment.
        unsafe { (*table()).heaps[index].live }.then_some(index)
    }

    /// Runs a compiled `__actor_run` adapter with `heap` current. Returns 0
    /// when the handler returned (its result is in `storage`), or the
    /// `TrapKind` when it trapped; `TRAP_KIND` reports that kind again.
    ///
    /// # Safety
    /// `entry` is the compiler's adapter for `storage`'s task layout.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn plenty_actor_run(
        heap: i64,
        storage: *mut u8,
        entry: unsafe extern "C" fn(*mut u8) -> *mut u8,
    ) -> i64 {
        check_owner();
        let Some(index) = live(heap) else {
            crate::fail("std.actor: the actor heap is not live");
        };
        if !RECOVERY.with(Cell::get).is_null() {
            crate::fail("std.actor: a handler cannot run another handler");
        }
        let caller = crate::CALLER.with(Cell::get);
        let queue = crate::memory::save_queue();
        let mut resume = Resume([0; RESUME_WORDS]);
        CURRENT.with(|current| current.set(index + 1));
        RECOVERY.with(|recovery| recovery.set(&mut resume));
        // SAFETY: the adapter reads its arguments from `storage` and writes
        // the result there; `resume` outlives the call.
        let status = unsafe { plenty_actor_enter(&mut resume, entry, storage) };
        RECOVERY.with(|recovery| recovery.set(std::ptr::null_mut()));
        CURRENT.with(|current| current.set(0));
        if status != 0 {
            // Any other contained trap inside an arena call abandons `within`
            // too. Its arena was allocated in this heap, which is discarded.
            ARENA.with(|slot| slot.set(std::ptr::null_mut()));
            ARENA_RESUME.with(|slot| slot.set(std::ptr::null_mut()));
            // Thread-local runtime state the abandoned frames may have changed.
            crate::CALLER.with(|current| current.set(caller));
            crate::memory::restore_queue(queue);
            LAST_TRAP.with(|last| last.set(status as i64));
        }
        status as i64
    }

    pub(crate) fn running() -> bool {
        !RECOVERY.with(Cell::get).is_null()
    }

    // ---------------------------------------------------------- operations

    /// Reserves the heap range once. Runtime globals that std initializes on
    /// first use are initialized here, outside every heap: one created by a
    /// handler would live in a heap that is later discarded.
    pub(super) fn prepare() -> i64 {
        check_owner();
        if !BASE.load(Ordering::Acquire).is_null() {
            return 0;
        }
        // Linux supports 4, 16 and 64 KiB pages on AArch64. Both slot
        // boundaries and commit increments must be multiples of a page.
        // SAFETY: sysconf has no pointer arguments.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if page <= 0 || !COMMIT_STEP.is_multiple_of(page as usize) {
            OWNER.store(0, Ordering::Release);
            return RESERVE_FAILED;
        }
        PAGE_BYTES.store(page as usize, Ordering::Relaxed);
        drop(std::io::stdout().lock());
        drop(std::io::stderr().lock());
        drop(std::io::stdin().lock());
        // SAFETY: a fresh anonymous reservation; nothing is accessible.
        let base = unsafe {
            mmap(
                std::ptr::null_mut(),
                SLOT_BYTES * MAX_HEAPS,
                PROT_NONE,
                MAP_PRIVATE_ANONYMOUS_NORESERVE,
                -1,
                0,
            )
        };
        if base as isize == -1 {
            // No heap/table state was published. A later caller may retry.
            OWNER.store(0, Ordering::Release);
            return RESERVE_FAILED;
        }
        BASE.store(base, Ordering::Release);
        0
    }

    pub(super) fn create(limit: i64) -> i64 {
        let Some(limit) = usize::try_from(limit)
            .ok()
            .filter(|&limit| (1..=SLOT_BYTES).contains(&limit))
        else {
            return BAD_LIMIT;
        };
        let prepared = prepare();
        if prepared != 0 {
            return prepared;
        }
        // SAFETY: owning thread; see the module comment.
        let heaps = unsafe { &mut (*table()).heaps };
        let Some(index) = heaps.iter().position(|heap| !heap.live) else {
            return TABLE_FULL;
        };
        heaps[index] = Heap {
            live: true,
            limit,
            ..EMPTY_HEAP
        };
        index as i64
    }

    /// Closes the heap's open file owners and returns its pages to the OS,
    /// without running any destructor. Returns the number of descriptors
    /// closed, or -1 when `heap` is not live or is current.
    pub(super) fn discard(heap: i64) -> i64 {
        let Some(index) = live(heap) else {
            return -1;
        };
        if CURRENT.with(Cell::get) == index + 1 {
            return -1;
        }
        let mut closed = 0;
        // SAFETY: owning thread; tracked owners in this slot are still mapped.
        unsafe {
            for entry in (*table()).files.iter_mut() {
                if !entry.is_null() && owner(entry.cast()) == Some(index) {
                    if crate::files::abandon(*entry) {
                        closed += 1;
                    }
                    *entry = std::ptr::null_mut();
                }
            }
            let heap = &mut (*table()).heaps[index];
            if heap.committed > 0 {
                madvise(slot(index), heap.committed, MADV_DONTNEED);
                mprotect(slot(index), heap.committed, PROT_NONE);
            }
            *heap = EMPTY_HEAP;
        }
        closed
    }

    pub(super) fn operate(op: i64, argument: i64) -> i64 {
        // Libraries lack the router and must remain safely unsupported on
        // every caller thread, without acquiring a process-wide actor owner.
        if !cfg!(plenty_runtime_embedded) && matches!(op, CREATE | PREPARE) {
            return UNSUPPORTED;
        }
        if op == TRAP_KIND {
            return LAST_TRAP.with(Cell::get);
        }
        if BASE.load(Ordering::Acquire).is_null() {
            // Queries before initialization need no owner and must not read
            // TABLE while another thread may be publishing its first heap.
            match op {
                USED_TOTAL | COMMITTED_TOTAL | LIVE_COUNT => return 0,
                USED | DISCARD => return -1,
                SWITCH if argument < 0 => return -1,
                _ => {}
            }
        }
        check_owner();
        // SAFETY (each table read): owning thread, checked above.
        let heaps = || unsafe { &(*table()).heaps };
        match op {
            CREATE => {
                if cfg!(plenty_runtime_embedded) {
                    create(argument)
                } else {
                    // Without the router, handler allocations would not
                    // reach the heap.
                    UNSUPPORTED
                }
            }
            PREPARE => {
                if cfg!(plenty_runtime_embedded) {
                    prepare()
                } else {
                    UNSUPPORTED
                }
            }
            DISCARD => discard(argument),
            USED => live(argument).map_or(-1, |index| heaps()[index].used as i64),
            USED_TOTAL => heaps()
                .iter()
                .filter(|heap| heap.live)
                .map(|heap| heap.used as i64)
                .sum(),
            COMMITTED_TOTAL => heaps().iter().map(|heap| heap.committed as i64).sum(),
            LIVE_COUNT => heaps().iter().filter(|heap| heap.live).count() as i64,
            TRAP_KIND => LAST_TRAP.with(Cell::get),
            SWITCH => {
                let next = if argument < 0 {
                    0
                } else {
                    match live(argument) {
                        Some(index) => index + 1,
                        None => crate::fail("std.actor: the actor heap is not live"),
                    }
                };
                CURRENT.with(|current| current.replace(next)) as i64 - 1
            }
            _ => crate::fail("std.actor: unknown heap operation"),
        }
    }
    #[cfg(test)]
    mod initialization_tests {
        use super::*;

        #[test]
        fn unsupported_entries_neither_claim_nor_require_an_owner() {
            std::thread::scope(|scope| {
                for _ in 0..8 {
                    scope.spawn(|| {
                        assert_eq!(operate(CREATE, 4096), UNSUPPORTED);
                        assert_eq!(operate(PREPARE, 0), UNSUPPORTED);
                    });
                }
            });
        }

        #[test]
        fn concurrent_first_calls_grant_exactly_one_owner() {
            let owner = AtomicUsize::new(0);
            let start = std::sync::Barrier::new(16);
            std::thread::scope(|scope| {
                let results = (1..=16)
                    .map(|token| {
                        let (owner, start) = (&owner, &start);
                        scope.spawn(move || {
                            start.wait();
                            claim_owner(owner, token)
                        })
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    results
                        .into_iter()
                        .map(|result| result.join().unwrap())
                        .filter(|claimed| *claimed)
                        .count(),
                    1
                );
            });
            let winner = owner.load(Ordering::Acquire);
            assert!(claim_owner(&owner, winner));
            assert!(!claim_owner(&owner, winner + 1));
        }
    }

    #[cfg(test)]
    mod recovery_tests {
        use super::*;

        unsafe extern "C" fn returns(_: *mut u8) -> *mut u8 {
            std::ptr::null_mut()
        }

        unsafe extern "C" fn resumes(storage: *mut u8) -> *mut u8 {
            // SAFETY: the test passes the live enter call's recovery frame.
            unsafe { plenty_actor_resume(storage.cast(), 37) }
        }

        // The callback deliberately violates the normal return convention,
        // then jumps directly to recovery. The wrapper verifies every AAPCS64
        // preserved GP/FP value while preserving the Rust caller's own state.
        #[cfg(target_arch = "aarch64")]
        std::arch::global_asm!(
            ".text",
            ".p2align 2",
            ".globl plenty_test_actor_registers",
            ".hidden plenty_test_actor_registers",
            ".type plenty_test_actor_registers,%function",
            "plenty_test_actor_registers:",
            "sub sp, sp, #176",
            "stp x19, x20, [sp, #0]",
            "stp x21, x22, [sp, #16]",
            "stp x23, x24, [sp, #32]",
            "stp x25, x26, [sp, #48]",
            "stp x27, x28, [sp, #64]",
            "stp x29, x30, [sp, #80]",
            "stp d8, d9, [sp, #96]",
            "stp d10, d11, [sp, #112]",
            "stp d12, d13, [sp, #128]",
            "stp d14, d15, [sp, #144]",
            "str x3, [sp, #160]",
            "mov x19, #100",
            "mov x20, #101",
            "mov x21, #102",
            "mov x22, #103",
            "mov x23, #104",
            "mov x24, #105",
            "mov x25, #106",
            "mov x26, #107",
            "mov x27, #108",
            "mov x28, #109",
            "mov x29, #110",
            "mov x9, #200",
            "fmov d8, x9",
            "mov x9, #201",
            "fmov d9, x9",
            "mov x9, #202",
            "fmov d10, x9",
            "mov x9, #203",
            "fmov d11, x9",
            "mov x9, #204",
            "fmov d12, x9",
            "mov x9, #205",
            "fmov d13, x9",
            "mov x9, #206",
            "fmov d14, x9",
            "mov x9, #207",
            "fmov d15, x9",
            "bl plenty_actor_enter",
            "ldr x9, [sp, #160]",
            "str x19, [x9, #0]",
            "str x20, [x9, #8]",
            "str x21, [x9, #16]",
            "str x22, [x9, #24]",
            "str x23, [x9, #32]",
            "str x24, [x9, #40]",
            "str x25, [x9, #48]",
            "str x26, [x9, #56]",
            "str x27, [x9, #64]",
            "str x28, [x9, #72]",
            "str x29, [x9, #80]",
            "str d8, [x9, #88]",
            "str d9, [x9, #96]",
            "str d10, [x9, #104]",
            "str d11, [x9, #112]",
            "str d12, [x9, #120]",
            "str d13, [x9, #128]",
            "str d14, [x9, #136]",
            "str d15, [x9, #144]",
            "ldp x19, x20, [sp, #0]",
            "ldp x21, x22, [sp, #16]",
            "ldp x23, x24, [sp, #32]",
            "ldp x25, x26, [sp, #48]",
            "ldp x27, x28, [sp, #64]",
            "ldp x29, x30, [sp, #80]",
            "ldp d8, d9, [sp, #96]",
            "ldp d10, d11, [sp, #112]",
            "ldp d12, d13, [sp, #128]",
            "ldp d14, d15, [sp, #144]",
            "add sp, sp, #176",
            "ret",
            ".size plenty_test_actor_registers, .-plenty_test_actor_registers",
            ".p2align 2",
            ".globl plenty_test_actor_clobber",
            ".hidden plenty_test_actor_clobber",
            ".type plenty_test_actor_clobber,%function",
            "plenty_test_actor_clobber:",
            "mov x19, #0",
            "mov x20, #0",
            "mov x21, #0",
            "mov x22, #0",
            "mov x23, #0",
            "mov x24, #0",
            "mov x25, #0",
            "mov x26, #0",
            "mov x27, #0",
            "mov x28, #0",
            "mov x29, #0",
            "fmov d8, xzr",
            "fmov d9, xzr",
            "fmov d10, xzr",
            "fmov d11, xzr",
            "fmov d12, xzr",
            "fmov d13, xzr",
            "fmov d14, xzr",
            "fmov d15, xzr",
            "mov x1, #37",
            "b plenty_actor_resume",
            ".size plenty_test_actor_clobber, .-plenty_test_actor_clobber",
        );

        #[cfg(target_arch = "aarch64")]
        #[test]
        fn nonlocal_recovery_restores_aapcs64_preserved_registers() {
            unsafe extern "C" {
                fn plenty_test_actor_registers(
                    resume: *mut Resume,
                    entry: unsafe extern "C" fn(*mut u8) -> *mut u8,
                    storage: *mut u8,
                    observed: *mut u64,
                ) -> u64;
                fn plenty_test_actor_clobber(storage: *mut u8) -> *mut u8;
            }
            for _ in 0..128 {
                let mut resume = Resume([0; RESUME_WORDS]);
                let mut observed = [0; 19];
                // SAFETY: the wrapper's callback only resumes this live frame.
                let status = unsafe {
                    plenty_test_actor_registers(
                        &mut resume,
                        plenty_test_actor_clobber,
                        (&raw mut resume).cast(),
                        observed.as_mut_ptr(),
                    )
                };
                assert_eq!(status, 37);
                assert_eq!(&observed[..11], &(100..111).collect::<Vec<_>>());
                assert_eq!(&observed[11..], &(200..208).collect::<Vec<_>>());
            }
        }

        #[test]
        fn normal_return_and_repeated_nonlocal_recovery() {
            for _ in 0..128 {
                let mut frame = Resume([0; RESUME_WORDS]);
                // SAFETY: each adapter either returns or resumes this enter.
                unsafe {
                    assert_eq!(
                        plenty_actor_enter(&mut frame, returns, std::ptr::null_mut()),
                        0
                    );
                    assert_eq!(
                        plenty_actor_enter(&mut frame, resumes, (&raw mut frame).cast()),
                        37
                    );
                }
            }
        }
    }
}

#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
mod native {
    use super::*;

    pub(crate) fn outside<R>(f: impl FnOnce() -> R) -> R {
        f()
    }
    pub(crate) fn recover(_: TrapKind) {}
    #[cfg(feature = "allocation-checks")]
    pub(crate) fn in_heap(_: *const u8) -> bool {
        false
    }

    pub(crate) fn foreign(_: *const u8) -> bool {
        false
    }

    /// No routing on this target: library allocations use System, and an
    /// allocation failure inside `within` aborts as it would elsewhere.
    pub(crate) struct Arena;

    impl Arena {
        pub(crate) fn new(_: usize) -> Option<Self> {
            Some(Self)
        }
    }

    pub(crate) fn within<R>(_: &mut Arena, job: impl FnOnce() -> R) -> Option<R> {
        Some(job())
    }

    pub(crate) fn abandon_arena_call() {}

    pub(crate) fn track_file(_: *mut crate::files::File) {}
    pub(crate) fn untrack_file(_: *mut crate::files::File) {}
    pub(crate) fn running() -> bool {
        false
    }

    /// Never reached: no heap can be created on this target.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn plenty_actor_run(
        _heap: i64,
        _storage: *mut u8,
        _entry: unsafe extern "C" fn(*mut u8) -> *mut u8,
    ) -> i64 {
        crate::fail("std.actor: actor heaps are not supported on this target")
    }

    pub(super) fn operate(op: i64, _argument: i64) -> i64 {
        match op {
            CREATE | PREPARE => UNSUPPORTED,
            USED_TOTAL | COMMITTED_TOTAL | LIVE_COUNT | TRAP_KIND => 0,
            SWITCH => -1,
            _ => -1,
        }
    }
}

/// Heap operations for `std.actor`; see the constants above.
#[unsafe(no_mangle)]
pub extern "C" fn plenty_actor_heap(op: i64, argument: i64) -> i64 {
    native::operate(op, argument)
}

#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod tests {
    use super::native::*;
    use super::*;
    use std::alloc::Layout;

    #[test]
    fn heap_allocations_are_limited_reused_and_released() {
        let heap = create(4096);
        assert!(heap >= 0);
        let index = heap as usize;
        let layout = Layout::from_size_align(100, 8).unwrap();
        // SAFETY: the heap is live and owned by this test's thread.
        unsafe {
            let a = heap_alloc(index, layout);
            assert!(!a.is_null());
            assert_eq!(operate(USED, heap), 128);
            heap_free(index, a, layout);
            assert_eq!(operate(USED, heap), 0);
            assert_eq!(heap_alloc(index, layout), a);
            let big = Layout::from_size_align(8192, 8).unwrap();
            assert!(heap_alloc(index, big).is_null());
        }
        assert_eq!(discard(heap), 0);
        assert_eq!(operate(USED, heap), -1);
        assert_eq!(discard(heap), -1);
        assert_eq!(create(0), BAD_LIMIT);
        assert_eq!(create(SLOT_BYTES as i64 + 1), BAD_LIMIT);
        // Cargo test builds have no router, so programs' entry refuses.
        assert_eq!(operate(CREATE, 4096), UNSUPPORTED);
    }

    #[test]
    fn arenas_route_reuse_and_release() {
        let mut arena = Arena::new(4096).unwrap();
        let (first, grown) = within(&mut arena, || {
            let small = Box::new([7u8; 100]);
            let first = std::ptr::from_ref(&*small) as usize;
            drop(small);
            // The freed block is reused, and growth stays in the arena.
            let mut values = Vec::<u8>::with_capacity(100);
            values.extend_from_slice(&[1; 300]);
            let grown = values.as_ptr() as usize;
            std::mem::forget(values);
            (first, grown)
        })
        .unwrap();
        let base = arena.data() as usize;
        assert!((base..base + 4096).contains(&first));
        assert!((base..base + 4096).contains(&grown));
        // The vector reused the box's 128-byte block, then grew to 512.
        assert_eq!(arena.high_water(), 128 + 512);
        // Outside `within`, allocations go to System again.
        let outside = Box::new(0u64);
        assert!(!(base..base + 4096).contains(&(std::ptr::from_ref(&*outside) as usize)));
        // An arena may be dropped on another thread.
        std::thread::spawn(move || drop(arena)).join().unwrap();
    }
}
