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

impl Range {
    pub fn new(start: u64, stop: u64, step: i64, signed: bool) -> Self {
        if step == 0 {
            crate::fail("range step cannot be zero");
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
            crate::fail("range length exceeds i64");
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
                    for i in 0..moved.len() {
                        if moved.ty(i).payload_bytes() != 0 {
                            relocate(moved.slot(i), moved.ty(i));
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
            active.relocate(destination.cast());
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

/// Repair the internal address after moving a complete typed storage slot.
pub(crate) unsafe fn relocate(slot: *mut u128, ty: &Type) {
    unsafe {
        let value = slot.read();
        if let Some(active) = ty.active_payload(value) {
            let destination = slot.add(1);
            slot.write((value & !(u64::MAX as u128)) | destination as u128);
            active.relocate(destination);
        }
    }
}
