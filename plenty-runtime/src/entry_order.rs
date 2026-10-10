//! Stable hash-entry slots, insertion order, and an allocation-free free list.
use crate::memory::{self, AllocError};

const END: usize = usize::MAX;
const DEAD: usize = usize::MAX - 1;

#[derive(Clone, Copy)]
struct Link {
    previous: usize,
    next: usize,
}

pub(crate) struct Order {
    links: Vec<Link>,
    first: usize,
    last: usize,
    free: usize,
    len: usize,
}

impl Order {
    pub fn new() -> Self {
        Self {
            links: Vec::new(),
            first: END,
            last: END,
            free: END,
            len: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn capacity(&self) -> usize {
        self.links.capacity()
    }
    pub fn contains(&self, index: usize) -> bool {
        self.links
            .get(index)
            .is_some_and(|link| link.previous != DEAD)
    }
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), AllocError> {
        let needed = self
            .len
            .checked_add(additional)
            .ok_or(AllocError::CapacityOverflow)?;
        let additional = needed.saturating_sub(self.links.len());
        memory::try_reserve(&mut self.links, additional)
    }
    pub fn vacant(&self) -> usize {
        if self.free == END {
            self.links.len()
        } else {
            self.free
        }
    }
    pub fn push(&mut self) {
        let index = self.vacant();
        let link = Link {
            previous: self.last,
            next: END,
        };
        if index == self.links.len() {
            assert!(index < self.links.capacity());
            self.links.push(link);
        } else {
            self.free = self.links[index].next;
            self.links[index] = link;
        }
        if self.last == END {
            self.first = index;
        } else {
            self.links[self.last].next = index;
        }
        self.last = index;
        self.len += 1;
    }
    pub fn remove(&mut self, index: usize) {
        let Link { previous, next } = self.links[index];
        assert_ne!(previous, DEAD);
        if previous == END {
            self.first = next;
        } else {
            self.links[previous].next = next;
        }
        if next == END {
            self.last = previous;
        } else {
            self.links[next].previous = previous;
        }
        self.links[index] = Link {
            previous: DEAD,
            next: self.free,
        };
        self.free = index;
        self.len -= 1;
    }
    pub fn first(&self) -> Option<usize> {
        (self.first != END).then_some(self.first)
    }
    pub fn next(&self, index: usize) -> Option<usize> {
        let next = self.links[index].next;
        (next != END).then_some(next)
    }
    pub fn indices(
        &self,
    ) -> impl DoubleEndedIterator<Item = usize> + ExactSizeIterator + Clone + '_ {
        Indices {
            links: &self.links,
            first: self.first,
            last: self.last,
            remaining: self.len,
        }
    }
    pub fn clear(&mut self) {
        self.links.clear();
        self.first = END;
        self.last = END;
        self.free = END;
        self.len = 0;
    }
    /// Clear logical membership while retaining the links for a borrowed drain.
    pub fn drain(&mut self) -> impl Iterator<Item = usize> + '_ {
        let links = self.links.as_ptr();
        let mut next = self.first;
        let len = self.len;
        self.clear();
        (0..len).map(move |_| {
            let index = next;
            // SAFETY: clear preserves initialized bytes and capacity; the
            // iterator's exclusive borrow prevents reuse until it is dropped.
            next = unsafe { (*links.add(index)).next };
            index
        })
    }
}

#[derive(Clone)]
struct Indices<'a> {
    links: &'a [Link],
    first: usize,
    last: usize,
    remaining: usize,
}
impl Iterator for Indices<'_> {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        if self.remaining == 0 {
            return None;
        }
        let index = self.first;
        self.first = self.links[index].next;
        self.remaining -= 1;
        Some(index)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}
impl DoubleEndedIterator for Indices<'_> {
    fn next_back(&mut self) -> Option<usize> {
        if self.remaining == 0 {
            return None;
        }
        let index = self.last;
        self.last = self.links[index].previous;
        self.remaining -= 1;
        Some(index)
    }
}
impl ExactSizeIterator for Indices<'_> {}
