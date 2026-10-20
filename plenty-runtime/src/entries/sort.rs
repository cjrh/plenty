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
