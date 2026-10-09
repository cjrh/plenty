//! Eager ordered map with a bounded window of outstanding jobs. Scheduling and
//! traversal live here; generated adapters only call the selected source function.
use super::*;
use crate::executors::{self, Job, Pool};

enum Error {
    Allocation(AllocError),
    Shutdown,
}
impl From<AllocError> for Error {
    fn from(error: AllocError) -> Self {
        Self::Allocation(error)
    }
}

struct Pending {
    jobs: Vec<*mut Job>,
    head: usize,
    len: usize,
    output: &'static Type,
}
impl Drop for Pending {
    fn drop(&mut self) {
        // An infrastructure failure drains already accepted jobs. Releasing
        // partial results does not roll back their user-visible effects.
        unsafe {
            for i in 0..self.len {
                let job = self.jobs[(self.head + i) % self.jobs.len()];
                if let Some(value) = executors::take_result(job) {
                    release(value, self.output);
                }
                plenty_release(job.cast());
            }
        }
    }
}

pub(crate) unsafe fn run(
    pool: *mut Pool,
    source: u128,
    input: &'static Type,
    entry: executors::Entry,
    output: &'static Type,
) -> u128 {
    // SAFETY: source is an owned list or an inline integer range; output is a
    // list of the adapter's concrete result type. No public future can observe
    // or cancel the private window. All allocations precede the corresponding
    // ownership transfer, and the source guard drops its unsubmitted suffix.
    unsafe {
        let _source = OwnedValue {
            value: source,
            ty: input,
        };
        let build = || -> Result<u128, Error> {
            let window = executors::map_window(pool).map_err(|_| Error::Shutdown)?;
            let count = if input.kind == b'R' {
                (*(source as *const Range)).len as usize
            } else {
                (*(source as *const Collection)).len()
            };
            let result = try_collection_new(output, count)?;
            let result_guard = OwnedValue {
                value: result as u128,
                ty: output,
            };
            let mut pending = Pending {
                jobs: Vec::new(),
                head: 0,
                len: 0,
                output: output.key(),
            };
            let window = window.min(count);
            memory::try_reserve(&mut pending.jobs, window)?;
            pending.jobs.resize(window, std::ptr::null_mut());
            let mut next = 0;
            while next < count || pending.len > 0 {
                while next < count && pending.len < window {
                    let value = if input.kind == b'R' {
                        (*(source as *const Range)).at(next, input.key().kind <= b'4')
                    } else {
                        (*(source as *const Collection)).entries.get(next).key
                    };
                    let job = executors::submit(
                        pool,
                        value,
                        input.key.unwrap(),
                        output.key.unwrap(),
                        entry,
                        false,
                    )
                    .map_err(|tag| match tag {
                        1 => Error::Shutdown,
                        2 => Error::Allocation(AllocError::OutOfMemory),
                        3 => Error::Allocation(AllocError::CapacityOverflow),
                        _ => unreachable!("blocking map submission cannot report full"),
                    })?;
                    if input.kind == b'L' {
                        // Relocation completed in submit; clear only the owner
                        // tag, without dereferencing a moved inline environment.
                        (*(source as *mut Collection))
                            .entries
                            .slot(next, false)
                            .write(0);
                    }
                    pending.jobs[(pending.head + pending.len) % window] = job;
                    pending.len += 1;
                    next += 1;
                }
                let job = pending.jobs[pending.head];
                let value =
                    executors::take_result(job).expect("private map job cannot be cancelled");
                // The complete result capacity was reserved before any worker
                // ran. Entries relocates owner-local payloads before cell free.
                (*result).entries.push(Entry {
                    key: value,
                    value: 0,
                });
                plenty_release(job.cast());
                pending.head = (pending.head + 1) % window;
                pending.len -= 1;
            }
            Ok(result_guard.into_value())
        };
        match build() {
            Ok(values) => wrap(values, 0),
            Err(Error::Allocation(error)) => wrap(wrap(wrap(0, error as u64), 0), 1),
            Err(Error::Shutdown) => wrap(wrap(0, 1), 1),
        }
    }
}
