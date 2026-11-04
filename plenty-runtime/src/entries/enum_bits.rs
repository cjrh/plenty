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

/// The complete type domain selects one, two, or four words once at creation.
/// Each operation dispatches to that width; sparse membership never changes it.
/// All alternatives fit inside the existing insertion-order storage. Unused
/// high bits stay zero; complement is only used under intersection with a set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bits {
    Small(u64),
    Medium([u64; 2]),
    Large([u64; 4]),
}
impl Default for Bits {
    fn default() -> Self {
        Self::Small(0)
    }
}
impl Bits {
    pub fn new(cardinality: usize) -> Self {
        match cardinality {
            1..=64 => Self::Small(0),
            65..=128 => Self::Medium([0; 2]),
            129..=256 => Self::Large([0; 4]),
            _ => unreachable!("finite enum domain"),
        }
    }
    pub fn empty(self) -> Self {
        match self {
            Self::Small(_) => Self::Small(0),
            Self::Medium(_) => Self::Medium([0; 2]),
            Self::Large(_) => Self::Large([0; 4]),
        }
    }
    pub fn contains(&self, index: usize) -> bool {
        match self {
            Self::Small(word) => {
                debug_assert!(index < 64);
                word & (1 << index) != 0
            }
            Self::Medium(words) => words[index / 64] & (1 << (index % 64)) != 0,
            Self::Large(words) => words[index / 64] & (1 << (index % 64)) != 0,
        }
    }
    #[cfg(test)]
    pub fn insert(&mut self, index: usize) {
        match self {
            Self::Small(word) => *word |= 1 << index,
            Self::Medium(words) => words[index / 64] |= 1 << (index % 64),
            Self::Large(words) => words[index / 64] |= 1 << (index % 64),
        }
    }
    pub fn len(&self) -> usize {
        match self {
            Self::Small(word) => word.count_ones() as usize,
            Self::Medium(words) => words.iter().map(|word| word.count_ones() as usize).sum(),
            Self::Large(words) => words.iter().map(|word| word.count_ones() as usize).sum(),
        }
    }
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Small(word) => *word == 0,
            Self::Medium(words) => *words == [0; 2],
            Self::Large(words) => *words == [0; 4],
        }
    }
    pub fn after(mut self, index: usize) -> Self {
        fn clear_prefix(words: &mut [u64], index: usize) {
            let word = index / 64;
            words[..word].fill(0);
            words[word] &= u64::MAX.checked_shl((index % 64 + 1) as u32).unwrap_or(0);
        }
        match &mut self {
            Self::Small(word) => *word &= u64::MAX.checked_shl(index as u32 + 1).unwrap_or(0),
            Self::Medium(words) => clear_prefix(words, index),
            Self::Large(words) => clear_prefix(words, index),
        }
        self
    }
}
impl std::ops::BitAnd for Bits {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::Small(a), Self::Small(b)) => Self::Small(a & b),
            (Self::Medium(a), Self::Medium(b)) => {
                Self::Medium(std::array::from_fn(|i| a[i] & b[i]))
            }
            (Self::Large(a), Self::Large(b)) => Self::Large(std::array::from_fn(|i| a[i] & b[i])),
            _ => unreachable!("sets of the same type have the same mask width"),
        }
    }
}
impl std::ops::BitOr for Bits {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::Small(a), Self::Small(b)) => Self::Small(a | b),
            (Self::Medium(a), Self::Medium(b)) => {
                Self::Medium(std::array::from_fn(|i| a[i] | b[i]))
            }
            (Self::Large(a), Self::Large(b)) => Self::Large(std::array::from_fn(|i| a[i] | b[i])),
            _ => unreachable!("sets of the same type have the same mask width"),
        }
    }
}
impl std::ops::BitXor for Bits {
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::Small(a), Self::Small(b)) => Self::Small(a ^ b),
            (Self::Medium(a), Self::Medium(b)) => {
                Self::Medium(std::array::from_fn(|i| a[i] ^ b[i]))
            }
            (Self::Large(a), Self::Large(b)) => Self::Large(std::array::from_fn(|i| a[i] ^ b[i])),
            _ => unreachable!("sets of the same type have the same mask width"),
        }
    }
}
impl std::ops::Not for Bits {
    type Output = Self;
    fn not(self) -> Self {
        match self {
            Self::Small(word) => Self::Small(!word),
            Self::Medium(words) => Self::Medium(words.map(|word| !word)),
            Self::Large(words) => Self::Large(words.map(|word| !word)),
        }
    }
}

fn pop_first(word: &mut u64) -> Option<usize> {
    if *word == 0 {
        return None;
    }
    let bit = word.trailing_zeros() as usize;
    *word &= *word - 1;
    Some(bit)
}
fn pop_last(word: &mut u64) -> Option<usize> {
    if *word == 0 {
        return None;
    }
    let bit = 63 - word.leading_zeros() as usize;
    *word &= !(1 << bit);
    Some(bit)
}
fn next_word(words: &mut [u64]) -> Option<usize> {
    words
        .iter_mut()
        .enumerate()
        .find_map(|(i, word)| pop_first(word).map(|bit| i * 64 + bit))
}
fn last_word(words: &mut [u64]) -> Option<usize> {
    words
        .iter_mut()
        .enumerate()
        .rev()
        .find_map(|(i, word)| pop_last(word).map(|bit| i * 64 + bit))
}

#[derive(Clone)]
pub(super) struct Indices(pub Bits);
impl Iterator for Indices {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        match &mut self.0 {
            Bits::Small(word) => pop_first(word),
            Bits::Medium(words) => next_word(words),
            Bits::Large(words) => next_word(words),
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.0.len();
        (len, Some(len))
    }
}
impl DoubleEndedIterator for Indices {
    fn next_back(&mut self) -> Option<usize> {
        match &mut self.0 {
            Bits::Small(word) => pop_last(word),
            Bits::Medium(words) => last_word(words),
            Bits::Large(words) => last_word(words),
        }
    }
}
impl ExactSizeIterator for Indices {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn algebra_preserves_selected_width_and_zero_high_bits() {
        for size in [1, 63, 64, 65, 127, 128, 129, 255, 256] {
            let mut left = Bits::new(size);
            let mut right = Bits::new(size);
            for i in 0..size {
                if i % 2 == 0 {
                    left.insert(i);
                }
                if i % 3 == 0 {
                    right.insert(i);
                }
            }
            for i in 0..(if size <= 64 {
                64
            } else if size <= 128 {
                128
            } else {
                256
            }) {
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
