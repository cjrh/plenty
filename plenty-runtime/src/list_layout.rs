//! Dense list ABI shared with the compiler, which reads and writes scalar list
//! rows inline (issue #509). Offsets are from a live list owner; the runtime
//! asserts them against its repr(C) `Collection` and `Entries`.

/// Address of row 0, a `*mut u128`.
pub const ROWS: i32 = 24;
/// Initialized row words, a `usize`; a dense list has `WORDS / stride` rows.
pub const WORDS: i32 = 32;
/// One byte, 1 while a popped row lies past the live rows, otherwise 0.
pub const REMOVED: i32 = 48;
/// A scalar (numeric or `bool`) row is two 16-byte words: the element's slot,
/// whose low 64 bits hold its payload, then an unused value word.
pub const SCALAR_ROW_SHIFT: u32 = 5;
