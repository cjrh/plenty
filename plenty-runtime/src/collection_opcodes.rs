//! Shared dispatcher numbers and the scratch-operand contract.
//! The compiler includes this file directly; the runtime is linked separately.

pub const SET_IS_SUBSET: i64 = 62;
pub const SET_IS_SUPERSET: i64 = 63;
pub const SET_IS_DISJOINT: i64 = 64;
pub const SET_UNION: i64 = 65;
pub const SET_INTERSECTION: i64 = 66;
pub const SET_DIFFERENCE: i64 = 67;
pub const SET_SYMMETRIC_DIFFERENCE: i64 = 68;
pub const KEY_VIEW_UNION: i64 = 580;
pub const KEY_VIEW_INTERSECTION: i64 = 581;
pub const KEY_VIEW_DIFFERENCE: i64 = 582;
pub const KEY_VIEW_SYMMETRIC_DIFFERENCE: i64 = 583;
pub const KEY_VIEW_EQUAL: i64 = 584;
pub const KEY_VIEW_TO_SET: i64 = 585;

/// Operands after the three fixed input words of `plenty_op_<N>` calls.
/// The result occupies word 3; only actual fourth/fifth inputs initialize
/// words 4/5. Keep this contract shared: codegen checks its emitted arity,
/// and the runtime uses it before reading either scratch word.
///
/// Operations absent from this list take at most three inputs. This is
/// native ABI arity, including hidden operands such as a column field index.
#[inline(always)]
pub const fn scratch_operand_count(op: i64) -> usize {
    match op {
        // Byte overwrite, membership/prefix scans, float formatting;
        // datagram send/receive.
        179 | 231 | 234 | 281 | 416 | 417 => 2,
        // Byte append, file/standard-stream output, scans and fill.
        178 | 188 | 189 | 191 | 230 | 232 | 233 | 236 | 237
        // Digest/HMAC update/verification, AEAD and HKDF.
        | 301 | 302 | 305 | 308..=311 | 315
        // Codec steps and checksums; readiness and selectors.
        | 321 | 325 | 419 | 421 | 422
        // TLS buffer transfers and details; primitive column assignment.
        | 476..=479 | 490..=492 | 527 => 1,
        _ => 0,
    }
}
