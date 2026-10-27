//! Native primitives behind `std.crypto`: RustCrypto digests/HMAC and ring
//! authenticated encryption, signatures, key exchange and derivation. The public API, range validation
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
    AesSeal = 8,
    AesOpen = 9,
    ChaChaSeal = 10,
    ChaChaOpen = 11,
    EdPublic = 12,
    EdSign = 13,
    EdVerify = 14,
    Hkdf = 15,
    XNew = 16,
    XPublic = 17,
    XAgree = 18,
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
            "__crypto_aes_seal" => Self::AesSeal,
            "__crypto_aes_open" => Self::AesOpen,
            "__crypto_chacha_seal" => Self::ChaChaSeal,
            "__crypto_chacha_open" => Self::ChaChaOpen,
            "__crypto_ed_public" => Self::EdPublic,
            "__crypto_ed_sign" => Self::EdSign,
            "__crypto_ed_verify" => Self::EdVerify,
            "__crypto_hkdf" => Self::Hkdf,
            "__crypto_x_new" => Self::XNew,
            "__crypto_x_public" => Self::XPublic,
            "__crypto_x_agree" => Self::XAgree,
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
            Self::AesSeal | Self::AesOpen | Self::ChaChaSeal | Self::ChaChaOpen | Self::Hkdf => {
                (vec![storage(), storage(), storage(), storage()], Ty::I64)
            }
            Self::EdPublic | Self::XPublic => (vec![storage(), storage()], Ty::I64),
            Self::EdSign | Self::EdVerify | Self::XAgree => {
                (vec![storage(), storage(), storage()], Ty::I64)
            }
            Self::XNew => (vec![], state()),
        }
    }

    /// Operands the primitive writes through: mutable state and output buffers.
    pub fn exclusive(self, operand: usize) -> bool {
        match self {
            Self::New | Self::NewKeyed | Self::Equal | Self::EdVerify | Self::XNew => false,
            Self::AesSeal | Self::AesOpen | Self::ChaChaSeal | Self::ChaChaOpen | Self::Hkdf => {
                operand == 3
            }
            Self::EdPublic | Self::XPublic => operand == 1,
            Self::EdSign => operand == 2,
            Self::XAgree => operand == 0 || operand == 2,
            Self::Finish => operand <= 1,
            Self::Update | Self::UpdateText | Self::Verify => operand == 0,
        }
    }

    pub fn opcode(self) -> i64 {
        300 + self as i64
    }
}
