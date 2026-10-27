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
//! on the thread that made the first heap. Other threads only ever allocate
//! from System, because their `CURRENT` is zero.
//!
//! Programs that never create a heap pay one thread-local read per
//! allocation and one compare per free; `BASE` stays null and every call goes
//! to System.

/// `plenty_actor_heap` operations; `stdlib/std/actor.plenty` mirrors them.
const CREATE: i64 = 0;
const DISCARD: i64 = 1;
const USED: i64 = 2;
const USED_TOTAL: i64 = 3;
const COMMITTED_TOTAL: i64 = 4;
const LIVE_COUNT: i64 = 5;
const SWITCH: i64 = 6;

/// `CREATE` failures.
const BAD_LIMIT: i64 = -1;
const TABLE_FULL: i64 = -2;
const RESERVE_FAILED: i64 = -3;
const UNSUPPORTED: i64 = -4;

#[cfg(all(
    target_os = "linux",
    target_arch = "x86_64",
    feature = "allocation-checks"
))]
pub(crate) use native::Router;
#[cfg(feature = "allocation-checks")]
pub(crate) use native::in_heap;
pub(crate) use native::{foreign, outside, recover, running, track_file, untrack_file};

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod native {
    use super::*;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::{Cell, UnsafeCell};
    use std::sync::atomic::{AtomicPtr, Ordering};

    /// Virtual bytes reserved per heap; also the largest limit.
    pub(super) const SLOT_BYTES: usize = 1 << 26;
    pub(super) const MAX_HEAPS: usize = 1024;
    /// Pages are made writable in steps of this many bytes.
    const COMMIT_STEP: usize = 1 << 16;
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

    thread_local! {
        /// One plus the index of the heap that receives this thread's
        /// allocations, or zero for System.
        static CURRENT: Cell<usize> = const { Cell::new(0) };
        /// The register save area of the running `plenty_actor_run`, or null.
        static RECOVERY: Cell<*mut Resume> = const { Cell::new(std::ptr::null_mut()) };
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

    #[cfg(feature = "allocation-checks")]
    pub(crate) fn in_heap(pointer: *const u8) -> bool {
        owner(pointer).is_some()
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

    /// # Safety
    /// `index` names a live heap of the calling (owning) thread.
    pub(super) unsafe fn heap_alloc(index: usize, layout: Layout) -> *mut u8 {
        let Some((size, class)) = class_of(layout) else {
            return std::ptr::null_mut();
        };
        // SAFETY: the owning thread has exclusive access to its heap entries.
        let heap = unsafe { &mut (*table()).heaps[index] };
        let head = heap.free[class];
        if !head.is_null() && (head as usize).is_multiple_of(layout.align()) {
            // SAFETY: a free block holds its successor in its first word.
            heap.free[class] = unsafe { head.cast::<*mut u8>().read() };
            heap.used += size;
            return head;
        }
        let align = layout.align().max(1 << MIN_SHIFT);
        let start = heap.bump.next_multiple_of(align);
        let Some(end) = start.checked_add(size).filter(|&end| end <= heap.limit) else {
            return std::ptr::null_mut();
        };
        if end > heap.committed {
            let target = end
                .next_multiple_of(COMMIT_STEP)
                .min(heap.limit.next_multiple_of(4096));
            // SAFETY: the range lies inside this heap's reserved slot.
            let status = unsafe {
                mprotect(
                    slot(index).add(heap.committed),
                    target - heap.committed,
                    PROT_READ_WRITE,
                )
            };
            if status != 0 {
                return std::ptr::null_mut();
            }
            heap.committed = target;
        }
        heap.bump = end;
        heap.used += size;
        slot(index).wrapping_add(start)
    }

    /// # Safety
    /// `pointer` came from `heap_alloc(index, layout)` and is still live.
    pub(super) unsafe fn heap_free(index: usize, pointer: *mut u8, layout: Layout) {
        // SAFETY: the owning thread has exclusive access to its heap entries.
        let heap = unsafe { &mut (*table()).heaps[index] };
        if !heap.live {
            // A discarded heap has no writable pages; its blocks are gone.
            return;
        }
        let Some((size, class)) = class_of(layout) else {
            return;
        };
        // SAFETY: the block is at least 16 bytes and writable while live.
        unsafe { pointer.cast::<*mut u8>().write(heap.free[class]) };
        heap.free[class] = pointer;
        heap.used -= size;
    }

    /// Routes allocations to the current actor heap, and frees to their owner.
    pub(crate) struct Router;

    // SAFETY: heap blocks are disjoint, aligned to the layout, and at least
    // the requested size; System handles every pointer outside the range.
    unsafe impl GlobalAlloc for Router {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            match CURRENT.with(Cell::get) {
                // SAFETY: as GlobalAlloc requires of the caller.
                0 => unsafe { System.alloc(layout) },
                // SAFETY: CURRENT only ever names a live heap of this thread.
                current => unsafe { heap_alloc(current - 1, layout) },
            }
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            match CURRENT.with(Cell::get) {
                // SAFETY: as GlobalAlloc requires; keeps calloc's fresh pages.
                0 => unsafe { System.alloc_zeroed(layout) },
                current => {
                    // SAFETY: as in `alloc`; a reused block may be dirty.
                    let pointer = unsafe { heap_alloc(current - 1, layout) };
                    if !pointer.is_null() {
                        // SAFETY: the block holds at least `layout.size()` bytes.
                        unsafe { pointer.write_bytes(0, layout.size()) };
                    }
                    pointer
                }
            }
        }
        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            match owner(pointer) {
                // SAFETY: the owner made this block with this layout.
                Some(index) => unsafe { heap_free(index, pointer, layout) },
                // SAFETY: as GlobalAlloc requires of the caller.
                None => unsafe { System.dealloc(pointer, layout) },
            }
        }
        unsafe fn realloc(&self, pointer: *mut u8, old: Layout, size: usize) -> *mut u8 {
            let Some(index) = owner(pointer) else {
                // Memory allocated outside every heap stays outside, even when
                // a handler grows it: the actor does not own it.
                // SAFETY: as GlobalAlloc requires of the caller.
                return unsafe { System.realloc(pointer, old, size) };
            };
            let Ok(new) = Layout::from_size_align(size, old.align()) else {
                return std::ptr::null_mut();
            };
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
    struct Resume([u64; 8]);

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

    /// Called after a trap's message is printed. Returns only when no handler
    /// is running on this thread, and the process then exits as before.
    pub(crate) fn recover() {
        let resume = RECOVERY.with(Cell::get);
        if resume.is_null() {
            return;
        }
        eprintln!("note: contained in an actor; its heap is discarded");
        // SAFETY: `resume` was filled by the enter call that is still on this
        // thread's stack, below every frame being abandoned.
        unsafe { plenty_actor_resume(resume, 1) }
    }

    fn live(heap: i64) -> Option<usize> {
        let index = usize::try_from(heap).ok().filter(|&i| i < MAX_HEAPS)?;
        // SAFETY: owning thread; see the module comment.
        unsafe { (*table()).heaps[index].live }.then_some(index)
    }

    /// Runs a compiled `__actor_run` adapter with `heap` current. Returns 0
    /// when the handler returned (its result is in `storage`) or 1 when it
    /// trapped.
    ///
    /// # Safety
    /// `entry` is the compiler's adapter for `storage`'s task layout.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn plenty_actor_run(
        heap: i64,
        storage: *mut u8,
        entry: unsafe extern "C" fn(*mut u8) -> *mut u8,
    ) -> i64 {
        let Some(index) = live(heap) else {
            crate::fail("std.actor: the actor heap is not live");
        };
        if !RECOVERY.with(Cell::get).is_null() {
            crate::fail("std.actor: a handler cannot run another handler");
        }
        let caller = crate::CALLER.with(Cell::get);
        let queue = crate::memory::save_queue();
        let mut resume = Resume([0; 8]);
        CURRENT.with(|current| current.set(index + 1));
        RECOVERY.with(|recovery| recovery.set(&mut resume));
        // SAFETY: the adapter reads its arguments from `storage` and writes
        // the result there; `resume` outlives the call.
        let status = unsafe { plenty_actor_enter(&mut resume, entry, storage) };
        RECOVERY.with(|recovery| recovery.set(std::ptr::null_mut()));
        CURRENT.with(|current| current.set(0));
        if status != 0 {
            // Thread-local runtime state the abandoned frames may have changed.
            crate::CALLER.with(|current| current.set(caller));
            crate::memory::restore_queue(queue);
        }
        status as i64
    }

    pub(crate) fn running() -> bool {
        !RECOVERY.with(Cell::get).is_null()
    }

    // ---------------------------------------------------------- operations

    pub(super) fn create(limit: i64) -> i64 {
        let Some(limit) = usize::try_from(limit)
            .ok()
            .filter(|&limit| (1..=SLOT_BYTES).contains(&limit))
        else {
            return BAD_LIMIT;
        };
        if BASE.load(Ordering::Acquire).is_null() {
            // Process-lifetime buffers std creates on first use must not land
            // in a heap that is later discarded.
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
                return RESERVE_FAILED;
            }
            BASE.store(base, Ordering::Release);
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
        // SAFETY (each table read): owning thread; see the module comment.
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
            DISCARD => discard(argument),
            USED => live(argument).map_or(-1, |index| heaps()[index].used as i64),
            USED_TOTAL => heaps()
                .iter()
                .filter(|heap| heap.live)
                .map(|heap| heap.used as i64)
                .sum(),
            COMMITTED_TOTAL => heaps().iter().map(|heap| heap.committed as i64).sum(),
            LIVE_COUNT => heaps().iter().filter(|heap| heap.live).count() as i64,
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
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
mod native {
    use super::*;

    pub(crate) fn outside<R>(f: impl FnOnce() -> R) -> R {
        f()
    }
    pub(crate) fn recover() {}
    #[cfg(feature = "allocation-checks")]
    pub(crate) fn in_heap(_: *const u8) -> bool {
        false
    }

    pub(crate) fn foreign(_: *const u8) -> bool {
        false
    }
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
            CREATE => UNSUPPORTED,
            USED_TOTAL | COMMITTED_TOTAL | LIVE_COUNT => 0,
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

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
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
}
