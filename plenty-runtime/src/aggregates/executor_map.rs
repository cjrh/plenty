//! Ordered parallel traversal with a bounded window of outstanding jobs.
use super::*;
use crate::executors::{self, Job, Pool};

pub(super) enum Error {
    Allocation(AllocError),
    Shutdown,
    Worker,
}
impl From<AllocError> for Error {
    fn from(error: AllocError) -> Self {
        Self::Allocation(error)
    }
}
impl Error {
    pub(super) fn submission(tag: u64) -> Self {
        match tag {
            1 => Self::Shutdown,
            2 => Self::Allocation(AllocError::OutOfMemory),
            3 => Self::Allocation(AllocError::CapacityOverflow),
            _ => unreachable!("blocking parallel submission cannot report full"),
        }
    }
}

/// A result stays in its job's inline storage until relocated to its new owner.
pub(super) struct Completed {
    job: *mut Job,
    pub(super) value: u128,
    output: &'static Type,
    moved: bool,
}
impl Completed {
    pub(super) fn transferred(&mut self) {
        self.moved = true;
    }
}
impl Drop for Completed {
    fn drop(&mut self) {
        unsafe {
            if !self.moved {
                release(self.value, self.output);
            }
            plenty_release(self.job.cast());
        }
    }
}

pub(super) struct Pending {
    jobs: Vec<*mut Job>,
    head: usize,
    pub(super) len: usize,
    output: &'static Type,
}
impl Pending {
    pub(super) fn new(window: usize, output: &'static Type) -> Result<Self, AllocError> {
        let mut jobs = Vec::new();
        memory::try_reserve(&mut jobs, window)?;
        jobs.resize(window, std::ptr::null_mut());
        Ok(Self {
            jobs,
            head: 0,
            len: 0,
            output,
        })
    }
    pub(super) fn push(&mut self, job: *mut Job) {
        assert!(self.len < self.jobs.len());
        let tail = (self.head + self.len) % self.jobs.len();
        self.jobs[tail] = job;
        self.len += 1;
    }
    pub(super) unsafe fn pop(&mut self) -> Completed {
        assert!(self.len > 0);
        let job = self.jobs[self.head];
        self.head = (self.head + 1) % self.jobs.len();
        self.len -= 1;
        Completed {
            job,
            value: unsafe { executors::take_result(job) }
                .expect("private parallel job cannot be cancelled"),
            output: self.output,
            moved: false,
        }
    }
}
impl Drop for Pending {
    fn drop(&mut self) {
        // Drain on every exit. Destruction never holds a scheduler lock.
        while self.len > 0 {
            unsafe { drop(self.pop()) };
        }
    }
}

pub(crate) unsafe fn run(
    pool: *mut Pool,
    source: u128,
    input: &'static Type,
    entry: executors::Entry,
    worker_output: &'static Type,
    result_type: &'static Type,
    out: *mut u128,
) {
    // SAFETY: input is an owned list/range; result_type describes the complete
    // caller-owned result slot. Workers cannot access these private futures.
    unsafe {
        let output = result_type.variants[0].fields[0];
        let error_type = result_type.variants[1].fields[0];
        let fallible = error_type.variants.len() == 3;
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
            let window = window.min(count);
            let mut pending = Pending::new(window, worker_output)?;
            let mut next = 0;
            let mut failure = None;
            while next < count || pending.len > 0 {
                while failure.is_none() && next < count && pending.len < window {
                    let value = if input.kind == b'R' {
                        (*(source as *const Range)).at(next, input.key().kind <= b'4')
                    } else {
                        (*(source as *const Collection)).entries.get(next).key
                    };
                    let job = match executors::submit(
                        pool,
                        value,
                        input.key.unwrap(),
                        worker_output,
                        entry,
                        false,
                    ) {
                        Ok(job) => job,
                        Err(tag) => {
                            failure = Some(Error::submission(tag));
                            break;
                        }
                    };
                    if input.kind == b'L' {
                        // submit relocated the owner before publication. Clear
                        // the old tag without dereferencing its moved payload.
                        (*(source as *mut Collection))
                            .entries
                            .slot(next, false)
                            .write(0);
                    }
                    pending.push(job);
                    next += 1;
                }
                if pending.len == 0 {
                    break;
                }
                let mut completed = pending.pop();
                if fallible && worker_output.tag(completed.value) == 1 {
                    // Input-order observation selects the lowest failed index.
                    // It also takes precedence over a later submission failure.
                    let error = error_type.pack(worker_output.unpack(completed.value), 2);
                    ranges::store(out, wrap(error, 1), result_type);
                    completed.transferred();
                    return Err(Error::Worker);
                }
                let value = if fallible {
                    worker_output.unpack(completed.value)
                } else {
                    completed.value
                };
                (*result).entries.push(Entry {
                    key: value,
                    value: 0,
                });
                completed.transferred();
            }
            if let Some(error) = failure {
                return Err(error);
            }
            Ok(result_guard.into_value())
        };
        match build() {
            Ok(values) => out.write(wrap(values, 0)),
            Err(Error::Allocation(error)) => {
                out.write(wrap(error_type.pack(wrap(0, error as u64), 0), 1))
            }
            Err(Error::Shutdown) => out.write(wrap(error_type.pack(0, 1), 1)),
            Err(Error::Worker) => {} // Already moved into caller-owned storage.
        }
    }
}
