use crate::aggregates::{release, Type};
use crate::memory::{self, Header};
use crate::ranges;

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

/// Initialize caller-owned storage. Metadata describes all slots, including
/// their inline payloads; captures are a packed prefix of ordinary value bits.
/// No callback runs and no memory is allocated by this operation.
#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_generator_init(
    g: *mut Generator,
    resume: Resume,
    count: u64,
    managed: *const *const Type,
    captures: *const u128,
    capture_count: u64,
) {
    // SAFETY: generated callers reserve the complete aligned frame layout and
    // pass immutable metadata and disjoint live capture operands.
    unsafe {
        assert!(capture_count <= count);
        g.write(Generator {
            header: Header::new(destroy_inline),
            resume,
            state: 0,
            running: 0,
            count,
            managed,
            slots: [],
        });
        let mut slot = std::ptr::addr_of_mut!((*g).slots).cast::<u128>();
        std::ptr::write_bytes(slot, 0, slot_words(g));
        for i in 0..capture_count as usize {
            let ty = &**managed.add(i);
            ranges::store(slot, *captures.add(i), ty);
            slot = slot.add(ty.slot_words());
        }
    }
}

/// Rebase embedded addresses after a bytewise move. Frame accesses are relative
/// to the new owner; recursive traversal visits only finite inline layouts.
pub(crate) unsafe fn relocate(g: *mut Generator) {
    unsafe {
        let mut slot = std::ptr::addr_of_mut!((*g).slots).cast::<u128>();
        for i in 0..(*g).count as usize {
            let ty = &**(*g).managed.add(i);
            ranges::relocate(slot, ty);
            slot = slot.add(ty.slot_words());
        }
    }
}

unsafe extern "C" fn destroy_inline(header: *mut Header) {
    unsafe { plenty_generator_finish(header.cast()) };
}

/// Inline storage must finish synchronously, even inside another object's drop
/// hook. It cannot be left on the heap-object destruction queue after its owner
/// returns. Temporary inspection aliases share the active frame's count.
pub(crate) unsafe fn release_inline(g: *mut Generator) {
    unsafe {
        if g.is_null() {
            return;
        }
        if (*g).header.release_last() {
            memory::with_nested_drops(|| plenty_generator_finish(g));
        }
    }
}

unsafe fn slot_words(g: *const Generator) -> usize {
    unsafe {
        (0..(*g).count as usize)
            .map(|i| (&**(*g).managed.add(i)).slot_words())
            .sum()
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_generator_finish(g: *mut Generator) {
    // SAFETY: generated frames match this prefix; their managed mask is immortal.
    // Do not create a mutable Rust view across releases, which can call user code.
    unsafe {
        let forward = memory::dropping();
        let mut offset = if forward { 0 } else { slot_words(g) };
        for n in 0..(*g).count as usize {
            let i = if forward {
                n
            } else {
                (*g).count as usize - 1 - n
            };
            let ty = &**(*g).managed.add(i);
            if !forward {
                offset -= ty.slot_words();
            }
            let slot = std::ptr::addr_of_mut!((*g).slots)
                .cast::<u128>()
                .add(offset);
            let value = slot.replace(0);
            if forward {
                offset += ty.slot_words();
            }
            release(value, ty);
        }
        (*g).state = u64::MAX;
    }
}
unsafe extern "C" fn destroy(header: *mut Header) {
    unsafe {
        let g = header.cast::<Generator>();
        plenty_generator_finish(g);
        memory::free::<Generator, u128>(g, slot_words(g));
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
    memory::checked_capacity::<u128>(slots)?;
    let slots = unsafe {
        (0..slots).try_fold(0usize, |n, i| {
            n.checked_add((&**managed.add(i)).slot_words())
                .ok_or(memory::AllocError::CapacityOverflow)
        })?
    };
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
                let mut slot = std::ptr::addr_of_mut!((*frame).slots).cast::<u128>();
                for i in 0..capture_count as usize {
                    let ty = &**managed.add(i);
                    ranges::store(slot, *captures.add(i), ty);
                    slot = slot.add(ty.slot_words());
                }
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
