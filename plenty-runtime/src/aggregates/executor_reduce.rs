//! Fixed adjacent-pair reduction. Tree shape never depends on scheduling.
use super::*;
use crate::executors::{self, Pool};
use executor_map::{Error, Pending};

/// Stable, type-sized scratch slots. A zero tag marks relinquished ownership;
/// descriptors already support releasing such moved-from slots.
struct Values {
    slots: Vec<u128>,
    ty: &'static Type,
}
impl Values {
    fn new(count: usize, ty: &'static Type) -> Result<Self, AllocError> {
        let words = count
            .checked_mul(ty.slot_words())
            .ok_or(AllocError::CapacityOverflow)?;
        let mut slots = Vec::new();
        memory::try_reserve(&mut slots, words)?;
        slots.resize(words, 0);
        Ok(Self { slots, ty })
    }
    fn slot(&mut self, index: usize) -> *mut u128 {
        // SAFETY: callers address only the original reserved input count.
        unsafe { self.slots.as_mut_ptr().add(index * self.ty.slot_words()) }
    }
}
impl Drop for Values {
    fn drop(&mut self) {
        for slot in self.slots.chunks_exact(self.ty.slot_words()) {
            unsafe { release(slot[0], self.ty) };
        }
    }
}

pub(crate) unsafe fn run(
    pool: *mut Pool,
    source: u128,
    input: &'static Type,
    entry: executors::Entry,
    job_input: &'static Type,
    result_type: &'static Type,
    out: *mut u128,
) {
    // SAFETY: the checked adapter consumes two values of the element type and
    // returns one of that same type. Its input is a two-capture inline closure.
    // All ownership moves complete before a job or result is published.
    unsafe {
        let element = input.key.unwrap();
        let _source = OwnedValue {
            value: source,
            ty: input,
        };
        let build = || -> Result<(), Error> {
            let window = executors::map_window(pool).map_err(|_| Error::Shutdown)?;
            let mut count = if input.kind == b'R' {
                (*(source as *const Range)).len as usize
            } else {
                (*(source as *const Collection)).len()
            };
            let take_input = |index: usize, destination| {
                if input.kind == b'R' {
                    let value = (*(source as *const Range)).at(index, element.kind <= b'4');
                    ranges::store(destination, value, element);
                } else {
                    let slot = (*(source as *mut Collection)).entries.slot(index, false);
                    ranges::store(destination, slot.read(), element);
                    slot.write(0);
                }
            };
            // Empty and singleton inputs need no scheduler or scratch allocation.
            if count == 0 {
                out.write(wrap(0, 0)); // Ok(Nothing)
                return Ok(());
            }
            if count == 1 {
                take_input(0, out);
                out.write(wrap(wrap(out.read(), 1), 0)); // Ok(Some(value))
                return Ok(());
            }
            let mut values = Values::new(count, element)?;
            let window = window.min(count / 2);
            let mut pending = Pending::new(window, element)?;
            for index in 0..count {
                take_input(index, values.slot(index));
            }
            while count > 1 {
                let pairs = count / 2;
                let mut submitted = 0;
                let mut finished = 0;
                while submitted < pairs || pending.len > 0 {
                    while submitted < pairs && pending.len < window {
                        let left = values.slot(2 * submitted);
                        let right = values.slot(2 * submitted + 1);
                        let job = executors::submit_pair(
                            pool,
                            [left.read(), right.read()],
                            job_input,
                            element,
                            entry,
                        )
                        .map_err(Error::submission)?;
                        left.write(0);
                        right.write(0);
                        pending.push(job);
                        submitted += 1;
                    }
                    let mut completed = pending.pop();
                    // Both original slots for this destination are already
                    // transferred; compact without a second scratch allocation.
                    ranges::store(values.slot(finished), completed.value, element);
                    completed.transferred();
                    finished += 1;
                }
                if count % 2 == 1 {
                    let odd = values.slot(count - 1);
                    ranges::store(values.slot(pairs), odd.read(), element);
                    odd.write(0);
                }
                count = count.div_ceil(2);
            }
            let slot = values.slot(0);
            ranges::store(out, wrap(wrap(slot.read(), 1), 0), result_type);
            slot.write(0);
            Ok(())
        };
        if let Err(error) = build() {
            let error = match error {
                Error::Allocation(error) => wrap(wrap(0, error as u64), 0),
                Error::Shutdown => wrap(0, 1),
                Error::Worker => unreachable!("reduce_tree does not unwrap application results"),
            };
            out.write(wrap(error, 1));
        }
    }
}
