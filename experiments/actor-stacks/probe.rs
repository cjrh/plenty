// Included inside actor_heap::native only in test builds. This uses the actual
// heap router and nonlocal trap recovery, rather than modelling them in C.
use super::*;
use std::mem::MaybeUninit;
use std::ptr;

const STACK_BYTES: usize = 64 * 1024;
const QUANTUM: usize = 17;
const WORK: usize = 10_000;

#[derive(Clone, Copy)]
struct RuntimeState {
    current: usize,
    recovery: *mut Resume,
    caller: *const crate::Site,
    queue: crate::memory::SavedQueue,
    last_trap: i64,
}

impl RuntimeState {
    fn capture() -> Self {
        // A production implementation must enforce these exclusions through
        // compiled callbacks too, not merely assert at runtime.
        assert!(ARENA.with(Cell::get).is_null());
        assert!(ARENA_RESUME.with(Cell::get).is_null());
        assert!(!crate::memory::dropping());
        Self {
            current: CURRENT.with(Cell::get),
            recovery: RECOVERY.with(Cell::get),
            caller: crate::CALLER.with(Cell::get),
            queue: crate::memory::save_queue(),
            last_trap: LAST_TRAP.with(Cell::get),
        }
    }

    fn restore(self) {
        CURRENT.with(|slot| slot.set(self.current));
        RECOVERY.with(|slot| slot.set(self.recovery));
        crate::CALLER.with(|slot| slot.set(self.caller));
        crate::memory::restore_queue(self.queue);
        LAST_TRAP.with(|slot| slot.set(self.last_trap));
    }
}

struct Actor {
    context: libc::ucontext_t,
    mapping: *mut libc::c_void,
    mapping_bytes: usize,
    heap: i64,
    state: RuntimeState,
    remaining: usize,
    checks: usize,
    yields: usize,
    progress: usize,
    finished: bool,
    trap: bool,
    status: i64,
    drops: usize,
    cancelled: bool,
    identity: crate::Site,
}

struct Harness {
    context: libc::ucontext_t,
    state: RuntimeState,
    actors: [Actor; 2],
    active: usize,
    budgeted: bool,
    turns: usize,
    peer_observed: usize,
    cancel_after: Option<usize>,
}

thread_local! {
    static HARNESS: Cell<*mut Harness> = const { Cell::new(ptr::null_mut()) };
}

// All harness accesses use raw pointers. Holding &mut Harness/Actor across a
// switch would alias the scheduler's accesses while that call is suspended.
fn active() -> (*mut Harness, *mut Actor) {
    let harness = HARNESS.with(Cell::get);
    assert!(!harness.is_null());
    // SAFETY: run_case pins this allocation until both actors have finished.
    let actor = unsafe { ptr::addr_of_mut!((*harness).actors[(*harness).active]) };
    (harness, actor)
}

fn checkpoint() -> bool {
    let (harness, actor) = active();
    // SAFETY: one native thread accesses stable records, without outstanding
    // Rust references to those records across swapcontext.
    unsafe {
        assert_eq!(CURRENT.with(Cell::get), (*actor).heap as usize + 1);
        let recovery = RECOVERY.with(Cell::get) as usize;
        let stack_base = (*actor).mapping as usize;
        assert!((stack_base..stack_base + (*actor).mapping_bytes).contains(&recovery));
        assert_eq!(LAST_TRAP.with(Cell::get), (*actor).heap + 100);
        assert_eq!(
            crate::CALLER.with(Cell::get),
            ptr::addr_of!((*actor).identity)
        );
        (*actor).checks += 1;
        if (*harness).budgeted {
            (*actor).remaining -= 1;
            if (*actor).remaining == 0 {
                (*actor).remaining = QUANTUM;
                (*actor).yields += 1;
                (*actor).state = RuntimeState::capture();
                (*harness).state.restore();
                assert_eq!(
                    libc::swapcontext(
                        ptr::addr_of_mut!((*actor).context),
                        ptr::addr_of!((*harness).context),
                    ),
                    0
                );
                assert_eq!(CURRENT.with(Cell::get), (*actor).heap as usize + 1);
                assert_eq!(RECOVERY.with(Cell::get), (*actor).state.recovery);
                assert_eq!(LAST_TRAP.with(Cell::get), (*actor).heap + 100);
                assert_eq!(
                    crate::CALLER.with(Cell::get),
                    ptr::addr_of!((*actor).identity)
                );
            }
        }
        (*actor).cancelled
    }
}

struct Cleanup(*mut Actor);

impl Drop for Cleanup {
    fn drop(&mut self) {
        // SAFETY: actor storage outlives every callback, including its cleanup.
        unsafe { (*self.0).drops += 1 };
    }
}

#[inline(never)]
fn nested(local: &mut usize, depth: usize) -> bool {
    // Keep work after the recursive call so an optimized build cannot turn
    // this into one tail-recursive loop with no retained caller frames.
    let canary = std::hint::black_box(depth + 1000);
    let cancelled = if depth > 0 {
        nested(local, depth - 1)
    } else {
        *local += 1;
        checkpoint()
    };
    assert_eq!(canary, depth + 1000);
    cancelled
}

unsafe extern "C" fn handler(_: *mut u8) -> *mut u8 {
    let (harness, actor) = active();
    // SAFETY: only this actor mutates its own local state; the peer observes
    // progress through raw harness fields, without accessing these locals.
    unsafe {
        let _cleanup = Cleanup(actor);
        LAST_TRAP.with(|slot| slot.set((*actor).heap + 100));
        let previous = crate::plenty_caller_enter(ptr::addr_of!((*actor).identity));
        let mut local = 0;
        let value = Box::new(0x1234_u64);
        assert_eq!(
            owner(ptr::from_ref(&*value).cast()),
            Some((*actor).heap as usize)
        );
        // Call the router directly: Rust may elide an unused standard
        // allocation, so its null test is not a reliable failure probe at -O.
        let oversized = Layout::from_size_align(2 * 1024 * 1024, 8).unwrap();
        assert!(Router.alloc(oversized).is_null());
        for _ in 0..WORK {
            (*actor).progress += 1;
            if (*harness).active == 1 && (*actor).progress == 1 {
                (*harness).peer_observed = (*harness).actors[0].progress;
            }
            if nested(&mut local, 3) {
                break;
            }
            assert_eq!(local, (*actor).progress);
            assert_eq!(*value, 0x1234);
            assert!(!foreign(ptr::from_ref(&*value).cast()));
            if (*actor).trap && local == 3 * QUANTUM + 1 {
                // Existing production trap path, after three resumptions.
                crate::io::plenty_trap_unwrap(ptr::null(), 1);
            }
        }
        crate::plenty_caller_leave(previous);
    }
    ptr::null_mut()
}

extern "C" fn actor_entry() {
    let (harness, actor) = active();
    // SAFETY: this entry runs on its mapped stack. Its recovery record lives
    // on that stack for the entire suspended callback's lifetime.
    unsafe {
        (*actor).status = plenty_actor_run((*actor).heap, ptr::null_mut(), handler);
        if (*actor).status != 0 {
            assert_eq!(LAST_TRAP.with(Cell::get), (*actor).status);
        }
        (*actor).finished = true;
        (*harness).state.restore();
        assert_eq!(
            libc::swapcontext(
                ptr::addr_of_mut!((*actor).context),
                ptr::addr_of!((*harness).context),
            ),
            0
        );
    }
    unreachable!("a finished actor must never resume");
}

fn new_actor(state: RuntimeState, trap: bool) -> Actor {
    // SAFETY: libc fills a stable context before it is activated. Contexts
    // contain internal pointers, so getcontext runs again after final placement.
    unsafe {
        let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        assert!(page.is_power_of_two());
        let mapping_bytes = STACK_BYTES + 2 * page;
        let mapping = libc::mmap(
            ptr::null_mut(),
            mapping_bytes,
            libc::PROT_NONE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );
        assert_ne!(mapping, libc::MAP_FAILED);
        assert_eq!(
            libc::mprotect(
                mapping.cast::<u8>().add(page).cast(),
                STACK_BYTES,
                libc::PROT_READ | libc::PROT_WRITE
            ),
            0
        );
        let heap = create(1024 * 1024);
        assert!(heap >= 0);
        Actor {
            context: MaybeUninit::zeroed().assume_init(),
            mapping,
            mapping_bytes,
            heap,
            state,
            remaining: QUANTUM,
            checks: 0,
            yields: 0,
            progress: 0,
            finished: false,
            trap,
            status: 0,
            drops: 0,
            cancelled: false,
            identity: crate::Site {
                line: 1,
                column: 1,
                file: 0,
                standard_library: 0,
            },
        }
    }
}

fn run_case(budgeted: bool, trap: bool, cancel_after: Option<usize>) {
    assert_eq!(prepare(), 0);
    let state = RuntimeState::capture();
    let harness = Box::into_raw(Box::new(Harness {
        // SAFETY: swapcontext initializes this before the first return to it.
        context: unsafe { MaybeUninit::zeroed().assume_init() },
        state,
        actors: [new_actor(state, trap), new_actor(state, false)],
        active: 0,
        budgeted,
        turns: 0,
        peer_observed: 0,
        cancel_after,
    }));
    HARNESS.with(|slot| slot.set(harness));
    // SAFETY: contexts and stack mappings are immovable until completion. The
    // scheduler never resumes a finished actor or reenters a suspended handler.
    unsafe {
        for index in 0..2 {
            let actor = ptr::addr_of_mut!((*harness).actors[index]);
            assert_eq!(libc::getcontext(ptr::addr_of_mut!((*actor).context)), 0);
            let page = ((*actor).mapping_bytes - STACK_BYTES) / 2;
            (*actor).context.uc_stack.ss_sp = (*actor).mapping.cast::<u8>().add(page).cast();
            (*actor).context.uc_stack.ss_size = STACK_BYTES;
            (*actor).context.uc_link = ptr::null_mut();
            libc::makecontext(ptr::addr_of_mut!((*actor).context), actor_entry, 0);
        }
        while !(*harness).actors.iter().all(|actor| actor.finished) {
            for index in 0..2 {
                let actor = ptr::addr_of_mut!((*harness).actors[index]);
                if (*actor).finished {
                    continue;
                }
                (*harness).active = index;
                (*harness).turns += 1;
                assert!((*harness).turns < 2000);
                if (*harness)
                    .cancel_after
                    .is_some_and(|turn| (*harness).turns >= turn)
                {
                    (*actor).cancelled = true;
                }
                (*actor).state.restore();
                assert_eq!(
                    libc::swapcontext(
                        ptr::addr_of_mut!((*harness).context),
                        ptr::addr_of!((*actor).context)
                    ),
                    0
                );
                assert_eq!(CURRENT.with(Cell::get), state.current);
                assert_eq!(RECOVERY.with(Cell::get), state.recovery);
                assert_eq!(crate::CALLER.with(Cell::get), state.caller);
                assert_eq!(LAST_TRAP.with(Cell::get), state.last_trap);
            }
        }
        let first = &(*harness).actors[0];
        let peer = &(*harness).actors[1];
        assert_eq!(
            (*harness).peer_observed,
            if budgeted { QUANTUM } else { WORK }
        );
        assert_eq!(first.status, if trap { TrapKind::Unwrap as i64 } else { 0 });
        assert_eq!(first.drops, usize::from(!trap));
        assert_eq!(first.checks, first.progress);
        assert_eq!(peer.checks, peer.progress);
        assert_eq!(
            first.yields,
            if budgeted {
                first.progress / QUANTUM
            } else {
                0
            }
        );
        assert_eq!(peer.status, 0);
        assert_eq!(peer.drops, 1);
        if trap {
            assert_eq!(first.yields, 3);
        }
        if cancel_after.is_some() {
            assert!(first.progress < WORK && peer.progress < WORK);
        } else {
            assert_eq!(peer.progress, WORK);
            assert_eq!(peer.yields, if budgeted { WORK / QUANTUM } else { 0 });
        }
        println!(
            "budgeted={budgeted} trap={trap} cancel={cancel_after:?} peer_at={} turns={} yields={}/{} progress={}/{}",
            (*harness).peer_observed,
            (*harness).turns,
            first.yields,
            peer.yields,
            first.progress,
            peer.progress
        );
        for actor in &(*harness).actors {
            assert_eq!(discard(actor.heap), 0);
            assert_eq!(libc::munmap(actor.mapping, actor.mapping_bytes), 0);
        }
        assert_eq!(operate(LIVE_COUNT, 0), 0);
        assert_eq!(operate(USED_TOTAL, 0), 0);
        assert_eq!(operate(COMMITTED_TOTAL, 0), 0);
        HARNESS.with(|slot| slot.set(ptr::null_mut()));
        drop(Box::from_raw(harness));
    }
}

#[test]
#[ignore = "isolated experiment: owns the process-wide actor heap table"]
fn actor_stack_switching_probe() {
    run_case(false, false, None);
    run_case(true, false, None);
    run_case(true, true, None);
    run_case(true, false, Some(9));
}
