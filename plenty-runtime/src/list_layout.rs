//! Dense list ABI shared with the compiler, which reads and writes scalar list
//! rows inline (issue #509). Offsets are from a live list owner; the runtime
//! asserts them against its repr(C) `Collection` and `Entries`.

/// Address of row 0, a `*mut u128`.
pub const ROWS: i32 = 24;
/// Initialized row bytes, a `usize`; a dense list has `BYTES / stride` rows.
pub const BYTES: i32 = 32;
/// One byte, 1 while a popped row lies past the live rows, otherwise 0.
pub const REMOVED: i32 = 48;
// Scalar rows occupy their declared 1, 2, 4 or 8 bytes. Other dense rows contain
// one aligned slot and its inline payload, without a dictionary's value slot.
