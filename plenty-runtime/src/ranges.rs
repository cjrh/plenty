//! Inline range payloads, including ranges nested in standard sum wrappers.
use crate::aggregates::Type;

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct Range {
    pub start: u64,
    pub stop: u64,
    pub step: i64,
    pub len: u64,
}
const _: () = assert!(size_of::<Range>() == 32);
// Compiled loops over a stored range read these fields directly
// (`lower_range_parts` in the compiler's codegen).
const _: () = assert!(std::mem::offset_of!(Range, start) == 0);
const _: () = assert!(std::mem::offset_of!(Range, step) == 16);
const _: () = assert!(std::mem::offset_of!(Range, len) == 24);

impl Range {
    #[cfg(test)]
    pub fn new(start: u64, stop: u64, step: i64, signed: bool) -> Self {
        Self::new_at(start, stop, step, signed, std::ptr::null())
    }

    /// `site` locates the `range(...)` call in a trap for invalid parameters.
    pub fn new_at(
        start: u64,
        stop: u64,
        step: i64,
        signed: bool,
        site: *const crate::Site,
    ) -> Self {
        if step == 0 {
            crate::fail_at(site, format_args!("range step cannot be zero"));
        }
        let number = |n: u64| if signed { n as i64 as i128 } else { n as i128 };
        let distance = if step > 0 {
            number(stop) - number(start)
        } else {
            number(start) - number(stop)
        };
        let magnitude = (step as i128).abs();
        let len = if distance <= 0 {
            0
        } else {
            (distance + magnitude - 1) / magnitude
        };
        if len > i64::MAX as i128 {
            crate::fail_at(site, format_args!("range length exceeds i64"));
        }
        Self {
            start,
            stop,
            step,
            len: len as u64,
        }
    }

    pub fn start(&self, signed: bool) -> i128 {
        if signed {
            self.start as i64 as i128
        } else {
            self.start as i128
        }
    }

    pub fn at(&self, index: usize, signed: bool) -> u128 {
        (self.start(signed) + index as i128 * self.step as i128) as u128
    }

    pub fn contains(&self, value: u128, signed: bool) -> bool {
        let value = if signed {
            value as i64 as i128
        } else {
            value as u64 as i128
        };
        let distance = value - self.start(signed);
        distance % self.step as i128 == 0
            && (0..self.len as i128).contains(&(distance / self.step as i128))
    }
}

impl Type {
    pub(crate) fn has_inline_range(&self) -> bool {
        self.kind == b'R' || self.inline_range
    }

    pub(crate) fn slot_words(&self) -> usize {
        1 + self.payload_bytes() / 16
    }

    pub(crate) fn payload_bytes(&self) -> usize {
        (self.inline_bytes as usize).max(if self.has_inline_range() { 32 } else { 0 })
    }

    /// Whether a class field of this type is stored flat: the record's bytes
    /// sit in the owner's record with no value word. Matches `Ty::flat_field`
    /// in the compiler: a class (after newtype layers, whose descriptors
    /// carry their field's layout) that keeps a record.
    pub(crate) fn flat_field(&self) -> bool {
        self.kind == b'C' && self.inline_bytes != 0
    }

    /// Words a class field of this type takes in a class record.
    pub(crate) fn field_words(&self) -> usize {
        if self.flat_field() {
            self.payload_bytes() / 16
        } else {
            self.slot_words()
        }
    }

    /// The bytes and kind of the owner-local storage `value` currently uses.
    fn active_payload(&self, value: u128) -> Option<Payload<'_>> {
        if let Some(record) = self.record(value) {
            return Some(Payload::Record(record));
        }
        match self.kind {
            b'R' | b'G' | b'H' if self.payload_bytes() != 0 && value as u64 != 0 => {
                Some(Payload::Object(self))
            }
            b'B' => self
                .payload(value)
                .and_then(|t| t.active_payload(self.unpack(value))),
            _ => None,
        }
    }
}

enum Payload<'a> {
    Object(&'a Type),
    Record(crate::aggregates::Record<'a>),
}
impl Payload<'_> {
    fn bytes(&self) -> usize {
        match self {
            Self::Object(ty) => ty.payload_bytes(),
            Self::Record(record) => record.words() * 16,
        }
    }
    /// Repair internal addresses after the payload's bytes moved to `destination`.
    unsafe fn relocate(&self, destination: *mut u128) {
        unsafe {
            match self {
                Self::Object(ty) if ty.kind == b'G' => {
                    crate::generators::relocate(destination.cast())
                }
                Self::Object(ty) if ty.kind == b'H' => crate::closures::relocate(destination),
                Self::Object(_) => {}
                Self::Record(record) => {
                    let moved = record.moved_to(destination);
                    for (field, ty) in moved.slots() {
                        if field.flat {
                            // Its record moved with the owner; only its own
                            // payloads need new addresses.
                            if !ty.fixed() {
                                Payload::Record(field.record(ty)).relocate(field.at);
                            }
                        } else if ty.payload_bytes() != 0 {
                            relocate(field.at, ty);
                        }
                    }
                }
            }
        }
    }
}

/// Transfer an inline payload to owner-provided storage, preserving sum tags.
/// The source must be live; destination reserves `ty.payload_bytes()` bytes.
/// For affine payloads the caller must relinquish the source ownership.
pub(crate) unsafe fn copy_payload(value: u128, ty: &Type, destination: *mut Range) -> u128 {
    if let Some(active) = ty.active_payload(value) {
        unsafe {
            std::ptr::copy(
                value as u64 as *const u8,
                destination.cast(),
                active.bytes(),
            );
            if !ty.fixed() {
                active.relocate(destination.cast());
            }
        }
        (value & !(u64::MAX as u128)) | destination as u128
    } else {
        value
    }
}

/// A typed storage slot starts with the ordinary scalar/sum bits, followed by
/// inline storage when that type contains an owner-local payload.
pub(crate) unsafe fn store(slot: *mut u128, value: u128, ty: &Type) {
    unsafe {
        let value = if ty.payload_bytes() != 0 {
            copy_payload(value, ty, slot.add(1).cast())
        } else {
            value
        };
        slot.write(value);
    }
}

/// Whether `ty` is a class or multi-field tuple whose fields keep no inline
/// storage apart from flat records of the same kind, so moving its slot only
/// changes the address in its value word. Mirrors `Ty::flat_record`.
pub(crate) fn flat_record(ty: &Type) -> bool {
    let plain =
        |field: &Type| field.payload_bytes() == 0 || field.flat_field() && flat_record(field);
    match ty.kind {
        b'C' => ty.variants.iter().all(|v| plain(v.fields[0])),
        b'B' => {
            ty.variants.len() == 1
                && ty.variants[0].fields.len() >= 2
                && ty.variants[0].fields.iter().all(|f| plain(f))
        }
        _ => false,
    }
}

/// `relocate` for a `flat_record` slot.
pub(crate) unsafe fn relocate_flat(slot: *mut u128) {
    unsafe {
        let value = slot.read();
        if value as u64 != 0 {
            slot.write((value & !(u64::MAX as u128)) | slot.add(1) as u128);
        }
    }
}

/// Repair the internal address after moving a complete typed storage slot.
pub(crate) unsafe fn relocate(slot: *mut u128, ty: &Type) {
    unsafe {
        let value = slot.read();
        if let Some(active) = ty.active_payload(value) {
            let destination = slot.add(1);
            slot.write((value & !(u64::MAX as u128)) | destination as u128);
            if !ty.fixed() {
                active.relocate(destination);
            }
        }
    }
}
