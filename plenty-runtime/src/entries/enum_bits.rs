//! Canonical read-only slots for payload-free enum set members. Membership
//! lives in the owner; these slots give borrowed iteration stable `&Enum`s.
use crate::aggregates::Type;

static VALUES: [u128; 64] = {
    let mut values = [0; 64];
    let mut tag = 0;
    while tag < 64 {
        values[tag] = (tag as u128) << 64;
        tag += 1;
    }
    values
};

pub(crate) fn eligible(key: &Type) -> bool {
    key.kind == b'B'
        && !key.affine
        && key.wrapper.is_none()
        && (1..=64).contains(&key.variants.len())
        && key.variants.iter().all(|variant| variant.fields.is_empty())
}

pub(super) fn value(index: usize) -> u128 {
    VALUES[index]
}

pub(super) fn reference(index: usize) -> *const u128 {
    &VALUES[index]
}

#[derive(Clone)]
pub(super) struct Indices(pub u64);
impl Iterator for Indices {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        if self.0 == 0 {
            return None;
        }
        let index = self.0.trailing_zeros() as usize;
        self.0 &= self.0 - 1;
        Some(index)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.0.count_ones() as usize;
        (len, Some(len))
    }
}
impl DoubleEndedIterator for Indices {
    fn next_back(&mut self) -> Option<usize> {
        if self.0 == 0 {
            return None;
        }
        let index = 63 - self.0.leading_zeros() as usize;
        self.0 &= !(1 << index);
        Some(index)
    }
}
impl ExactSizeIterator for Indices {}
