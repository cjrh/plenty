//! Compile-time integer constants and source string storage.

/// A handle into the compilation's string-literal pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StrId(u32);

/// A sized integer literal. Runtime values are represented by native code.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
}

/// Source string literals retained for the duration of compilation.
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
    /// happen through a bug in the compiler — never through a user's program.
    pub fn str(&self, id: StrId) -> &str {
        &self.strings[id.0 as usize]
    }
}
