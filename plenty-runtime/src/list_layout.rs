//! Dense list ABI shared with the compiler, which reads and writes scalar list
//! rows inline (issue #509). Offsets are from a live list owner; the runtime
//! asserts them against its repr(C) `Collection` and `Entries`.

/// Address of row 0, a `*mut u128`.
pub const ROWS: i32 = 24;
/// Initialized row bytes, a `usize`; a dense list has `BYTES / stride` rows.
pub const BYTES: i32 = 32;
/// One byte, 1 while a popped row lies past the live rows, otherwise 0.
pub const REMOVED: i32 = 48;

/// Bytes of a scalar row of descriptor kind `kind`: its declared 1, 2, 4 or 8
/// bytes. `None` for a kind stored in a slot.
pub const fn scalar_bytes(kind: u8) -> Option<usize> {
    match kind {
        b'1' | b'5' | b'b' => Some(1),
        b'2' | b'6' => Some(2),
        b'3' | b'7' | b'f' => Some(4),
        b'4' | b'8' | b'd' => Some(8),
        _ => None,
    }
}

/// Distance between a dense list's rows, from the element descriptor's kind
/// and inline payload. A row that is not a scalar holds one aligned 16-byte
/// slot and its inline payload, without a dictionary's value slot. Slice views
/// compute it in compiled code, so it must stay a function of the descriptor.
pub const fn row_bytes(kind: u8, inline_bytes: usize, inline_range: bool) -> usize {
    match scalar_bytes(kind) {
        Some(bytes) => bytes,
        None => {
            let payload = if inline_range && inline_bytes < 32 {
                32
            } else {
                inline_bytes
            };
            (1 + payload / 16) * 16
        }
    }
}
