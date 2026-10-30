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

/// One job's share of the tree: an aligned run of `count` adjacent slots,
/// reduced serially with the same pairing the level-by-level loop uses.
struct Block {
    first: *mut u128,
    count: usize,
    element: &'static Type,
    pair: &'static Type,
    entry: executors::Entry,
    /// Room for one pair environment and its result.
    scratch: *mut u128,
}

/// A block job's input is the address of its `Block`, which owns nothing.
static BLOCK: Type = Type {
    kind: b'4',
    affine: false,
    reflexive: true,
    inline_range: false,
    inline_bytes: 0,
    key: None,
    value: None,
    name: "reduce block",
    variants: &[],
    drop: None,
    wrapper: None,
    plan: None,
    redacted: false,
};

/// The width of the aligned blocks reduced inside one job: the smallest power
/// of two that leaves at most one block per worker. Block `j` covers leaves
/// `j * width..` and, because every level maps index `i` to `i / 2` (paired
/// or carried), it is exactly the subtree that the level-by-level tree reduces
/// to index `j` after `log2(width)` levels, including a short final block.
fn block_width(count: usize, workers: usize) -> usize {
    let mut width = 1usize;
    while count.div_ceil(width) > workers.max(1) {
        width *= 2;
    }
    width
}

unsafe extern "C" fn reduce_block(input: *const u128, out: *mut u128, _shared: *const u128) {
    // SAFETY: `input` holds the address of a `Block` the caller keeps alive and
    // untouched until this job's result is taken. The block's slots and scratch
    // belong to this job alone; each value moves once and its slot is zeroed.
    unsafe {
        let block = &*(input.read() as usize as *const Block);
        let words = block.element.slot_words();
        let slot = |index: usize| block.first.add(index * words);
        let environment = block.scratch.add(1);
        let result = block.scratch.add(block.pair.slot_words());
        let mut count = block.count;
        while count > 1 {
            let pairs = count / 2;
            for index in 0..pairs {
                // The same inline environment `submit_pair` builds.
                environment.write(block.pair as *const Type as u128);
                let mut capture = environment.add(1);
                for (source, ty) in [slot(2 * index), slot(2 * index + 1)]
                    .into_iter()
                    .zip(block.pair.variants[0].fields)
                {
                    ranges::store(capture, source.read(), ty);
                    capture = capture.add(ty.slot_words());
                    source.write(0);
                }
                block.scratch.write(environment as u128);
                (block.entry)(block.scratch, result, std::ptr::null());
                ranges::store(slot(index), result.read(), block.element);
            }
            if count % 2 == 1 {
                let odd = slot(count - 1);
                ranges::store(slot(pairs), odd.read(), block.element);
                odd.write(0);
            }
            count = count.div_ceil(2);
        }
        ranges::store(out, slot(0).read(), block.element);
        slot(0).write(0);
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
                    let entries = &mut (*(source as *mut Collection)).entries;
                    ranges::store(destination, entries.get(index).key, element);
                    entries.forget_key(index);
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
            let width = block_width(count, executors::workers(pool));
            let blocks = count.div_ceil(width);
            // Declared before `pending`, so they outlive its drain on every exit.
            let mut descriptors = Vec::new();
            let mut scratch = Vec::new();
            if width > 1 {
                let words = job_input.slot_words() + element.slot_words();
                memory::try_reserve(&mut descriptors, blocks)?;
                let total = blocks
                    .checked_mul(words)
                    .ok_or(AllocError::CapacityOverflow)?;
                memory::try_reserve(&mut scratch, total)?;
                scratch.resize(total, 0u128);
                for block in 0..blocks {
                    descriptors.push(Block {
                        first: values.slot(block * width),
                        count: width.min(count - block * width),
                        element,
                        pair: job_input,
                        entry,
                        scratch: scratch.as_mut_ptr().add(block * words),
                    });
                }
            }
            let window = window.min(count / 2);
            let mut pending = Pending::new(window, element)?;
            for index in 0..count {
                take_input(index, values.slot(index));
            }
            if width > 1 {
                let mut submitted = 0;
                let mut finished = 0;
                while submitted < blocks || pending.len > 0 {
                    while submitted < blocks && pending.len < window {
                        let block = &descriptors[submitted] as *const Block;
                        let job = executors::submit(
                            pool,
                            block as usize as u128,
                            &BLOCK,
                            element,
                            reduce_block,
                            false,
                        )
                        .map_err(Error::submission)?;
                        pending.push(job);
                        submitted += 1;
                    }
                    let mut completed = pending.pop();
                    // Slot `finished` lies in an earlier, completed block, and
                    // later blocks only touch slots at or beyond their start.
                    ranges::store(values.slot(finished), completed.value, element);
                    completed.transferred();
                    finished += 1;
                }
                count = blocks;
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
