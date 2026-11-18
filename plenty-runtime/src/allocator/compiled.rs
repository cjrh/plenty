//! Transactional registration of compiler-certified Plenty providers.
use super::*;
use crate::aggregates::{Type, allocator_owners};
use crate::memory::{self, Header, plenty_release};

// Private scalar callback ABI, independent of Plenty/Rust enum layouts.
const PROVIDER_OK: u64 = 0;
const PROVIDER_OUT_OF_MEMORY: u64 = 1;
const PROVIDER_CAPACITY_OVERFLOW: u64 = 2;
const REGISTRATION_OUT_OF_MEMORY: u64 = 0;
const REGISTRATION_CAPACITY_OVERFLOW: u64 = 1;
const REGISTRATION_UNSUPPORTED_CONTEXT: u64 = 2;

type Allocate = unsafe extern "C" fn(*mut Header, u64, u64, *mut u64) -> u64;
type Release = unsafe extern "C" fn(*mut Header, *const u64);

struct CompiledProvider {
    state: *mut Header,
    allocate: Allocate,
    release: Release,
}
// SAFETY: only compiler-generated registration constructs this value. Admission
// checks the entire state, opaque contracts, callbacks and transitive destruction
// for worker eligibility. Native State serializes all mutable callback access.
unsafe impl Send for CompiledProvider {}

impl Provider for CompiledProvider {
    fn allocate(&mut self, layout: Layout) -> Result<Block, AllocError> {
        let mut words = [0u64; 4];
        let status = unsafe {
            (self.allocate)(
                self.state,
                layout.size() as u64,
                layout.align() as u64,
                words.as_mut_ptr(),
            )
        };
        match status {
            PROVIDER_OK => Ok(ManuallyDrop::into_inner(unsafe {
                bindings::read_block(words.as_ptr() as u128)
            })),
            PROVIDER_OUT_OF_MEMORY => Err(AllocError::OutOfMemory),
            PROVIDER_CAPACITY_OVERFLOW => Err(AllocError::CapacityOverflow),
            _ => crate::fail("invalid allocator provider callback status"),
        }
    }
    fn release(&mut self, block: Block) {
        let mut words = [0u64; 4];
        unsafe {
            bindings::write_block(words.as_mut_ptr() as u128, block);
            (self.release)(self.state, words.as_ptr());
        }
    }
}

unsafe fn destroy_compiled(header: NonNull<ProviderHeader>) {
    // The last handle owns the control allocation. Replace its dead State
    // with a continuation that holds storage until the boxed fields finish.
    const {
        assert!(size_of::<State<CompiledProvider>>() >= size_of::<StateCleanup>());
        assert!(align_of::<State<CompiledProvider>>() >= align_of::<StateCleanup>());
    }
    unsafe {
        let state = header.cast::<State<CompiledProvider>>().as_ptr();
        let provider = ptr::read(ptr::addr_of!((*state).provider))
            .into_inner()
            .unwrap_or_else(|e| e.into_inner());
        let storage = ptr::read(ptr::addr_of!((*state).storage));
        let cleanup = state.cast::<StateCleanup>();
        cleanup.write(StateCleanup {
            header: Header::new(release_state),
            child: provider.state,
            storage,
        });
        with_callback_scope(|| {
            memory::with_provider_state_drops(|| plenty_release(cleanup.cast()))
        });
    }
}

#[repr(C)]
struct StateCleanup {
    header: Header,
    child: *mut Header,
    storage: ManuallyDrop<Block>,
}
unsafe extern "C" fn release_state(header: *mut Header) {
    unsafe {
        let cleanup = header.cast::<StateCleanup>();
        let child = ptr::replace(ptr::addr_of_mut!((*cleanup).child), ptr::null_mut());
        if child.is_null() {
            drop(ManuallyDrop::take(&mut (*cleanup).storage));
        } else {
            memory::continue_after(header, child);
        }
    }
}

/// Inputs: borrowed bootstrap, packed borrowed P, allocate/release adapters.
/// The static descriptor is Box[P]. On every failure P remains with the caller;
/// on success the caller immediately commits its source without destruction.
pub(crate) unsafe fn register(
    [bootstrap, source, allocate, release]: [u128; 4],
    ty: *const Type,
) -> u128 {
    let error = |code: u64| (1u128 << 64) | u128::from(code);
    if !crate::actor_heap::allocator_registration_allowed() {
        return error(REGISTRATION_UNSUPPORTED_CONTEXT);
    }
    let bootstrap = ManuallyDrop::new(Allocator(NonNull::new(bootstrap as *mut ProviderHeader)));
    let ty = unsafe { &*ty };
    let state = match unsafe { allocator_owners::box_new_in(&bootstrap, ty, source) } {
        Ok(state) => state,
        Err(AllocError::OutOfMemory) => return error(REGISTRATION_OUT_OF_MEMORY),
        Err(AllocError::CapacityOverflow) => return error(REGISTRATION_CAPACITY_OVERFLOW),
    };
    let provider = CompiledProvider {
        state,
        allocate: unsafe { std::mem::transmute::<usize, Allocate>(allocate as usize) },
        release: unsafe { std::mem::transmute::<usize, Release>(release as usize) },
    };
    match unsafe { Allocator::new_with_destroy(&bootstrap, provider, destroy_compiled) } {
        Ok(allocator) => {
            let allocator = ManuallyDrop::new(allocator);
            allocator.0.unwrap().as_ptr() as u128
        }
        Err(rejected) => {
            unsafe { allocator_owners::provider_box_rollback(rejected.provider.state, ty) };
            error(match rejected.error {
                CreateError::Allocation(AllocError::OutOfMemory) => REGISTRATION_OUT_OF_MEMORY,
                CreateError::Allocation(AllocError::CapacityOverflow) => {
                    REGISTRATION_CAPACITY_OVERFLOW
                }
                CreateError::UnsupportedContext => REGISTRATION_UNSUPPORTED_CONTEXT,
            })
        }
    }
}

#[cfg(test)]
mod tests;
