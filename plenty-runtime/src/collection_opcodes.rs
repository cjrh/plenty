//! Shared dispatcher numbers for set operations and borrowed key projections.
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
