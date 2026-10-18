//! Compile-time integer constants and source string storage.

/// A handle into the compilation's string-literal pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StrId(u32);

/// A sized integer literal. Runtime values are represented by native code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

/// Source string and byte literals retained for the duration of compilation.
/// Both kinds share one handle space because both become one read-only data
/// symbol with the same counted layout.
#[derive(Default)]
pub struct Heap {
    literals: Vec<Literal>,
}

enum Literal {
    Text(String),
    Bytes(Box<[u8]>),
}

impl Heap {
    /// Store `s` and return a handle to it.
    pub fn add_str(&mut self, s: String) -> StrId {
        self.add(Literal::Text(s))
    }

    /// Store the contents of a `b"..."` literal and return a handle to it.
    pub fn add_bytes(&mut self, bytes: Box<[u8]>) -> StrId {
        self.add(Literal::Bytes(bytes))
    }

    fn add(&mut self, literal: Literal) -> StrId {
        let id = StrId(self.literals.len() as u32);
        self.literals.push(literal);
        id
    }

    /// The literal's bytes and its scalar count. A byte literal has no
    /// scalars; its count repeats the byte length.
    pub fn data(&self, id: StrId) -> (&[u8], usize) {
        match &self.literals[id.0 as usize] {
            Literal::Text(text) => (text.as_bytes(), text.chars().count()),
            Literal::Bytes(bytes) => (bytes, bytes.len()),
        }
    }
}
