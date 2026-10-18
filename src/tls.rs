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
    Builder,
    BuilderRoots,
    BuilderCertificate,
    BuilderProtocol,
    BuilderVersion,
    BuilderSuite,
    BuilderRevocations,
    Build,
    Protocol,
    ServerName,
    PeerCertificate,
    Version,
    Suite,
    // The operations above reading their `source` operand from a `bytes`
    // literal, which the runtime tells from a buffer by its header.
    WriteLiteral,
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
            "__tls_builder" => Self::Builder,
            "__tls_builder_roots" => Self::BuilderRoots,
            "__tls_builder_certificate" => Self::BuilderCertificate,
            "__tls_builder_protocol" => Self::BuilderProtocol,
            "__tls_builder_version" => Self::BuilderVersion,
            "__tls_builder_suite" => Self::BuilderSuite,
            "__tls_builder_revocations" => Self::BuilderRevocations,
            "__tls_build" => Self::Build,
            "__tls_protocol" => Self::Protocol,
            "__tls_server_name" => Self::ServerName,
            "__tls_peer_certificate" => Self::PeerCertificate,
            "__tls_version" => Self::Version,
            "__tls_suite" => Self::Suite,
            _ => return None,
        })
    }

    /// The operand the primitive only reads bytes from, which a `Bytes`
    /// buffer or a `bytes` literal can supply.
    pub fn source(self) -> Option<usize> {
        match self.buffer_form() {
            Self::Write => Some(1),
            _ => None,
        }
    }

    /// The variant reading `source` from a `bytes` literal.
    pub fn literal(self) -> Option<Self> {
        Some(match self {
            Self::Write => Self::WriteLiteral,
            _ => return None,
        })
    }

    fn buffer_form(self) -> Self {
        match self {
            Self::WriteLiteral => Self::Write,
            other => other,
        }
    }

    /// Roots, configurations and sessions are byte storage owned by
    /// `std.tls`. Failures are negative codes that `std.tls` names; transfers
    /// return a byte count.
    pub fn signature(self) -> (Vec<Ty>, Ty) {
        let form = self.buffer_form();
        if form != self {
            let (mut inputs, output) = form.signature();
            inputs[form.source().unwrap()] = Ty::ByteStr;
            return (inputs, output);
        }
        let handle = || Ty::ByteStorage;
        let opened = || sum::result(handle(), Ty::I64);
        let range = || vec![handle(), handle(), Ty::I64, Ty::I64];
        match self {
            // PEM text.
            Self::Roots => (vec![Ty::Str], opened()),
            Self::RootsCount | Self::State | Self::Close | Self::Version | Self::Suite => {
                (vec![handle()], Ty::I64)
            }
            Self::ClientConfig | Self::Server | Self::Build => (vec![handle()], opened()),
            // 1 for a server builder, 0 for a client's.
            Self::Builder => (vec![Ty::I64], opened()),
            // Builder, roots, and whether clients must present a certificate.
            Self::BuilderRoots => (vec![handle(), handle(), Ty::I64], Ty::I64),
            // Builder, certificate chain and private key.
            Self::BuilderCertificate => (vec![handle(), Ty::Str, Ty::Str], Ty::I64),
            // Builder and an ALPN name or revocation list PEM.
            Self::BuilderProtocol | Self::BuilderRevocations => (vec![handle(), Ty::Str], Ty::I64),
            // Builder and minimum version (12 or 13) or suite index.
            Self::BuilderVersion | Self::BuilderSuite => (vec![handle(), Ty::I64], Ty::I64),
            // Session and the buffer that receives the detail.
            Self::Protocol | Self::ServerName | Self::PeerCertificate => (range(), Ty::I64),
            // Certificate chain and private key, both PEM.
            Self::ServerConfig => (vec![Ty::Str, Ty::Str], opened()),
            // Configuration and server name.
            Self::Client => (vec![handle(), Ty::Str], opened()),
            // Session, buffer, start, stop.
            Self::Feed | Self::Take | Self::Read | Self::Write => (range(), Ty::I64),
            Self::WriteLiteral => {
                unreachable!("a literal variant defers to its buffer form")
            }
        }
    }

    /// The buffer that `Take`, `Read` and the detail copies fill. A session's state lives behind
    /// a lock in the runtime, never in its byte view, so sessions are shared
    /// operands; so is a builder's plan.
    pub fn exclusive(self, operand: usize) -> bool {
        let form = self.buffer_form();
        if form != self {
            return form.exclusive(operand);
        }
        matches!(
            self,
            Self::Take | Self::Read | Self::Protocol | Self::ServerName | Self::PeerCertificate
        ) && operand == 1
    }

    /// Starting a session draws randomness; feeding one may verify a
    /// certificate against the wall clock. The rest is memory and computation.
    pub fn effect(self) -> Effect {
        let form = self.buffer_form();
        if form != self {
            return form.effect();
        }
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
            | Self::Close
            | Self::Builder
            | Self::BuilderRoots
            | Self::BuilderCertificate
            | Self::BuilderProtocol
            | Self::BuilderVersion
            | Self::BuilderSuite
            | Self::BuilderRevocations
            | Self::Build
            | Self::Protocol
            | Self::ServerName
            | Self::PeerCertificate
            | Self::Version
            | Self::Suite => Effect::Compute,
            Self::WriteLiteral => {
                unreachable!("a literal variant defers to its buffer form")
            }
        }
    }

    pub fn opcode(self) -> i64 {
        let form = self.buffer_form();
        if form != self {
            return form.opcode();
        }
        470 + self as i64
    }
}
