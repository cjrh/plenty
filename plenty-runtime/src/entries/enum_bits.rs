//! Canonical read-only slots and dense ordinals for small finite enum domains. Membership
//! lives in the owner; these slots give borrowed iteration stable `&Enum`s.
use crate::aggregates::Type;

static VALUES: [u128; 256] = {
    let mut values = [0; 256];
    let mut tag = 0;
    while tag < 256 {
        values[tag] = (tag as u128) << 64;
        tag += 1;
    }
    values
};

pub(crate) fn eligible(key: &Type) -> bool {
    key.kind == b'B'
        && !key.affine
        && key.wrapper.is_none()
        && (key.finite.is_some()
            || ((1..=256).contains(&key.variants.len())
                && key.variants.iter().all(|variant| variant.fields.is_empty())))
}

pub(crate) fn cardinality(key: &Type) -> usize {
    match key.kind {
        b'b' => 2,
        _ => key
            .finite
            .map_or(key.variants.len(), |finite| finite.values.len()),
    }
}

/// Dense ordinal in the compiler's variant/declaration-order Cartesian product.
/// The caller supplies a valid value of a compiler-proven finite type. Record
/// pointers may address either ordinary inline storage or canonical metadata.
pub(crate) unsafe fn index(key: &Type, value: u128) -> usize {
    match key.kind {
        b'b' => value as usize,
        _ => {
            let tag = key.tag(value);
            let Some(finite) = key.finite else { return tag };
            let mut ordinal = 0;
            if let Some(record) = key.record(value) {
                for (field, ty) in record.slots() {
                    // SAFETY: the caller supplies a valid record and the compiler
                    // bounded every field's domain before emitting this metadata.
                    ordinal = ordinal * cardinality(ty) + unsafe { index(ty, field.read()) };
                }
            } else if let Some(ty) = key.payload(value) {
                // SAFETY: unpack preserves the sole field's native representation.
                ordinal = unsafe { index(ty, key.unpack(value)) };
            }
            usize::from(finite.offsets[tag]) + ordinal
        }
    }
}

pub(super) fn value(key: &Type, index: usize) -> u128 {
    key.finite.map_or(&VALUES[..], |finite| finite.values)[index]
}

pub(super) fn reference(key: &Type, index: usize) -> *const u128 {
    &key.finite.map_or(&VALUES[..], |finite| finite.values)[index]
}

/// Four portable words fit inside the existing insertion-order storage, so
/// widening the mask does not enlarge list, dictionary, or set owners. Unused
/// high bits stay zero; complement is only used under intersection with a set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Bits([u64; 4]);
impl Bits {
    pub fn contains(&self, index: usize) -> bool {
        self.0[index / 64] & (1 << (index % 64)) != 0
    }
    pub fn insert(&mut self, index: usize) {
        self.0[index / 64] |= 1 << (index % 64);
    }
    pub fn remove(&mut self, index: usize) {
        self.0[index / 64] &= !(1 << (index % 64));
    }
    pub fn len(&self) -> usize {
        self.0.iter().map(|word| word.count_ones() as usize).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.0 == [0; 4]
    }
    pub fn after(mut self, index: usize) -> Self {
        let word = index / 64;
        self.0[..word].fill(0);
        self.0[word] &= u64::MAX.checked_shl((index % 64 + 1) as u32).unwrap_or(0);
        self
    }
}
impl std::ops::BitAnd for Bits {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i] & rhs.0[i]))
    }
}
impl std::ops::BitOr for Bits {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i] | rhs.0[i]))
    }
}
impl std::ops::BitXor for Bits {
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i] ^ rhs.0[i]))
    }
}
impl std::ops::Not for Bits {
    type Output = Self;
    fn not(self) -> Self {
        Self(self.0.map(|word| !word))
    }
}

#[derive(Clone)]
pub(super) struct Indices(pub Bits);
impl Iterator for Indices {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        for (i, word) in self.0.0.iter_mut().enumerate() {
            if *word != 0 {
                let index = i * 64 + word.trailing_zeros() as usize;
                *word &= *word - 1;
                return Some(index);
            }
        }
        None
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.0.len();
        (len, Some(len))
    }
}
impl DoubleEndedIterator for Indices {
    fn next_back(&mut self) -> Option<usize> {
        for (i, word) in self.0.0.iter_mut().enumerate().rev() {
            if *word != 0 {
                let bit = 63 - word.leading_zeros() as usize;
                *word &= !(1 << bit);
                return Some(i * 64 + bit);
            }
        }
        None
    }
}
impl ExactSizeIterator for Indices {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn algebra_preserves_all_four_words_and_zero_high_bits() {
        for size in [1, 63, 64, 65, 127, 128, 129, 255, 256] {
            let mut left = Bits::default();
            let mut right = Bits::default();
            for i in 0..size {
                if i % 2 == 0 {
                    left.insert(i);
                }
                if i % 3 == 0 {
                    right.insert(i);
                }
            }
            for i in 0..256 {
                let a = i < size && i % 2 == 0;
                let b = i < size && i % 3 == 0;
                assert_eq!((left | right).contains(i), a || b);
                assert_eq!((left & right).contains(i), a && b);
                assert_eq!((left & !right).contains(i), a && !b);
                assert_eq!((left ^ right).contains(i), a != b);
            }
            assert!((left & !left).is_empty());
            assert_eq!(
                Indices(left | right).len(),
                (0..size).filter(|i| i % 2 == 0 || i % 3 == 0).count()
            );
        }
    }
}
