//! Native primitives behind `std.crypto`: RustCrypto digests, HMAC, BLAKE3 and
//! constant-time comparison in the runtime. The public API, range validation
//! and error policy live in `std.crypto` and `std.bytes`.
use crate::{op::Ty, sum};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CryptoOp {
    New,
    NewKeyed,
    Update,
    UpdateText,
    Finish,
    Verify,
    Equal,
    NewDeriveKey,
}

impl CryptoOp {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__crypto_new" => Self::New,
            "__crypto_new_keyed" => Self::NewKeyed,
            "__crypto_update" => Self::Update,
            "__crypto_update_text" => Self::UpdateText,
            "__crypto_finish" => Self::Finish,
            "__crypto_verify" => Self::Verify,
            "__crypto_equal" => Self::Equal,
            "__crypto_new_derive_key" => Self::NewDeriveKey,
            _ => return None,
        })
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
        let storage = || Ty::ByteStorage;
        let range = || vec![storage(), storage(), Ty::I64, Ty::I64];
        let state = || sum::result(storage(), sum::alloc_error());
        match self {
            Self::New => (vec![Ty::U8], state()),
            Self::NewKeyed => (vec![Ty::U8, storage(), Ty::I64, Ty::I64], state()),
            // False once the state has finished.
            Self::Update => (range(), Ty::Bool),
            Self::UpdateText => (vec![storage(), Ty::Str], Ty::Bool),
            Self::Finish => (vec![storage(), storage(), Ty::I64], Ty::Bool),
            // 1 for a match, 0 for a mismatch, -1 once finished.
            Self::Verify => (range(), Ty::I64),
            Self::Equal => (vec![storage(), storage()], Ty::Bool),
            Self::NewDeriveKey => (vec![Ty::Str], state()),
        }
    }

    /// Operands the primitive writes through: the hasher state, and the
    /// output buffer of `Finish`.
    pub fn exclusive(self, operand: usize) -> bool {
        match self {
            Self::New | Self::NewKeyed | Self::Equal | Self::NewDeriveKey => false,
            Self::Finish => operand <= 1,
            Self::Update | Self::UpdateText | Self::Verify => operand == 0,
        }
    }

    pub fn opcode(self) -> i64 {
        300 + self as i64
    }
}
