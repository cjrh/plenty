//! Native primitives behind `std.tls`: rustls with its ring provider in the
//! runtime. A session never touches a socket: `std.tls` moves ciphertext
//! between it and a `std.net` stream, so the stream's deadlines and
//! cancellation bound every handshake and transfer. See
//! book/src/design/stdlib/tls.md.
use crate::effects::Effect;
use crate::{op::Ty, sum};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TlsOp {
    Roots,
    RootsCount,
    ClientConfig,
    ServerConfig,
    Client,
    Server,
    Feed,
    Take,
    Read,
    Write,
    State,
    Close,
}

impl TlsOp {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__tls_roots" => Self::Roots,
            "__tls_roots_count" => Self::RootsCount,
            "__tls_client_config" => Self::ClientConfig,
            "__tls_server_config" => Self::ServerConfig,
            "__tls_client" => Self::Client,
            "__tls_server" => Self::Server,
            "__tls_feed" => Self::Feed,
            "__tls_take" => Self::Take,
            "__tls_read" => Self::Read,
            "__tls_write" => Self::Write,
            "__tls_state" => Self::State,
            "__tls_close" => Self::Close,
            _ => return None,
        })
    }

    /// Roots, configurations and sessions are byte storage owned by
    /// `std.tls`. Failures are negative codes that `std.tls` names; transfers
    /// return a byte count.
    pub fn signature(self) -> (Vec<Ty>, Ty) {
        let handle = || Ty::ByteStorage;
        let opened = || sum::result(handle(), Ty::I64);
        let range = || vec![handle(), handle(), Ty::I64, Ty::I64];
        match self {
            // PEM text.
            Self::Roots => (vec![Ty::Str], opened()),
            Self::RootsCount | Self::State | Self::Close => (vec![handle()], Ty::I64),
            Self::ClientConfig | Self::Server => (vec![handle()], opened()),
            // Certificate chain and private key, both PEM.
            Self::ServerConfig => (vec![Ty::Str, Ty::Str], opened()),
            // Configuration and server name.
            Self::Client => (vec![handle(), Ty::Str], opened()),
            // Session, buffer, start, stop.
            Self::Feed | Self::Take | Self::Read | Self::Write => (range(), Ty::I64),
        }
    }

    /// The buffer that `Take` and `Read` fill. A session's state lives behind
    /// a lock in the runtime, never in its byte view, so sessions are shared
    /// operands.
    pub fn exclusive(self, operand: usize) -> bool {
        matches!(self, Self::Take | Self::Read) && operand == 1
    }

    /// Starting a session draws randomness; feeding one may verify a
    /// certificate against the wall clock. The rest is memory and computation.
    pub fn effect(self) -> Effect {
        match self {
            Self::Client | Self::Server => Effect::Entropy,
            Self::Feed => Effect::Clock,
            Self::Roots
            | Self::RootsCount
            | Self::ClientConfig
            | Self::ServerConfig
            | Self::Take
            | Self::Read
            | Self::Write
            | Self::State
            | Self::Close => Effect::Compute,
        }
    }

    pub fn opcode(self) -> i64 {
        470 + self as i64
    }
}
