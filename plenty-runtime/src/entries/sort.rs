//! Dynamic-width rows cannot use Rust's slice sort: moving an inline payload
//! must repair its embedded addresses before the next comparison.
use super::*;

impl Entries {
    fn median_row(
        &self,
        a: usize,
        b: usize,
        c: usize,
        compare: &mut impl FnMut(u128, u128) -> Ordering,
    ) -> usize {
        let mut indices = [a, b, c];
        for (a, b) in [(0, 1), (1, 2), (0, 1)] {
            if compare(self.key_at(indices[a]), self.key_at(indices[b])).is_gt() {
                indices.swap(a, b);
            }
        }
        indices[1]
    }

    pub(super) fn sort_rows(
        &mut self,
        mut start: usize,
        mut end: usize,
        mut depth: u32,
        relocation: Relocation,
        compare: &mut impl FnMut(u128, u128) -> Ordering,
    ) {
        while end - start > 16 {
            if depth == 0 {
                self.heap_sort_rows(start, end - start, relocation, compare);
                return;
            }
            depth -= 1;
            let pivot = end - 1;
            let middle = start + (end - start) / 2;
            // A median of three alone repeatedly selects extreme values in
            // organ-pipe and almost-ordered partitions. Sample across large
            // partitions before reducing the nine candidates to three medians.
            let selected = if end - start >= 64 {
                let step = (end - start) / 8;
                let a = self.median_row(start, start + step, start + 2 * step, compare);
                let b = self.median_row(middle - step, middle, middle + step, compare);
                let c = self.median_row(pivot - 2 * step, pivot - step, pivot, compare);
                self.median_row(a, b, c, compare)
            } else {
                self.median_row(start, middle, pivot, compare)
            };
            self.exchange(selected, pivot, relocation);
            let (mut less, mut scan, mut greater) = (start, start, pivot);
            // The pivot stays in its row while partitioning. Never cache its
            // address across a swap: an inline value's address follows its row.
            while scan < greater {
                match compare(self.key_at(scan), self.key_at(pivot)) {
                    Ordering::Less => {
                        self.exchange(scan, less, relocation);
                        less += 1;
                        scan += 1;
                    }
                    Ordering::Equal => scan += 1,
                    Ordering::Greater => {
                        greater -= 1;
                        self.exchange(scan, greater, relocation);
                    }
                }
            }
            self.exchange(greater, pivot, relocation);
            // Recurse only into the smaller partition. Stack use is logarithmic
            // even with an inconsistent comparator; the depth budget also bounds
            // total comparison work by falling back to heapsort.
            if less - start < end - greater - 1 {
                self.sort_rows(start, less, depth, relocation, compare);
                start = greater + 1;
            } else {
                self.sort_rows(greater + 1, end, depth, relocation, compare);
                end = less;
            }
        }
        for i in start + usize::from(start < end)..end {
            let mut j = i;
            while j > start && compare(self.key_at(j), self.key_at(j - 1)).is_lt() {
                self.exchange(j, j - 1, relocation);
                j -= 1;
            }
        }
    }
}

/// Runs this short are sorted by stable insertion before merging.
const STABLE_RUN: usize = 16;

impl Entries {
    /// Sort dense rows stably: equal rows keep their relative order. Workspace
    /// for `floor(len / 2)` rows is reserved before any row moves, so failure
    /// leaves the list unchanged. Already ordered input reserves nothing.
    ///
    /// An inconsistent `compare` gives an unspecified order but every row
    /// remains exactly once: each merge step advances one side, and the
    /// scratch rows always return to the list before this returns.
    pub fn sort_stable_rows_by(
        &mut self,
        mut compare: impl FnMut(u128, u128) -> Ordering,
    ) -> Result<(), memory::AllocError> {
        assert!(self.order.is_none(), "only dense lists sort");
        let len = self.len();
        if (1..len).all(|i| !compare(self.key_at(i - 1), self.key_at(i)).is_gt()) {
            return Ok(());
        }
        let bytes = (len / 2)
            .checked_mul(self.stride)
            .ok_or(memory::AllocError::CapacityOverflow)?;
        // Aligned words, as list storage uses, hold the rows for direct reads.
        let mut scratch = Vec::<u128>::new();
        memory::try_reserve(&mut scratch, bytes.div_ceil(16))?;
        let relocation = Relocation::of(self.key);
        self.merge_sort_rows(0, len, &mut scratch, relocation, &mut compare);
        Ok(())
    }

    fn merge_sort_rows(
        &mut self,
        start: usize,
        end: usize,
        scratch: &mut Vec<u128>,
        relocation: Relocation,
        compare: &mut impl FnMut(u128, u128) -> Ordering,
    ) {
        if end - start <= STABLE_RUN {
            // Only a strictly smaller row moves left, so equal rows keep order.
            for i in start + 1..end {
                let mut j = i;
                while j > start && compare(self.key_at(j), self.key_at(j - 1)).is_lt() {
                    self.exchange(j, j - 1, relocation);
                    j -= 1;
                }
            }
            return;
        }
        let middle = start + (end - start) / 2;
        self.merge_sort_rows(start, middle, scratch, relocation, compare);
        self.merge_sort_rows(middle, end, scratch, relocation, compare);
        // Adjacent runs already in order need no merge: sorted input costs
        // one comparison per merge instead of a pass through scratch.
        if compare(self.key_at(middle - 1), self.key_at(middle)).is_gt() {
            self.merge_rows(start, middle, end, scratch, relocation, compare);
        }
    }

    /// Merge `start..middle` and `middle..end`. The left run moves to scratch,
    /// where each row's inline address is repaired so comparisons observe it as
    /// they would the list row it came from; output fills from `start` and
    /// never overtakes the unread right run.
    fn merge_rows(
        &mut self,
        start: usize,
        middle: usize,
        end: usize,
        scratch: &mut Vec<u128>,
        relocation: Relocation,
        compare: &mut impl FnMut(u128, u128) -> Ordering,
    ) {
        let stride = self.stride;
        let left = middle - start;
        // `left <= len / 2` rows, which the caller reserved.
        debug_assert!(left * stride <= scratch.capacity() * 16);
        let base = self.data.as_mut_ptr();
        let packed = self.packed();
        let ty = self.key;
        let mut hole = Hole {
            saved: scratch.as_mut_ptr().cast(),
            base,
            stride,
            ty,
            relocation,
            next: 0,
            left,
            target: start,
        };
        // SAFETY: rows `start..end` are live rows of `ty` in this exclusively
        // held list, and scratch has room for `left` rows. A moved row's bytes
        // are copied once and its inline address repaired at the destination;
        // no owner is retained or released. List rows `target..j` are the
        // hole whose owners are scratch rows `next..left`; `Hole` fills it
        // when the merge finishes or `compare` unwinds.
        unsafe {
            std::ptr::copy_nonoverlapping(base.add(start * stride), hole.saved, left * stride);
            for i in 0..left {
                relocation.repair(hole.saved.add(i * stride).cast(), ty);
            }
            let key = |row: *const u8| read_row(row, ty, None, packed).key;
            let mut j = middle;
            while hole.next < left && j < end {
                let right = base.add(j * stride);
                let held = hole.saved.add(hole.next * stride);
                // Take from the right only when strictly smaller: stability.
                let source = if compare(key(right), key(held)).is_lt() {
                    j += 1;
                    right
                } else {
                    hole.next += 1;
                    held
                };
                hole.place(source, hole.target, 1);
                hole.target += 1;
            }
        }
        // Dropping `hole` moves the remaining left rows; any remaining right
        // rows are already in place.
    }
}

/// The merge's unfilled list rows and the scratch rows that own them.
struct Hole {
    saved: *mut u8,
    base: *mut u8,
    stride: usize,
    ty: &'static Type,
    relocation: Relocation,
    next: usize,
    left: usize,
    target: usize,
}

impl Hole {
    /// Copy `count` rows from `source` to list row `target` and repair them.
    ///
    /// # Safety
    /// `source` holds `count` live rows and list rows from `target` are free.
    unsafe fn place(&self, source: *const u8, target: usize, count: usize) {
        unsafe {
            let destination = self.base.add(target * self.stride);
            std::ptr::copy_nonoverlapping(source, destination, count * self.stride);
            for row in 0..count {
                self.relocation
                    .repair(destination.add(row * self.stride).cast(), self.ty);
            }
        }
    }
}

impl Drop for Hole {
    fn drop(&mut self) {
        // SAFETY: the hole spans exactly `left - next` free list rows from
        // `target`, owned by the scratch rows from `next`.
        unsafe {
            self.place(
                self.saved.add(self.next * self.stride),
                self.target,
                self.left - self.next,
            );
        }
    }
}
