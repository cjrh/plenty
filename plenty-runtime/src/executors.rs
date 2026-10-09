//! Bounded, joinable worker pools with fallible owned job submission.
//!
//! A pool owns pinned worker slots and a pointer ring in one allocation. Each
//! submission allocates one independently owned future/environment/result cell.
//! No user callback or destructor runs under a scheduler or future lock.
use crate::aggregates::{self, Type};
use crate::memory::{self, plenty_release, plenty_retain, AllocError, Header};
use crate::{ranges, threads};
use std::ffi::c_void;
use std::sync::{Condvar, Mutex, MutexGuard};

#[cfg(test)]
mod tests;

pub(crate) type Entry = unsafe extern "C" fn(*const u128, *mut u128);

#[repr(C)]
struct Worker {
    thread: usize,
    pool: *mut Pool,
}

struct Queue {
    head: usize,
    len: usize,
    closed: bool,
}

#[repr(C, align(16))]
pub(crate) struct Pool {
    header: Header,
    workers: *mut Worker,
    ring: *mut *mut Job,
    words: usize,
    count: usize,
    capacity: usize,
    queue: Mutex<Queue>,
    readable: Condvar,
    writable: Condvar,
    // Only the owning thread can call shutdown. Compiler worker eligibility
    // excludes pools/futures, including through captures and stored fields.
    joined: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Pending,
    Running,
    Ready,
    Cancelled,
    Taken,
}

#[repr(C, align(16))]
pub(crate) struct Job {
    header: Header,
    input: &'static Type,
    output: &'static Type,
    entry: Entry,
    data: *mut u128,
    words: usize,
    phase: Mutex<Phase>,
    completed: Condvar,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|_| crate::fail("poisoned executor mutex"))
}
fn wait<'a, T>(condition: &Condvar, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    condition
        .wait(guard)
        .unwrap_or_else(|_| crate::fail("poisoned executor mutex"))
}

/// Tags are PoolError.InvalidSize / Allocation(AllocError) / Thread(ThreadError).
pub(crate) fn create(count: usize, capacity: usize) -> Result<*mut Pool, u128> {
    if count == 0 || capacity == 0 {
        return Err(0);
    }
    let pointer = allocate_pool(count, capacity)
        .map_err(|e| error_payload(1, aggregates::wrap(0, e as u64)))?;
    unsafe {
        for index in 0..count {
            let worker = (*pointer).workers.add(index);
            worker.write(Worker {
                thread: 0,
                pool: pointer,
            });
            let status = threads::plenty_thread_start(worker.cast(), run_worker);
            if status != 0 {
                stop(pointer, index, false);
                plenty_release(pointer.cast());
                return Err(error_payload(2, aggregates::wrap(status as u128, 0)));
            }
        }
        Ok(pointer)
    }
}

fn allocate_pool(count: usize, capacity: usize) -> Result<*mut Pool, AllocError> {
    let bytes = count
        .checked_mul(size_of::<Worker>())
        .and_then(|n| {
            capacity
                .checked_mul(size_of::<*mut Job>())
                .and_then(|m| n.checked_add(m))
        })
        .and_then(|n| n.checked_add(15))
        .ok_or(AllocError::CapacityOverflow)?;
    let words = bytes / 16;
    let pointer = memory::try_allocate::<Pool, u128>(words)?;
    // SAFETY: all flexible-tail pointers derive from the allocation, never from
    // a shared Pool reference. Worker records stay pinned through every join.
    unsafe {
        let workers = pointer.add(1).cast::<Worker>();
        pointer.write(Pool {
            header: Header::new(destroy_pool),
            workers,
            ring: workers.add(count).cast(),
            words,
            count,
            capacity,
            queue: Mutex::new(Queue {
                head: 0,
                len: 0,
                closed: false,
            }),
            readable: Condvar::new(),
            writable: Condvar::new(),
            joined: false,
        });
        Ok(pointer)
    }
}

/// Encode a three/four-variant inline error (two tag bits).
fn error_payload(tag: u64, value: u128) -> u128 {
    aggregates::wrap(aggregates::wrap(value, tag >> 1), tag & 1)
}

unsafe extern "C" fn run_worker(argument: *mut c_void) -> *mut c_void {
    // SAFETY: the parent retains the pool and its worker records through join.
    // The queue lock hands each job's execution ownership to exactly one worker.
    unsafe {
        let pool = &*(*argument.cast::<Worker>()).pool;
        loop {
            let job = {
                let mut queue = lock(&pool.queue);
                while queue.len == 0 && !queue.closed {
                    queue = wait(&pool.readable, queue);
                }
                if queue.len == 0 {
                    break;
                }
                let job = pool.ring.add(queue.head).read();
                queue.head = (queue.head + 1) % pool.capacity;
                queue.len -= 1;
                job
            };
            pool.writable.notify_one();
            let run = {
                let mut phase = lock(&(*job).phase);
                if *phase == Phase::Pending {
                    *phase = Phase::Running;
                    true
                } else {
                    false
                }
            };
            if run {
                let data = (*job).data;
                // Separate output storage keeps input environments alive until
                // the adapter finishes, including reusable-closure cleanup.
                ((*job).entry)(data, data.add((*job).input.slot_words()));
                *lock(&(*job).phase) = Phase::Ready;
                (*job).completed.notify_all();
            }
            plenty_release(job.cast());
        }
    }
    std::ptr::null_mut()
}

/// Shut down idempotently, always joining. Cancellation only affects queued jobs.
pub(crate) unsafe fn shutdown(pointer: *mut Pool, cancel_pending: bool) {
    unsafe {
        stop(pointer, (*pointer).count, cancel_pending);
    }
}

unsafe fn stop(pointer: *mut Pool, started: usize, cancel_pending: bool) {
    unsafe {
        if (*pointer).joined {
            return;
        }
        let pool = &*pointer;
        {
            let mut queue = lock(&pool.queue);
            queue.closed = true;
            if cancel_pending {
                for i in 0..queue.len {
                    cancel(pool.ring.add((queue.head + i) % pool.capacity).read());
                }
            }
        }
        pool.readable.notify_all();
        pool.writable.notify_all();
        for index in 0..started {
            threads::plenty_thread_join(pool.workers.add(index).cast());
        }
        (*pointer).joined = true;
    }
}

unsafe extern "C" fn destroy_pool(header: *mut Header) {
    unsafe {
        let pointer = header.cast::<Pool>();
        shutdown(pointer, false);
        let words = (*pointer).words;
        std::ptr::drop_in_place(pointer);
        memory::free::<Pool, u128>(pointer, words);
    }
}

/// Failure codes match SubmitError: Full, Shutdown, OutOfMemory, CapacityOverflow.
/// On error the caller still owns value. On success its relocated copy belongs
/// to the worker and the returned cell owns the single public future handle.
pub(crate) unsafe fn submit(
    pointer: *mut Pool,
    value: u128,
    input: &'static Type,
    output: &'static Type,
    entry: Entry,
    nowait: bool,
) -> Result<*mut Job, u64> {
    unsafe {
        submit_initialized(pointer, input, output, entry, nowait, |slot| {
            ranges::store(slot, value, input);
        })
    }
}

/// Initialize a binary worker's inline consuming environment directly in its
/// job cell. On failure both values remain owned by the caller.
pub(crate) unsafe fn submit_pair(
    pointer: *mut Pool,
    values: [u128; 2],
    input: &'static Type,
    output: &'static Type,
    entry: Entry,
) -> Result<*mut Job, u64> {
    unsafe {
        submit_initialized(pointer, input, output, entry, false, |slot| {
            let environment = slot.add(1);
            environment.write(input as *const Type as u128);
            let mut capture = environment.add(1);
            for (value, ty) in values.into_iter().zip(input.variants[0].fields) {
                ranges::store(capture, value, ty);
                capture = capture.add(ty.slot_words());
            }
            slot.write(environment as u128);
        })
    }
}

/// Initialization is infallible, transfers ownership only after allocation,
/// and must neither call user code nor lock another runtime object.
unsafe fn submit_initialized(
    pointer: *mut Pool,
    input: &'static Type,
    output: &'static Type,
    entry: Entry,
    nowait: bool,
    initialize: impl FnOnce(*mut u128),
) -> Result<*mut Job, u64> {
    unsafe {
        let pool = &*pointer;
        let mut queue = lock(&pool.queue);
        while queue.len == pool.capacity && !queue.closed {
            if nowait {
                return Err(0);
            }
            queue = wait(&pool.writable, queue);
        }
        if queue.closed {
            return Err(1);
        }
        let words = input
            .slot_words()
            .checked_add(output.slot_words())
            .ok_or(3u64)?;
        let job = memory::try_allocate::<Job, u128>(words).map_err(|e| 2 + e as u64)?;
        job.write(Job {
            header: Header::new(destroy_job),
            input,
            output,
            entry,
            data: job.add(1).cast(),
            words,
            phase: Mutex::new(Phase::Pending),
            completed: Condvar::new(),
        });
        initialize((*job).data);
        plenty_retain(job.cast()); // worker/queue + public future
        pool.ring
            .add((queue.head + queue.len) % pool.capacity)
            .write(job);
        queue.len += 1;
        drop(queue);
        pool.readable.notify_one();
        Ok(job)
    }
}

pub(crate) unsafe fn cancel(job: *mut Job) -> bool {
    unsafe {
        let mut phase = lock(&(*job).phase);
        match *phase {
            Phase::Pending => {
                *phase = Phase::Cancelled;
                (*job).completed.notify_all();
                true
            }
            Phase::Cancelled => true,
            _ => false,
        }
    }
}

pub(crate) unsafe fn done(job: *mut Job) -> bool {
    unsafe {
        matches!(
            *lock(&(*job).phase),
            Phase::Ready | Phase::Cancelled | Phase::Taken
        )
    }
}

pub(crate) unsafe fn map_window(pool: *mut Pool) -> Result<usize, ()> {
    unsafe {
        if lock(&(*pool).queue).closed {
            Err(())
        } else {
            Ok((*pool).count + (*pool).capacity)
        }
    }
}

/// The returned payload stays pinned until the caller releases its job count.
pub(crate) unsafe fn take_result(job: *mut Job) -> Option<u128> {
    unsafe {
        let mut phase = lock(&(*job).phase);
        while matches!(*phase, Phase::Pending | Phase::Running) {
            phase = wait(&(*job).completed, phase);
        }
        match *phase {
            Phase::Cancelled => None,
            Phase::Ready => {
                *phase = Phase::Taken;
                Some((*job).data.add((*job).input.slot_words()).read())
            }
            _ => crate::fail("executor result consumed twice"),
        }
    }
}

/// Transfer the result into caller-owned storage. The affine public handle is
/// consumed by the generated caller after this call. No allocation is required.
pub(crate) unsafe fn result(job: *mut Job, out: *mut u128) {
    unsafe {
        match take_result(job) {
            None => out.write(aggregates::wrap(0, 1)),
            Some(value) => {
                let value = ranges::copy_payload(value, (*job).output, out.add(1).cast());
                out.write(aggregates::wrap(value, 0));
            }
        }
    }
}

unsafe extern "C" fn destroy_job(header: *mut Header) {
    unsafe {
        let job = header.cast::<Job>();
        let phase = *lock(&(*job).phase);
        match phase {
            Phase::Cancelled => aggregates::release((*job).data.read(), (*job).input),
            Phase::Ready => aggregates::release(
                (*job).data.add((*job).input.slot_words()).read(),
                (*job).output,
            ),
            Phase::Taken => {}
            _ => crate::fail("unfinished executor job lost its worker owner"),
        }
        let words = (*job).words;
        std::ptr::drop_in_place(job);
        memory::free::<Job, u128>(job, words);
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_executor(
    op: i64,
    args: *const u128,
    descriptor: *const Type,
    out: *mut u128,
) {
    // SAFETY: checked operations supply exact arity and full inline output
    // storage. The callback is compiler emitted, never a user function pointer.
    unsafe {
        match op {
            0 => out.write(match create(*args as usize, *args.add(1) as usize) {
                Ok(pool) => aggregates::wrap(pool as u128, 0),
                Err(error) => aggregates::wrap(error, 1),
            }),
            1 | 2 => {
                let ty = &*descriptor;
                let input = ty.variants[1].fields[0].variants[0].fields[0];
                let output = ty.variants[0].fields[0].key.unwrap();
                let entry: Entry = std::mem::transmute(*args.add(2) as usize);
                let value = match submit(
                    *args as *mut Pool,
                    *args.add(1),
                    input,
                    output,
                    entry,
                    op == 2,
                ) {
                    Ok(job) => aggregates::wrap(job as u128, 0),
                    Err(tag) => aggregates::wrap(error_payload(tag, *args.add(1)), 1),
                };
                out.write(ranges::copy_payload(value, ty, out.add(1).cast()));
            }
            3 => result(*args as *mut Job, out),
            4 => out.write(done(*args as *mut Job) as u128),
            5 => out.write(cancel(*args as *mut Job) as u128),
            6 => {
                shutdown(*args as *mut Pool, *args.add(1) != 0);
                out.write(0);
            }
            7 | 8 => {
                let entry: Entry = std::mem::transmute(*args.add(2) as usize);
                let input = &*(*args.add(3) as *const Type);
                let worker_output = &*(*args.add(4) as *const Type);
                aggregates::executor_map::run(
                    *args as *mut Pool,
                    *args.add(1),
                    input,
                    entry,
                    worker_output,
                    &*descriptor,
                    out,
                );
            }
            9 => {
                let entry: Entry = std::mem::transmute(*args.add(2) as usize);
                let input = &*(*args.add(3) as *const Type);
                let job_input = &*(*args.add(4) as *const Type);
                aggregates::executor_reduce::run(
                    *args as *mut Pool,
                    *args.add(1),
                    input,
                    entry,
                    job_input,
                    &*descriptor,
                    out,
                );
            }
            _ => crate::fail("invalid executor operation"),
        }
    }
}
