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
/// Slice syntax, `xs[start:stop:step]`: list, text, byte and column slices.
pub const LIST_SLICE: i64 = 590;
pub const TEXT_SLICE: i64 = 591;
pub const BYTES_SLICE: i64 = 283;
/// Linear reverse search over two arbitrary byte views.
pub const BYTES_RFIND: i64 = 284;
pub const CLIST_SLICE: i64 = 532;
/// Bits of a slice's flags operand: which bounds the source wrote. An omitted
/// bound's default depends on the step's sign, so no bound value can stand
/// for it.
pub const SLICE_START: i64 = 1;
pub const SLICE_STOP: i64 = 2;
/// `&substr` views of text (issue #595). A view is the address of its first
/// byte and its byte count.
pub const TEXT_VIEW_SUBVIEW: i64 = 592;
pub const TEXT_VIEW_OF_STR: i64 = 593;
pub const TEXT_VIEW_LEN: i64 = 594;
/// Operands: two texts or views, then bit 0 (first) and bit 1 (second) set
/// for each `str` operand. The result is -1, 0, or 1.
pub const TEXT_VIEW_COMPARE: i64 = 595;
pub const TEXT_VIEW_SCALAR: i64 = 596;
pub const TEXT_VIEW_SCALAR_BEFORE: i64 = 597;
pub const TEXT_VIEW_INDEX: i64 = 598;
pub const TEXT_VIEW_COPY: i64 = 599;
/// Operands: the view, the needle, and the mode shifted left once, with bit 0
/// set for a `str` needle. Modes are `TEXT_SEARCH_*`.
pub const TEXT_VIEW_SEARCH: i64 = 600;
pub const TEXT_SEARCH_STARTS: i64 = 0;
pub const TEXT_SEARCH_ENDS: i64 = 1;
pub const TEXT_SEARCH_FIND: i64 = 2;
pub const TEXT_SEARCH_CONTAINS: i64 = 3;
/// `dict.get_or_insert`: the dictionary reference, the owned key and the
/// owned default (issue #748).
pub const DICT_GET_OR_INSERT: i64 = 601;
/// The high word of a `DICT_GET_OR_INSERT` result whose key was present. An
/// `Ok` word's high word is otherwise 0 and an `Err` word's is odd. On a hit
/// the caller still owns the copyable operands; on a miss the table owns them
/// (issue #762).
pub const GET_OR_INSERT_HIT: i64 = 2;
/// `list.insert(index, element)`: the list, the index and the owned element
/// (issue #798).
pub const LIST_INSERT: i64 = 602;
/// Borrowed list[str] to a private packed C pointer table and string owner.
pub const C_STRINGS: i64 = 603;

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
        // Float formatting; datagram receive.
        281 | 417 => 2,
        // Slices: receiver, start, stop, step and the bound flags.
        LIST_SLICE | TEXT_SLICE | BYTES_SLICE | CLIST_SLICE => 2,
        // A text view's source, start, stop and the bound flags.
        TEXT_VIEW_SUBVIEW | TEXT_VIEW_OF_STR => 1,
        // Child spawn: separate redirected files and explicit inherited files.
        197 => 2,
        // File input, fill and copy_within.
        188 | 237 | 282
        // AEAD and HKDF.
        | 308..=311 | 315
        // Key signing and RSA verification.
        | 345 | 346
        // Readiness and selectors.
        | 419 | 421 | 422
        // TLS buffer transfers and details; primitive column assignment.
        | 477 | 478 | 490..=492 | 527 => 1,
        _ => 0,
    }
}
