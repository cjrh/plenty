use crate::memory::{self, plenty_release, Header};

type Resume = unsafe extern "C" fn(*mut Generator, *mut u64) -> u8;
#[repr(C)]
pub(crate) struct Generator {
    header: Header,
    resume: Resume,
    state: u64,
    running: u64,
    count: u64,
    managed: *const u8,
    slots: [u64; 0],
}
const _: () = assert!(std::mem::offset_of!(Generator, slots) == 56);

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
            let slot = std::ptr::addr_of_mut!((*g).slots).cast::<u64>().add(i);
            let value = slot.replace(0);
            if *(*g).managed.add(i) != 0 {
                plenty_release(value as *mut Header);
            }
        }
        (*g).state = u64::MAX;
    }
}
unsafe extern "C" fn destroy(header: *mut Header) {
    unsafe {
        let g = header.cast::<Generator>();
        plenty_generator_finish(g);
        memory::free::<Generator, u64>(g, (*g).count as usize);
    }
}
#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_generator_new(
    resume: Resume,
    count: u64,
    managed: *const u8,
) -> *mut Generator {
    let g = memory::allocate::<Generator, u64>(count as usize);
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
    g
}
#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_generator_resume(g: *mut Generator, out: *mut u64) -> u8 {
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
