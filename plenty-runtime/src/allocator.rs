//! Retained, thread-safe allocator storage for B13 builtin-owner integration.
//!
//! Policies receive checked owned blocks, never arbitrary pointers. All provider
//! state comes from an explicit bootstrap allocator. The system root is static;
//! it bypasses ambient actor heaps. No public ABI or compiler binding yet.
use crate::memory::AllocError;
use std::alloc::Layout;
#[cfg(not(feature = "allocation-checks"))]
use std::alloc::{GlobalAlloc, System};
use std::cell::Cell;
use std::mem::ManuallyDrop;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

/// The future compiler bridge must additionally certify state, callback effects
/// and destruction. Native callback scopes prohibit nonlocal trap recovery;
/// Send alone still cannot certify arbitrary compiled callbacks.
pub(crate) trait Provider: Send + 'static {
    fn allocate(&mut self, layout: Layout) -> Result<Block, AllocError>;
    fn release(&mut self, block: Block);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CreateError {
    Allocation(AllocError),
    UnsupportedContext,
}

pub(crate) struct Rejected<P> {
    pub error: CreateError,
    pub provider: P,
}

#[repr(C)]
struct ProviderHeader {
    refs: AtomicUsize,
    allocate: unsafe fn(NonNull<ProviderHeader>, Layout) -> Result<Block, AllocError>,
    release: unsafe fn(NonNull<ProviderHeader>, Block),
    destroy: unsafe fn(NonNull<ProviderHeader>),
}
#[repr(C)]
struct State<P> {
    header: ProviderHeader,
    active_thread: AtomicUsize,
    provider: Mutex<P>,
    // This block contains this State. Final release moves it out before freeing.
    storage: ManuallyDrop<Block>,
}

/// None denotes the explicitly selected, allocation-free system root. It is
/// never substituted for an exhausted provider or an omitted allocator.
pub(crate) struct Allocator(Option<NonNull<ProviderHeader>>);

// SAFETY: only Send providers enter State; every callback has exclusive access
// under its mutex. Refcounts and final-state destruction synchronize ownership.
unsafe impl Send for Allocator {}
unsafe impl Sync for Allocator {}

fn retain(refs: &AtomicUsize) {
    if refs
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            (n < isize::MAX as usize).then_some(n + 1)
        })
        .is_err()
    {
        crate::fail("allocator reference count overflow");
    }
}
fn final_release(refs: &AtomicUsize) -> bool {
    if refs.fetch_sub(1, Ordering::Release) != 1 {
        return false;
    }
    std::sync::atomic::fence(Ordering::Acquire);
    true
}
impl Clone for Allocator {
    fn clone(&self) -> Self {
        if let Some(header) = self.0 {
            // SAFETY: this handle owns a live reference.
            retain(unsafe { &header.as_ref().refs });
        }
        Self(self.0)
    }
}
impl Drop for Allocator {
    fn drop(&mut self) {
        if let Some(header) = self.0 {
            // SAFETY: the last reference exclusively owns state destruction.
            unsafe {
                if final_release(&header.as_ref().refs) {
                    (header.as_ref().destroy)(header);
                }
            }
        }
    }
}

static NEXT_THREAD: AtomicUsize = AtomicUsize::new(1);
thread_local! {
    static THREAD_ID: Cell<usize> = const { Cell::new(0) };
    static CALLBACK_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Actor and arena recovery must not discard a provider's lock or moved owners.
/// This is thread-local, allocation-free, and remains active in nested callbacks.
pub(crate) fn callback_active() -> bool {
    CALLBACK_ACTIVE.with(Cell::get)
}
fn callback<R>(f: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            CALLBACK_ACTIVE.with(|active| active.set(self.0));
        }
    }
    let _restore = Restore(CALLBACK_ACTIVE.with(|active| active.replace(true)));
    // A callback may run while an outer managed-object drop queue is draining.
    // Its own managed cleanup must finish before its lock and fatal-trap scope
    // end, not be deferred into that outer queue. Preserve the outer queue on
    // both normal return and native unwinding, just as for Plenty user hooks.
    crate::memory::with_nested_drops(f)
}

fn thread_id() -> usize {
    THREAD_ID.with(|id| {
        if id.get() == 0 {
            // Never reuse IDs: TLS addresses can be reused by later threads,
            // and the pre-lock recursion check may observe an older marker.
            let next = NEXT_THREAD
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .unwrap_or_else(|_| crate::fail("allocator thread identity overflow"));
            id.set(next);
        }
        id.get()
    })
}
struct Access<'a, P> {
    guard: MutexGuard<'a, P>,
    active: &'a AtomicUsize,
}
impl<P> Drop for Access<'_, P> {
    fn drop(&mut self) {
        self.active.store(0, Ordering::Relaxed);
        // The mutex unlocks when guard is dropped, after the marker is cleared.
    }
}
fn access<P>(state: &State<P>) -> Access<'_, P> {
    let thread = thread_id();
    if state.active_thread.load(Ordering::Relaxed) == thread {
        crate::fail("reentrant allocator callback");
    }
    let guard = state.provider.lock().unwrap_or_else(|_| {
        crate::fail("poisoned allocator provider");
    });
    state.active_thread.store(thread, Ordering::Relaxed);
    Access {
        guard,
        active: &state.active_thread,
    }
}
unsafe fn allocate<P: Provider>(
    header: NonNull<ProviderHeader>,
    layout: Layout,
) -> Result<Block, AllocError> {
    // SAFETY: the vtable was installed for this exact State<P> allocation.
    let state = unsafe { header.cast::<State<P>>().as_ref() };
    access(state).guard.allocate(layout)
}
unsafe fn release<P: Provider>(header: NonNull<ProviderHeader>, block: Block) {
    // SAFETY: the retained allocator keeps State<P> alive across the callback.
    let state = unsafe { header.cast::<State<P>>().as_ref() };
    callback(|| access(state).guard.release(block));
}
unsafe fn destroy<P: Provider>(header: NonNull<ProviderHeader>) {
    let state = header.cast::<State<P>>().as_ptr();
    // SAFETY: no handles or callbacks remain. Move the owning block out first;
    // dropping the provider may release other blocks, but cannot free this state.
    callback(|| unsafe {
        let storage = ManuallyDrop::into_inner(ptr::read(ptr::addr_of!((*state).storage)));
        ptr::drop_in_place(ptr::addr_of_mut!((*state).provider));
        drop(storage);
    });
}

impl Allocator {
    pub(crate) const fn system() -> Self {
        Self(None)
    }
    pub(crate) fn new<P: Provider>(bootstrap: &Self, provider: P) -> Result<Self, Rejected<P>> {
        // Reject before bootstrap allocation or ownership transfer. Thread-safe
        // state alone does not establish survival across heap abandonment.
        if !crate::actor_heap::allocator_registration_allowed() {
            return Err(Rejected {
                error: CreateError::UnsupportedContext,
                provider,
            });
        }
        let mut storage = match bootstrap.allocate(Layout::new::<State<P>>()) {
            Ok(block) => block,
            Err(error) => {
                return Err(Rejected {
                    error: CreateError::Allocation(error),
                    provider,
                });
            }
        };
        // SAFETY: allocate validated the complete State<P> layout. This state
        // owns storage until its final reference; no safe byte view escapes.
        let state = unsafe { storage.as_mut_ptr().cast::<State<P>>() };
        unsafe {
            state.write(State {
                header: ProviderHeader {
                    refs: AtomicUsize::new(1),
                    allocate: allocate::<P>,
                    release: release::<P>,
                    destroy: destroy::<P>,
                },
                active_thread: AtomicUsize::new(0),
                provider: Mutex::new(provider),
                storage: ManuallyDrop::new(storage),
            });
            Ok(Self(Some(NonNull::new_unchecked(state.cast()))))
        }
    }
    pub(crate) fn allocate(&self, layout: Layout) -> Result<Block, AllocError> {
        let (whole, offset) = Layout::new::<Backing>()
            .extend(layout)
            .map_err(|_| AllocError::CapacityOverflow)?;
        let whole = whole.pad_to_align();
        let (base, source) = match self.0 {
            None => {
                // Explicit system selection, not Rust's ambient global router.
                #[cfg(feature = "allocation-checks")]
                let base = unsafe { crate::accounting::system_alloc_zeroed(whole) };
                #[cfg(not(feature = "allocation-checks"))]
                let base = unsafe { System.alloc_zeroed(whole) };
                let base = NonNull::new(base).ok_or(AllocError::OutOfMemory)?;
                (base, Source::System(whole))
            }
            Some(header) => callback(|| {
                // Include validation: an invalid returned block must not escape
                // through actor recovery after the callback unlocks its mutex.
                // SAFETY: our live count keeps the matching vtable/state alive.
                let mut block = unsafe { (header.as_ref().allocate)(header, whole)? };
                if block.len < whole.size() || block.alignment() < whole.align() {
                    crate::fail("allocator provider returned insufficient extent or alignment");
                }
                // A cached provider block need not arrive zeroed.
                block.bytes_mut().fill(0);
                // SAFETY: exclusive owned block, validated layout, no views.
                let base = unsafe { NonNull::new_unchecked(block.as_mut_ptr()) };
                Ok((
                    base,
                    Source::Provider {
                        allocator: self.clone(),
                        block,
                    },
                ))
            })?,
        };
        let backing = base.cast::<Backing>();
        // SAFETY: whole contains the aligned Backing followed by the requested
        // initialized byte range. Source owns this exact storage until release.
        unsafe {
            backing.as_ptr().write(Backing {
                refs: AtomicUsize::new(1),
                source,
            });
            Ok(Block {
                backing,
                data: NonNull::new_unchecked(base.as_ptr().add(offset)),
                len: layout.size(),
            })
        }
    }
}

enum Source {
    System(Layout),
    Provider { allocator: Allocator, block: Block },
}
struct Backing {
    refs: AtomicUsize,
    source: Source,
}

/// Affine initialized bytes. Splitting creates disjoint ranges that retain a
/// common backing owner. No public constructor or cloning can create overlap.
pub(crate) struct Block {
    backing: NonNull<Backing>,
    data: NonNull<u8>,
    len: usize,
}
// SAFETY: ranges are disjoint, shared byte access is read-only, and backing
// ownership is atomic. All retained provider state is serialized and Send.
unsafe impl Send for Block {}
unsafe impl Sync for Block {}
impl Block {
    pub(crate) fn bytes(&self) -> &[u8] {
        // SAFETY: this range is initialized and retained for the borrow.
        unsafe { std::slice::from_raw_parts(self.data.as_ptr(), self.len) }
    }
    pub(crate) fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: exclusive borrow of an affine, disjoint range.
        unsafe { std::slice::from_raw_parts_mut(self.data.as_ptr(), self.len) }
    }
    pub(crate) fn alignment(&self) -> usize {
        1usize << self.data.as_ptr().addr().trailing_zeros()
    }
    pub(crate) fn split_off(&mut self, at: usize) -> Self {
        assert!(at <= self.len, "Block.split_off: at");
        // SAFETY: both resulting ranges lie inside the retained allocation.
        unsafe {
            retain(&self.backing.as_ref().refs);
            let result = Self {
                backing: self.backing,
                data: NonNull::new_unchecked(self.data.as_ptr().add(at)),
                len: self.len - at,
            };
            self.len = at;
            result
        }
    }
    /// Trusted typed consumers must keep all byte views private while values
    /// are initialized here, and destroy those values before dropping the block.
    pub(crate) unsafe fn as_mut_ptr(&mut self) -> *mut u8 {
        self.data.as_ptr()
    }
}
impl Drop for Block {
    fn drop(&mut self) {
        // SAFETY: the final range owns the backing. Move Source out before any
        // clearing/freeing; it may contain the block that physically holds it.
        unsafe {
            if !final_release(&self.backing.as_ref().refs) {
                return;
            }
            let source = ptr::read(ptr::addr_of!((*self.backing.as_ptr()).source));
            match source {
                Source::System(layout) => {
                    #[cfg(feature = "allocation-checks")]
                    crate::accounting::system_dealloc(self.backing.as_ptr().cast(), layout);
                    #[cfg(not(feature = "allocation-checks"))]
                    System.dealloc(self.backing.as_ptr().cast(), layout);
                }
                Source::Provider {
                    allocator,
                    mut block,
                } => {
                    // Typed padding can have become uninitialized. Clear the
                    // entire range before any safe provider byte inspection.
                    ptr::write_bytes(block.as_mut_ptr(), 0, block.len);
                    let header = allocator.0.expect("provider source");
                    (header.as_ref().release)(header, block);
                    // Keep allocator alive until its callback has returned.
                }
            }
        }
    }
}

pub(crate) mod bindings;

#[cfg(test)]
mod managed_tests;
#[cfg(test)]
mod tests;
