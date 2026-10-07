use crate::aggregates::{release, Type};
use crate::memory::{self, Header};

type Resume = unsafe extern "C" fn(*mut Generator, *mut u128) -> u8;
#[repr(C)]
pub(crate) struct Generator {
    header: Header,
    resume: Resume,
    state: u64,
    running: u64,
    count: u64,
    managed: *const *const Type,
    slots: [u128; 0],
}
const _: () = assert!(std::mem::offset_of!(Generator, slots) == 64);

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_generator_finish(g: *mut Generator) {
    // SAFETY: generated frames match this prefix; their managed mask is immortal.
    // Do not create a mutable Rust view across releases, which can call user code.
    unsafe {
        for n in 0..(*g).count as usize {
            let i = if memory::dropping() {
                n
            } else {
                (*g).count as usize - 1 - n
            };
            let slot = std::ptr::addr_of_mut!((*g).slots).cast::<u128>().add(i);
            let value = slot.replace(0);
            release(value, &**(*g).managed.add(i));
        }
        (*g).state = u64::MAX;
    }
}
unsafe extern "C" fn destroy(header: *mut Header) {
    unsafe {
        let g = header.cast::<Generator>();
        plenty_generator_finish(g);
        memory::free::<Generator, u128>(g, (*g).count as usize);
    }
}
#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_generator_new(
    resume: Resume,
    count: u64,
    managed: *const *const Type,
) -> *mut Generator {
    // SAFETY: forwarded callback and metadata retain the constructor contract.
    unsafe { try_new(resume, count, managed) }
        .unwrap_or_else(|_| crate::fail("generator allocation failed"))
}

/// Allocate only the frame. Callers transfer captures after success, and retain
/// responsibility for them on failure. The managed mask must outlive the frame.
pub(crate) unsafe fn try_new(
    resume: Resume,
    count: u64,
    managed: *const *const Type,
) -> Result<*mut Generator, memory::AllocError> {
    let slots = usize::try_from(count).map_err(|_| memory::AllocError::CapacityOverflow)?;
    let g = memory::try_allocate::<Generator, u128>(slots)?;
    unsafe {
        g.write(Generator {
            header: Header::new(destroy),
            resume,
            state: 0,
            running: 0,
            count,
            managed,
            slots: [],
        });
    }
    Ok(g)
}

/// Native ABI: consumes the capture prefix on both success and failure. The
/// metadata covers all slots; captures and out are aligned, disjoint buffers.
#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_generator_try_new(
    resume: Resume,
    count: u64,
    managed: *const *const Type,
    captures: *const u128,
    capture_count: u64,
    out: *mut u128,
) {
    unsafe {
        assert!(capture_count <= count);
        let result = match try_new(resume, count, managed) {
            Ok(frame) => {
                std::ptr::copy_nonoverlapping(
                    captures,
                    std::ptr::addr_of_mut!((*frame).slots).cast(),
                    capture_count as usize,
                );
                crate::aggregates::wrap(frame as u128, 0)
            }
            Err(error) => {
                for i in (0..capture_count as usize).rev() {
                    release(*captures.add(i), &**managed.add(i));
                }
                crate::aggregates::wrap(crate::aggregates::wrap(0, error as u64), 1)
            }
        };
        out.write(result);
    }
}
#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_generator_resume(g: *mut Generator, out: *mut u128) -> u8 {
    unsafe {
        if (*g).running != 0 {
            crate::fail("generator is already running");
        }
        if (*g).state == u64::MAX {
            return 0;
        }
        (*g).running = 1;
        let result = ((*g).resume)(g, out);
        (*g).running = 0;
        result
    }
}
