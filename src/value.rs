//! The data layer: the values that live on the Plenty stack, and the heap
//! that backs the ones too large to store inline.

/// A handle to a string held in a [`Heap`].
///
/// Four bytes wide, so a string-typed stack slot is no more expensive than an
/// integer one. A `StrId` is only meaningful to the `Heap` that issued it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StrId(u32);

/// A value on the Plenty stack.
///
/// Deliberately small — 16 bytes — because the stack is the one data structure
/// the language cannot avoid touching. A million integers cost 16 MB, not 32+.
/// Anything variable-sized (text today, arrays later) lives in the [`Heap`] and
/// is referenced here by a compact handle, never stored inline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Str(StrId),
}

/// Backing store for values that do not fit in a 16-byte stack slot.
///
/// Append-only: strings produced at runtime are added and never removed. This
/// keeps the implementation trivial. Reclaiming unused strings — deduplicating
/// interning, or a collector — is a deliberate later step, not a missing piece.
#[derive(Default)]
pub struct Heap {
    strings: Vec<String>,
}

impl Heap {
    /// Store `s` and return a handle to it.
    pub fn add_str(&mut self, s: String) -> StrId {
        let id = StrId(self.strings.len() as u32);
        self.strings.push(s);
        id
    }

    /// Borrow the string behind `id`.
    ///
    /// Panics only if given a handle this `Heap` never issued, which can only
    /// happen through a bug in the VM — never through a user's program.
    pub fn str(&self, id: StrId) -> &str {
        &self.strings[id.0 as usize]
    }
}
