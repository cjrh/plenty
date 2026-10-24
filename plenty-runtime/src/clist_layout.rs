//! Packed clist ABI shared with the compiler. The runtime asserts these
//! offsets against its repr(C) header; generated loads must guard null owners.
pub const LEN: i32 = 16;
pub const CAPACITY: i32 = 24;
pub const DATA: i32 = 40;
