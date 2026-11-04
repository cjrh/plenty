//! Canonical read-only slots and dense ordinals for small finite enum domains. Membership
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
        && (key.finite.is_some()
            || ((1..=64).contains(&key.variants.len())
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
